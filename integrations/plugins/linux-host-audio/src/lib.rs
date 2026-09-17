#![allow(incomplete_features)]
#![feature(adt_const_params)]
#![feature(unsized_const_params)]

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use synapto_interface::cognitive_output_audio::AudioOutputPlugin;
use synapto_interface::peer_input_audio::AudioInputPlugin;
use synapto_interface::plugin::Plugin;

mod audio_utils;
mod capture;
mod speaker;

pub use speaker::play;

#[derive(Serialize, Deserialize, Clone, Debug, Default, JsonSchema)]
pub struct HostAudioInputConfig {
    pub audio_input_target: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, JsonSchema)]
pub struct HostAudioOutputConfig {
    pub audio_output_target: Option<String>,
}

#[derive(Debug)]
pub struct Terminate;

pub struct HostAudioInputPlugin<const DEFAULT_TARGET: &'static str = ""> {
    config: HostAudioInputConfig,
    capture_quit_tx: Arc<Mutex<Option<pipewire::channel::Sender<Terminate>>>>,
}

#[async_trait::async_trait]
impl<const DEFAULT_TARGET: &'static str> Plugin for HostAudioInputPlugin<DEFAULT_TARGET> {
    fn register<R: synapto_interface::plugin::PluginRegistry + ?Sized>(
        self: std::sync::Arc<Self>,
        registry: &mut R,
    ) where
        Self: Sized,
    {
        registry.register_audio_input(self);
    }

    async fn create(
        context: &synapto_interface::plugin::PluginInitContext<'_>,
    ) -> Result<Self, String> {
        let mut config: HostAudioInputConfig = context.config()?;
        if config.audio_input_target.is_none() && !DEFAULT_TARGET.is_empty() {
            config.audio_input_target = Some(DEFAULT_TARGET.to_string());
        }
        Ok(Self {
            config,
            capture_quit_tx: Arc::new(Mutex::new(None)),
        })
    }
}

#[async_trait]
impl<const DEFAULT_TARGET: &'static str> AudioInputPlugin for HostAudioInputPlugin<DEFAULT_TARGET> {
    async fn start(
        &self,
        tx: synapto_interface::sync::mpsc::Sender<
            synapto_interface::peer_input_audio::PeerInputAudio,
        >,
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

impl<const DEFAULT_TARGET: &'static str> Drop for HostAudioInputPlugin<DEFAULT_TARGET> {
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

pub struct HostAudioOutputPlugin<const DEFAULT_TARGET: &'static str = ""> {
    config: HostAudioOutputConfig,
    playback_quit_tx: Arc<Mutex<Option<pipewire::channel::Sender<Terminate>>>>,
}

#[async_trait]
impl<const DEFAULT_TARGET: &'static str> Plugin for HostAudioOutputPlugin<DEFAULT_TARGET> {
    fn register<R: synapto_interface::plugin::PluginRegistry + ?Sized>(
        self: std::sync::Arc<Self>,
        registry: &mut R,
    ) where
        Self: Sized,
    {
        registry.register_audio_output(self);
    }

    async fn create(
        context: &synapto_interface::plugin::PluginInitContext<'_>,
    ) -> Result<Self, String> {
        let mut config: HostAudioOutputConfig = context.config()?;
        if config.audio_output_target.is_none() && !DEFAULT_TARGET.is_empty() {
            config.audio_output_target = Some(DEFAULT_TARGET.to_string());
        }
        Ok(Self {
            config,
            playback_quit_tx: Arc::new(Mutex::new(None)),
        })
    }
}

#[async_trait]
impl<const DEFAULT_TARGET: &'static str> AudioOutputPlugin
    for HostAudioOutputPlugin<DEFAULT_TARGET>
{
    async fn start(
        &self,
        rx: synapto_interface::sync::mpsc::Receiver<
            synapto_interface::cognitive_output_audio::CognitiveOutputAudio,
        >,
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

impl<const DEFAULT_TARGET: &'static str> Drop for HostAudioOutputPlugin<DEFAULT_TARGET> {
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
