use std::marker::PhantomData;
use std::sync::Arc;
use synapto_interface::cognitive::{CognitiveOutputSpeech, CognitiveStateUpdate};
use synapto_interface::cognitive_output_audio::CognitiveOutputAudio;
use synapto_interface::cognitive_output_text::CognitiveOutputText;
use synapto_interface::interaction::NotClearInteractionMemory;
use synapto_interface::interaction::Timestamp;
use synapto_interface::peer_input::PeerInputSpeech;
use synapto_interface::peer_input_audio::PeerInputAudio;
use synapto_interface::peer_input_text::PeerInputText;
//
use synapto_interface::audio_recorder::AudioRecorderPlugin;
use synapto_interface::call::CallPlugin;
use synapto_interface::chat::ChatPlugin;
use synapto_interface::cognitive_output_audio::AudioOutputPlugin;
use synapto_interface::peer_input_audio::AudioInputPlugin;
use synapto_interface::plugin::Plugin;
use synapto_interface::speech_to_text::{DiarizationPlugin, STTPlugin, TTSPlugin};
//
use synapto_interface::speech_to_text::SpeakerSegment;

use crate::{
    cognitive::{CognitiveDirectInterrupt, CognitiveDirectTrigger},
    interactions::Interaction,
};
use std::process::ExitCode;
use synapto_interface::sync::{broadcast, mpsc, watch};
use synapto_telemetry::tracing::Tracing;

mod cognitive;
pub use cognitive::CognitiveLLMContent;
pub mod config;
pub mod credentials;
pub mod prompt_provider;
pub mod storage;
mod utils;

pub mod data_dir;
mod google_credentials;
mod interactions;
mod speaking_coordinator;

mod speech_to_text;

mod users;
mod working_memory;

use synapto_interface::speech_to_text::{InputVoiceAudio, SpeechDetected, SpeechTranscript};

#[derive(Clone)]
struct PluginInitFactory<C> {
    config_provider: Arc<C>,
    llm_executor: synapto_interface::llm::LlmExecutor,
    decision_handle: synapto_interface::decision::DecisionHandle,
    storage: synapto_interface::storage::StorageHandle,
    credentials: synapto_interface::credentials::CredentialsHandle,
    timeout: std::time::Duration,
}

impl<C: config::ConfigProvider> PluginInitFactory<C> {
    async fn init_plugin<P: synapto_interface::plugin::Plugin>(&self) -> Arc<P> {
        let full_path = core::any::type_name::<P>();
        let base_path = full_path.split('<').next().unwrap_or(full_path);
        let plugin_identity = base_path.to_string();
        Tracing::add_plugin_to_log(&plugin_identity);

        let crate_name = full_path
            .split("::")
            .next()
            .unwrap_or("")
            .to_string()
            .replace('-', "_");
        let plugin_type_name = base_path.split("::").last().unwrap_or("").to_string();

        let plugin_config = self
            .config_provider
            .get_plugin_config_value(&crate_name, &plugin_type_name);

        let safe_namespace = base_path.replace("::", "_").replace(" ", "");

        let init_context = synapto_interface::plugin::PluginInitContext::new(
            self.llm_executor.clone(),
            self.decision_handle.clone(),
            &plugin_config,
            self.storage.clone(),
            &safe_namespace,
            self.credentials.clone(),
        );

        let start = std::time::Instant::now();
        let timeout_duration = self.timeout;
        let plugin_result =
            match tokio::time::timeout(timeout_duration, P::create(&init_context)).await {
                Ok(res) => res,
                Err(_) => Err(format!(
                    "Plugin initialization timed out after {:?}",
                    timeout_duration
                )),
            };
        let elapsed = start.elapsed();

        let plugin = Arc::new(plugin_result.unwrap_or_else(|e| {
            panic!("Failed to initialize plugin '{}': {}", plugin_identity, e)
        }));

        tracing::debug!(
            target: "synapto",
            "Plugin {} instantiated in {:?}",
            plugin_identity,
            elapsed
        );

        plugin
    }
}

#[async_trait::async_trait]
pub trait PluginTuple<
    C: config::ConfigProvider,
    S: synapto_interface::storage::StorageConnection
        + synapto_interface::storage::KeyValueStore
        + synapto_interface::storage::RecordStore,
    PR: prompt_provider::CognitivePromptProvider,
    CR: credentials::CredentialsTuple = (),
>
{
    async fn register_plugins(synapto: Synapto<C, S, PR, CR>) -> Synapto<C, S, PR, CR>;
}

#[async_trait::async_trait]
impl<
    C: config::ConfigProvider,
    S: synapto_interface::storage::StorageConnection
        + synapto_interface::storage::KeyValueStore
        + synapto_interface::storage::RecordStore,
    PR: prompt_provider::CognitivePromptProvider,
    CR: credentials::CredentialsTuple,
> PluginTuple<C, S, PR, CR> for ()
{
    async fn register_plugins(synapto: Synapto<C, S, PR, CR>) -> Synapto<C, S, PR, CR> {
        synapto
    }
}

macro_rules! impl_plugin_tuple {
    ($($T:ident),+) => {
        #[async_trait::async_trait]
        impl<
            C: config::ConfigProvider,
            S: synapto_interface::storage::StorageConnection + synapto_interface::storage::KeyValueStore + synapto_interface::storage::RecordStore,
            PR: prompt_provider::CognitivePromptProvider,
            CR: credentials::CredentialsTuple,
            $($T: synapto_interface::plugin::Plugin),+
        > PluginTuple<C, S, PR, CR> for ($($T,)+) {
            #[allow(non_snake_case)]
            async fn register_plugins(mut synapto: Synapto<C, S, PR, CR>) -> Synapto<C, S, PR, CR> {
                let start = std::time::Instant::now();
                let factory = synapto.plugin_init_factory();
                let ($($T,)+) = tokio::join!(
                    $(factory.init_plugin::<$T>(),)+
                );
                let count = [$(stringify!($T)),+].len();
                tracing::debug!(
                    target: "synapto",
                    "All {} plugins instantiated concurrently in {:?}",
                    count,
                    start.elapsed()
                );
                $(synapto.attach_plugin($T);)+
                synapto
            }
        }
    };
}

impl_plugin_tuple!(P1);
impl_plugin_tuple!(P1, P2);
impl_plugin_tuple!(P1, P2, P3);
impl_plugin_tuple!(P1, P2, P3, P4);
impl_plugin_tuple!(P1, P2, P3, P4, P5);
impl_plugin_tuple!(P1, P2, P3, P4, P5, P6);
impl_plugin_tuple!(P1, P2, P3, P4, P5, P6, P7);
impl_plugin_tuple!(P1, P2, P3, P4, P5, P6, P7, P8);
impl_plugin_tuple!(P1, P2, P3, P4, P5, P6, P7, P8, P9);
impl_plugin_tuple!(P1, P2, P3, P4, P5, P6, P7, P8, P9, P10);
impl_plugin_tuple!(P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11);
impl_plugin_tuple!(P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12);
impl_plugin_tuple!(P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12, P13);
impl_plugin_tuple!(P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12, P13, P14);
impl_plugin_tuple!(
    P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12, P13, P14, P15
);
impl_plugin_tuple!(
    P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12, P13, P14, P15, P16
);

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
        ) + Send,
>;

type DocumentProviderSpawner =
    Box<dyn FnOnce(mpsc::Sender<synapto_interface::document::AddDocumentRequest>) + Send>;

type DocumentsSpawner =
    Box<dyn FnOnce(mpsc::Receiver<synapto_interface::document::AddDocumentRequest>) + Send>;

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
            std::sync::Arc<synapto_interface::context::ContextRegistries>,
            std::sync::mpsc::Receiver<String>,
        ) + Send,
>;

type CameraSpawner =
    Box<dyn FnOnce(&mut Option<watch::Sender<synapto_interface::camera::CameraInputFrame>>) + Send>;

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

/// Marker for unconfigured mandatory configuration provider.
pub struct NoConfig;

/// Marker for unconfigured mandatory storage backend.
pub struct NoStorage;

/// Marker for absent decision provider.
pub struct NoDecision;

/// Marker for configured singleton decision provider.
pub struct WithDecision<D>(pub(crate) PhantomData<D>);

/// Public trait defining decision provider setup into Synapto core.
pub trait DecisionSetup<
    C: config::ConfigProvider,
    S: synapto_interface::storage::StorageConnection
        + synapto_interface::storage::KeyValueStore
        + synapto_interface::storage::RecordStore,
    PR: prompt_provider::CognitivePromptProvider,
    CR: credentials::CredentialsTuple,
>
{
    fn setup(synapto: &mut Synapto<C, S, PR, CR>) -> Result<(), String>;
}

impl<
    C: config::ConfigProvider,
    S: synapto_interface::storage::StorageConnection
        + synapto_interface::storage::KeyValueStore
        + synapto_interface::storage::RecordStore,
    PR: prompt_provider::CognitivePromptProvider,
    CR: credentials::CredentialsTuple,
> DecisionSetup<C, S, PR, CR> for NoDecision
{
    fn setup(_synapto: &mut Synapto<C, S, PR, CR>) -> Result<(), String> {
        Ok(())
    }
}

impl<
    C: config::ConfigProvider,
    S: synapto_interface::storage::StorageConnection
        + synapto_interface::storage::KeyValueStore
        + synapto_interface::storage::RecordStore,
    PR: prompt_provider::CognitivePromptProvider,
    CR: credentials::CredentialsTuple,
    D,
> DecisionSetup<C, S, PR, CR> for WithDecision<D>
where
    D: synapto_interface::decision::DecisionProvider,
{
    fn setup(synapto: &mut Synapto<C, S, PR, CR>) -> Result<(), String> {
        let full_path = core::any::type_name::<D>();
        let crate_name = full_path
            .split("::")
            .next()
            .unwrap_or("")
            .to_string()
            .replace('-', "_");
        let base_path = full_path.split('<').next().unwrap_or(full_path);
        let provider_type_name = base_path.split("::").last().unwrap_or("").to_string();

        let raw_config = synapto
            .config_provider
            .get_decision_config_value(&crate_name, &provider_type_name);

        let config: D::Config = serde_json::from_value(raw_config).map_err(|e| {
            format!(
                "Failed to parse config for decision provider '{}': {}",
                provider_type_name, e
            )
        })?;

        let provider = D::init(config, synapto.credentials.clone())?;
        synapto
            .decision_handle
            .set_arc_backend(provider.raw_decision_executor());
        Tracing::add_plugin_to_log(&provider_type_name);
        tracing::info!("  Decision capability registered: {}", provider_type_name);
        Ok(())
    }
}

/// Zero-cost typestate builder for Synapto bundles.
pub struct SynaptoBuilder<C, S, PR, CR, D, P> {
    _marker: PhantomData<(C, S, PR, CR, D, P)>,
}

impl Synapto<NoConfig, NoStorage, prompt_provider::EmptyPromptProvider, ()> {
    /// Entry point for fluent bundle composition.
    pub fn builder()
    -> SynaptoBuilder<NoConfig, NoStorage, prompt_provider::EmptyPromptProvider, (), NoDecision, ()>
    {
        SynaptoBuilder {
            _marker: PhantomData,
        }
    }
}

impl<C, S, PR, CR, D, P> SynaptoBuilder<C, S, PR, CR, D, P> {
    /// Sets the configuration provider sources (plural: accepts tuple).
    pub fn configs<NewC: config::ConfigProvider>(self) -> SynaptoBuilder<NewC, S, PR, CR, D, P> {
        SynaptoBuilder {
            _marker: PhantomData,
        }
    }

    /// Sets the shared storage backend (singular: accepts single storage type).
    pub fn storage<NewS>(self) -> SynaptoBuilder<C, NewS, PR, CR, D, P>
    where
        NewS: synapto_interface::storage::StorageConnection
            + synapto_interface::storage::KeyValueStore
            + synapto_interface::storage::RecordStore,
    {
        SynaptoBuilder {
            _marker: PhantomData,
        }
    }

    /// Overrides the cognitive prompt provider (singular: accepts single prompt provider, defaults to EmptyPromptProvider).
    pub fn prompt<NewPR: prompt_provider::CognitivePromptProvider>(
        self,
    ) -> SynaptoBuilder<C, S, NewPR, CR, D, P> {
        SynaptoBuilder {
            _marker: PhantomData,
        }
    }

    /// Overrides credentials providers (plural: accepts tuple, defaults to ()).
    pub fn credentials<NewCR: credentials::CredentialsTuple>(
        self,
    ) -> SynaptoBuilder<C, S, PR, NewCR, D, P> {
        SynaptoBuilder {
            _marker: PhantomData,
        }
    }

    /// Registers the plugin tuple (plural: accepts tuple).
    pub fn plugins<NewP>(self) -> SynaptoBuilder<C, S, PR, CR, D, NewP> {
        SynaptoBuilder {
            _marker: PhantomData,
        }
    }
}

/// Singleton decision provider registration is available only when NoDecision is present.
impl<C, S, PR, CR, P> SynaptoBuilder<C, S, PR, CR, NoDecision, P> {
    /// Registers the singular decision provider (singular: accepts single decision provider).
    /// Calling this method a second time is prevented at compile time.
    /// Cannot be called with standard plugins (must implement DecisionProvider).
    pub fn decision<D: synapto_interface::decision::DecisionProvider>(
        self,
    ) -> SynaptoBuilder<C, S, PR, CR, WithDecision<D>, P> {
        SynaptoBuilder {
            _marker: PhantomData,
        }
    }
}

/// Terminal execution method: available only when mandatory infrastructure is provided.
impl<C, S, PR, CR, D, P> SynaptoBuilder<C, S, PR, CR, D, P>
where
    C: config::ConfigProvider,
    S: synapto_interface::storage::StorageConnection
        + synapto_interface::storage::KeyValueStore
        + synapto_interface::storage::RecordStore,
    PR: prompt_provider::CognitivePromptProvider,
    CR: credentials::CredentialsTuple,
    D: DecisionSetup<C, S, PR, CR>,
    P: PluginTuple<C, S, PR, CR>,
{
    pub async fn run(self) -> ExitCode {
        let mut synapto = Synapto::<C, S, PR, CR>::new();
        if let Err(e) = D::setup(&mut synapto) {
            panic!("Failed to initialize decision provider: {}", e);
        }
        let synapto = P::register_plugins(synapto).await;
        synapto.run_internal().await
    }
}

pub struct Synapto<C = NoConfig, S = NoStorage, PR = prompt_provider::EmptyPromptProvider, CR = ()>
{
    config: config::Config,
    config_provider: Arc<C>,
    _prompt_provider: std::marker::PhantomData<PR>,
    _storage_provider: std::marker::PhantomData<S>,
    _credentials_provider: std::marker::PhantomData<CR>,
    credentials: synapto_interface::credentials::CredentialsHandle,
    audio_input_spawners: Vec<AudioInputSpawner>,
    audio_output_spawners: Vec<AudioOutputSpawner>,
    stt_spawners: Vec<SttSpawner>,
    tts_spawners: Vec<TtsSpawner>,
    chat_spawner: Option<ChatSpawner>,
    documents_spawner: Option<DocumentsSpawner>,
    document_provider_spawners: Vec<DocumentProviderSpawner>,
    diarization_spawner: Option<DiarizationSpawner>,
    diarization_heuristic: Option<synapto_interface::speech_to_text::SpeakerHeuristicCallback>,
    call_spawner: Option<CallSpawner>,
    audio_recorder_spawners: Vec<RecorderSpawner>,
    plugins_names: Vec<String>,
    plugins: std::collections::HashMap<std::any::TypeId, Arc<dyn std::any::Any + Send + Sync>>,
    registries: synapto_interface::context::EngineRegistries,
    storage: synapto_interface::storage::StorageHandle,
    #[allow(clippy::type_complexity)]
    interaction_observer_spawners: Vec<(
        String,
        Box<
            dyn FnOnce(mpsc::Receiver<synapto_interface::interaction::ObservedInteraction>)
                + Send
                + Sync,
        >,
    )>,
    #[allow(clippy::type_complexity)]
    rollout_controller_spawners: Vec<(
        String,
        Box<dyn FnOnce(watch::Sender<synapto_interface::interaction::Timestamp>) + Send + Sync>,
    )>,
    #[allow(clippy::type_complexity)]
    retrospective_consolidation_spawners: Vec<
        Box<
            dyn FnOnce(
                    watch::Receiver<synapto_interface::interaction::NotClearInteractionMemory>,
                    mpsc::Sender<synapto_interface::interaction::Timestamp>,
                ) + Send
                + Sync,
        >,
    >,
    llm_executor: synapto_interface::llm::LlmExecutor,
    decision_handle: synapto_interface::decision::DecisionHandle,
    gui_spawner: Option<GuiSpawner>,
    camera_spawner: Option<CameraSpawner>,
    error_rx: Option<std::sync::mpsc::Receiver<String>>,
    current_context_tx: watch::Sender<serde_json::Value>,
    _tracing: Tracing,
}

impl<
    C: config::ConfigProvider,
    S: synapto_interface::storage::StorageConnection
        + synapto_interface::storage::KeyValueStore
        + synapto_interface::storage::RecordStore,
    PR: prompt_provider::CognitivePromptProvider,
    CR: credentials::CredentialsTuple,
> Synapto<C, S, PR, CR>
{
    #[allow(clippy::new_without_default)]
    fn new() -> Self {
        Self::with_config_provider(C::init())
    }

    fn with_config_provider(config_provider: C) -> Self {
        let credentials = CR::build_handle(&config_provider)
            .unwrap_or_else(|e| panic!("Failed to build credentials handle: {}", e));
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
            google_service_account_credentials: config
                .google_service_account_credentials
                .clone()
                .map(|secret| secret.into_secret()),
            gemini_api_key: config.gemini_api_key.clone(),
        };
        let llm_executor = synapto_interface::llm::LlmExecutor::new(
            synapto_llm::ConcreteLlmExecutor::new(executor_config),
        );

        let (current_context_tx, _current_context_rx) = watch::channel(serde_json::Value::Null);

        let registries = synapto_interface::context::EngineRegistries::default();
        let storage_resolver = Arc::new(CoreStorageConfigResolver {
            provider: config_provider.clone(),
        });
        let storage = synapto_interface::storage::StorageHandle::new(storage_resolver);

        Self {
            config_provider,
            _prompt_provider: std::marker::PhantomData,
            _storage_provider: std::marker::PhantomData,
            _credentials_provider: std::marker::PhantomData,
            credentials,
            config,
            _tracing: tracing,
            audio_input_spawners: Vec::new(),
            audio_output_spawners: Vec::new(),
            stt_spawners: Vec::new(),
            tts_spawners: Vec::new(),
            chat_spawner: None,
            documents_spawner: None,
            document_provider_spawners: Vec::new(),
            diarization_spawner: None,
            diarization_heuristic: None,
            call_spawner: None,
            audio_recorder_spawners: Vec::new(),
            plugins_names: Vec::new(),
            plugins: std::collections::HashMap::new(),
            registries,
            storage,
            interaction_observer_spawners: Vec::new(),
            rollout_controller_spawners: Vec::new(),
            retrospective_consolidation_spawners: Vec::new(),
            gui_spawner: None,
            camera_spawner: None,
            error_rx: Some(error_rx),
            current_context_tx,

            llm_executor,
            decision_handle: synapto_interface::decision::DecisionHandle::empty(),
        }
    }

    // fn load_plugin_config<P: Plugin>(&self) -> serde_json::Value {
    //     let full_path = std::any::type_name::<P>();
    //     let crate_name = full_path
    //         .split("::")
    //         .next()
    //         .unwrap_or("")
    //         .to_string()
    //         .replace('-', "_");
    //     let base_path = full_path.split('<').next().unwrap_or(full_path);
    //     let plugin_type_name = base_path.split("::").last().unwrap_or("").to_string();

    //     self.config_provider
    //         .get_plugin_config_value(&crate_name, &plugin_type_name)
    // }

    fn plugin_init_factory(&self) -> PluginInitFactory<C> {
        PluginInitFactory {
            config_provider: self.config_provider.clone(),
            llm_executor: self.llm_executor.clone(),
            decision_handle: self.decision_handle.clone(),
            storage: self.storage.clone(),
            credentials: self.credentials.clone(),
            timeout: std::time::Duration::from_secs(self.config.plugin_init_timeout_secs),
        }
    }

    fn attach_plugin<P: Plugin>(&mut self, plugin: Arc<P>) {
        let full_path = core::any::type_name::<P>();
        let base_path = full_path.split('<').next().unwrap_or(full_path);
        let plugin_identity = base_path.to_string();
        self.plugins_names.push(plugin_identity.clone());

        let type_id = std::any::TypeId::of::<P>();
        self.plugins.insert(type_id, plugin.clone());
        tracing::info!("Plugin {} registered.", plugin_identity);

        let start = std::time::Instant::now();
        plugin.register(self);
        let elapsed = start.elapsed();
        tracing::debug!(
            target: "synapto",
            "Plugin {} registration took {:?}",
            plugin_identity,
            elapsed
        );
    }

    async fn run_internal(self) -> ExitCode {
        let disable_ctrl_c = self.config.disable_ctrl_c;
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
            mpsc::channel::<synapto_interface::document::AddDocumentRequest>(10);

        let (mut video_tx_opt, video_rx_opt) = if self.camera_spawner.is_some() {
            let (video_tx, video_rx) =
                watch::channel(synapto_interface::camera::CameraInputFrame { data: Vec::new() });
            (Some(video_tx), Some(video_rx))
        } else {
            (None, None)
        };

        let (cognitive_speaking_rx, cognitive_speaking_semaphore) = speaking_coordinator::start(
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
                cognitive_speaking_rx.clone(),
                call_active_tx,
            );
        }

        let (speaker_segment_tx, speaker_segment_rx_opt) = if self.diarization_spawner.is_some() {
            let (tx, rx) = mpsc::channel::<SpeakerSegment>(100);
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };

        let (speech_transcript_tx, core_voice_audio_rx, input_voice_audio_tx) =
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
            watch::channel(NotClearInteractionMemory::default());

        let (resolve_not_clear_tx, resolve_not_clear_rx) = mpsc::channel::<Timestamp>(100);

        for spawner in self.retrospective_consolidation_spawners {
            spawner(not_clear_memory_rx.clone(), resolve_not_clear_tx.clone());
        }

        let mut observers_tx = Vec::new();
        let mut interaction_rollout_receivers = Vec::new();

        for (name, spawner) in self.rollout_controller_spawners {
            let (rollout_tx, rollout_rx) =
                watch::channel(synapto_interface::interaction::Timestamp(i64::MAX));
            interaction_rollout_receivers.push((name, rollout_rx));
            spawner(rollout_tx);
        }

        for (_name, spawner) in self.interaction_observer_spawners {
            let (observer_tx, observer_rx) =
                mpsc::channel::<synapto_interface::interaction::ObservedInteraction>(100);
            observers_tx.push(observer_tx);

            spawner(observer_rx);
        }

        let (resolve_in_flight_tool_tx, resolve_in_flight_tool_rx) =
            mpsc::channel::<synapto_interface::tool::ToolCallId>(100);

        // TODO explore whether the "core" plugin context could be used more than for reusing storage provider initialization logic
        let core_config = serde_json::json!({});
        let core_namespace = "core";
        let core_plugin_context = synapto_interface::plugin::PluginInitContext::new(
            self.llm_executor.clone(),
            self.decision_handle.clone(),
            &core_config,
            self.storage.clone(),
            core_namespace,
            self.credentials.clone(),
        );

        let core_storage = match core_plugin_context.store::<S>().await {
            Ok(store) => store,
            Err(e) => {
                panic!("Failed to initialize core storage: {e}");
            }
        };

        let working_memory_store = working_memory::start(
            &mut observers_tx,
            self.registries.clone(),
            self.llm_executor.clone(),
            self.decision_handle.clone(),
            self.config.cognitive.clone(),
        );

        interactions::start(
            new_interaction_rx,
            interaction_rollout_receivers,
            observers_tx,
            interaction_memory_tx.clone(),
            resolve_not_clear_rx,
            not_clear_memory_tx,
            resolve_in_flight_tool_rx,
            core_storage,
        )
        .await;

        let registries = self.registries.clone();

        // Background task: monitors current context provider updates, gathers active context values,
        // and broadcasts them over `current_context_tx`.
        {
            let current_context_tx = self.current_context_tx;
            let mut current_update_rx = registries.context.current.subscribe();
            let registries = registries.clone();
            tokio::spawn(async move {
                while current_update_rx.changed().await.is_ok() {
                    let request = synapto_interface::context::ContextRequest::default();
                    let current_contexts =
                        registries.context.current.gather_contexts(&request).await;
                    if let Ok(value) = serde_json::to_value(current_contexts)
                        && current_context_tx.receiver_count() > 0
                        && let Err(e) = current_context_tx.send(value)
                    {
                        tracing::error!("Failed to broadcast current context: {:?}", e);
                    }
                }
            });
        }

        if let Some(gui_spawner) = self.gui_spawner {
            gui_spawner(
                registries.context.clone(),
                self.error_rx.expect("error_rx missing"),
            );
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
            let mut cognitive_speech_rx_opt = Some(cognitive_speech_tx.subscribe());
            spawner(
                &mut cognitive_speech_rx_opt,
                &mut cognitive_output_audio_tx_opt,
            );
        }

        let has_chat_plugin = self.chat_spawner.is_some();

        for spawner in self.document_provider_spawners {
            spawner(add_document_tx.clone());
        }

        if let Some(spawner) = self.documents_spawner {
            spawner(add_document_rx);
        }

        if let Some(spawner) = self.chat_spawner {
            spawner(
                peer_input_text_tx.clone(),
                cognitive_output_text_rx,
                cognitive_state_tx.subscribe(),
            );
        }

        cognitive::start::<PR>(
            self.config,
            self.llm_executor.clone(),
            trigger_cognitive_direct,
            interrupt_cognitive_direct,
            cognitive_speaking_semaphore,
            peer_input_text_broadcast_tx.subscribe(),
            peer_input_speech_rx,
            interaction_memory_rx,
            cognitive_speech_tx,
            new_interaction_tx,
            video_rx_opt,
            registries.clone(),
            if has_chat_plugin {
                Some(cognitive_output_text_tx)
            } else {
                None
            },
            cognitive_state_tx,
            self.decision_handle.clone(),
            resolve_in_flight_tool_tx,
            working_memory_store,
        )
        .await;

        tracing::info!("--- System is running. Waiting for events... ---\n");

        let ctrl_c_fut = async {
            if !disable_ctrl_c {
                tokio::signal::ctrl_c().await
            } else {
                std::future::pending::<std::io::Result<()>>().await
            }
        };

        let exit_code = better_tokio_select::tokio_select!(match .. {
            .. if let res = shutdown_rx.recv() => {
                match res {
                    Some(synapto_shutdown::ShutdownResult(Ok(()))) => {
                        tracing::info!("Standard shutdown requested.");
                        ExitCode::SUCCESS
                    }
                    Some(synapto_shutdown::ShutdownResult(Err(e))) => {
                        tracing::error!(error = %e, "Fatal error, shutting down.");
                        ExitCode::FAILURE
                    }
                    None => {
                        tracing::warn!("Shutdown channel closed unexpectedly.");
                        ExitCode::FAILURE
                    }
                }
            }
            .. if let Ok(()) = ctrl_c_fut => {
                tracing::info!("Ctrl+C received, shutting down.");
                ExitCode::SUCCESS
            }
        });

        tracing::info!("\n--- Shutting down ---");
        synapto_shutdown::trigger_graceful();
        tracing::info!("\n--- Shutdown ---");

        exit_code
    }
}

static DYNAMIC_CAPABILITIES: std::sync::OnceLock<std::sync::Mutex<Vec<String>>> =
    std::sync::OnceLock::new();

fn register_dynamic_capability(cap: String) {
    DYNAMIC_CAPABILITIES
        .get_or_init(|| std::sync::Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|e| panic!("DYNAMIC_CAPABILITIES lock poisoned: {:?}", e))
        .push(cap);
}

fn get_dynamic_capabilities() -> Vec<String> {
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
    C: config::ConfigProvider,
    S: synapto_interface::storage::StorageConnection
        + synapto_interface::storage::KeyValueStore
        + synapto_interface::storage::RecordStore,
    PR: prompt_provider::CognitivePromptProvider,
    CR: credentials::CredentialsTuple,
> synapto_interface::plugin::PluginRegistry for Synapto<C, S, PR, CR>
{
    fn register_audio_input<P: AudioInputPlugin>(&mut self, plugin: Arc<P>) {
        self.audio_input_spawners.push(Box::new(move |tx_opt| {
            let tx = tx_opt.take().expect("Multiple AudioInputPlugins registered! This capability must be provided by exactly one plugin.");
            let p = plugin.clone();
            tokio::spawn(async move {
                p.start(tx)
                    .await
                    .inspect_err(|e| tracing::error!("Audio input plugin failed: {:?}", e))
                    .ok();
            });
        }));
        tracing::info!("  Audio input capability registered.");
    }

    fn register_audio_output<P: AudioOutputPlugin>(&mut self, plugin: Arc<P>) {
        self.audio_output_spawners.push(Box::new(move |rx_opt| {
            let rx = rx_opt.take().expect("Multiple AudioOutputPlugins registered! This capability must be provided by exactly one plugin.");
            let p = plugin.clone();
            tokio::spawn(async move {
                p.start(rx)
                    .await
                    .inspect_err(|e| tracing::error!("Audio output plugin failed: {:?}", e))
                    .ok();
            });
        }));
        tracing::info!("  Audio output capability registered.");
    }

    fn register_stt<P: STTPlugin>(&mut self, plugin: Arc<P>) {
        self.stt_spawners.push(Box::new(
            move |audio_rx_opt, transcript_tx, speech_detected| {
                let audio_rx = audio_rx_opt.take().expect("Multiple STTPlugins registered! This capability must be provided by exactly one plugin.");
                let p = plugin.clone();
                tokio::spawn(async move {
                    p.start(audio_rx, transcript_tx, speech_detected)
                        .await
                        .inspect_err(|e| tracing::error!("STT plugin failed: {:?}", e))
                        .ok();
                });
            },
        ));
        tracing::info!("  STT capability registered.");
    }

    fn register_tts<P: TTSPlugin>(&mut self, plugin: Arc<P>) {
        self.tts_spawners
            .push(Box::new(move |speech_rx_opt, audio_tx_opt| {
                let speech_rx = speech_rx_opt
                    .take()
                    .expect("Multiple TTSPlugins registered! (speech_rx already taken)");
                let audio_tx = audio_tx_opt
                    .take()
                    .expect("Multiple TTSPlugins registered! (audio_tx already taken)");
                let p = plugin.clone();
                tokio::spawn(async move {
                    p.start(speech_rx, audio_tx)
                        .await
                        .inspect_err(|e| tracing::error!("TTS plugin failed: {:?}", e))
                        .ok();
                });
            }));
        tracing::info!("  TTS capability registered.");
    }

    fn register_chat<P: ChatPlugin>(&mut self, plugin: Arc<P>) {
        self.chat_spawner = Some(Box::new(
            move |peer_input_text_tx, cognitive_output_text_rx, cognitive_state_rx| {
                let p = plugin.clone();
                tokio::spawn(async move {
                    p.start(
                        peer_input_text_tx,
                        cognitive_output_text_rx,
                        cognitive_state_rx,
                    )
                    .await
                    .inspect_err(|e| tracing::error!("Chat plugin failed: {:?}", e))
                    .ok();
                });
            },
        ));
        tracing::info!("  Chat capability registered.");
    }

    fn register_document_provider<P: synapto_interface::document::DocumentProviderPlugin>(
        &mut self,
        plugin: Arc<P>,
    ) {
        self.document_provider_spawners
            .push(Box::new(move |add_document_tx| {
                let p = plugin.clone();
                tokio::spawn(async move {
                    p.start_document_provider(add_document_tx)
                        .await
                        .inspect_err(|e| {
                            tracing::error!("Document provider plugin failed: {:?}", e)
                        })
                        .ok();
                });
            }));
        tracing::info!("  Document provider capability registered.");
    }

    fn register_documents<P: synapto_interface::document::DocumentsPlugin>(
        &mut self,
        plugin: Arc<P>,
    ) {
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

    fn register_interaction_observer<P: synapto_interface::interaction::InteractionObserver>(
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

    fn register_rollout_controller<P: synapto_interface::rollout::RolloutController>(
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
        P: synapto_interface::interaction::RetrospectiveConsolidationPlugin,
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

    fn register_context_provider<P: synapto_interface::context::IntoContextProvider>(
        &mut self,
        provider: P,
    ) {
        let erased = provider.into_erased_context_provider();
        let name = erased.name();
        let scope_str = match erased.scope() {
            synapto_interface::context::TemporalScope::Historical => {
                self.registries.context.historical.register_erased(erased);
                "Historical"
            }
            synapto_interface::context::TemporalScope::Current => {
                self.registries.context.current.register_erased(erased);
                "Current"
            }
            synapto_interface::context::TemporalScope::Prospective => {
                self.registries.context.prospective.register_erased(erased);
                "Prospective"
            }
        };
        tracing::info!("  {} context provider '{}' registered.", scope_str, name);
    }

    fn register_command<Cmd: synapto_interface::command::Command>(&mut self, command: Cmd) {
        let command_arc: Arc<dyn synapto_interface::command::ErasedCommand> = Arc::new(command);
        self.registries.commands.register_erased(command_arc);
    }

    fn register_tool<T: synapto_interface::tool::Tool>(&mut self, tool: T) {
        let tool_arc: Arc<dyn synapto_interface::tool::ErasedTool> = Arc::new(tool);
        self.registries.tools.register_erased(tool_arc);
    }

    fn register_erased_tool(&mut self, tool: synapto_interface::tool::ToolHandle) {
        self.registries.tools.register_erased(tool.into_inner());
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
                  cognitive_speaking_rx,
                  call_active_tx| {
                let p = plugin.clone();
                tokio::spawn(async move {
                    if let Err(e) = p
                        .start(
                            peer_input_text_rx,
                            cognitive_output_text_tx,
                            last_voice_time_rx,
                            cognitive_speaking_rx,
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

    fn register_gui<P: synapto_interface::gui::GuiPlugin>(&mut self, plugin: Arc<P>) {
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

    fn register_camera<P: synapto_interface::camera::CameraPlugin>(&mut self, plugin: Arc<P>) {
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
