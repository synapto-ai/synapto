#[cfg(feature = "rerun")]
pub struct RerunLoggingLayer {
    pub rec: rerun::RecordingStream,
    pub path: String,
}

#[cfg(feature = "rerun")]
impl<S> tracing_subscriber::Layer<S> for RerunLoggingLayer
where
    S: tracing::Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    fn on_event(&self, event: &tracing::Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        use std::fmt::Write;

        let meta = event.metadata();
        let mut msg = String::new();

        if let (&tracing::Level::ERROR, Some(file), Some(line)) =
            (meta.level(), meta.file(), meta.line())
        {
            write!(msg, "[{}:{}] ", file, line)
                .unwrap_or_else(|e| unreachable!("String formatting cannot fail: {:?}", e));
        }

        let spans = ctx
            .event_scope(event)
            .map(|scope| {
                scope
                    .from_root()
                    .map(|s| s.metadata().name().to_string())
                    .collect::<Vec<_>>()
                    .join("::")
            })
            .unwrap_or_default();

        let span_prefix = if spans.is_empty() {
            String::new()
        } else {
            format!("{spans}: ")
        };
        write!(msg, "{}{}: ", span_prefix, meta.target())
            .unwrap_or_else(|e| unreachable!("String formatting cannot fail: {:?}", e));

        struct Visitor<'a> {
            buf: &'a mut String,
        }
        impl<'a> tracing::field::Visit for Visitor<'a> {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    write!(self.buf, "{:?} ", value)
                        .unwrap_or_else(|e| unreachable!("String formatting cannot fail: {:?}", e));
                } else {
                    write!(self.buf, "{}={:?} ", field.name(), value)
                        .unwrap_or_else(|e| unreachable!("String formatting cannot fail: {:?}", e));
                }
            }
        }

        let mut v = Visitor { buf: &mut msg };
        event.record(&mut v);

        let rerun_level = match *meta.level() {
            tracing::Level::ERROR => rerun::TextLogLevel::ERROR,
            tracing::Level::WARN => rerun::TextLogLevel::WARN,
            tracing::Level::INFO => rerun::TextLogLevel::INFO,
            tracing::Level::DEBUG => rerun::TextLogLevel::DEBUG,
            tracing::Level::TRACE => rerun::TextLogLevel::TRACE,
        };

        let text = rerun::archetypes::TextLog::new(msg).with_level(rerun_level);
        if let Err(e) = self.rec.log(self.path.as_str(), &text) {
            // SAFETY: Do not use `tracing::error!` here to avoid triggering infinite recursion.
            eprintln!("Failed to log to Rerun: {:?}", e);
        }
    }
}
