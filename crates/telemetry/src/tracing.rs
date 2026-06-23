use std::path::Path;

use tracing::Event;
use tracing::Metadata;

pub struct ShutdownDowngradeLayer;

impl<S: tracing::Subscriber> Layer<S> for ShutdownDowngradeLayer {
    fn on_event(&self, event: &Event<'_>, _cx: tracing_subscriber::layer::Context<'_, S>) {
        if *event.metadata().level() == tracing::Level::ERROR
            && synapto_shutdown::is_shutting_down()
        {
            let mut visitor = MsgVisitor::default();
            event.record(&mut visitor);
            let m = visitor.msg;
            if m.contains("Channel send failed")
                || m.contains("closed")
                || m.contains("background task failed")
            {
                // We emit a new TRACE event with the same message.
                tracing::trace!(is_downgraded = true, "{}", m);
            }
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
        meta.level() == &tracing::Level::ERROR
    }

    fn event_enabled(
        &self,
        event: &Event<'_>,
        _cx: &tracing_subscriber::layer::Context<'_, S>,
    ) -> bool {
        if !synapto_shutdown::is_shutting_down() {
            return true;
        }
        let mut visitor = MsgVisitor::default();
        event.record(&mut visitor);
        let m = &visitor.msg;
        if m.contains("Channel send failed")
            || m.contains("closed")
            || m.contains("background task failed")
        {
            return false;
        }
        true
    }
}

#[derive(Default)]
struct MsgVisitor {
    msg: String,
}
impl tracing::field::Visit for MsgVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.msg = format!("{:?}", value);
        }
    }
}

use tracing::level_filters::LevelFilter;
use tracing_appender::non_blocking::WorkerGuard;
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
        self.modify(|filter| {
            if let Ok(d) = directive.parse() {
                *filter = filter.clone().add_directive(d);
            }
        })
        .unwrap_or_else(|_| panic!("Failed to modify filter for directive: {}", directive));
    }
}

pub struct Tracing {
    _guards: (WorkerGuard, Option<TracyGuard>),
    reload_handle: Box<dyn ReloadHandle>,
}

impl Tracing {
    pub fn setup(gui_layer: GuiErrorLayer) -> Self {
        // --- GLOBAL FILTER CONFIGURATION ---
        // By default, all unlisted crates are restricted to WARN level.
        // `synapto` gets DEBUG access explicitly.
        // Plugins get DEBUG access dynamically via `add_plugin_to_log()`.
        //
        // HOW TO ADD ANOTHER WORKSPACE CRATE (e.g., `ai-llm-client`):
        // Append `,crate_name_with_underscores=debug` to the format string below.
        // Rust's tracing replaces hyphens in crate names with underscores.
        let log_filter =
            EnvFilter::new("warn,synapto=debug,synapto_llm=debug,telemetry=trace");

        let (reloadable_filter, reload_handle) =
            tracing_subscriber::reload::Layer::new(log_filter.clone());

        let (non_blocking, guard) = {
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

        let file_error_layer = tracing_subscriber::fmt::layer()
            .with_span_events(FmtSpan::CLOSE)
            .with_writer(non_blocking.clone())
            .with_ansi(false)
            .with_file(true)
            .with_line_number(true)
            .with_filter(ShutdownErrorFilter)
            .with_filter(LevelFilter::DEBUG);

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
            .with(stdout_non_error_layer)
            .with(file_error_layer.boxed())
            .with(file_non_error_layer.boxed());

        #[allow(unused_mut)]
        let mut tracy_guard: Option<TracyGuard> = None;

        #[cfg(feature = "tracy")]
        {
            tracy_guard = Some(TracyGuard::new());
        }

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
                        rec,
                        path: "logs/tracing".into(),
                    }
                    .with_filter(LevelFilter::DEBUG)
                    .with_filter(log_filter)
                    .boxed(),
                )
                .with(super::rerun::RerunTelemetryLayer::default().boxed())
                .with(super::graph_layer::RerunGraphLayer::new().boxed())
        };

        subscriber.with(reloadable_filter).init();

        Self {
            _guards: (guard, tracy_guard),
            reload_handle: Box::new(reload_handle),
        }
    }

    pub fn add_plugin_to_log(&self, plugin_name: &str) {
        let plugin_name_sanitized = plugin_name.replace("-", "_");
        let directive = format!("{}=debug,telemetry=trace", plugin_name_sanitized);
        self.reload_handle.add_directive(directive);
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
