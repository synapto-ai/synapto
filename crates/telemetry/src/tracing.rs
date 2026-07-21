#[cfg(feature = "log-to-file")]
use std::path::Path;

use tracing::Event;
use tracing::Metadata;

pub struct ShutdownDowngradeLayer;

impl<S: tracing::Subscriber> Layer<S> for ShutdownDowngradeLayer {
    fn on_event(&self, event: &Event<'_>, _cx: tracing_subscriber::layer::Context<'_, S>) {
        if cfg!(debug_assertions) {
            return;
        }

        if *event.metadata().level() == tracing::Level::ERROR
            && synapto_shutdown::is_shutting_down()
        {
            let mut visitor = MsgVisitor::default();
            event.record(&mut visitor);
            let m = visitor.message;
            tracing::trace!(is_downgraded = true, "{}", m);
        }
    }
}

pub struct ShutdownErrorFilter;
impl<S> tracing_subscriber::layer::Filter<S> for ShutdownErrorFilter {
    fn enabled(
        &self,
        meta: &Metadata<'_>,
        _cx: &tracing_subscriber::layer::Context<'_, S>,
    ) -> bool {
        if cfg!(debug_assertions) {
            true
        } else {
            !(meta.level() == &tracing::Level::ERROR && synapto_shutdown::is_shutting_down())
        }
    }
}

#[derive(Default)]
struct MsgVisitor {
    message: String,
}
impl tracing::field::Visit for MsgVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        }
    }
}

use tracing::level_filters::LevelFilter;
use tracing_subscriber::fmt::format::FmtSpan;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

pub trait ReloadHandle: Send + Sync {
    fn add_directive(&self, directive: String);
}

// Implement the trait for the actual reload::Handle
// This works for ANY subscriber S
impl<S> ReloadHandle for tracing_subscriber::reload::Handle<EnvFilter, S>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a> + Send + Sync,
{
    fn add_directive(&self, directive: String) {
        // We don't unwrap/panic on failure here because during sequential test runs,
        // we might be trying to modify a reload handle belonging to a dropped
        // tracing subscriber layer from a previous test.
        if let Err(e) = self.modify(|filter| {
            if let Ok(d) = directive.parse() {
                *filter = filter.clone().add_directive(d);
            }
        }) {
            tracing::warn!(
                "Failed to modify tracing filter for directive '{}'. This is expected if running multiple tests sequentially: {}",
                directive,
                e
            );
        }
    }
}

use std::sync::OnceLock;

static GLOBAL_RELOAD_HANDLE: OnceLock<Box<dyn ReloadHandle>> = OnceLock::new();

pub struct Tracing {
    #[cfg(feature = "log-to-file")]
    _file_guard: tracing_appender::non_blocking::WorkerGuard,
    _tracy_guard: Option<TracyGuard>,
}

impl Tracing {
    pub fn setup(gui_layer: GuiErrorLayer) -> Self {
        // --- GLOBAL FILTER CONFIGURATION ---
        // By default, all unlisted crates are restricted to WARN level.
        // `synapto` and `synapto_*` workspace crates get TRACE access explicitly.
        // Plugins get TRACE access dynamically via `add_plugin_to_log()`.
        //
        // HOW TO ADD ANOTHER WORKSPACE CRATE (e.g., `synapto-something`):
        // Append `,synapto_something=trace` to the format string below.
        // Rust's tracing replaces hyphens in crate names with underscores.
        let log_filter = EnvFilter::new(
            "warn,synapto=trace,synapto_llm=trace,synapto_shutdown=trace,synapto_interface=trace,telemetry=trace",
        );

        let (reloadable_filter, reload_handle) =
            tracing_subscriber::reload::Layer::new(log_filter.clone());

        #[cfg(feature = "log-to-file")]
        let (non_blocking, file_guard) = {
            let directory = Path::new("logs");
            std::fs::create_dir_all(directory)
                .unwrap_or_else(|e| panic!("failed to create log directory: {:?}", e));

            let start_time = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
            let log_path = directory.join(format!("run-{}.log", start_time));

            let log_writer = file_rotate::FileRotate::new(
                log_path,
                file_rotate::suffix::AppendTimestamp::with_format(
                    "%Y%m%dT%H%M%S.log",
                    file_rotate::suffix::FileLimit::MaxFiles(24),
                    file_rotate::suffix::DateFrom::Now,
                ),
                file_rotate::ContentLimit::Time(file_rotate::TimeFrequency::Hourly),
                file_rotate::compression::Compression::OnRotate(1),
                None,
            );

            tracing_appender::non_blocking(log_writer)
        };

        #[cfg(feature = "log-to-file")]
        let file_error_layer = tracing_subscriber::fmt::layer()
            .with_span_events(FmtSpan::CLOSE)
            .with_writer(non_blocking.clone())
            .with_ansi(false)
            .with_file(true)
            .with_line_number(true)
            .with_filter(ShutdownErrorFilter)
            .with_filter(tracing_subscriber::filter::filter_fn(|meta| {
                meta.level() == &tracing::Level::ERROR && meta.target() != "panic"
            }));

        #[cfg(feature = "log-to-file")]
        let file_panic_layer = tracing_subscriber::fmt::layer()
            .with_span_events(FmtSpan::CLOSE)
            .with_writer(non_blocking.clone())
            .with_ansi(false)
            .with_filter(ShutdownErrorFilter)
            .with_filter(tracing_subscriber::filter::filter_fn(|meta| {
                meta.level() == &tracing::Level::ERROR && meta.target() == "panic"
            }));

        #[cfg(feature = "log-to-file")]
        let file_non_error_layer = tracing_subscriber::fmt::layer()
            .with_span_events(FmtSpan::CLOSE)
            .with_writer(non_blocking)
            .with_ansi(false)
            .with_filter(tracing_subscriber::filter::filter_fn(|meta| {
                meta.level() != &tracing::Level::ERROR
            }))
            .with_filter(LevelFilter::DEBUG);

        let stdout_error_layer = {
            let layer = tracing_subscriber::fmt::layer()
                .with_span_events(FmtSpan::CLOSE)
                .with_writer(std::io::stdout)
                .with_file(true)
                .with_line_number(true);

            let boxed_layer = if std::env::var("LOG_FORMAT")
                .map(|v| v == "json")
                .unwrap_or(false)
            {
                layer.json().flatten_event(true).boxed()
            } else {
                layer.boxed()
            };

            boxed_layer
                .with_filter(ShutdownErrorFilter)
                .with_filter(tracing_subscriber::filter::filter_fn(|meta| {
                    meta.level() == &tracing::Level::ERROR && meta.target() != "panic"
                }))
                .with_filter(log_filter.clone())
                .with_filter(
                    EnvFilter::builder()
                        .with_default_directive(LevelFilter::INFO.into())
                        .from_env_lossy(),
                )
        };

        let stdout_panic_layer = {
            let layer = tracing_subscriber::fmt::layer()
                .with_span_events(FmtSpan::CLOSE)
                .with_writer(std::io::stdout);

            let boxed_layer = if std::env::var("LOG_FORMAT")
                .map(|v| v == "json")
                .unwrap_or(false)
            {
                layer.json().flatten_event(true).boxed()
            } else {
                layer.boxed()
            };

            boxed_layer
                .with_filter(ShutdownErrorFilter)
                .with_filter(tracing_subscriber::filter::filter_fn(|meta| {
                    meta.level() == &tracing::Level::ERROR && meta.target() == "panic"
                }))
                .with_filter(log_filter.clone())
                .with_filter(
                    EnvFilter::builder()
                        .with_default_directive(LevelFilter::INFO.into())
                        .from_env_lossy(),
                )
        };

        let stdout_non_error_layer = {
            let layer = tracing_subscriber::fmt::layer()
                .with_span_events(FmtSpan::CLOSE)
                .with_writer(std::io::stdout);

            let boxed_layer = if std::env::var("LOG_FORMAT")
                .map(|v| v == "json")
                .unwrap_or(false)
            {
                layer.json().flatten_event(true).boxed()
            } else {
                layer.boxed()
            };

            boxed_layer
                .with_filter(tracing_subscriber::filter::filter_fn(|meta| {
                    meta.level() != &tracing::Level::ERROR
                }))
                .with_filter(log_filter.clone())
                .with_filter(
                    EnvFilter::builder()
                        .with_default_directive(LevelFilter::INFO.into())
                        .from_env_lossy(),
                )
        };

        let subscriber = tracing_subscriber::registry()
            .with(ShutdownDowngradeLayer)
            .with(stdout_error_layer)
            .with(stdout_panic_layer)
            .with(stdout_non_error_layer);

        #[cfg(feature = "log-to-file")]
        let subscriber = subscriber
            .with(file_error_layer.boxed())
            .with(file_panic_layer.boxed())
            .with(file_non_error_layer.boxed());

        let tracy_guard = cfg_select!(
            feature = "tracy" => Some(TracyGuard::new()),
            _ => None
        );

        #[cfg(feature = "tracy")]
        let subscriber = subscriber.with(tracing_tracy::TracyLayer::default().boxed());

        let subscriber = subscriber.with(gui_layer.with_filter(ShutdownErrorFilter).boxed());

        #[cfg(feature = "rerun")]
        let subscriber = {
            let binary_name = std::env::current_exe()
                .ok()
                .and_then(|path| {
                    path.file_stem() // Using file_stem() strips .exe on Windows
                        .map(|stem| stem.to_string_lossy().into_owned())
                })
                .unwrap_or_else(|| "unknown".to_string());
            let rec = rerun::RecordingStreamBuilder::new(binary_name.clone())
                .spawn()
                .or_else(|_e| {
                    rerun::RecordingStreamBuilder::new(binary_name)
                        .connect_grpc_opts("rerun+http://host.docker.internal:9876/proxy") // TODO consider security concerns
                })
                .unwrap_or_else(|e| panic!("Rerun problem: {:?}", e));

            rerun::RecordingStream::set_global(rerun::StoreKind::Recording, Some(rec.clone()));
            subscriber
                .with(
                    super::rerun_logging::RerunLoggingLayer {
                        path: "logs/tracing".into(),
                    }
                    .with_filter(LevelFilter::DEBUG)
                    .with_filter(log_filter)
                    .boxed(),
                )
                .with(super::rerun::RerunTelemetryLayer::default().boxed())
                .with(super::graph_layer::RerunGraphLayer::new().boxed())
        };

        // Use `try_init` instead of `init` to prevent panics when running multiple tests
        // sequentially, as the global tracing subscriber from the first test remains active.
        if let Err(e) = subscriber.with(reloadable_filter).try_init() {
            eprintln!(
                "Warning: Failed to set global tracing subscriber. This is expected during sequential test runs: {}",
                e
            );
        } else {
            // Store the handle for the successfully initialized global subscriber.
            // We ignore the error if it was already set.
            GLOBAL_RELOAD_HANDLE.set(Box::new(reload_handle)).ok();
        }

        Self {
            #[cfg(feature = "log-to-file")]
            _file_guard: file_guard,
            _tracy_guard: tracy_guard,
        }
    }

    pub fn add_plugin_to_log(plugin_name: &str) {
        let plugin_name_sanitized = plugin_name.replace("-", "_");
        let directive = format!("{}=trace,telemetry=trace", plugin_name_sanitized);
        if let Some(handle) = GLOBAL_RELOAD_HANDLE.get() {
            handle.add_directive(directive);
        }
    }
}

pub struct TracyGuard(#[cfg(feature = "tracy")] tracy_client::Client);

impl TracyGuard {
    #[cfg(feature = "tracy")]
    fn new() -> Self {
        Self(tracy_client::Client::start())
    }
}

pub struct GuiErrorLayer {
    sender: std::sync::mpsc::Sender<String>,
}

impl GuiErrorLayer {
    pub fn new() -> (Self, std::sync::mpsc::Receiver<String>) {
        let (sender, receiver) = std::sync::mpsc::channel();
        (Self { sender }, receiver)
    }
}

impl<S: tracing::Subscriber> Layer<S> for GuiErrorLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if *event.metadata().level() == tracing::Level::ERROR {
            let mut visitor = ErrorVisitor::default();
            event.record(&mut visitor);

            self.sender
                .send(visitor.message)
                .inspect_err(|e| {
                    // SAFETY: Do not use `tracing::error!` here to avoid triggering infinite recursion.
                    eprintln!("{}", e)
                })
                .ok();
        }
    }
}

#[derive(Default)]
struct ErrorVisitor {
    message: String,
}

impl tracing::field::Visit for ErrorVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        }
    }
}
