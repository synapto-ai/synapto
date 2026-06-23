//! # Development Guide: Rerun Telemetry
//!
//! This module provides a generic metrics representer for the [Rerun](https://rerun.io/) window.
//! It is primarily used to visualize real-time timeseries data, such as voice detection levels,
//! directly within the Rerun viewer.
//!
//! ## Prerequisites
//!
//! To use this telemetry, you must have the `rerun` binary installed and available in your PATH.
//! You can install it via:
//!
//! ```bash
//! cargo install rerun-cli --locked
//! ```
//!
//! ## Usage
//!
//! The telemetry layer is enabled via the `rerun` feature flag. When running the AI system,
//! ensure you include the feature:
//!
//! ```bash
//! cargo run --features rerun
//! ```
//!
//! Once the system is running, the metrics will be automatically streamed to the global
//! Rerun recording stream.
//!
//! ## Visualization
//!
//! In the Rerun viewer, metrics are logged under the `metrics/` path. For voice detection,
//! you will see a `metrics/voice_level` (or similar) entry which can be viewed as a
//! Timeseries graph.
//!
//! ### How it works
//!
//! The `RerunTelemetryLayer` is a `tracing` Layer that visits events looking for specific
//! fields:
//! - `metric`: The name of the metric (e.g., "voice_level").
//! - `value`: The numeric value to log (supports `f64`, `i64`, `u64`, and `bool`).
//!
//! ### How to log metrics exclusively to Rerun
//!
//! To send a metric to Rerun **without** cluttering the console or file logs, use the combination of the `trace!` level and `target: "telemetry"`:
//!
//! 1. **`target: "telemetry"`**: Ensures the `RerunTelemetryLayer` intercepts and processes the event.
//! 2. **`tracing::trace!`**: Because stdout filters out anything below `INFO` and file logs filter out anything below `DEBUG`, using the `TRACE` level intentionally hides this metric from standard logs while still allowing Rerun to graph it.
//!
//! Example:
//! ```rust
//! tracing::trace!(target: "telemetry", metric = "my_custom_metric", value = 42.0, "Logging a value");
//! ```
//!
//! ### Advanced: Direct Native Rerun Integration
//!
//! While `tracing` is excellent for text logs and simple scalar metrics, it acts as a bottleneck for rich, high-bandwidth data (e.g., camera feeds, massive JSON structures).
//! Instead of coercing rich data into stringified `tracing` macros, developers should use the global Rerun instance directly via `log_to_rerun`.
//!
//! ```rust,ignore
//! use synapto::telemetry::log_to_rerun;
//! use rerun::archetypes::Image;
//!
//! // Directly push native archetypes!
//! log_to_rerun("sensors/camera", &Image::from_rgba32(rgba_data, width, height));
//! ```
//!
//! Note: `log_to_rerun` acts as a zero-cost no-op when the system is compiled without the `rerun` feature flag.
//!
//! ### Profiling vs Telemetry
//!
//! It is important to distinguish between **Profiling** and **Telemetry** in this system:
//!
//! - **Telemetry (Rerun)**: Real-time, streaming visualization. Metrics are sent to the Rerun viewer as they happen. This is enabled via the `rerun` feature. Metrics are automatically enriched with hierarchical context (`Subsystem/SpanName`) if emitted within an instrumented span.
//! - **Profiling (Tracy)**: Deep instrumentation via the Tracy profiler. Enabled via the `tracy` feature.
//!
//! ### Hierarchical Metrics
//!
//! When logging metrics with the `telemetry` target inside an instrumented span, the metric name is automatically prefixed:
//!
//! `metrics/<subsystem>/<span_name>/<metric_name>`
//!
//! For LLM calls, this is handled automatically via the `track_stats = true` field on the span,
//! which generates `latency`, `avg`, and `max` metrics.

#![cfg(feature = "rerun")]

use crate::find_parent_subsystem;
use dashmap::{DashMap, DashSet};
use rerun::RecordingStream;
use std::time::Instant;
use tracing::Subscriber;
use tracing::field::Visit;
use tracing_subscriber::Layer;
use tracing_subscriber::registry::{LookupSpan, SpanRef};

#[derive(Debug, Default)]
struct SpanStats {
    count: u64,
    total_time_us: u128,
    max_time_us: u128,
}

pub struct RerunTelemetryLayer {
    rec: RecordingStream,
    initialized_paths: DashSet<String>,
    stats: DashMap<String, SpanStats>,
}

impl Default for RerunTelemetryLayer {
    fn default() -> Self {
        let rec = rerun::RecordingStream::global(rerun::StoreKind::Recording).unwrap_or_else(|| {
        unreachable!(
            "Rerun recording stream not found. The global stream must be initialized prior to constructing the telemetry layer."
        )
    });

        Self {
            rec,
            initialized_paths: DashSet::new(),
            stats: DashMap::new(),
        }
    }
}

struct SpanStartTime(Instant);

impl RerunTelemetryLayer {
    fn log_metric(&self, path: String, value: f64) {
        let metric_path = format!("metrics/{path}");
        if self.initialized_paths.insert(metric_path.clone()) {
            self.rec
                .log_static(metric_path.clone(), &rerun::SeriesPoints::new())
                .inspect_err(|e| tracing::error!("{}", e))
                .ok();
        }
        self.rec
            .log(metric_path, &rerun::Scalars::new([value]))
            .ok();
    }
}

#[derive(Default)]
struct MetricsVisitor {
    track_stats: bool,
    metric_name: Option<String>,
    value: Option<f64>,
}

impl Visit for MetricsVisitor {
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        if field.name() == "track_stats" {
            self.track_stats = value;
        } else if field.name() == "value" {
            self.value = Some(if value { 1.0 } else { 0.0 });
        }
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        if field.name() == "value" {
            self.value = Some(value);
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        if field.name() == "value" {
            self.value = Some(value as f64);
        }
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if field.name() == "value" {
            self.value = Some(value as f64);
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "metric" {
            self.metric_name = Some(value.to_string());
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "metric" {
            self.metric_name = Some(format!("{:?}", value));
        }
    }
}

#[derive(Default)]
struct ChatVisitor {
    role: Option<String>,
    reasoning: Option<String>,
    modality: Option<String>,
    messages: Option<String>,
    tools: Option<String>,
}

impl Visit for ChatVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "role" => self.role = Some(value.to_string()),
            "reasoning" => self.reasoning = Some(value.to_string()),
            "modality" => self.modality = Some(value.to_string()),
            "messages" => self.messages = Some(value.to_string()),
            "tools" => self.tools = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "role" => self.role = Some(format!("{:?}", value)),
            "reasoning" => self.reasoning = Some(format!("{:?}", value)),
            "modality" => self.modality = Some(format!("{:?}", value)),
            "messages" => self.messages = Some(format!("{:#?}", value)),
            "tools" => self.tools = Some(format!("{:?}", value)),
            _ => {}
        }
    }
}
impl<S> Layer<S> for RerunTelemetryLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut metrics_visitor = MetricsVisitor::default();
        attrs.record(&mut metrics_visitor);

        if metrics_visitor.track_stats
            && let Some(span) = ctx.span(id)
        {
            span.extensions_mut().insert(SpanStartTime(Instant::now()));
        }
    }

    fn on_close(&self, id: tracing::span::Id, ctx: tracing_subscriber::layer::Context<'_, S>) {
        let Some(span) = ctx.span(&id) else {
            tracing::error!("Span not found for id: {:?}", id);
            return;
        };

        let elapsed_us = {
            let extensions = span.extensions();
            extensions
                .get::<SpanStartTime>()
                .map(|start| start.0.elapsed().as_micros())
        };

        if let Some(elapsed_us) = elapsed_us {
            let duration_ms = elapsed_us as f64 / 1000.0;
            let path = resolve_span_path(span);

            let mut entry = self.stats.entry(path.clone()).or_default();
            entry.count += 1;
            entry.total_time_us += elapsed_us;
            if elapsed_us > entry.max_time_us {
                entry.max_time_us = elapsed_us;
            }

            let avg_ms = (entry.total_time_us as f64 / entry.count as f64) / 1000.0;
            let max_ms = entry.max_time_us as f64 / 1000.0;

            self.log_metric(format!("{path}/duration"), duration_ms);
            self.log_metric(format!("{path}/avg"), avg_ms);
            self.log_metric(format!("{path}/max"), max_ms);
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: tracing_subscriber::layer::Context<'_, S>) {
        if event.metadata().target() != "telemetry" {
            return;
        }

        let mut metrics_visitor = MetricsVisitor::default();
        let mut chat_visitor = ChatVisitor::default();

        event.record(&mut metrics_visitor);
        event.record(&mut chat_visitor);

        if let (Some(name), Some(value)) = (metrics_visitor.metric_name, metrics_visitor.value) {
            let prefix = ctx
                .event_span(event)
                .map(|span| {
                    let path = resolve_span_path(span);
                    format!("{path}/")
                })
                .unwrap_or_default();

            self.log_metric(format!("{prefix}{name}"), value);
        }

        if let Some(role) = chat_visitor.role {
            let path = if let Some(m) = chat_visitor.modality {
                format!("chat/{role}/{m}")
            } else {
                format!("chat/{role}")
            };

            self.rec
                .log(
                    path,
                    &rerun::archetypes::TextLog::new(chat_visitor.messages.unwrap_or_default()),
                )
                .ok();

            if role == "assistant" {
                if let Some(tools) = chat_visitor.tools {
                    self.rec
                        .log(
                            "chat/assistant/tools",
                            &rerun::archetypes::TextLog::new(tools),
                        )
                        .ok();
                }
                if let Some(reasoning) = chat_visitor.reasoning {
                    self.rec
                        .log(
                            "chat/assistant/reasoning",
                            &rerun::archetypes::TextLog::new(reasoning),
                        )
                        .ok();
                }
            }
        }
    }
}

fn resolve_span_path<S>(span: SpanRef<'_, S>) -> String
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    let target = span.metadata().target();
    let span_name = span.name();

    if let Some(ss) = find_parent_subsystem(span) {
        format!("{ss}/{span_name}@{target}")
    } else {
        format!("{target}/{span_name}")
    }
}
