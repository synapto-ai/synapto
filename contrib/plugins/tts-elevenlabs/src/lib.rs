use synapto_interface::cognitive_output_audio::types::CognitiveOutputAudio;
use synapto_interface::sync::mpsc;
use synapto_interface::types::CognitiveOutputSpeech;
use synapto_interface::{Plugin, TTSPlugin};
use async_trait::async_trait;
use elevenlabs_sdk::{
    ClientConfig, ElevenLabsClient,
    types::{OutputFormat, TextToSpeechRequest, VoiceSettings},
};
use serde::{Deserialize, Serialize};
use tracing::{Instrument, info_span};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ElevenLabsTtsConfig {
    pub elevenlabs_api_key: String,
    pub voice_id: String,
    pub model_id: Option<String>,
    pub voice_settings: Option<VoiceSettings>,
    pub language_code: Option<String>,
}

pub struct TtsElevenLabsPlugin {
    config: ElevenLabsTtsConfig,
}

#[async_trait]
impl Plugin for TtsElevenLabsPlugin {
    fn register<R: synapto_interface::PluginRegistry + ?Sized>(
        self: std::sync::Arc<Self>,
        registry: &mut R,
    ) where
        Self: Sized,
    {
        registry.register_tts(self);
    }

    async fn create(context: synapto_interface::types::PluginContext) -> Result<Self, String> {
        let config: ElevenLabsTtsConfig = context.config()?;
        Ok(Self { config })
    }
}

#[async_trait]
impl TTSPlugin for TtsElevenLabsPlugin {
    async fn start(
        &self,
        ai_speech_rx: synapto_interface::sync::broadcast::Receiver<CognitiveOutputSpeech>,
        cognitive_output_audio_tx: mpsc::Sender<CognitiveOutputAudio>,
    ) -> Result<(), String> {
        run_elevenlabs_tts(self.config.clone(), ai_speech_rx, cognitive_output_audio_tx).await;
        Ok(())
    }
}

async fn run_elevenlabs_tts(
    config: ElevenLabsTtsConfig,
    mut ai_speech_rx: synapto_interface::sync::broadcast::Receiver<CognitiveOutputSpeech>,
    cognitive_output_audio_tx: mpsc::Sender<CognitiveOutputAudio>,
) {
    let elevenlabs_config = ClientConfig::builder(config.elevenlabs_api_key).build();
    let client = ElevenLabsClient::new(elevenlabs_config).unwrap_or_else(|e| panic!("Error: {:?}", e));

    let model_id = config
        .model_id
        .unwrap_or_else(|| "eleven_multilingual_v2".to_string());
    let language_code = config.language_code.unwrap_or_else(|| "cs".to_string());

    loop {
        match ai_speech_rx.recv().await {
            Ok(text) => {
                let mut request = TextToSpeechRequest::new(text.text);
                request.model_id = Some(model_id.clone());
                request.language_code = Some(language_code.clone());
                request.voice_settings = config.voice_settings.clone();
                request.apply_text_normalization =
                    Some(elevenlabs_sdk::types::TextNormalization::On);

                match client
                    .text_to_speech()
                    .convert(
                        &config.voice_id,
                        &request,
                        Some(OutputFormat::Opus_48000_96),
                        Some(3),
                    )
                    .instrument(info_span!("ElevenLabs TTS"))
                    .await
                {
                    Ok(audio) => {
                        if let Err(e) = cognitive_output_audio_tx.send(CognitiveOutputAudio(audio.to_vec())).await
                        {
                            tracing::error!("Failed to send output audio: {:?}", e);
                        }
                    }
                    Err(e) => {
                        tracing::error!("ElevenLabs TTS error: {:?}", e);
                    }
                }
            }
            Err(synapto_interface::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(synapto_interface::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}
