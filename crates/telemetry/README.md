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

### Hierarchical Metrics & Automatic Latency Tracking

When logging metrics with the `telemetry` target inside an instrumented span, the metric name is automatically prefixed:

`metrics/<subsystem>/<span_name>/<metric_name>`

#### Generic Span Latency Instrumentation (`track_stats = true`)

Any asynchronous or synchronous operation can automatically measure and record execution latency by annotating its `tracing` span with `track_stats = true`:

```rust,ignore
#[tracing::instrument(
    level = "info",
    skip_all,
    fields(
        track_stats = true,
        operation = "my_critical_path"
    )
)]
async fn my_function() {
    // ...
}
```

When `track_stats = true` is present on a span:
1. **Span Open (`on_new_span`)**: The `RerunTelemetryLayer` stamps the opening instant using `Instant::now()`.
2. **Span Close (`on_close`)**: Upon completion, the layer computes elapsed duration and automatically emits three timeseries metrics under `metrics/<path>`:
   - `duration`: Call latency for this specific execution (in milliseconds).
   - `avg`: Cumulative rolling average execution time across all recorded calls.
   - `max`: Peak execution time recorded so far.

This pattern is applied centrally to infrastructure gateways:
- **LLM Subsystem**: `synapto-llm::LLMClient::call_inner` tracks model invocation and token generation latency.
- **Decision Subsystem**: `synapto_interface::decision::DecisionHandle::evaluate` tracks decision provider evaluation latency across all discrete judgment backends.
- **Speaker Recognition**: Vector embedding processing in `speaker-recognizer`.
