use std::sync::Arc;
use synapto_interface::cognitive_output_audio::types::CognitiveOutputAudio;
use synapto_interface::cognitive_output_text::types::CognitiveOutputText;
use synapto_interface::peer_input_audio::types::PeerInputAudio;
use synapto_interface::peer_input_text::types::PeerInputText;
use synapto_interface::types::{CognitiveOutputSpeech, CognitiveStateUpdate, PeerInputSpeech};
use synapto_interface::{
    AudioInputPlugin, AudioOutputPlugin, AudioRecorderPlugin, CallPlugin, ChatPlugin,
    DiarizationPlugin, Plugin, STTPlugin, TTSPlugin, speech_to_text::types::SpeakerSegment,
};

use crate::{
    cognitive::{CognitiveDirectInterrupt, CognitiveDirectTrigger},
    interactions::Interaction,
};
use std::process::ExitCode;
use synapto_interface::sync::{broadcast, mpsc, watch};
use synapto_telemetry::tracing::Tracing;

pub mod cognitive;
pub mod config;

pub mod enveloped;
pub mod google_credentials;

pub mod interactions;

pub mod speech_to_text;

pub mod users;
pub mod utils;

use synapto_interface::speech_to_text::types::{InputVoiceAudio, SpeechDetected, SpeechTranscript};

type AudioInputSpawner = Box<dyn FnOnce(&mut Option<mpsc::Sender<PeerInputAudio>>) + Send>;
type AudioOutputSpawner = Box<dyn FnOnce(&mut Option<mpsc::Receiver<CognitiveOutputAudio>>) + Send>;
type SttSpawner = Box<
    dyn FnOnce(
            &mut Option<mpsc::Receiver<InputVoiceAudio>>,
            mpsc::Sender<SpeechTranscript>,
            SpeechDetected,
        ) + Send,
>;
type TtsSpawner = Box<
    dyn FnOnce(
            &mut Option<broadcast::Receiver<CognitiveOutputSpeech>>,
            &mut Option<mpsc::Sender<CognitiveOutputAudio>>,
        ) + Send,
>;
type ChatSpawner = Box<
    dyn FnOnce(
            mpsc::Sender<PeerInputText>,
            mpsc::Receiver<CognitiveOutputText>,
            broadcast::Receiver<CognitiveStateUpdate>,
            Option<mpsc::Sender<synapto_interface::types::AddDocumentRequest>>,
        ) + Send,
>;

type DocumentsSpawner =
    Box<dyn FnOnce(mpsc::Receiver<synapto_interface::types::AddDocumentRequest>) + Send>;

type DiarizationSpawner =
    Box<dyn FnOnce(broadcast::Receiver<InputVoiceAudio>, mpsc::Sender<SpeakerSegment>) + Send>;

type CallSpawner = Box<
    dyn FnOnce(
            broadcast::Receiver<PeerInputText>,
            mpsc::Sender<CognitiveOutputText>,
            watch::Receiver<std::time::Instant>,
            watch::Receiver<bool>,
            watch::Sender<bool>,
        ) + Send,
>;

type RecorderSpawner =
    Box<dyn FnOnce(watch::Receiver<bool>, broadcast::Receiver<InputVoiceAudio>) + Send>;

type GuiSpawner = Box<
    dyn FnOnce(
            std::sync::Arc<synapto_interface::types::ContextRegistries>,
            std::sync::mpsc::Receiver<String>,
        ) + Send,
>;

type CameraSpawner =
    Box<dyn FnOnce(&mut Option<watch::Sender<synapto_interface::types::CameraInputFrame>>) + Send>;

struct CoreStorageConfigResolver<C: crate::config::ConfigProvider> {
    provider: std::sync::Arc<C>,
}

impl<C: crate::config::ConfigProvider> synapto_interface::storage::StorageConfigResolver
    for CoreStorageConfigResolver<C>
{
    fn resolve_config(
        &self,
        crate_name: &str,
        storage_type_name: &str,
    ) -> Option<serde_json::Value> {
        Some(
            self.provider
                .get_storage_config(crate_name, storage_type_name),
        )
    }
}

pub struct Synapto<
    C: crate::config::ConfigProvider,
    PR: crate::cognitive::prompt_provider::CognitivePromptProvider,
> {
    pub config: config::Config,
    config_provider: Arc<C>,
    _prompt_provider: std::marker::PhantomData<PR>,
    tracing: Tracing,
    audio_input_spawners: Vec<AudioInputSpawner>,
    audio_output_spawners: Vec<AudioOutputSpawner>,
    stt_spawners: Vec<SttSpawner>,
    tts_spawners: Vec<TtsSpawner>,
    chat_spawner: Option<ChatSpawner>,
    documents_spawner: Option<DocumentsSpawner>,
    diarization_spawner: Option<DiarizationSpawner>,
    diarization_heuristic:
        Option<synapto_interface::speech_to_text::types::SpeakerHeuristicCallback>,
    call_spawner: Option<CallSpawner>,
    audio_recorder_spawners: Vec<RecorderSpawner>,
    plugins_names: Vec<String>,
    plugins: std::collections::HashMap<std::any::TypeId, Arc<dyn std::any::Any + Send + Sync>>,
    pub registries: Arc<synapto_interface::types::ContextRegistries>,
    pub tools: Arc<synapto_interface::types::ToolRegistryBuilder>,
    pub commands: Arc<synapto_interface::types::CommandRegistryBuilder>,
    pub storage: Arc<synapto_interface::storage::StorageRegistry>,
    #[allow(clippy::type_complexity)]
    interaction_observer_spawners: Vec<(
        String,
        Box<
            dyn FnOnce(mpsc::Receiver<synapto_interface::types::ObservedInteraction>) + Send + Sync,
        >,
    )>,
    #[allow(clippy::type_complexity)]
    rollout_controller_spawners: Vec<(
        String,
        Box<dyn FnOnce(watch::Sender<synapto_interface::types::Timestamp>) + Send + Sync>,
    )>,
    #[allow(clippy::type_complexity)]
    retrospective_consolidation_spawners: Vec<
        Box<
            dyn FnOnce(
                    watch::Receiver<synapto_interface::types::NotClearInteractionMemory>,
                    mpsc::Sender<synapto_interface::types::Timestamp>,
                ) + Send
                + Sync,
        >,
    >,
    pub llm_executor: std::sync::Arc<dyn synapto_interface::llm::LlmExecutor>,
    gui_spawner: Option<GuiSpawner>,
    camera_spawner: Option<CameraSpawner>,
    error_rx: Option<std::sync::mpsc::Receiver<String>>,
    pub current_context_tx: watch::Sender<serde_json::Value>,
    pub current_context_rx: watch::Receiver<serde_json::Value>,
}

impl<
    C: crate::config::ConfigProvider,
    PR: crate::cognitive::prompt_provider::CognitivePromptProvider,
> Synapto<C, PR>
{
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self::with_config_provider(C::init())
    }

    pub fn with_config_provider(config_provider: C) -> Self {
        let config_provider = std::sync::Arc::new(config_provider);
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();

        let (gui_layer, error_rx) = synapto_telemetry::tracing::GuiErrorLayer::new();

        let tracing = Tracing::setup(gui_layer);

        {
            let full_path = core::any::type_name::<C>();
            tracing::info!("{} config provider intialized", full_path);
        }

        let config = config_provider.get_core_config();
        let executor_config = synapto_llm::LLMClientConfig {
            google_vertex_ai_location: config.google_vertex_ai_location.clone(),
            google_project_id: config.google_project_id.clone(),
            google_service_account_credentials: Some(String::from(
                config.google_service_account_credentials.clone(),
            )),
            gemini_api_key: config.gemini_api_key.clone(),
        };
        let llm_executor =
            std::sync::Arc::new(synapto_llm::ConcreteLlmExecutor::new(executor_config));

        let (current_context_tx, current_context_rx) = watch::channel(serde_json::Value::Null);

        let registries = Arc::new(synapto_interface::types::ContextRegistries::default());
        let tools = Arc::new(synapto_interface::types::ToolRegistryBuilder::default());
        let commands = Arc::new(synapto_interface::types::CommandRegistryBuilder::default());
        let storage = Arc::new(synapto_interface::storage::StorageRegistry::default());

        Self {
            config_provider,
            _prompt_provider: std::marker::PhantomData,
            config,
            tracing,
            audio_input_spawners: Vec::new(),
            audio_output_spawners: Vec::new(),
            stt_spawners: Vec::new(),
            tts_spawners: Vec::new(),
            chat_spawner: None,
            documents_spawner: None,
            diarization_spawner: None,
            diarization_heuristic: None,
            call_spawner: None,
            audio_recorder_spawners: Vec::new(),
            plugins_names: Vec::new(),
            plugins: std::collections::HashMap::new(),
            registries,
            tools,
            commands,
            storage,
            interaction_observer_spawners: Vec::new(),
            rollout_controller_spawners: Vec::new(),
            retrospective_consolidation_spawners: Vec::new(),
            gui_spawner: None,
            camera_spawner: None,
            error_rx: Some(error_rx),
            current_context_tx,
            current_context_rx,
            llm_executor,
        }
    }

    pub fn load_plugin_config<P: Plugin>(&self) -> serde_json::Value {
        let full_path = std::any::type_name::<P>();
        let crate_name = full_path
            .split("::")
            .next()
            .unwrap_or("")
            .to_string()
            .replace('-', "_");
        let base_path = full_path.split('<').next().unwrap_or(full_path);
        let plugin_type_name = base_path.split("::").last().unwrap_or("").to_string();

        self.config_provider
            .get_plugin_config_value(&crate_name, &plugin_type_name)
    }

    fn get_or_init_plugin<P: Plugin>(&mut self) -> Arc<P> {
        let type_id = std::any::TypeId::of::<P>();
        if let Some(plugin) = self.plugins.get(&type_id) {
            plugin.clone().downcast::<P>().unwrap_or_else(|e| {
                panic!(
                    "Downcast failed to target type: {}. Actual dynamic type of value was: {:?}",
                    std::any::type_name::<P>(),
                    e
                )
            })
        } else {
            let full_path = core::any::type_name::<P>();
            let base_path = full_path.split('<').next().unwrap_or(full_path);
            let plugin_identity = base_path.to_string();
            let plugin_config = self.load_plugin_config_internal::<P>();

            // Generate the safe, unique database namespace based on the Rust type
            let safe_namespace = base_path.replace("::", "_").replace(" ", "");

            let plugin_context = synapto_interface::types::PluginContext::new(
                self.config.data_dir.clone(),
                self.llm_executor.clone(),
                plugin_config,
                self.storage.clone(),
                safe_namespace,
                std::sync::Arc::new(CoreStorageConfigResolver {
                    provider: self.config_provider.clone(),
                }),
                self.current_context_rx.clone(),
            );

            // Safely bridge the async initialization back into the synchronous builder
            // This is entirely safe during the application boot phase.
            let future = P::create(plugin_context);
            let plugin_result =
                tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(future));

            let plugin = Arc::new(plugin_result.unwrap_or_else(|e| {
                panic!("Failed to initialize plugin '{}': {}", plugin_identity, e)
            }));

            self.plugins.insert(type_id, plugin.clone());
            plugin
        }
    }

    fn load_plugin_config_internal<P: Plugin>(&self) -> serde_json::Value {
        let full_path = core::any::type_name::<P>();
        let crate_name = full_path
            .split("::")
            .next()
            .unwrap_or("")
            .to_string()
            .replace('-', "_");
        let base_path = full_path.split('<').next().unwrap_or(full_path);
        let plugin_type_name = base_path.split("::").last().unwrap_or("").to_string();

        self.config_provider
            .get_plugin_config_value(&crate_name, &plugin_type_name)
    }

    pub fn register_plugin<P: Plugin>(mut self) -> Self {
        let full_path = core::any::type_name::<P>();
        let base_path = full_path.split('<').next().unwrap_or(full_path);
        let plugin_identity = base_path.to_string();
        self.tracing.add_plugin_to_log(&plugin_identity);
        self.plugins_names.push(plugin_identity.clone());

        let plugin = self.get_or_init_plugin::<P>();
        tracing::info!("Plugin {} registered.", plugin_identity);

        plugin.register(&mut self);
        self
    }

    #[allow(unused_mut)]
    pub async fn run(mut self) -> ExitCode {
        tracing::debug!("Configuration {:?}", &self.config);

        let mut shutdown_rx = synapto_shutdown::init();

        let (last_voice_time_tx, last_voice_time_rx) = watch::channel(std::time::Instant::now());

        let trigger_cognitive_direct = CognitiveDirectTrigger::default();

        let interrupt_cognitive_direct = CognitiveDirectInterrupt::default();

        let (peer_input_audio_tx, peer_input_audio_rx) = mpsc::channel::<PeerInputAudio>(20);

        // TODO consider move to cognitive
        let (cognitive_speech_tx, _) = broadcast::channel::<CognitiveOutputSpeech>(10);

        let (cognitive_output_text_tx, cognitive_output_text_rx) =
            mpsc::channel::<CognitiveOutputText>(10);
        let (peer_input_text_tx, mut peer_input_text_rx) = mpsc::channel::<PeerInputText>(10);
        let (peer_input_text_broadcast_tx, _) = broadcast::channel::<PeerInputText>(10);

        let peer_input_text_broadcast_tx_clone = peer_input_text_broadcast_tx.clone();
        tokio::spawn(async move {
            while let Some(msg) = peer_input_text_rx.recv().await {
                peer_input_text_broadcast_tx_clone
                    .send(msg)
                    .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                    .ok();
            }
        });

        let (cognitive_output_audio_tx, _cognitive_output_audio_rx) =
            broadcast::channel::<CognitiveOutputAudio>(10);

        let (cognitive_output_audio_tx_plugin, cognitive_output_audio_rx_plugin) =
            mpsc::channel::<CognitiveOutputAudio>(100);
        let (cognitive_output_audio_tx_speaker, cognitive_output_audio_rx_speaker) =
            mpsc::channel::<CognitiveOutputAudio>(100);

        let mut cognitive_output_audio_rx_broadcast = cognitive_output_audio_tx.subscribe();
        tokio::spawn(async move {
            while let Ok(msg) = cognitive_output_audio_rx_broadcast.recv().await {
                cognitive_output_audio_tx_speaker
                    .send(msg)
                    .await
                    .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                    .ok();
            }
        });

        let (peer_input_speech_tx, peer_input_speech_rx) = mpsc::channel::<PeerInputSpeech>(100);

        let (cognitive_state_tx, _cognitive_state_rx) =
            broadcast::channel::<CognitiveStateUpdate>(10);

        let (new_interaction_tx, new_interaction_rx) = mpsc::channel::<Interaction>(10);

        let (add_document_tx, add_document_rx) =
            mpsc::channel::<synapto_interface::types::AddDocumentRequest>(10);

        let (mut video_tx_opt, video_rx_opt) = if self.camera_spawner.is_some() {
            let (video_tx, video_rx) =
                watch::channel(synapto_interface::types::CameraInputFrame { data: Vec::new() });
            (Some(video_tx), Some(video_rx))
        } else {
            (None, None)
        };

        let (ai_speaking_rx, ai_speaking_semaphore) = cognitive::speaking_coordinator::start(
            interrupt_cognitive_direct.clone(),
            cognitive_speech_tx.clone(),
            cognitive_output_audio_rx_plugin,
            cognitive_output_audio_tx.clone(),
        )
        .await;

        let (call_active_tx, call_active_rx) = watch::channel(false);

        if let Some(spawner) = self.call_spawner {
            spawner(
                peer_input_text_broadcast_tx.subscribe(),
                cognitive_output_text_tx.clone(),
                last_voice_time_rx.clone(),
                ai_speaking_rx.clone(),
                call_active_tx,
            );
        }

        let (speaker_segment_tx, speaker_segment_rx_opt) = if self.diarization_spawner.is_some() {
            let (tx, rx) = mpsc::channel::<SpeakerSegment>(100);
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };

        let (speech_transcript_tx, mut core_voice_audio_rx, input_voice_audio_tx) =
            speech_to_text::start(
                peer_input_audio_rx,
                peer_input_speech_tx.clone(),
                speaker_segment_rx_opt,
                self.diarization_heuristic.clone(),
                trigger_cognitive_direct.clone(),
                last_voice_time_tx,
            )
            .await;

        let speech_detected = SpeechDetected::new(interrupt_cognitive_direct.inner().clone());

        if let Some(spawner) = self.camera_spawner {
            spawner(&mut video_tx_opt);
        }

        if let Some(mut video_rx) = video_rx_opt.clone() {
            let trigger = trigger_cognitive_direct.clone();
            tokio::spawn(async move {
                while video_rx.changed().await.is_ok() {
                    trigger.trigger();
                }
            });
        }

        let (interaction_memory_tx, interaction_memory_rx) =
            watch::channel(interactions::InteractionMemory::default());

        let (not_clear_memory_tx, not_clear_memory_rx) =
            watch::channel(interactions::NotClearInteractionMemory::default());

        let (resolve_not_clear_tx, resolve_not_clear_rx) =
            mpsc::channel::<interactions::Timestamp>(100);

        for spawner in self.retrospective_consolidation_spawners {
            spawner(not_clear_memory_rx.clone(), resolve_not_clear_tx.clone());
        }

        let mut observers_tx = Vec::new();
        let mut interaction_rollout_receivers = Vec::new();

        for (name, spawner) in self.rollout_controller_spawners {
            let (rollout_tx, rollout_rx) =
                watch::channel(synapto_interface::types::Timestamp(i64::MAX));
            interaction_rollout_receivers.push((name, rollout_rx));
            spawner(rollout_tx);
        }

        for (_name, spawner) in self.interaction_observer_spawners {
            let (observer_tx, observer_rx) =
                mpsc::channel::<synapto_interface::types::ObservedInteraction>(100);
            observers_tx.push(observer_tx);

            spawner(observer_rx);
        }

        let (resolve_in_flight_tool_tx, resolve_in_flight_tool_rx) =
            mpsc::channel::<synapto_interface::types::ToolCallId>(100);

        interactions::start(
            self.config.clone(),
            new_interaction_rx,
            interaction_rollout_receivers,
            observers_tx,
            interaction_memory_tx.clone(),
            resolve_not_clear_rx,
            not_clear_memory_tx,
            resolve_in_flight_tool_rx,
        )
        .await;

        let registries = self.registries.clone();

        {
            let interaction_provider = std::sync::Arc::new(
                crate::interactions::recent::InteractionMemoryContextProvider::new(
                    interaction_memory_rx.clone(),
                ),
            );
            registries.current.register_erased(interaction_provider);
        }

        {
            let current_context_tx = self.current_context_tx;
            let mut current_update_rx = registries.current.subscribe();
            let registries = registries.clone();
            tokio::spawn(async move {
                while current_update_rx.changed().await.is_ok() {
                    let mut current_contexts = std::collections::BTreeMap::new();
                    let request = synapto_interface::types::ContextRequest::default();
                    let providers: Vec<_> = registries
                        .current
                        .providers
                        .read()
                        .unwrap_or_else(|e| panic!("Current providers lock poisoned: {:?}", e))
                        .clone();
                    for provider in providers {
                        if let Ok(value) = provider.erased_context(&request).await {
                            current_contexts.insert(provider.name().to_string(), value);
                        }
                    }
                    if let Ok(value) = serde_json::to_value(current_contexts) {
                        current_context_tx
                            .send(value)
                            .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                            .ok();
                    }
                }
            });
        }

        if let Some(gui_spawner) = self.gui_spawner {
            gui_spawner(registries.clone(), self.error_rx.expect("error_rx missing"));
        }

        let mut peer_input_audio_tx_opt = Some(peer_input_audio_tx);
        let mut cognitive_output_audio_tx_opt = Some(cognitive_output_audio_tx_plugin);
        let mut cognitive_output_audio_rx_opt = Some(cognitive_output_audio_rx_speaker);
        let mut core_voice_audio_rx_opt = Some(core_voice_audio_rx);

        for spawner in self.audio_input_spawners {
            spawner(&mut peer_input_audio_tx_opt);
        }

        for spawner in self.audio_output_spawners {
            spawner(&mut cognitive_output_audio_rx_opt);
        }

        for spawner in self.stt_spawners {
            spawner(
                &mut core_voice_audio_rx_opt,
                speech_transcript_tx.clone(),
                speech_detected.clone(),
            );
        }

        if let Some(spawner) = self.diarization_spawner {
            spawner(
                input_voice_audio_tx.subscribe(),
                speaker_segment_tx.expect("speaker_segment_tx should be initialized"),
            );
        }

        for spawner in self.audio_recorder_spawners {
            spawner(call_active_rx.clone(), input_voice_audio_tx.subscribe());
        }

        for spawner in self.tts_spawners {
            let mut ai_speech_rx_opt = Some(cognitive_speech_tx.subscribe());
            spawner(&mut ai_speech_rx_opt, &mut cognitive_output_audio_tx_opt);
        }

        let has_chat_plugin = self.chat_spawner.is_some();
        let has_documents_plugin = self.documents_spawner.is_some();

        if let Some(spawner) = self.documents_spawner {
            spawner(add_document_rx);
        }

        if let Some(spawner) = self.chat_spawner {
            let doc_tx = if has_documents_plugin {
                Some(add_document_tx.clone())
            } else {
                None
            };

            spawner(
                peer_input_text_tx.clone(),
                cognitive_output_text_rx,
                cognitive_state_tx.subscribe(),
                doc_tx,
            );
        }

        cognitive::start::<PR>(
            self.config,
            self.llm_executor.clone(),
            trigger_cognitive_direct,
            interrupt_cognitive_direct,
            ai_speaking_semaphore,
            peer_input_text_broadcast_tx.subscribe(),
            peer_input_speech_rx,
            interaction_memory_rx,
            cognitive_speech_tx,
            new_interaction_tx,
            video_rx_opt,
            registries.clone(),
            self.tools.clone(),
            self.commands.clone(),
            if has_chat_plugin {
                Some(cognitive_output_text_tx)
            } else {
                None
            },
            cognitive_state_tx,
            resolve_in_flight_tool_tx,
        )
        .await;

        tracing::info!("--- System is running. Waiting for events... ---\n");

        let exit_code = tokio::select! {
            res = shutdown_rx.recv() => {
                match res {
                    Some(synapto_shutdown::ShutdownResult(Ok(()))) => { tracing::info!("Standard shutdown requested."); ExitCode::SUCCESS}
                    Some(synapto_shutdown::ShutdownResult(Err(e))) => {
                        tracing::error!(error = %e, "Fatal error, shutting down.");
                        ExitCode::FAILURE
                    }
                    None => {
                        tracing::warn!("Shutdown channel closed unexpectedly.");
                        ExitCode::FAILURE
                    }
                }
            },
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Ctrl+C received, shutting down.");
                ExitCode::SUCCESS
            },
        };

        tracing::info!("\n--- Shutting down ---");
        synapto_shutdown::trigger_graceful();
        tracing::info!("\n--- Shutdown ---");

        exit_code
    }
}

static DYNAMIC_CAPABILITIES: std::sync::OnceLock<std::sync::Mutex<Vec<String>>> =
    std::sync::OnceLock::new();

pub fn register_dynamic_capability(cap: String) {
    DYNAMIC_CAPABILITIES
        .get_or_init(|| std::sync::Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|e| panic!("DYNAMIC_CAPABILITIES lock poisoned: {:?}", e))
        .push(cap);
}

pub fn get_dynamic_capabilities() -> Vec<String> {
    DYNAMIC_CAPABILITIES
        .get()
        .map(|m| {
            m.lock()
                .unwrap_or_else(|e| panic!("DYNAMIC_CAPABILITIES lock poisoned: {:?}", e))
                .clone()
        })
        .unwrap_or_default()
}

impl<
    C: crate::config::ConfigProvider,
    PR: crate::cognitive::prompt_provider::CognitivePromptProvider,
> synapto_interface::PluginRegistry for Synapto<C, PR>
{
    fn register_audio_input<P: AudioInputPlugin>(&mut self, plugin: Arc<P>) {
        self.audio_input_spawners.push(Box::new(move |tx_opt| {
            if let Some(tx) = tx_opt.take() {
                let p = plugin.clone();
                tokio::spawn(async move {
                    p.start(tx)
                        .await
                        .inspect_err(|e| tracing::error!("Audio input plugin failed: {:?}", e))
                        .ok();
                });
            }
        }));
        tracing::info!("  Audio input capability registered.");
    }

    fn register_audio_output<P: AudioOutputPlugin>(&mut self, plugin: Arc<P>) {
        self.audio_output_spawners.push(Box::new(move |rx_opt| {
            if let Some(rx) = rx_opt.take() {
                let p = plugin.clone();
                tokio::spawn(async move {
                    p.start(rx)
                        .await
                        .inspect_err(|e| tracing::error!("Audio output plugin failed: {:?}", e))
                        .ok();
                });
            }
        }));
        tracing::info!("  Audio output capability registered.");
    }

    fn register_stt<P: STTPlugin>(&mut self, plugin: Arc<P>) {
        self.stt_spawners.push(Box::new(
            move |audio_rx_opt, transcript_tx, speech_detected| {
                if let Some(audio_rx) = audio_rx_opt.take() {
                    let p = plugin.clone();
                    tokio::spawn(async move {
                        p.start(audio_rx, transcript_tx, speech_detected)
                            .await
                            .inspect_err(|e| tracing::error!("STT plugin failed: {:?}", e))
                            .ok();
                    });
                }
            },
        ));
        tracing::info!("  STT capability registered.");
    }

    fn register_tts<P: TTSPlugin>(&mut self, plugin: Arc<P>) {
        self.tts_spawners
            .push(Box::new(move |speech_rx_opt, audio_tx_opt| {
                if let (Some(speech_rx), Some(audio_tx)) =
                    (speech_rx_opt.take(), audio_tx_opt.take())
                {
                    let p = plugin.clone();
                    tokio::spawn(async move {
                        p.start(speech_rx, audio_tx)
                            .await
                            .inspect_err(|e| tracing::error!("TTS plugin failed: {:?}", e))
                            .ok();
                    });
                }
            }));
        tracing::info!("  TTS capability registered.");
    }

    fn register_chat<P: ChatPlugin>(&mut self, plugin: Arc<P>) {
        self.chat_spawner = Some(Box::new(
            move |peer_input_text_tx,
                  cognitive_output_text_rx,
                  cognitive_state_rx,
                  add_document_tx| {
                let p = plugin.clone();
                tokio::spawn(async move {
                    p.start(
                        peer_input_text_tx,
                        cognitive_output_text_rx,
                        cognitive_state_rx,
                        add_document_tx,
                    )
                    .await
                    .inspect_err(|e| tracing::error!("Chat plugin failed: {:?}", e))
                    .ok();
                });
            },
        ));
        tracing::info!("  Chat capability registered.");
    }

    fn register_documents<P: synapto_interface::DocumentsPlugin>(&mut self, plugin: Arc<P>) {
        self.documents_spawner = Some(Box::new(move |add_document_rx| {
            let p = plugin.clone();
            tokio::spawn(async move {
                p.start(add_document_rx)
                    .await
                    .inspect_err(|e| tracing::error!("Documents plugin failed: {:?}", e))
                    .ok();
            });
        }));
        tracing::info!("  Documents capability registered.");
    }

    fn register_interaction_observer<P: synapto_interface::InteractionObserver>(
        &mut self,
        plugin: Arc<P>,
    ) {
        let name = std::any::type_name::<P>().to_string();
        self.interaction_observer_spawners.push((
            name,
            Box::new(move |interaction_rx| {
                let p = plugin.clone();
                tokio::spawn(async move {
                    if let Err(e) = p.start(interaction_rx).await {
                        tracing::error!(
                            "InteractionObserver plugin {} failed: {}",
                            std::any::type_name::<P>(),
                            e
                        );
                    }
                });
            }),
        ));
        tracing::info!("  InteractionObserver capability registered.");
    }

    fn register_rollout_controller<P: synapto_interface::RolloutController>(
        &mut self,
        plugin: Arc<P>,
    ) {
        let name = std::any::type_name::<P>().to_string();
        self.rollout_controller_spawners.push((
            name,
            Box::new(move |rollout_tx| {
                let p = plugin.clone();
                tokio::spawn(async move {
                    if let Err(e) = p.start(rollout_tx).await {
                        tracing::error!(
                            "RolloutController plugin {} failed: {}",
                            std::any::type_name::<P>(),
                            e
                        );
                    }
                });
            }),
        ));
        tracing::info!("  RolloutController capability registered.");
    }

    fn register_retrospective_consolidation<
        P: synapto_interface::RetrospectiveConsolidationPlugin,
    >(
        &mut self,
        plugin: Arc<P>,
    ) {
        self.retrospective_consolidation_spawners.push(Box::new(
            move |not_clear_rx, resolve_tx| {
                let p = plugin.clone();
                tokio::spawn(async move {
                    if let Err(e) = p.start(not_clear_rx, resolve_tx).await {
                        tracing::error!("Retrospective consolidation plugin error: {}", e);
                    }
                });
            },
        ));
        tracing::info!("  Retrospective consolidation capability registered.");
    }

    fn register_context_provider<P: synapto_interface::types::ContextProvider>(
        &mut self,
        provider: Arc<P>,
    ) {
        match P::SCOPE {
            synapto_interface::types::TemporalScope::Historical => {
                self.registries.historical.register_erased(provider.clone());
            }
            synapto_interface::types::TemporalScope::Current => {
                self.registries.current.register_erased(provider.clone());
            }
            synapto_interface::types::TemporalScope::Prospective => {
                self.registries
                    .prospective
                    .register_erased(provider.clone());
            }
        }
        tracing::info!("  Context provider capability '{}' registered.", P::NAME);
    }

    fn register_command<Cmd: synapto_interface::types::Command>(&mut self, command: Cmd) {
        let command_arc: Arc<dyn synapto_interface::types::ErasedCommand> = Arc::new(command);
        self.commands.register_erased(command_arc);
    }

    fn register_tool<T: synapto_interface::types::Tool>(&mut self, tool: T) {
        let tool_arc: Arc<dyn synapto_interface::types::ErasedTool> = Arc::new(tool);
        self.tools.register_erased(tool_arc);
    }

    fn register_diarization<P: DiarizationPlugin>(&mut self, plugin: Arc<P>) {
        self.diarization_heuristic = plugin.heuristic();
        self.diarization_spawner = Some(Box::new(move |audio_rx, segment_tx| {
            let p = plugin.clone();
            tokio::spawn(async move {
                p.start(audio_rx, segment_tx)
                    .await
                    .inspect_err(|e| tracing::error!("Diarization plugin failed: {:?}", e))
                    .ok();
            });
        }));
        tracing::info!("  Diarization capability registered.");
    }

    fn register_call<P: CallPlugin>(&mut self, plugin: Arc<P>, capability: Option<&'static str>) {
        if let Some(desc) = capability {
            register_dynamic_capability(desc.to_string());
        }

        self.call_spawner = Some(Box::new(
            move |peer_input_text_rx,
                  cognitive_output_text_tx,
                  last_voice_time_rx,
                  ai_speaking_rx,
                  call_active_tx| {
                let p = plugin.clone();
                tokio::spawn(async move {
                    if let Err(e) = p
                        .start(
                            peer_input_text_rx,
                            cognitive_output_text_tx,
                            last_voice_time_rx,
                            ai_speaking_rx,
                            call_active_tx,
                        )
                        .await
                    {
                        tracing::error!("Call plugin error: {}", e);
                    }
                });
            },
        ));
        tracing::info!("  Call capability registered.");
    }

    fn register_recorder<P: AudioRecorderPlugin>(&mut self, plugin: Arc<P>) {
        self.audio_recorder_spawners
            .push(Box::new(move |call_active_rx, input_voice_audio_rx| {
                let p = plugin.clone();
                tokio::spawn(async move {
                    if let Err(e) = p.start(call_active_rx, input_voice_audio_rx).await {
                        tracing::error!("Audio recorder plugin error: {}", e);
                    }
                });
            }));
        tracing::info!("  Audio recorder capability registered.");
    }

    fn register_gui<P: synapto_interface::GuiPlugin>(&mut self, plugin: Arc<P>) {
        self.gui_spawner = Some(Box::new(move |registries, error_rx| {
            let p = plugin.clone();
            tokio::spawn(async move {
                if let Err(e) = p.start(registries, error_rx).await {
                    tracing::error!("GUI plugin error: {}", e);
                }
            });
        }));
        tracing::info!("  GUI capability registered.");
    }

    fn register_camera<P: synapto_interface::CameraPlugin>(&mut self, plugin: Arc<P>) {
        self.camera_spawner = Some(Box::new(move |tx_opt| {
            if let Some(tx) = tx_opt.take() {
                let p = plugin.clone();
                tokio::spawn(async move {
                    if let Err(e) = p.start(tx).await {
                        tracing::error!("Camera plugin error: {}", e);
                    }
                });
            }
        }));
        tracing::info!("  Camera capability registered.");
    }
}
