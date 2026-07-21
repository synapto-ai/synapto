#![doc = include_str!("../README.md")]

//! # Telemetry and Logging Architecture
//!
//! This module provides the central telemetry and logging configuration for the AI system.
//! It uses `tracing` and `tracing-subscriber` to route logs to multiple destinations (stdout, file, Rerun UI)
//! with varying levels of verbosity.
//!
//! ## Filtering Overview
//!
//! The system uses a layered filtering approach. A **Global Filter** establishes the absolute maximum verbosity,
//! while **Local Filters** on specific output layers further restrict what gets logged.
//!
//! ### The Global Filter
//! The global filter is applied to the root subscriber and defaults to:
//! `warn,synapto=debug,telemetry=trace`
//!
//! - **`synapto`**: Allowed up to `DEBUG`.
//! - **`telemetry`** target: Allowed up to `TRACE`.
//! - **Everything else (3rd-party crates)**: Capped at `WARN`.
//!
//! As plugins are dynamically loaded, they are added to this global filter via `add_plugin_to_log`, granting them `DEBUG` level access.
//!
//! ### Output Layers and Their Rules
//!
//! | Output Layer | Local Filter | Effective `synapto` / Plugins Level | Effective Other Crates Level | Effective `telemetry` Target Level |
//! | :--- | :--- | :--- | :--- | :--- |
//! | **Stdout** | `INFO` (can be overridden via `RUST_LOG`) | `>= INFO` | `>= WARN` | `>= INFO` |
//! | **File Logs** | `DEBUG` (`logs/run-*.log`) | `>= DEBUG` | `>= WARN` | `>= DEBUG` |
//! | **Rerun Text Logs**| `DEBUG` (`logs/tracing`) | `>= DEBUG` | `>= WARN` | `>= DEBUG` |
//! | **Rerun Telemetry**| *None* (only accepts `target="telemetry"`) | *N/A* (Ignored) | *N/A* (Ignored) | `>= TRACE` |
//!
//! #### Why `telemetry` `TRACE` logs don't appear in files or stdout:
//! Metrics are typically logged using `tracing::trace!(target: "telemetry", metric = "...", value = ...);`.
//! Because the File layer has a hard `DEBUG` filter, and Stdout has an `INFO` filter, these `TRACE` level events
//! are dropped by those layers. However, the `RerunTelemetryLayer` has no local level restrictions, allowing the
//! Global Filter's `telemetry=trace` directive to pass these events exclusively to the Rerun UI.
//!
//! ## How to Setup
//!
//! The entire logging stack is initialized at startup by calling `Tracing::setup(gui_layer)`.
//! This configures:
//! 1. Rolling file appender in the `logs/` directory.
//! 2. Standard output layer (with optional JSON formatting via `LOG_FORMAT=json`).
//! 3. Rerun integrations (if the `rerun` feature is enabled).
//! 4. Optional GUI error layer.
//!
//! To register a dynamic plugin's logs at runtime, call:
//! ```rust,ignore
//! tracing_handle.add_plugin_to_log("plugin_name");
//! ```
//! This safely mutates the global filter at runtime to allow `DEBUG` logs from the newly loaded plugin.
//!
//! ## How to Enhance or Change
//!
//! - **Change Stdout Verbosity**: Set the `RUST_LOG` environment variable (e.g., `RUST_LOG=debug`). By default, it falls back to `INFO`.
//! - **Change File Verbosity**: Modify the `.with_filter(LevelFilter::DEBUG)` constraint on the `file_layer` inside `tracing.rs`.
//! - **Log Telemetry to File**: If you want `telemetry` traces in the log files, change the `file_layer` filter from `LevelFilter::DEBUG` to an `EnvFilter` that explicitly allows `telemetry=trace`.
//! - **Add Another Workspace Crate**: Instructions for granting `DEBUG` access to other crates (like `synapto-llm`) are documented directly above the `EnvFilter::new` call in `tracing.rs`.
//!

pub mod graph_layer;
pub mod rerun;
pub mod rerun_logging;
pub mod tracing;

#[cfg(feature = "rerun")]
pub fn log_to_rerun<A: ::rerun::AsComponents>(path: impl Into<::rerun::EntityPath>, archetype: &A) {
    if let Some(rec) = ::rerun::RecordingStream::global(::rerun::StoreKind::Recording) {
        rec.log(path, archetype)
            .inspect_err(|e| ::tracing::error!("{}", e))
            .ok();
    }
}

#[cfg(not(feature = "rerun"))]
pub fn log_to_rerun<T>(_path: &str, _archetype: &T) {}

#[cfg(feature = "rerun")]
pub use ::rerun as rerun_core;

#[cfg(feature = "rerun")]
pub fn find_parent_subsystem<S>(span: tracing_subscriber::registry::SpanRef<S>) -> Option<String>
where
    S: ::tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    let mut current = Some(span);
    while let Some(s) = current {
        if s.metadata().fields().field("subsystem").is_some() {
            return Some(s.metadata().target().to_string());
        }
        current = s.parent();
    }
    None
}
