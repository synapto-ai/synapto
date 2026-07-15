use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;
use synapto_interface::speech_to_text::types::{
    InputVoiceAudio, SpeechDetected, SpeechTranscript, Word,
};
use synapto_interface::sync::mpsc;
use synapto_interface::{Plugin, STTPlugin};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::client::generate_key;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::Message;
use url::Url;

const PEER_INPUT_AUDIO_SAMPLE_RATE: usize = 16_000;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ElevenLabsSttConfig {
    pub elevenlabs_api_key: String,
    pub language_code: Option<String>,
}

pub struct SttElevenLabsPlugin {
    config: ElevenLabsSttConfig,
}

#[async_trait]
impl Plugin for SttElevenLabsPlugin {
    fn register<R: synapto_interface::PluginRegistry + ?Sized>(
        self: std::sync::Arc<Self>,
        registry: &mut R,
    ) where
        Self: Sized,
    {
        registry.register_stt(self);
    }

    async fn create(context: &synapto_interface::plugin::PluginInitContext<'_>) -> Result<Self, String> {
        let config: ElevenLabsSttConfig = context.config()?;
        Ok(Self { config })
    }
}

#[async_trait]
impl STTPlugin for SttElevenLabsPlugin {
    async fn start(
        &self,
        audio_rx: mpsc::Receiver<InputVoiceAudio>,
        transcript_tx: mpsc::Sender<SpeechTranscript>,
        speech_detected: SpeechDetected,
    ) -> Result<(), String> {
        run_elevenlabs(
            self.config.clone(),
            audio_rx,
            transcript_tx,
            speech_detected,
        )
        .await;
        Ok(())
    }
}

async fn run_elevenlabs(
    config: ElevenLabsSttConfig,
    mut audio_rx: mpsc::Receiver<InputVoiceAudio>,
    transcript_tx: mpsc::Sender<SpeechTranscript>,
    speech_detected: SpeechDetected,
) {
    if let Err(e) = rustls::crypto::ring::default_provider().install_default() {
        tracing::trace!(
            "Rustls default provider already installed or failed to install: {:?}",
            e
        );
    }

    loop {
        let first_voice = loop {
            match audio_rx.recv().await {
                Some(InputVoiceAudio::Voice(audio)) => break audio,
                Some(_) => {}
                None => return,
            }
        };

        let base_index = first_voice.index;
        let url = Url::parse(&format!(
            "wss://api.elevenlabs.io/v1/speech-to-text/realtime?model_id=scribe_v2_realtime&commit_strategy=vad&vad_threshold=0.1&language_code={}&include_timestamps=true",
            config.language_code.as_deref().unwrap_or("cs")
        ))
        .unwrap_or_else(|e| panic!("Invalid URL: {:?}", e));

        let mut req = url
            .into_client_request()
            .unwrap_or_else(|e| panic!("Error: {:?}", e));
        req.headers_mut().insert(
            "Sec-WebSocket-Key",
            HeaderValue::from_str(&generate_key()).unwrap_or_else(|e| panic!("Error: {:?}", e)),
        );
        if let Ok(key_val) = HeaderValue::from_str(&config.elevenlabs_api_key) {
            req.headers_mut().insert("xi-api-key", key_val);
        }

        let (ws_stream, _) = match connect_async(req).await {
            Ok(res) => res,
            Err(e) => {
                tracing::error!("Failed to connect to ElevenLabs: {:?}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let (mut write_ws, mut read_ws) = ws_stream.split();

        // Initial handshake
        if let Some(msg_result) = read_ws.next().await {
            match msg_result {
                Ok(Message::Text(text)) => {
                    if let Ok(response) = serde_json::from_str::<serde_json::Value>(&text) {
                        let msg_type = response
                            .get("message_type")
                            .or(response.get("event"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if msg_type == "session_started" {
                            tracing::debug!("ElevenLabs session started: {:?}", response);
                        } else {
                            tracing::error!("Unexpected message from ElevenLabs: {:?}", response);
                            continue;
                        }
                    }
                }
                _ => continue,
            }
        }

        let first_chunk = json!({
            "message_type": "input_audio_chunk",
            "audio_base_64": data_encoding::BASE64.encode(<Vec<u8>>::from(first_voice.audio).as_ref()),
            "sample_rate": PEER_INPUT_AUDIO_SAMPLE_RATE
        });

        if write_ws
            .send(Message::Text(first_chunk.to_string().into()))
            .await
            .is_err()
        {
            continue;
        }

        loop {
            better_tokio_select::tokio_select!(match .. {
                .. if let audio_opt = audio_rx.recv() => {
                    match audio_opt {
                        Some(InputVoiceAudio::Voice(audio_chunk))
                        | Some(InputVoiceAudio::NoVoice(audio_chunk)) => {
                            let msg = json!({
                                "message_type": "input_audio_chunk",
                                "audio_base_64": data_encoding::BASE64.encode(<Vec<u8>>::from(audio_chunk.audio).as_ref()),
                                "sample_rate": PEER_INPUT_AUDIO_SAMPLE_RATE
                            });
                            if write_ws
                                .send(Message::Text(msg.to_string().into()))
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        None => return,
                    }
                }
                .. if let Some(msg_result) = read_ws.next() => {
                    match msg_result {
                        Ok(Message::Text(text)) => {
                            if let Ok(response) = serde_json::from_str::<serde_json::Value>(&text) {
                                let msg_type = response
                                    .get("message_type")
                                    .or(response.get("event"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("");
                                if msg_type == "partial_transcript" {
                                    speech_detected.notify();
                                } else if msg_type == "committed_transcript_with_timestamps" {
                                    let transcript = response
                                        .get("text")
                                        .or(response.get("transcript"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .trim()
                                        .to_string();
                                    if let Some(words) =
                                        response.get("words").and_then(|v| v.as_array())
                                    {
                                        let mut mapped_words = Vec::new();
                                        for word_val in words {
                                            if let Some(word_obj) = word_val.as_object() {
                                                let word_str = match word_obj
                                                    .get("text")
                                                    .or(word_obj.get("word"))
                                                    .and_then(|v| v.as_str())
                                                {
                                                    Some(" ") | None => continue,
                                                    Some(w) => w.to_string(),
                                                };
                                                let (start_index, end_index) = if let (
                                                    Some(s),
                                                    Some(e),
                                                ) = (
                                                    word_obj
                                                        .get("start")
                                                        .or(word_obj.get("start_time"))
                                                        .and_then(|v| v.as_f64()),
                                                    word_obj
                                                        .get("end")
                                                        .or(word_obj.get("end_time"))
                                                        .and_then(|v| v.as_f64()),
                                                ) {
                                                    let (si, ei) = synapto_interface::speech_to_text::types::calculate_chunk_indices(base_index, s, e);
                                                    (Some(si), Some(ei))
                                                } else {
                                                    (None, None)
                                                };
                                                mapped_words.push(Word {
                                                    start_index,
                                                    end_index,
                                                    word: word_str,
                                                    speaker_hint: None,
                                                });
                                            }
                                        }
                                        if let Err(e) = transcript_tx
                                            .send(SpeechTranscript {
                                                start_index: mapped_words
                                                    .first()
                                                    .and_then(|w| w.start_index)
                                                    .unwrap_or(base_index),
                                                end_index: mapped_words
                                                    .last()
                                                    .and_then(|w| w.end_index)
                                                    .unwrap_or(base_index),
                                                transcript,
                                                words: mapped_words,
                                            })
                                            .await
                                        {
                                            tracing::error!("Failed to send transcript: {:?}", e);
                                        }
                                    }
                                }
                            }
                        }
                        Ok(Message::Close(_)) | Err(_) => break,
                        _ => {}
                    }
                }
            })
        }
    }
}
