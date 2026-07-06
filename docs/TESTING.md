# TESTING GUIDELINES

This document serves as the master blueprint and reference for creating and executing tests across the `ai` codebase.

---

## 1. Testing Classifications & Philosophy

To maintain absolute system reliability, we classify tests into four distinct tiers based on execution time, deterministic isolation, and external dependency requirements:

| Test Tier | Scope & Rules | Execution Frequency | CI Requirement |
| :--- | :--- | :--- | :--- |
| **Unit Tests** | Strictly local, in-memory logic. Zero network calls, zero third-party integrations, zero database access. | Every commit / PR checks | **Mandatory** |
| **Integration Tests** | Dedicated validation of third-party API clients, credentials, state mutations, and external protocol logic (e.g., Google TTS, ElevenLabs). | On-demand / scheduled integration lanes | **Skipped by default (uses `#[ignore]`)** |
| **E2E Tests (Scenarios)** | Unified scenario workflows driven by YAML manifolds under `scenarios/`. Evaluates cognitive thought processing and end-to-end system outputs. | On-demand / Nightly runs | *Postponed / Under Maintenance* |
| **Research Tests** | Specific, one-off diagnostic or timing scripts located in the root directory (e.g., `test_ogg_duration.rs`). | Ad-hoc developer execution | **None (Excluded from CI)** |

---

## 2. Test Scope & Crate Rules

To prevent code pollution and keep build dependencies isolated, all tests must respect crate boundaries:

1. **Plugin Tests**: Must be fully contained inside the respective plugin crate (e.g., `ai/plugins/tts-google`). They must never depend on `core` or the orchestrator.
2. **Core Tests**: Scope to the `synapto` crate (`ai/core`). Used for validating core document ingestion, RAG memories, system prompts, and orchestrator state.
3. **Interface Tests**: Scope to the `synapto-interface` crate (`ai/interface`). Used solely for validating named channels, sync boundaries, or proxy structures.
   * *Note:* Real-world third-party integration tests are non-sensical inside `synapto-interface` since the interface only defines semantic types and trait boundaries.

---

## 3. How to Run Tests

### Standard Offline Suite (Unit Tests Only)
Run all fast, offline unit tests across the entire workspace:
```sh
cargo test
```

### Run Crate-Specific Unit Tests
* **Core Unit Tests:**
  ```sh
  cargo test -p synapto
  ```
* **Interface Unit Tests:**
  ```sh
  cargo test -p synapto-interface
  ```
* **Specific Plugin Unit Tests:**
  ```sh
  cargo test -p tts-google
  ```

### Run Live Third-Party Integration Tests
Integration tests are marked with standard `#[ignore]` attributes because they require credentials (either a local `test_config.json` inside the plugin crate or the global configuration fallback) and active internet access, so they should not fail compilation on standard developers' machines or raw CI nodes.

* **Run all ignored integration tests in the workspace:**
  ```sh
  cargo test -- --ignored
  ```
* **Run integration tests for a specific plugin only:**
  ```sh
  cargo test -p tts-google --test tts_google_integration_test -- --ignored
  ```

---

## 4. How to Create Tests

### 4.1. Creating a Unit Test
Unit tests are written inline or inside standard localized sub-modules within the target source file.

**Template (`src/my_module.rs`):**
```rust
pub fn add_one(x: i32) -> i32 {
    x + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_one() {
        assert_eq!(add_one(2), 3);
    }
}
```

### 4.2. Creating an Integration Test
Integration tests are written inside a dedicated `tests/` directory under the target crate root.

#### Localized Configuration (`test_config.json`)
To prevent credential leaks, each integration test should support loading configuration parameters from a local file named `test_config.json` situated directly in the plugin's crate directory.
* **Ignored by Default:** `test_config.json` is universally ignored under the `plugins/.gitignore` rules to guarantee developers do not accidentally commit sensitive private keys or API tokens.
* **Fallback Protocol:** If `test_config.json` is absent or empty, the test should fallback to reading the workspace-level configuration located under `.profiles/assistant/` (or print a skip message and exit early if neither exists).

**Layout:**
```
plugins/tts-google/
├── Cargo.toml
├── test_config.json                 # Ignored by Git, contains credentials
├── src/
│   └── lib.rs
└── tests/
    └── tts_google_integration_test.rs
```

**Template (`tests/tts_google_integration_test.rs`):**
```rust
use std::fs;
use tts_google::TtsGooglePlugin;
use synapto_interface::plugin::{Plugin, TTSPlugin};
use synapto_interface::cognitive::CognitiveOutputSpeech;

#[tokio::test]
#[ignore] // MUST ignore integration tests by default to keep standard offline compiles clean
async fn test_google_tts_live_synthesis() {
    // 1. Locate and load configuration (prefer local test_config.json, fallback to global profile)
    let local_config_path = "test_config.json";
    let fallback_config_path = "../../.profiles/assistant/test/config.json";

    let config_content = if fs::metadata(local_config_path).is_ok() {
        let content = fs::read_to_string(local_config_path).unwrap();
        if content.trim().is_empty() {
            if fs::metadata(fallback_config_path).is_ok() {
                let fallback_content = fs::read_to_string(fallback_config_path).unwrap();
                let json: serde_json::Value = serde_json::from_str(&fallback_content).unwrap();
                json.get("plugins")
                    .and_then(|p| p.get("tts_google"))
                    .and_then(|c| c.get("TtsGooglePlugin"))
                    .map(|v| serde_json::to_string(v).unwrap())
                    .unwrap_or_else(|| "{}".to_string())
            } else {
                println!("Skipping test: test_config.json is empty and fallback config not found");
                return;
            }
        } else {
            content
        }
    } else if fs::metadata(fallback_config_path).is_ok() {
        fs::read_to_string(fallback_config_path).unwrap()
    } else {
        println!("Skipping test: neither local test_config.json nor fallback config was found");
        return;
    };

    // 2. Instantiate plugin
    let config = serde_json::from_str(&config_content).unwrap();
    let plugin = TtsGooglePlugin::new(config).expect("Failed to create plugin");

    // 3. Create mock communication channels
    let (speech_tx, speech_rx) = synapto_interface::sync::broadcast::channel(10);
    let (audio_tx, mut audio_rx) = synapto_interface::sync::mpsc::channel(10);

    // 4. Spawn the plugin loop inside tokio sandbox
    tokio::spawn(async move {
        plugin.start(speech_rx, audio_tx).await.unwrap();
    });

    // 5. Inject payload containing XML unsafe characters
    speech_tx.send(CognitiveOutputSpeech {
        target_channel: synapto_interface::plugin::MessageChannel {
            context: serde_json::Value::Null,
        },
        text: "Ahoj & čau, toto je integrační test.".to_string(),
    }).unwrap();

    // 6. Assert audio response is received under strict timeout limits
    let response = tokio::time::timeout(std::time::Duration::from_secs(5), audio_rx.recv()).await;
    match response {
        Ok(Some(audio)) => {
            assert!(!audio.0.is_empty(), "Returned audio buffer must not be empty");
        }
        Ok(None) => panic!("Channel closed prematurely"),
        Err(_) => panic!("Test timed out waiting for audio bytes"),
    }
}
```
