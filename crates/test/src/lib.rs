#![feature(iter_array_chunks)]

pub mod ephemeral_datadir;
pub mod local_storage;
#[path = "plugins/mod.rs"]
pub mod plugins;
pub mod test_datadir;

pub use plugins::chat::MockChatPlugin;
pub use plugins::diarization::MockDiarizationPlugin;
pub use plugins::documents::MockDocumentsPlugin;
pub use plugins::stt::MockSttPlugin;
pub use plugins::tools::MockSlowReadPlugin;
pub use plugins::tts::MockTtsPlugin;

use std::sync::Arc;
use synapto_interface::document::{
    AddDocumentRequest, DocumentIngestionPolicy, DocumentRegistrationRequest,
};
use synapto_interface::peer_input::MessageText;
use synapto_interface::peer_input_audio::AudioInputPlugin;
use synapto_interface::peer_input_audio::{
    PEER_INPUT_AUDIO_CHUNK_SIZE as AUDIO_INPUT_CHUNK_SIZE,
    PEER_INPUT_AUDIO_SAMPLE_RATE as AUDIO_INPUT_SAMPLE_RATE, PeerInputAudio,
};
use synapto_interface::peer_input_text::PeerInputText;
use synapto_interface::peer_input_text::SenderId;
use synapto_interface::plugin::MessageChannel;
use synapto_interface::speech_to_text::{SpeechDetected, SpeechTranscript};
use synapto_interface::sync::mpsc;

pub static ACTIVE_COORDINATOR: std::sync::Mutex<Option<Arc<ScenarioCoordinator>>> =
    std::sync::Mutex::new(None);

pub struct MockAudioInputPlugin;

#[async_trait::async_trait]
impl synapto_interface::plugin::Plugin for MockAudioInputPlugin {
    async fn create(
        _context: &synapto_interface::plugin::PluginInitContext<'_>,
    ) -> Result<Self, String> {
        Ok(Self)
    }

    fn register<R: synapto_interface::plugin::PluginRegistry + ?Sized>(
        self: std::sync::Arc<Self>,
        registry: &mut R,
    ) where
        Self: Sized,
    {
        registry.register_audio_input(self);
    }
}

#[async_trait::async_trait]
impl AudioInputPlugin for MockAudioInputPlugin {
    async fn start(&self, tx: mpsc::Sender<PeerInputAudio>) -> Result<(), String> {
        let coordinator = ACTIVE_COORDINATOR.lock().unwrap().clone().ok_or_else(|| {
            "ScenarioCoordinator is not initialized in ACTIVE_COORDINATOR Mutex".to_string()
        })?;

        coordinator.peer_input_audio_tx.set(tx).ok();
        Ok(())
    }
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct ScenarioManifest {
    pub name: Option<String>,
    pub timeout_secs: Option<u64>,
    pub steps: Vec<ScenarioStep>,
}

#[derive(serde::Deserialize, Debug, Clone)]
#[serde(tag = "action")]
pub enum ScenarioStep {
    #[serde(rename = "user_writes")]
    UserWrites {
        #[serde(default)]
        text: String,
        #[serde(default)]
        attachments: Vec<std::path::PathBuf>,
    },
    #[serde(rename = "user_says")]
    UserSays { transcript: String },
    #[serde(rename = "await_response")]
    AwaitResponse {
        #[serde(default)]
        assert_contains: Option<String>,
        #[serde(default)]
        assert_all: Vec<String>,
        #[serde(default)]
        assert_any: Vec<String>,
        #[serde(default)]
        case_sensitive: bool,
        timeout_secs: Option<u64>,
    },
    #[serde(rename = "play_audio")]
    PlayAudio { audio_stream: std::path::PathBuf },
    #[serde(rename = "wait")]
    Wait { millis: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepStatus {
    Pending,
    Executing,
    Completed,
    Failed(String),
}

pub struct ScenarioCoordinator {
    pub steps: Vec<ScenarioStep>,
    pub current_step_idx: tokio::sync::Mutex<usize>,
    pub step_status: tokio::sync::Mutex<StepStatus>,
    pub response_buffer: tokio::sync::Mutex<String>,
    pub state_change_notify: Arc<tokio::sync::Notify>,
    pub scenario_path: std::path::PathBuf,
    // Captured channels from plugins
    pub peer_input_text_tx: std::sync::OnceLock<mpsc::Sender<PeerInputText>>,
    pub transcript_tx: std::sync::OnceLock<mpsc::Sender<SpeechTranscript>>,
    pub speech_detected: std::sync::OnceLock<SpeechDetected>,
    pub add_document_tx: std::sync::OnceLock<mpsc::Sender<AddDocumentRequest>>,
    pub peer_input_audio_tx: std::sync::OnceLock<mpsc::Sender<PeerInputAudio>>,
}

pub async fn run_scenario<F, Fut>(
    scenario_manifest_path: impl AsRef<std::path::Path>,
    test_bundle: F,
) where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    // Load scenario manifest
    let yaml_str = match tokio::fs::read_to_string(&scenario_manifest_path).await {
        Ok(s) => s,
        Err(e) => {
            panic!(
                "Failed to read scenario file {:?}: {}",
                scenario_manifest_path.as_ref(),
                e
            );
        }
    };

    let manifest: ScenarioManifest = match serde_yaml::from_str(&yaml_str) {
        Ok(m) => m,
        Err(e) => {
            panic!("Failed to parse scenario manifest: {}", e);
        }
    };

    // Initialize Scenario Coordinator
    let coordinator = Arc::new(ScenarioCoordinator::new(
        manifest.steps,
        scenario_manifest_path.as_ref().to_path_buf(),
    ));

    *ACTIVE_COORDINATOR.lock().unwrap() = Some(coordinator.clone());

    let (tx, rx) = tokio::sync::oneshot::channel();

    // Spawn coordination driver thread
    tokio::spawn(async move {
        let result = coordinator.drive().await;
        synapto_shutdown::trigger_graceful(); // Gracefully shutdown Synapto::run
        let _ = tx.send(result);
    });

    // Boot the bundle which will block until shutdown
    test_bundle().await;

    // Check the scenario result
    let result = rx
        .await
        .unwrap_or_else(|_| Err("Coordinator task panicked or dropped before completing".into()));

    // Clear global state so the next test can run safely
    *ACTIVE_COORDINATOR.lock().unwrap() = None;

    if let Err(e) = result {
        panic!("Scenario Failed: {}", e);
    }
}

impl ScenarioCoordinator {
    pub fn new(steps: Vec<ScenarioStep>, scenario_path: std::path::PathBuf) -> Self {
        Self {
            steps,
            current_step_idx: tokio::sync::Mutex::new(0),
            step_status: tokio::sync::Mutex::new(StepStatus::Pending),
            response_buffer: tokio::sync::Mutex::new(String::new()),
            state_change_notify: Arc::new(tokio::sync::Notify::new()),
            scenario_path,
            peer_input_text_tx: std::sync::OnceLock::new(),
            transcript_tx: std::sync::OnceLock::new(),
            speech_detected: std::sync::OnceLock::new(),
            add_document_tx: std::sync::OnceLock::new(),
            peer_input_audio_tx: std::sync::OnceLock::new(),
        }
    }

    pub async fn check_text_response(&self, text: &str) {
        let idx: usize = *self.current_step_idx.lock().await;
        if idx >= self.steps.len() {
            return;
        }
        let step = &self.steps[idx];
        if let ScenarioStep::AwaitResponse {
            assert_contains,
            assert_all,
            assert_any,
            case_sensitive,
            ..
        } = step
        {
            let mut buffer = self.response_buffer.lock().await;
            buffer.push_str(text);

            let buffer_content = if *case_sensitive {
                buffer.to_string()
            } else {
                buffer.to_lowercase()
            };

            let mut all_met = true;

            // Check assert_contains (legacy / convenience)
            if let Some(needle) = assert_contains {
                let needle = if *case_sensitive {
                    needle.clone()
                } else {
                    needle.to_lowercase()
                };
                if !buffer_content.contains(&needle) {
                    all_met = false;
                }
            }

            // Check assert_all
            if all_met {
                for needle in assert_all {
                    let needle = if *case_sensitive {
                        needle.clone()
                    } else {
                        needle.to_lowercase()
                    };
                    if !buffer_content.contains(&needle) {
                        all_met = false;
                        break;
                    }
                }
            }

            // Check assert_any
            let any_met = if assert_any.is_empty() {
                true
            } else {
                let mut found_any = false;
                for needle in assert_any {
                    let needle = if *case_sensitive {
                        needle.clone()
                    } else {
                        needle.to_lowercase()
                    };
                    if buffer_content.contains(&needle) {
                        found_any = true;
                        break;
                    }
                }
                found_any
            };

            if all_met && any_met {
                tracing::info!("All assertions matched in buffered response");
                let mut status = self.step_status.lock().await;
                *status = StepStatus::Completed;
                self.state_change_notify.notify_waiters();
            }
        }
    }

    pub async fn drive(&self) -> Result<(), String> {
        // Determine which mock channels are actually required by the scenario steps
        let mut requires_text = false;
        let mut requires_audio = false;
        for step in &self.steps {
            match step {
                ScenarioStep::UserWrites { .. } => requires_text = true,
                ScenarioStep::UserSays { .. } => requires_audio = true,
                _ => {}
            }
        }

        // Wait for required mock plugins to be fully initialized
        let start_time = tokio::time::Instant::now();
        let use_stt_test_plugin = self
            .scenario_path
            .parent()
            .unwrap_or(std::path::Path::new(""))
            .join("stt.ndjson")
            .exists();
        loop {
            let text_ok = !requires_text || self.peer_input_text_tx.get().is_some();
            let audio_ok = !requires_audio
                || if use_stt_test_plugin {
                    self.peer_input_audio_tx.get().is_some()
                } else {
                    self.transcript_tx.get().is_some() && self.speech_detected.get().is_some()
                };

            if text_ok && audio_ok {
                break;
            }
            if start_time.elapsed() > tokio::time::Duration::from_secs(10) {
                return Err("Timeout waiting for mock plugins to initialize".to_string());
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        let global_timeout = 60; // default timeout in seconds

        for idx in 0..self.steps.len() {
            {
                let mut current_idx = self.current_step_idx.lock().await;
                *current_idx = idx;
                let mut status = self.step_status.lock().await;
                *status = StepStatus::Executing;
            }

            let step = &self.steps[idx];
            tracing::info!("Executing Scenario Step {}: {:?}", idx + 1, step);

            match step {
                ScenarioStep::UserWrites { text, attachments } => {
                    let mut resolved_doc_ids = Vec::new();

                    if !attachments.is_empty() {
                        let add_doc_tx = self
                            .add_document_tx
                            .get()
                            .ok_or_else(|| "add_document_tx is not available".to_string())?;

                        for attachment_path in attachments {
                            let base_dir = self
                                .scenario_path
                                .parent()
                                .unwrap_or(std::path::Path::new(""));
                            let full_path = base_dir.join(attachment_path);

                            let data = tokio::fs::read(&full_path).await.map_err(|e| {
                                format!("Failed to read attachment {:?}: {}", full_path, e)
                            })?;

                            let original_filename = full_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("unknown")
                                .to_string();

                            let mime_type = match full_path.extension().and_then(|e| e.to_str()) {
                                Some("pdf") => "application/pdf",
                                Some("txt") => "text/plain",
                                _ => "application/octet-stream",
                            }
                            .to_string();

                            let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();

                            add_doc_tx
                                .send(AddDocumentRequest {
                                    request: DocumentRegistrationRequest {
                                        original_filename,
                                        mime_type,
                                        data,
                                        policy: DocumentIngestionPolicy::StoreAndParse,
                                    },
                                    reply_tx: resp_tx,
                                })
                                .await
                                .map_err(|e| format!("Failed to send AddDocumentRequest: {}", e))?;

                            let doc_id = resp_rx.await.map_err(|e| {
                                format!("Failed to receive DocumentId from oneshot channel: {}", e)
                            })?;
                            resolved_doc_ids.push(doc_id);
                        }
                    }

                    let peer_input_text_tx = self.peer_input_text_tx.get().unwrap();
                    peer_input_text_tx
                        .send(PeerInputText {
                            channel: MessageChannel {
                                context: serde_json::json!({}),
                            },
                            sender_id: SenderId::from("test_user".to_string()),
                            text: MessageText(text.clone()),
                            attached_documents: resolved_doc_ids,
                            explicitly_addressed: true,
                        })
                        .await
                        .map_err(|e| format!("Failed to send PeerInputText: {}", e))?;
                }
                ScenarioStep::UserSays { transcript } => {
                    let transcript_tx = self.transcript_tx.get().unwrap();
                    let speech_detected = self.speech_detected.get().unwrap();

                    let words_list: Vec<&str> = transcript.split_whitespace().collect();
                    let word_count = words_list.len();

                    // Heuristic: ~150 words per minute -> 400ms per word
                    let word_duration = tokio::time::Duration::from_millis(400);
                    let total_duration = word_duration * word_count as u32;

                    // 1. Initial "speech detected" - trigger speech detected mechanism
                    tracing::info!("Simulating speech detection start");
                    speech_detected.notify();

                    // Wait for a realistic "speaking" duration
                    tokio::time::sleep(total_duration).await;

                    let mut words = Vec::new();
                    for w in words_list {
                        words.push(synapto_interface::speech_to_text::Word {
                            start_index: Some(0),
                            end_index: Some(100),
                            word: w.to_string(),
                            speaker_hint: None,
                        });
                    }

                    tracing::info!("Simulating speech end (final transcript): '{}'", transcript);
                    // Construct and send a final mock SpeechTranscript
                    transcript_tx
                        .send(SpeechTranscript {
                            start_index: 0,
                            end_index: 100, // mock indexes
                            transcript: transcript.clone(),
                            words,
                        })
                        .await
                        .map_err(|e| format!("Failed to send final SpeechTranscript: {}", e))?;
                }
                ScenarioStep::AwaitResponse {
                    assert_contains,
                    assert_all,
                    assert_any,
                    case_sensitive: _,
                    timeout_secs,
                } => {
                    {
                        let mut buffer = self.response_buffer.lock().await;
                        buffer.clear();
                    }

                    let timeout = timeout_secs.unwrap_or(global_timeout);
                    let step_start = tokio::time::Instant::now();

                    let mut matched = false;
                    while step_start.elapsed() < tokio::time::Duration::from_secs(timeout) {
                        let status = self.step_status.lock().await;
                        if *status == StepStatus::Completed {
                            matched = true;
                            break;
                        }
                        drop(status);

                        better_tokio_select::tokio_select!(match .. {
                            .. if let _ = self.state_change_notify.notified() => {}
                            .. if let _ =
                                tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {}
                        });
                    }

                    if !matched {
                        let needles = [
                            assert_contains.clone().into_iter().collect::<Vec<_>>(),
                            assert_all.clone(),
                            assert_any.clone(),
                        ]
                        .concat();
                        return Err(format!(
                            "Timeout waiting for response matching assertions: {:?}",
                            needles
                        ));
                    }
                }
                ScenarioStep::PlayAudio { audio_stream } => {
                    let peer_input_audio_tx = self
                        .peer_input_audio_tx
                        .get()
                        .ok_or_else(|| "peer_input_audio_tx is not available. Ensure MockAudioInputPlugin is registered.".to_string())?;

                    let base_dir = self
                        .scenario_path
                        .parent()
                        .unwrap_or(std::path::Path::new(""));
                    let full_path = base_dir.join(audio_stream);

                    tracing::info!("Playing FLAC audio stream from {:?}", full_path);

                    let mut reader = claxon::FlacReader::open(&full_path)
                        .map_err(|e| format!("Failed to open FLAC file {:?}: {}", full_path, e))?;

                    let info = reader.streaminfo();
                    if info.channels != 1 {
                        return Err(format!(
                            "Only mono FLAC files are supported (found {} channels)",
                            info.channels
                        ));
                    }
                    if info.sample_rate != AUDIO_INPUT_SAMPLE_RATE as u32 {
                        return Err(format!(
                            "Only {}Hz FLAC files are supported (found {}Hz)",
                            AUDIO_INPUT_SAMPLE_RATE, info.sample_rate
                        ));
                    }
                    if info.bits_per_sample != 16 {
                        return Err(format!(
                            "Only 16-bit FLAC files are supported (found {}-bit)",
                            info.bits_per_sample
                        ));
                    }

                    let samples_iter = reader.samples().flatten().map(|s| s as i16);

                    for chunk in samples_iter.array_chunks::<AUDIO_INPUT_CHUNK_SIZE>() {
                        peer_input_audio_tx
                            .send(PeerInputAudio::new(chunk))
                            .await
                            .map_err(|e| format!("Failed to send PeerInputAudio: {}", e))?;

                        tokio::time::sleep(
                            synapto_interface::peer_input_audio::PEER_INPUT_AUDIO_CHUNK_DURATION,
                        )
                        .await;
                    }

                    tracing::info!("Finished playing FLAC audio stream.");
                }
                ScenarioStep::Wait { millis } => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(*millis)).await;
                }
            }
        }
        Ok(())
    }
}
