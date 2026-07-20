#![allow(clippy::disallowed_methods)]

use synapto_interface::cognitive::CognitiveOutputSpeech;
use synapto_interface::plugin::{MessageChannel, Plugin, PluginInitContext};
use synapto_interface::speech_to_text::TTSPlugin;
use std::fs;
use synapto_plugin_tts_google::TtsGooglePlugin;

#[tokio::test]
#[ignore]
async fn test_google_tts_live_synthesis_xml_escaping() {
    // 1. Locate and check live configuration (prefer local test_config.json, fallback to global profile)
    let local_config_path = "test_config.json";
    let fallback_config_path = "../../.profiles/assistant/test/config.json";

    let config_content = if fs::metadata(local_config_path).is_ok() {
        let content = fs::read_to_string(local_config_path).unwrap();
        if content.trim().is_empty() {
            // Fallback if the user-created local file is empty
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

    // 2. Instantiate Google TTS plugin
    struct DummyLlmExecutor;

    #[async_trait::async_trait]
    impl synapto_interface::llm::LlmExecutor for DummyLlmExecutor {
        async fn execute_raw(
            &self,
            _model: &str,
            _system_prompt: &str,
            _prompt: &str,
            _options: synapto_interface::llm::RawLlmOptions,
        ) -> Result<synapto_interface::llm::genai::chat::ChatResponse, String> {
            panic!("Not implemented");
        }
    }

    struct DummyStorageConfigResolver;

    impl synapto_interface::storage::StorageConfigResolver for DummyStorageConfigResolver {
        fn resolve_config(
            &self,
            _crate_name: &str,
            _storage_type_name: &str,
        ) -> Option<serde_json::Value> {
            Some(serde_json::Value::Null)
        }
    }

    let config = serde_json::from_str(&config_content).unwrap();
    let plugin = TtsGooglePlugin::create(&PluginInitContext::new(
        std::sync::Arc::new(DummyLlmExecutor),
        &config,
        std::sync::Arc::new(synapto_interface::storage::StorageRegistry::default()),
        "tts_google",
        std::sync::Arc::new(DummyStorageConfigResolver),
    ))
    .await
    .expect("Failed to create Google TTS plugin");

    // 3. Setup mock channels
    let (speech_tx, speech_rx) = synapto_interface::sync::broadcast::channel(10);
    let (audio_tx, mut audio_rx) = synapto_interface::sync::mpsc::channel(10);

    // 4. Start the plugin loop inside tokio sandbox
    tokio::spawn(async move {
        plugin.start(speech_rx, audio_tx).await.unwrap();
    });

    // 5. Inject speech with XML unsafe characters to verify robust escaping
    speech_tx
        .send(CognitiveOutputSpeech {
            target_channel: MessageChannel {
                context: serde_json::Value::Null,
            },
            text: "Ahoj & čau, toto je robustní integrační test s unescaped XML znaky.".to_string(),
        })
        .unwrap();

    // 6. Assert audio response is successfully synthesized within 5 seconds
    let response = tokio::time::timeout(std::time::Duration::from_secs(5), audio_rx.recv()).await;
    match response {
        Ok(Some(audio)) => {
            assert!(
                !audio.0.is_empty(),
                "Returned audio buffer must not be empty"
            );
        }
        Ok(None) => panic!("Audio channel closed prematurely"),
        Err(_) => panic!("Test timed out waiting for audio output synthesis"),
    }
}
