use async_trait::async_trait;
use data_encoding::BASE64;
use serde::Deserialize;
use synapto_credentials_google::GoogleCloudTarget;
use synapto_interface::cognitive::CognitiveOutputSpeech;
use synapto_interface::cognitive_output_audio::CognitiveOutputAudio;
use synapto_interface::credentials::CredentialsHandle;
use synapto_interface::plugin::Plugin;
use synapto_interface::speech_to_text::TTSPlugin;
use synapto_interface::sync::{broadcast, mpsc};
use tracing::{Instrument, info_span, instrument};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Deserialize, Clone, Debug, Default)]
pub struct GoogleTtsConfig {
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

pub struct TtsGooglePlugin {
    config: GoogleTtsConfig,
    credentials: CredentialsHandle,
}

#[async_trait::async_trait]
impl Plugin for TtsGooglePlugin {
    fn register<R: synapto_interface::plugin::PluginRegistry + ?Sized>(
        self: std::sync::Arc<Self>,
        registry: &mut R,
    ) where
        Self: Sized,
    {
        registry.register_tts(self);
    }

    async fn create(
        context: &synapto_interface::plugin::PluginInitContext<'_>,
    ) -> Result<Self, String> {
        let config: GoogleTtsConfig = context.config()?;
        Ok(Self {
            config,
            credentials: context.credentials(),
        })
    }
}

#[async_trait]
impl TTSPlugin for TtsGooglePlugin {
    async fn start(
        &self,
        cognitive_speech_rx: broadcast::Receiver<CognitiveOutputSpeech>,
        cognitive_output_audio_tx: mpsc::Sender<CognitiveOutputAudio>,
    ) -> Result<(), String> {
        run_google_tts(
            self.config.clone(),
            self.credentials.clone(),
            cognitive_speech_rx,
            cognitive_output_audio_tx,
        )
        .await
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
    let fixed = fix_shouting(text).replace('`', "'");
    format!(
        "<speak><prosody rate=\"120%\">{}</prosody></speak>",
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
    credentials: CredentialsHandle,
    mut cognitive_speech_rx: broadcast::Receiver<CognitiveOutputSpeech>,
    cognitive_output_audio_tx: mpsc::Sender<CognitiveOutputAudio>,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let target = GoogleCloudTarget {
        scopes: vec!["https://www.googleapis.com/auth/cloud-platform".to_string()],
    };
    let url = "https://texttospeech.googleapis.com/v1/text:synthesize";

    loop {
        match cognitive_speech_rx.recv().await {
            Ok(text) => {
                let token = match credentials.resolve_bearer_token(&target).await {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::error!("Failed to resolve bearer token for Google TTS: {e}");
                        continue;
                    }
                };

                let request_body = serde_json::json!({
                    "input": {
                        "ssml": normalize(text.text.as_str())
                    },
                    "voice": {
                        "languageCode": config.language_code,
                        "name": config.voice_name,
                        "ssmlGender": config.voice_gender
                    },
                    "audioConfig": {
                        "audioEncoding": "OGG_OPUS",
                        "sampleRateHertz": 16000
                    },
                    "advancedVoiceOptions": {
                        "relaxSafetyFilters": config.relax_safety_filters
                    }
                });

                let response = client
                    .post(url)
                    .bearer_auth(token.expose_secret())
                    .json(&request_body)
                    .send()
                    .instrument(info_span!("Google TTS"))
                    .await;

                match response {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            tracing::error!("Google TTS API returned HTTP {}", resp.status());
                            continue;
                        }
                        match resp.json::<serde_json::Value>().await {
                            Ok(json) => {
                                if let Some(audio_b64) = json["audioContent"].as_str() {
                                    match BASE64.decode(audio_b64.as_bytes()) {
                                        Ok(audio_bytes) => {
                                            if let Err(e) = cognitive_output_audio_tx
                                                .send(CognitiveOutputAudio(audio_bytes))
                                                .await
                                            {
                                                tracing::error!(
                                                    "Failed to send output audio: {:?}",
                                                    e
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            tracing::error!(
                                                "Failed to decode base64 audio: {:?}",
                                                e
                                            );
                                        }
                                    }
                                } else {
                                    tracing::error!("Missing audioContent in TTS response");
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to parse TTS JSON response: {:?}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("Google TTS request error: {:?}", e);
                    }
                }
            }
            Err(synapto_interface::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(synapto_interface::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
    Ok(())
}
