# Synapto Scenario Testing Framework

## Overview

The `synapto-test` crate provides an integration testing framework to validate the cognitive lifecycle, tool calling, and multi-modal interactions of the Synapto AI using deterministic YAML-based scenarios.

## How It Works

Tests are driven by a `ScenarioCoordinator` which acts as the environment interacting with the AI. It simulates user inputs (text, speech) and waits for specific conditions (assertions on AI responses) to be met. The coordinator intercepts AI outputs by utilizing mock plugins injected at boot time.

## Creating a Scenario

### 1. The YAML Manifest

Scenarios are defined in `scenario.yaml` files. They consist of a sequence of `steps`.

Available actions:
- `user_writes`: Simulates a user typing in the chat.
  - `text`: (String) The message content.
  - `attachments`: (Optional List) Attached files/documents.
- `user_says`: Simulates the user speaking (feeds transcribed text).
  - `transcript`: (String) The spoken text.
- `wait`: Pauses the scenario execution for a duration (useful to let asynchronous tools or background tasks fire/resolve before following up).
  - `millis`: (Integer)
- `await_response`: Halts scenario progression until the AI responds matching specific criteria or a timeout occurs.
  - `assert_contains`: (Optional String) Exact substring match.
  - `assert_all`: (Optional List of Strings) All strings must be present in the response.
  - `assert_any`: (Optional List of Strings) At least one string must be present.
  - `case_sensitive`: (Optional Boolean, defaults to false)
  - `timeout_secs`: (Optional Integer) Maximum time to wait for the assertion to succeed.
- `play_audio`: Stream a raw audio file into the system.
  - `audio_stream`: (String) Path to the audio file.

**Example (`scenario.yaml`):**
```yaml
steps:
  - action: user_writes
    text: "Call the `mock_slow_read` tool right now to get information. The architect is what I need."

  - action: wait
    millis: 2000

  - action: user_writes
    text: "Tell me something about bitcoin."

  - action: await_response
    assert_all:
      - "bitcoin"
      - "Alice"
    timeout_secs: 60
```

### 2. The Rust Harness

To run the YAML scenario, write a standard `#[tokio::test]` that loads the scenario and boots a test bundle containing mock plugins. This usually resides in a `tests/scenario_tests.rs` file.

```rust
use synapto::Synapto;
use synapto_test::{run_scenario, MockAudioInputPlugin, MockChatPlugin, MockSlowReadPlugin};

// Define your bundle with Ephemeral datadirs and Mock plugins
async fn test_bundle() {
    Synapto::<
        test_datadir_ephemeral::EphemeralDir,
        (synapto::config::DotEnv, synapto::config::Env),
        test_storage_local::LocalStorage,
        synapto::prompt_provider::EmptyPromptProvider,
    >::run::<(
        MockAudioInputPlugin,
        MockChatPlugin,
        MockSlowReadPlugin,
        // ... other mock/real plugins
    )>()
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore] // Scenarios are integration tests; ignored by default
async fn my_new_scenario() {
    run_scenario("scenarios/my-new-scenario/scenario.yaml", test_bundle).await;
}
```

## Running Scenario Tests

Because scenario tests boot the entire Synapto system, query the real LLM endpoint, and register global OS state (like `tracing_subscriber` handlers and a custom panic/shutdown hook), **they must be run sequentially and are ignored by default**.

Running them sequentially ensures that the global singletons do not collide across threads. We recently updated the tracing and shutdown handlers to safely re-initialize or warn contextually across sequential runs, but concurrent execution will still lead to channel races or garbled states.

To execute them, you must provide the `--ignored` and `--test-threads=1` flags:
```bash
cargo test -p synapto-test --test scenario_tests -- --ignored --test-threads=1
```

To run a single specific scenario:
```bash
cargo test -p synapto-test --test scenario_tests my_new_scenario -- --ignored --test-threads=1
```

## Testing Plugins (External to Synapto)

If you are developing a plugin in a repository outside of the `synapto` workspace, you can still leverage the `synapto-test` scenario framework. 

### 1. Add Test Dependencies

In your plugin's `Cargo.toml`, add `synapto` and `synapto-test` as `dev-dependencies`:

```toml
[dev-dependencies]
synapto = { git = "https://github.com/synapto-ai/synapto.git" }
synapto-test = { git = "https://github.com/synapto-ai/synapto.git" }
test-datadir-ephemeral = { git = "https://github.com/synapto-ai/synapto.git" }
test-storage-local = { git = "https://github.com/synapto-ai/synapto.git" }
tokio = { version = "1.0", features = ["full"] }
```

### 2. Write the Scenario Test

Create your scenario YAML files (e.g., `scenarios/my-plugin-scenario/scenario.yaml`). Then, create a Rust integration test file (e.g., `tests/scenario_tests.rs`) that registers your *real* plugin alongside the *mock* plugins provided by `synapto-test`.

```rust
// tests/scenario_tests.rs
use my_plugin::MyChatPlugin; // Your actual plugin
use synapto::Synapto;
use synapto_test::{
    MockAudioInputPlugin, MockDiarizationPlugin, MockDocumentsPlugin,
    MockSlowReadPlugin, MockSttPlugin, MockTtsPlugin, run_scenario,
};

// Define a test bundle substituting MockChatPlugin for your Real plugin
async fn test_bundle() {
    Synapto::<
        test_datadir_ephemeral::EphemeralDir,
        (synapto::config::DotEnv, synapto::config::Env),
        test_storage_local::LocalStorage,
        synapto::prompt_provider::EmptyPromptProvider,
    >::run::<(
        MockAudioInputPlugin,
        MyChatPlugin, // Inject your real plugin here
        MockDocumentsPlugin,
        MockSlowReadPlugin,
        MockTtsPlugin,
        MockSttPlugin,
        MockDiarizationPlugin,
    )>()
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore] // Integration scenarios should be ignored by default
async fn test_my_chat_plugin_scenario() {
    run_scenario("scenarios/my-plugin-scenario/scenario.yaml", test_bundle).await;
}
```

### 3. Run Your External Plugin Tests

Execute them sequentially and request ignored tests just like inside the workspace:

```bash
cargo test --test scenario_tests -- --ignored --test-threads=1
```
