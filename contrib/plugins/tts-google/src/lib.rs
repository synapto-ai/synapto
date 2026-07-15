//! # Google Text-to-Speech (TTS) Plugin
//!
//! Provides a high-fidelity Text-to-Speech (TTS) engine integration using Google Cloud Text-to-Speech API.
//!
//! ## Provided Plugins
//!
//! - `TtsGooglePlugin`: Connects to Google's Cloud Text-to-Speech API, handling speech synthesis requests, text normalization, shouting fixes, and robust XML/SSML escaping.

use synapto_interface::cognitive_output_audio::types::CognitiveOutputAudio;
use synapto_interface::sync::mpsc;
use synapto_interface::types::CognitiveOutputSpeech;
use synapto_interface::{Plugin, TTSPlugin};
use async_trait::async_trait;
use google_cloud_texttospeech_v1::{
    client::TextToSpeech,
    model::{
        AdvancedVoiceOptions, AudioConfig, AudioEncoding, SsmlVoiceGender, SynthesisInput,
        VoiceSelectionParams,
    },
};
use serde::Deserialize;
use tracing::{Instrument, info_span, instrument};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Deserialize, Clone, Debug, Default)]
pub struct GoogleServiceAccountCredentials(pub serde_json::Value);

#[derive(Deserialize, Clone, Debug, Default)]
pub struct GoogleTtsConfig {
    /// Google service account credentials (standard service_account JSON key format).
    pub google_service_account_credentials: GoogleServiceAccountCredentials,
    /// BCP-47 language code of the voice (e.g., "cs-CZ", "en-US").
    pub language_code: String,
    /// Exact voice name to use (e.g., "cs-CZ-Wavenet-A", "cs-CZ-Chirp3-HD-Schedar").
    pub voice_name: String,
    /// Gender of the voice ("MALE", "FEMALE", or "NEUTRAL").
    pub voice_gender: String,
    /// Whether to relax safety filters for speech synthesis.
    #[serde(default)]
    pub relax_safety_filters: bool,
}

#[derive(Deserialize)]
pub struct TtsGooglePlugin {
    #[serde(default)]
    config: GoogleTtsConfig,
}

#[async_trait::async_trait]
impl Plugin for TtsGooglePlugin {
    fn register<R: synapto_interface::PluginRegistry + ?Sized>(
        self: std::sync::Arc<Self>,
        registry: &mut R,
    ) where
        Self: Sized,
    {
        registry.register_tts(self);
    }

    async fn create(context: &synapto_interface::plugin::PluginInitContext<'_>) -> Result<Self, String> {
        let config: GoogleTtsConfig = context.config()?;
        Ok(Self { config })
    }
}

#[async_trait]
impl TTSPlugin for TtsGooglePlugin {
    async fn start(
        &self,
        ai_speech_rx: synapto_interface::sync::broadcast::Receiver<CognitiveOutputSpeech>,
        cognitive_output_audio_tx: mpsc::Sender<CognitiveOutputAudio>,
    ) -> Result<(), String> {
        run_google_tts(self.config.clone(), ai_speech_rx, cognitive_output_audio_tx).await
    }
}

fn escape_xml(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

fn normalize(text: &str) -> String {
    let fixed = fix_shouting(text).replace("`", "'");
    format!(
        "<speak><prosody rate=\"fast\">{}</prosody></speak>",
        escape_xml(&fixed)
    )
}

fn fix_shouting(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    for sentence in text.split_sentence_bounds() {
        let mut first_word_seen = false;
        for word in sentence.split_word_bounds() {
            let is_word = word.chars().any(|c| c.is_alphabetic());
            if is_word {
                if !first_word_seen {
                    result.push_str(word);
                    first_word_seen = true;
                } else {
                    let is_uppercased = word
                        .chars()
                        .filter(|c| c.is_alphabetic())
                        .all(|c| c.is_uppercase());
                    if is_uppercased && word != "I" {
                        result.push_str(&word.to_lowercase());
                    } else {
                        result.push_str(word);
                    }
                }
            } else {
                result.push_str(word);
            }
        }
    }
    result
}

#[instrument(skip_all)]
async fn run_google_tts(
    config: GoogleTtsConfig,
    mut ai_speech_rx: synapto_interface::sync::broadcast::Receiver<CognitiveOutputSpeech>,
    cognitive_output_audio_tx: mpsc::Sender<CognitiveOutputAudio>,
) -> Result<(), String> {
    let json_value = serde_json::to_value(&config.google_service_account_credentials.0)
        .map_err(|e| format!("Failed to serialize credentials: {e}"))?;
    let creds = google_cloud_auth::credentials::service_account::Builder::new(json_value)
        .build()
        .map_err(|e| format!("Failed to build Google credentials: {e}"))?;

    let text_to_speech_client = TextToSpeech::builder()
        .with_credentials(creds)
        .build()
        .await
        .map_err(|e| format!("Failed to create Google TTS client: {e}"))?;

    let prepared_response = text_to_speech_client
        .synthesize_speech()
        .set_audio_config(
            AudioConfig::new()
                .set_audio_encoding(AudioEncoding::OggOpus)
                .set_sample_rate_hertz(16_000),
        )
        .set_voice(
            VoiceSelectionParams::new()
                .set_name(config.voice_name)
                .set_ssml_gender(SsmlVoiceGender::from(config.voice_gender.as_str()))
                .set_language_code(config.language_code),
        )
        .set_advanced_voice_options(
            AdvancedVoiceOptions::default().set_relax_safety_filters(config.relax_safety_filters),
        );

    loop {
        match ai_speech_rx.recv().await {
            Ok(text) => {
                match prepared_response
                    .clone()
                    .set_input(SynthesisInput::new().set_ssml(normalize(text.text.as_str())))
                    .send()
                    .instrument(info_span!("Google TTS"))
                    .await
                {
                    Ok(response) => {
                        if let Err(e) = cognitive_output_audio_tx
                            .send(CognitiveOutputAudio(response.audio_content.to_vec()))
                            .await
                        {
                            tracing::error!("Failed to send output audio: {:?}", e);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Google TTS error: {:?}", e);
                    }
                }
            }
            Err(synapto_interface::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(synapto_interface::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
    Ok(())
}
