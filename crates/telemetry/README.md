# Telemetry & Visualization

This crate provides a generic metrics representer for the [Rerun](https://rerun.io/) window. It is primarily used to visualize real-time timeseries data, such as voice detection levels, directly within the Rerun viewer.

## Prerequisites

To use this telemetry, you must have the `rerun` binary installed and available in your PATH.
You can install it via:

```bash
cargo install rerun-cli --locked
```

## Usage

The telemetry layer is enabled via the `rerun` feature flag. When running the AI system, ensure you include the feature:

```bash
cargo run --features rerun
```

Once the system is running, the metrics will be automatically streamed to the global Rerun recording stream.

## Visualization

In the Rerun viewer, metrics are logged under the `metrics/` path. For voice detection, you will see a `metrics/voice_level` (or similar) entry which can be viewed as a Timeseries graph.

### How it works

The `RerunTelemetryLayer` is a `tracing` Layer that visits events looking for specific fields:

- `metric`: The name of the metric (e.g., "voice_level").
- `value`: The numeric value to log (supports `f64`, `i64`, `u64`, and `bool`).

Example of logging a metric:

```rust,ignore
tracing::trace!(target: "telemetry", metric = "my_custom_metric", value = 42.0, "Logging a value");
```

### Profiling vs Telemetry

It is important to distinguish between **Profiling** and **Telemetry** in this system:

- **Telemetry (Rerun)**: Real-time, streaming visualization. Metrics are sent to the Rerun viewer as they happen. This is enabled via the `rerun` feature. Metrics are automatically enriched with hierarchical context (`Subsystem/SpanName`) if emitted within an instrumented span.
- **Profiling (Tracy)**: Deep instrumentation via the Tracy profiler. Enabled via the `tracy` feature.

### Hierarchical Metrics

When logging metrics with the `telemetry` target inside an instrumented span, the metric name is automatically prefixed:

`metrics/<subsystem>/<span_name>/<metric_name>`

For LLM calls, this is handled automatically via the `track_stats = true` field on the span, which generates `duration`, `avg`, and `max` metrics.
