## Development Guide: Rerun Telemetry

This module provides a generic metrics representer for the [Rerun](https://rerun.io/) window.
It is primarily used to visualize real-time timeseries data, such as voice detection levels,
directly within the Rerun viewer.

### Prerequisites

To use this telemetry, you must have the `rerun` binary installed and available in your PATH.
You can install it via:

```bash
cargo install rerun-cli --locked
```

### Usage

The telemetry layer is enabled via the `rerun` feature flag. When running the AI system,
ensure you include the feature:

```bash
cargo run --features rerun
```

Once the system is running, the metrics will be automatically streamed to the global
Rerun recording stream.

### Visualization

In the Rerun viewer, metrics are logged under the `metrics/` path. For voice detection,
you will see a `metrics/voice_level` (or similar) entry which can be viewed as a
Timeseries graph.

#### How it works

The `RerunTelemetryLayer` is a `tracing` Layer that visits events looking for specific
fields:

- `metric`: The name of the metric (e.g., "voice_level").
- `value`: The numeric value to log (supports `f64`, `i64`, `u64`, and `bool`).

Example of logging a metric:

```rust
tracing::trace!(target: "telemetry", metric = "my_custom_metric", value = 42.0, "Logging a value");
```

#### Profiling vs Telemetry

It is important to distinguish between **Profiling** and **Telemetry** in this system:

- **Telemetry (Rerun)**: Real-time, streaming visualization. Metrics are sent to the Rerun viewer as they happen. This is enabled via the `rerun` feature. Metrics are automatically enriched with hierarchical context (`Subsystem/SpanName`) if emitted within an instrumented span.
- **Profiling (Tracy)**: Deep instrumentation via the Tracy profiler. Enabled via the `tracy` feature.

#### Hierarchical Metrics

When logging metrics with the `telemetry` target inside an instrumented span, the metric name is automatically prefixed:

`metrics/<subsystem>/<span_name>/<metric_name>`

For LLM calls, this is handled automatically via the `track_stats = true` field on the span,
which generates `duration`, `avg`, and `max` metrics.

---

## Testing & Verification

We maintain strict test-driven development practices across the entire workspace. All code contributions must be accompanied by relevant tests.

For detailed guidelines on running unit tests, setting up live integration tests with `test_config.json`, or creating new tests for core modules and plugins, refer to our [Testing Guidelines](TESTING.md).

---

## Code Style & Clippy Guidelines

### Allowing `clippy::new_without_default`

You may use `#[allow(clippy::new_without_default)]` locally on a struct's `new` method when implementing `Default` is semantically incorrect or doesn't make sense. Specifically, when `new` doesn't return exactly the same initialized struct, it can be `new(...)` and not `default()`.

### Error Handling: Silently Dropping Results

Silently dropping `Result` values using `let _ = result;` is an anti-pattern as it hides failure modes and makes debugging difficult. While this is already enforced by project lints, you must handle errors properly when replacing them.

Do not ignore errors. Instead, ensure visibility into failures by explicitly logging them.

These rules don't apply in tests.

**Preferred Patterns:**

1.  **For standalone statements (logging and discarding):**

    ```rust
    if let Err(e) = result {
        tracing::error!("Operation failed: {}", e);
    }
    ```

2.  **When chaining method calls (logging and discarding):**

    ```rust
    result.inspect_err(|e| tracing::error!("Operation failed: {}", e)).ok();
    ```

    _Note:_ `result.unwrap_or_else(|e| tracing::error!("{}", e));` is acceptable _only_ if the `Ok` type is `()`, but `if let Err(e)` is generally preferred for clarity.

3.  **For theoretically impossible errors or missing values:**
    Use `unreachable!` when an error variant or `None` variant exists in the type signature but the specific state of your program makes it impossible to reach.

    ```rust
    // For Result types:
    result.unwrap_or_else(|e| unreachable!("This state should be impossible because... {}", e));

    // For Option types:
    option.unwrap_or_else(|| unreachable!("This state should be impossible because..."));
    ```

4.  **For fatal, unrecoverable errors (Result types):**
    Never use `.expect("<message>")` on `Result` types. Instead, always use `.unwrap_or_else` with a lazy `panic!`, appending the error context. This avoids formatting overhead on the happy path and ensures the error is preserved.

    Discarding the error variable (e.g., using `|_| panic!("<message>")`) is **strictly forbidden**. The error must always be part of the panic message.

    ```rust
    // Do not do this:
    // let client = plugin.expect("Failed to create plugin");
    // let client = plugin.unwrap_or_else(|_| panic!("Failed to create plugin"));

    // Instead, do this:
    let client = plugin.unwrap_or_else(|e| panic!("Failed to create plugin: {:?}", e));
    ```

    _Exceptions for non-Debug/non-Display errors (such as `Box<dyn Any>` downcasting):_
    If the `Err` variant cannot be formatted because it does not implement `Debug` or `Display`, you must still capture the error variable and include helpful dynamic type context (using `std::any::type_name_of_val(&*e)` or its TypeId) in the panic message rather than discarding it with `|_|`.

    ```rust
    // For Downcasting any types (Do not use `|_|`):
    let plugin = any_value.downcast::<MyPlugin>().unwrap_or_else(|e| {
        panic!(
            "Downcast failed to target type: {}. Actual dynamic type of error value was: {}",
            std::any::type_name::<MyPlugin>(),
            std::any::type_name_of_val(&*e)
        )
    });
    ```

    _Note:_ This rule also applies to `Option` types when a `None` value represents a fatal, unrecoverable developer error or invariant violation, but only if you are adding dynamic context (e.g., `.unwrap_or_else(|| panic!("State invalid: {}", variable))`). If you are only providing a static string message with no formatting, using `.expect("<message>")` on an `Option` is fully allowed and preferred for conciseness.

5.  **Readability vs. Allocations on Happy Path:**
    Readability is more important than avoiding minor allocations. It is completely acceptable to use simple allocations like `.ok_or("error message".to_string())?` or `.unwrap_or("default".to_string())`. The goal of avoiding "overhead on the happy path" is specifically to prevent calling heavy formatting functions (like `format!`) or executing complex logic directly in method arguments, not to forbid all basic allocations.
