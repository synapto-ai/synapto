use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
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

#[derive(Deserialize, Debug)]
struct SmResponse {
    message: String,
    results: Option<Vec<SmResult>>,
}

#[derive(Deserialize, Debug, Clone)]
struct SmResult {
    alternatives: Option<Vec<SmAlternative>>,
    #[serde(rename = "type")]
    item_type: String,
    start_time: Option<f64>,
    end_time: Option<f64>,
}

#[derive(Deserialize, Debug, Clone)]
struct SmAlternative {
    content: String,
    speaker: Option<String>,
}

#[derive(Clone, Debug)]
struct InternalResult {
    speaker: Option<String>,
    content: String,
    item_type: String,
    start_time: Option<f64>,
    end_time: Option<f64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SpeechmaticsConfig {
    pub speechmatics_api_key: String,
    pub language_code: Option<String>,
}

pub struct SttSpeechmaticsPlugin {
    config: SpeechmaticsConfig,
}

#[async_trait]
#[async_trait::async_trait]
impl Plugin for SttSpeechmaticsPlugin {
    fn register<R: synapto_interface::PluginRegistry + ?Sized>(
        self: std::sync::Arc<Self>,
        registry: &mut R,
    ) where
        Self: Sized,
    {
        registry.register_stt(self);
    }

    async fn create(context: synapto_interface::types::PluginContext) -> Result<Self, String> {
        let config: SpeechmaticsConfig = context.config()?;
        Ok(Self { config })
    }
}

#[async_trait]
impl STTPlugin for SttSpeechmaticsPlugin {
    async fn start(
        &self,
        audio_rx: mpsc::Receiver<InputVoiceAudio>,
        transcript_tx: mpsc::Sender<SpeechTranscript>,
        speech_detected: SpeechDetected,
    ) -> Result<(), String> {
        run_speechmatics(
            self.config.clone(),
            audio_rx,
            transcript_tx,
            speech_detected,
        )
        .await;
        Ok(())
    }
}

async fn run_speechmatics(
    config: SpeechmaticsConfig,
    mut audio_rx: mpsc::Receiver<InputVoiceAudio>,
    transcript_tx: mpsc::Sender<SpeechTranscript>,
    speech_detected: SpeechDetected,
) {
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
            "wss://eu2.rt.speechmatics.com/v2/{}",
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
        req.headers_mut().insert(
            "Authorization",
            HeaderValue::from_str(&format!("Bearer {}", config.speechmatics_api_key))
                .unwrap_or_else(|e| panic!("Error: {:?}", e)),
        );

        let (ws_stream, _) = match connect_async(req).await {
            Ok(res) => res,
            Err(e) => {
                tracing::error!("Failed to connect to Speechmatics: {:?}", e);
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        let (mut write_ws, mut read_ws) = ws_stream.split();
        let config_msg = json!({
            "message": "StartRecognition",
            "audio_format": { "type": "raw", "encoding": "pcm_s16le", "sample_rate": PEER_INPUT_AUDIO_SAMPLE_RATE },
            "transcription_config": {
                "language": config.language_code.as_deref().unwrap_or("cs"),
                "diarization": "speaker",
                "enable_partials": false,
                "operating_point": "enhanced",
                "max_delay": 4.0,
                "transcript_filtering_config": { "remove_disfluencies": true },
                "conversation_config": { "end_of_utterance_silence_trigger": 1.85 },
            }
        });

        if write_ws
            .send(Message::Text(config_msg.to_string().into()))
            .await
            .is_err()
        {
            continue;
        }
        if write_ws
            .send(Message::Binary(<Vec<u8>>::from(first_voice.audio).into()))
            .await
            .is_err()
        {
            continue;
        }

        let mut results_buffer: Vec<InternalResult> = Vec::new();

        loop {
            better_tokio_select::tokio_select!(match .. {
                .. if let audio_opt = audio_rx.recv() => {
                    match audio_opt {
                        Some(InputVoiceAudio::Voice(audio_chunk))
                        | Some(InputVoiceAudio::NoVoice(audio_chunk)) => {
                            if write_ws
                                .send(Message::Binary(<Vec<u8>>::from(audio_chunk.audio).into()))
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
                            if let Ok(response) = serde_json::from_str::<SmResponse>(&text) {
                                if response.message == "AddTranscript" {
                                    if let Some(results) = response.results {
                                        for item in results {
                                            if let Some(alts) = item.alternatives
                                                && let Some(best_alt) = alts.first()
                                            {
                                                let speech_speaker = best_alt.speaker.clone();
                                                if let Some(last_item) = results_buffer.last()
                                                    && last_item.speaker != speech_speaker
                                                {
                                                    emit_transcript(
                                                        base_index,
                                                        &results_buffer,
                                                        &transcript_tx,
                                                    )
                                                    .await;
                                                    results_buffer.clear();
                                                }
                                                results_buffer.push(InternalResult {
                                                    speaker: speech_speaker,
                                                    content: best_alt.content.clone(),
                                                    item_type: item.item_type.clone(),
                                                    start_time: item.start_time,
                                                    end_time: item.end_time,
                                                });
                                                speech_detected.notify();
                                            }
                                        }
                                    }
                                } else if response.message == "EndOfUtterance"
                                    && !results_buffer.is_empty()
                                {
                                    emit_transcript(base_index, &results_buffer, &transcript_tx)
                                        .await;
                                    results_buffer.clear();
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

async fn emit_transcript(
    base_index: u64,
    buffer: &[InternalResult],
    tx: &mpsc::Sender<SpeechTranscript>,
) {
    let mut words = Vec::new();
    let mut transcript_text = String::new();
    for item in buffer {
        if !transcript_text.is_empty() && item.item_type == "word" {
            transcript_text.push(' ');
        }
        transcript_text.push_str(&item.content);
        let (start_index, end_index) = if let (Some(s), Some(e)) = (item.start_time, item.end_time)
        {
            let (si, ei) =
                synapto_interface::speech_to_text::types::calculate_chunk_indices(base_index, s, e);
            (Some(si), Some(ei))
        } else {
            (None, None)
        };
        words.push(Word {
            start_index,
            end_index,
            word: item.content.clone(),
            speaker_hint: item.speaker.clone(),
        });
    }
    if let Err(e) = tx
        .send(SpeechTranscript {
            start_index: words
                .first()
                .and_then(|w| w.start_index)
                .unwrap_or(base_index),
            end_index: words.last().and_then(|w| w.end_index).unwrap_or(base_index),
            transcript: transcript_text.trim().to_string(),
            words,
        })
        .await
    {
        tracing::error!("Failed to send transcript: {:?}", e);
    }
}
