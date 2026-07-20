use synapto_interface::peer_input_audio::AudioInputPlugin;
use synapto_interface::cognitive_output_audio::AudioOutputPlugin;
use synapto_interface::plugin::Plugin;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

mod audio_utils;
mod capture;
mod speaker;

pub use speaker::play;

#[derive(Serialize, Deserialize, Clone, Debug, Default, JsonSchema)]
pub struct HostAudioConfig {
    pub audio_input_target: Option<String>,
    pub audio_output_target: Option<String>,
}

#[derive(Debug)]
pub struct Terminate;

pub struct HostAudioInputPlugin {
    config: HostAudioConfig,
    capture_quit_tx: Arc<Mutex<Option<pipewire::channel::Sender<Terminate>>>>,
}

#[async_trait::async_trait]
impl Plugin for HostAudioInputPlugin {
    fn register<R: synapto_interface::plugin::PluginRegistry + ?Sized>(
        self: std::sync::Arc<Self>,
        registry: &mut R,
    ) where
        Self: Sized,
    {
        registry.register_audio_input(self);
    }

    async fn create(context: &synapto_interface::plugin::PluginInitContext<'_>) -> Result<Self, String> {
        let config: HostAudioConfig = context.config()?;
        Ok(Self {
            config,
            capture_quit_tx: Arc::new(Mutex::new(None)),
        })
    }
}

#[async_trait]
impl AudioInputPlugin for HostAudioInputPlugin {
    async fn start(
        &self,
        tx: synapto_interface::sync::mpsc::Sender<synapto_interface::peer_input_audio::PeerInputAudio>,
    ) -> Result<(), String> {
        let config = self.config.clone();
        let capture_quit_tx = self.capture_quit_tx.clone();
        std::thread::spawn(move || {
            if let Err(e) = capture::run_capture_task(config, tx, capture_quit_tx) {
                tracing::error!("HostAudio Capture task error: {}", e);
            }
        });
        Ok(())
    }
}

impl Drop for HostAudioInputPlugin {
    fn drop(&mut self) {
        tracing::debug!("Dropping HostAudioInputPlugin, signalling capture thread to quit");
        if let Some(tx) = self
            .capture_quit_tx
            .lock()
            .unwrap_or_else(|e| panic!("Failed to lock: {:?}", e))
            .take()
            && let Err(e) = tx.send(Terminate)
        {
            tracing::error!("Channel send failed: {:?}", e);
        }
    }
}

pub struct HostAudioOutputPlugin {
    config: HostAudioConfig,
    playback_quit_tx: Arc<Mutex<Option<pipewire::channel::Sender<Terminate>>>>,
}

#[async_trait]
impl Plugin for HostAudioOutputPlugin {
    fn register<R: synapto_interface::plugin::PluginRegistry + ?Sized>(
        self: std::sync::Arc<Self>,
        registry: &mut R,
    ) where
        Self: Sized,
    {
        registry.register_audio_output(self);
    }

    async fn create(context: &synapto_interface::plugin::PluginInitContext<'_>) -> Result<Self, String> {
        let config: HostAudioConfig = context.config()?;
        Ok(Self {
            config,
            playback_quit_tx: Arc::new(Mutex::new(None)),
        })
    }
}

#[async_trait]
impl AudioOutputPlugin for HostAudioOutputPlugin {
    async fn start(
        &self,
        rx: synapto_interface::sync::mpsc::Receiver<synapto_interface::cognitive_output_audio::CognitiveOutputAudio>,
    ) -> Result<(), String> {
        let config = self.config.clone();
        let playback_quit_tx = self.playback_quit_tx.clone();
        std::thread::spawn(move || {
            if let Err(e) = speaker::run_playback_task(config, rx, playback_quit_tx) {
                tracing::error!("HostAudio Playback task error: {}", e);
            }
        });
        Ok(())
    }
}

impl Drop for HostAudioOutputPlugin {
    fn drop(&mut self) {
        tracing::debug!("Dropping HostAudioOutputPlugin, signalling playback thread to quit");
        if let Some(tx) = self
            .playback_quit_tx
            .lock()
            .unwrap_or_else(|e| panic!("Failed to lock: {:?}", e))
            .take()
            && let Err(e) = tx.send(Terminate)
        {
            tracing::error!("Channel send failed: {:?}", e);
        }
    }
}
