use async_trait::async_trait;
use gcp_auth::TokenProvider;
use serde::Deserialize;
use std::time::Duration;
use synapto_interface::speech_to_text::{
    InputVoiceAudio, PeerInputAudioIndexed, SpeechDetected, SpeechTranscript, Word,
};
use synapto_interface::sync::{mpsc, watch};
use synapto_interface::plugin::Plugin;
use synapto_interface::speech_to_text::STTPlugin;
use tokio_stream::StreamExt;

use googleapis_tonic_google_cloud_speech_v1::google::cloud::speech::v1::recognition_config::AudioEncoding as AudioEncodingV1;
use googleapis_tonic_google_cloud_speech_v1::google::cloud::speech::v1::speech_client::SpeechClient as SpeechClientV1;
use googleapis_tonic_google_cloud_speech_v1::google::cloud::speech::v1::streaming_recognize_request::StreamingRequest as StreamingRequestV1;
use googleapis_tonic_google_cloud_speech_v1::google::cloud::speech::v1::{
    RecognitionConfig as RecognitionConfigV1,
    StreamingRecognitionConfig as StreamingRecognitionConfigV1,
    StreamingRecognizeRequest as StreamingRecognizeRequestV1,
};

struct StreamingRecognizeRequestV1Wrapped(StreamingRecognizeRequestV1);

struct StreamingRecognizeRequestV2Wrapped(StreamingRecognizeRequestV2);
use googleapis_tonic_google_cloud_speech_v2::google::cloud::speech::v2::explicit_decoding_config::AudioEncoding as AudioEncodingV2;
use googleapis_tonic_google_cloud_speech_v2::google::cloud::speech::v2::recognition_config as recognition_config_v2;
use googleapis_tonic_google_cloud_speech_v2::google::cloud::speech::v2::speech_client::SpeechClient as SpeechClientV2;
use googleapis_tonic_google_cloud_speech_v2::google::cloud::speech::v2::streaming_recognize_request::StreamingRequest as StreamingRequestV2;
use googleapis_tonic_google_cloud_speech_v2::google::cloud::speech::v2::{
    ExplicitDecodingConfig as ExplicitDecodingConfigV2, RecognitionConfig as RecognitionConfigV2,
    RecognitionFeatures as RecognitionFeaturesV2,
    StreamingRecognitionConfig as StreamingRecognitionConfigV2,
    StreamingRecognitionFeatures as StreamingRecognitionFeaturesV2,
    StreamingRecognizeRequest as StreamingRecognizeRequestV2,
};

use tonic::transport::ClientTlsConfig;

const SCOPES: &[&str; 1] = &["https://www.googleapis.com/auth/cloud-platform"];

#[derive(Deserialize, Clone, Debug, Default)]
pub enum GoogleSttVersion {
    #[default]
    V1,
    V2,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct GoogleServiceAccountCredentials(serde_json::Value);

impl From<GoogleServiceAccountCredentials> for String {
    fn from(value: GoogleServiceAccountCredentials) -> Self {
        serde_json::to_string(&value.0).unwrap_or_else(|e| panic!("Failed to serialize: {:?}", e))
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct GoogleSttConfig {
    #[serde(default)]
    pub version: GoogleSttVersion,
    pub google_project_id: String,
    pub google_service_account_credentials: GoogleServiceAccountCredentials,
    pub language_code: Option<String>,
}

pub struct SttGooglePlugin {
    config: GoogleSttConfig,
}

#[async_trait::async_trait]
impl Plugin for SttGooglePlugin {
    fn register<R: synapto_interface::plugin::PluginRegistry + ?Sized>(
        self: std::sync::Arc<Self>,
        registry: &mut R,
    ) where
        Self: Sized,
    {
        registry.register_stt(self);
    }

    async fn create(context: &synapto_interface::plugin::PluginInitContext<'_>) -> Result<Self, String> {
        let config: GoogleSttConfig = context.config()?;
        Ok(Self { config })
    }
}

#[async_trait]
impl STTPlugin for SttGooglePlugin {
    async fn start(
        &self,
        audio_rx: mpsc::Receiver<InputVoiceAudio>,
        transcript_tx: mpsc::Sender<SpeechTranscript>,
        speech_detected: SpeechDetected,
    ) -> Result<(), String> {
        match self.config.version {
            GoogleSttVersion::V1 => {
                run_v1(
                    self.config.clone(),
                    audio_rx,
                    transcript_tx,
                    speech_detected,
                )
                .await;
            }
            GoogleSttVersion::V2 => {
                run_v2(
                    self.config.clone(),
                    audio_rx,
                    transcript_tx,
                    speech_detected,
                )
                .await;
            }
        }
        Ok(())
    }
}

async fn run_v1(
    config: GoogleSttConfig,
    mut audio_rx: mpsc::Receiver<InputVoiceAudio>,
    transcript_tx: mpsc::Sender<SpeechTranscript>,
    speech_detected: SpeechDetected,
) {
    let url = "https://speech.googleapis.com".to_string();
    let account = match gcp_auth::CustomServiceAccount::from_json(&String::from(
        config.google_service_account_credentials,
    )) {
        Ok(acc) => acc,
        Err(e) => {
            tracing::error!("Failed to load Google service account credentials: {}", e);
            return;
        }
    };

    let streaming_config = StreamingRecognitionConfigV1 {
        config: Some(RecognitionConfigV1 {
            encoding: AudioEncodingV1::Linear16 as i32,
            sample_rate_hertz: 16000,
            audio_channel_count: 1,
            enable_separate_recognition_per_channel: false,
            language_code: config.language_code.unwrap_or_else(|| "cs-CZ".to_string()),
            max_alternatives: 1,
            profanity_filter: false,
            speech_contexts: vec![],
            enable_word_time_offsets: true,
            enable_automatic_punctuation: true,
            diarization_config: None,
            metadata: None,
            model: "latest_long".to_string(),
            use_enhanced: false,
            ..Default::default()
        }),
        single_utterance: false,
        interim_results: true,
        ..Default::default()
    };

    let mut pending_audio: Vec<PeerInputAudioIndexed> = Vec::new();

    loop {
        let tls_config = ClientTlsConfig::new().with_native_roots();
        let channel = match tonic::transport::Channel::from_shared(url.clone()) {
            Ok(chan) => match chan.tls_config(tls_config) {
                Ok(chan) => match chan.connect().await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::warn!(
                            "Failed to connect to Google Speech V1 API: {}. Retrying...",
                            e
                        );
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        continue;
                    }
                },
                Err(e) => {
                    tracing::error!("Failed to configure TLS for Google Speech API: {}", e);
                    return;
                }
            },
            Err(e) => {
                tracing::error!("Failed to create channel for Google Speech API: {}", e);
                return;
            }
        };

        let token = match account.token(SCOPES).await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("Failed to get GCP token: {}. Retrying...", e);
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };

        let mut client =
            SpeechClientV1::with_interceptor(channel, move |mut req: tonic::Request<()>| {
                req.metadata_mut().insert(
                    "authorization",
                    format!("Bearer {}", token.as_str())
                        .parse()
                        .unwrap_or_else(|e| panic!("Failed to parse: {:?}", e)),
                );
                Ok(req)
            });

        if pending_audio.is_empty() {
            loop {
                match audio_rx.recv().await {
                    Some(InputVoiceAudio::Voice(audio)) => {
                        pending_audio.push(audio);
                        break;
                    }
                    Some(_) => {}
                    None => return,
                }
            }
        }

        let base_index = pending_audio.first().map(|v| v.index).unwrap_or(0);
        let (is_pending_tx, is_pending_rx) = watch::channel(true);
        let (tx, rx) = mpsc::channel::<StreamingRecognizeRequestV1Wrapped>(100);
        let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(|v| v.0);

        if tx
            .send(StreamingRecognizeRequestV1Wrapped(
                StreamingRecognizeRequestV1 {
                    streaming_request: Some(StreamingRequestV1::StreamingConfig(
                        streaming_config.clone(),
                    )),
                },
            ))
            .await
            .is_err()
        {
            continue;
        }

        let speech_detected_clone = speech_detected.clone();
        let transcript_tx_clone = transcript_tx.clone();
        let receive_side = async move {
            if let Ok(response) = client.streaming_recognize(request_stream).await {
                let mut response_stream = response.into_inner();
                while let Ok(Some(reco_result)) = response_stream.message().await {
                    let mut all_is_final = true;
                    for result in reco_result.results {
                        if result.is_final {
                            if let Some(alt) = result.alternatives.first() {
                                let mut words = Vec::new();
                                for w in &alt.words {
                                    if let (Some(s), Some(e)) =
                                        (w.start_time.as_ref(), w.end_time.as_ref())
                                    {
                                        let (si, ei) = synapto_interface::speech_to_text::calculate_chunk_indices(base_index, s.seconds as f64 + s.nanos as f64 / 1e9, e.seconds as f64 + e.nanos as f64 / 1e9);
                                        words.push(Word {
                                            start_index: Some(si),
                                            end_index: Some(ei),
                                            word: w.word.clone(),
                                            speaker_hint: None,
                                        });
                                    }
                                }
                                if let Err(e) = transcript_tx_clone
                                    .send(SpeechTranscript {
                                        start_index: words
                                            .first()
                                            .and_then(|w| w.start_index)
                                            .unwrap_or(base_index),
                                        end_index: words
                                            .last()
                                            .and_then(|w| w.end_index)
                                            .unwrap_or(base_index),
                                        transcript: alt.transcript.clone(),
                                        words,
                                    })
                                    .await
                                {
                                    tracing::error!("Failed to send transcript: {:?}", e);
                                }
                            }
                        } else {
                            all_is_final = false;
                            speech_detected_clone.notify();
                        }
                    }
                    if let Err(e) = is_pending_tx.send(!all_is_final) {
                        tracing::error!("Failed to update pending status: {:?}", e);
                    }
                }
            }
        };

        let mut receive_task = tokio::spawn(receive_side);
        better_tokio_select::tokio_select!(match .. {
            .. if let _ =
                audio_bridge_v1(&mut audio_rx, tx, &is_pending_rx, &mut pending_audio) =>
            {
                if let Err(e) = receive_task.await {
                    tracing::error!("Receive task error: {:?}", e);
                }
            }
            .. if let res = &mut receive_task => {
                if let Err(e) = res {
                    tracing::error!("Receive task error: {:?}", e);
                }
            }
        });
    }
}

async fn audio_bridge_v1(
    audio_rx: &mut mpsc::Receiver<InputVoiceAudio>,
    audio_sender: mpsc::Sender<StreamingRecognizeRequestV1Wrapped>,
    is_pending_rx: &watch::Receiver<bool>,
    pending_audio: &mut Vec<PeerInputAudioIndexed>,
) {
    for voice in pending_audio.iter() {
        if let Err(e) = audio_sender
            .send(StreamingRecognizeRequestV1Wrapped(
                StreamingRecognizeRequestV1 {
                    streaming_request: Some(StreamingRequestV1::AudioContent(
                        voice.audio.clone().into(),
                    )),
                },
            ))
            .await
        {
            tracing::error!("Failed to send audio content: {:?}", e);
        }
    }

    let mut is_pending_rx_clone = is_pending_rx.clone();
    loop {
        better_tokio_select::tokio_select!(match .. {
            .. if let res = is_pending_rx_clone.changed() => {
                if res.is_ok() && !*is_pending_rx_clone.borrow() {
                    pending_audio.clear();
                }
            }
            .. if let msg = audio_rx.recv() => {
                match msg {
                    Some(InputVoiceAudio::Voice(voice)) => {
                        pending_audio.push(voice.clone());
                        if audio_sender
                            .send(StreamingRecognizeRequestV1Wrapped(
                                StreamingRecognizeRequestV1 {
                                    streaming_request: Some(StreamingRequestV1::AudioContent(
                                        voice.audio.into(),
                                    )),
                                },
                            ))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some(InputVoiceAudio::NoVoice(voice)) => {
                        if !pending_audio.is_empty() {
                            pending_audio.push(voice.clone());
                        }
                        if audio_sender
                            .send(StreamingRecognizeRequestV1Wrapped(
                                StreamingRecognizeRequestV1 {
                                    streaming_request: Some(StreamingRequestV1::AudioContent(
                                        voice.audio.into(),
                                    )),
                                },
                            ))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    None => break,
                }
            }
        })
    }
}

async fn run_v2(
    config: GoogleSttConfig,
    mut audio_rx: mpsc::Receiver<InputVoiceAudio>,
    transcript_tx: mpsc::Sender<SpeechTranscript>,
    speech_detected: SpeechDetected,
) {
    let location = "eu";
    let recognizer_name = format!(
        "projects/{}/locations/{}/recognizers/_",
        config.google_project_id, location
    );
    let url = format!("https://{location}-speech.googleapis.com");
    let account = match gcp_auth::CustomServiceAccount::from_json(&String::from(
        config.google_service_account_credentials,
    )) {
        Ok(acc) => acc,
        Err(e) => {
            tracing::error!("Failed to load Google service account credentials: {}", e);
            return;
        }
    };

    let streaming_config_request = StreamingRecognizeRequestV2 {
        recognizer: recognizer_name.clone(),
        streaming_request: Some(StreamingRequestV2::StreamingConfig(
            StreamingRecognitionConfigV2 {
                config: Some(RecognitionConfigV2 {
                    model: "latest_long".to_string(),
                    language_codes: vec![
                        config.language_code.unwrap_or_else(|| "cs-CZ".to_string()),
                    ],
                    features: Some(RecognitionFeaturesV2 {
                        enable_automatic_punctuation: true,
                        enable_word_time_offsets: true,
                        max_alternatives: 1,
                        ..Default::default()
                    }),
                    decoding_config: Some(
                        recognition_config_v2::DecodingConfig::ExplicitDecodingConfig(
                            ExplicitDecodingConfigV2 {
                                encoding: AudioEncodingV2::Linear16 as i32,
                                sample_rate_hertz: 16000,
                                audio_channel_count: 1,
                            },
                        ),
                    ),
                    ..Default::default()
                }),
                streaming_features: Some(StreamingRecognitionFeaturesV2 {
                    interim_results: true,
                    ..Default::default()
                }),
                ..Default::default()
            },
        )),
    };

    let mut pending_audio: Vec<PeerInputAudioIndexed> = Vec::new();

    loop {
        let tls_config = ClientTlsConfig::new().with_native_roots();
        let channel = match tonic::transport::Channel::from_shared(url.clone()) {
            Ok(chan) => match chan.tls_config(tls_config) {
                Ok(chan) => match chan.connect().await {
                    Ok(c) => c,
                    Err(e) => {
                        tracing::error!(
                            "Failed to connect to Google Speech V2 API: {}. Retrying...",
                            e
                        );
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        continue;
                    }
                },
                Err(e) => {
                    tracing::error!("TLS error: {}", e);
                    return;
                }
            },
            Err(e) => {
                tracing::error!("Channel error: {}", e);
                return;
            }
        };

        let token = match account.token(SCOPES).await {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("Failed to get GCP token: {}. Retrying...", e);
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
        };

        let mut client =
            SpeechClientV2::with_interceptor(channel, move |mut req: tonic::Request<()>| {
                req.metadata_mut().insert(
                    "authorization",
                    format!("Bearer {}", token.as_str())
                        .parse()
                        .unwrap_or_else(|e| panic!("Failed to parse: {:?}", e)),
                );
                Ok(req)
            });

        if pending_audio.is_empty() {
            loop {
                match audio_rx.recv().await {
                    Some(InputVoiceAudio::Voice(audio)) => {
                        pending_audio.push(audio);
                        break;
                    }
                    Some(_) => {}
                    None => return,
                }
            }
        }

        let base_index = pending_audio.first().map(|v| v.index).unwrap_or(0);
        let (is_pending_tx, is_pending_rx) = watch::channel(true);
        let (tx, rx) = mpsc::channel::<StreamingRecognizeRequestV2Wrapped>(100);
        let request_stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(|v| v.0);

        if tx
            .send(StreamingRecognizeRequestV2Wrapped(
                streaming_config_request.clone(),
            ))
            .await
            .is_err()
        {
            continue;
        }

        let speech_detected_clone = speech_detected.clone();
        let transcript_tx_clone = transcript_tx.clone();
        let receive_side = async move {
            if let Ok(response) = client.streaming_recognize(request_stream).await {
                let mut response_stream = response.into_inner();
                while let Ok(Some(reco_result)) = response_stream.message().await {
                    let mut all_is_final = true;
                    for result in reco_result.results {
                        if result.is_final {
                            if let Some(alt) = result.alternatives.first() {
                                let mut words = Vec::new();
                                for w in &alt.words {
                                    if let (Some(s), Some(e)) =
                                        (w.start_offset.as_ref(), w.end_offset.as_ref())
                                    {
                                        let (si, ei) = synapto_interface::speech_to_text::calculate_chunk_indices(base_index, s.seconds as f64 + s.nanos as f64 / 1e9, e.seconds as f64 + e.nanos as f64 / 1e9);
                                        words.push(Word {
                                            start_index: Some(si),
                                            end_index: Some(ei),
                                            word: w.word.clone(),
                                            speaker_hint: None,
                                        });
                                    }
                                }
                                if let Err(e) = transcript_tx_clone
                                    .send(SpeechTranscript {
                                        start_index: words
                                            .first()
                                            .and_then(|w| w.start_index)
                                            .unwrap_or(base_index),
                                        end_index: words
                                            .last()
                                            .and_then(|w| w.end_index)
                                            .unwrap_or(base_index),
                                        transcript: alt.transcript.clone(),
                                        words,
                                    })
                                    .await
                                {
                                    tracing::error!("Failed to send transcript: {:?}", e);
                                }
                            }
                        } else {
                            all_is_final = false;
                            speech_detected_clone.notify();
                        }
                    }
                    if let Err(e) = is_pending_tx.send(!all_is_final) {
                        tracing::error!("Failed to update pending status: {:?}", e);
                    }
                }
            }
        };

        let mut receive_task = tokio::spawn(receive_side);
        better_tokio_select::tokio_select!(match .. {
            .. if let _ = audio_bridge_v2(
                &recognizer_name,
                &mut audio_rx,
                tx,
                &is_pending_rx,
                &mut pending_audio
            ) =>
            {
                if let Err(e) = receive_task.await {
                    tracing::error!("Receive task error: {:?}", e);
                }
            }
            .. if let res = &mut receive_task => {
                if let Err(e) = res {
                    tracing::error!("Receive task error: {:?}", e);
                }
            }
        });
    }
}

async fn audio_bridge_v2(
    recognizer_name: &str,
    audio_rx: &mut mpsc::Receiver<InputVoiceAudio>,
    audio_sender: mpsc::Sender<StreamingRecognizeRequestV2Wrapped>,
    is_pending_rx: &watch::Receiver<bool>,
    pending_audio: &mut Vec<PeerInputAudioIndexed>,
) {
    for voice in pending_audio.iter() {
        if let Err(e) = audio_sender
            .send(StreamingRecognizeRequestV2Wrapped(
                StreamingRecognizeRequestV2 {
                    recognizer: recognizer_name.to_string(),
                    streaming_request: Some(StreamingRequestV2::Audio(voice.audio.clone().into())),
                },
            ))
            .await
        {
            tracing::error!("Failed to send audio content: {:?}", e);
        }
    }

    let mut is_pending_rx_clone = is_pending_rx.clone();
    loop {
        better_tokio_select::tokio_select!(match .. {
            .. if let res = is_pending_rx_clone.changed() => {
                if res.is_ok() && !*is_pending_rx_clone.borrow() {
                    pending_audio.clear();
                }
            }
            .. if let msg = audio_rx.recv() => {
                match msg {
                    Some(InputVoiceAudio::Voice(voice)) => {
                        pending_audio.push(voice.clone());
                        if audio_sender
                            .send(StreamingRecognizeRequestV2Wrapped(
                                StreamingRecognizeRequestV2 {
                                    recognizer: recognizer_name.to_string(),
                                    streaming_request: Some(StreamingRequestV2::Audio(
                                        voice.audio.into(),
                                    )),
                                },
                            ))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some(InputVoiceAudio::NoVoice(voice)) => {
                        if !pending_audio.is_empty() {
                            pending_audio.push(voice.clone());
                        }
                        if audio_sender
                            .send(StreamingRecognizeRequestV2Wrapped(
                                StreamingRecognizeRequestV2 {
                                    recognizer: recognizer_name.to_string(),
                                    streaming_request: Some(StreamingRequestV2::Audio(
                                        voice.audio.into(),
                                    )),
                                },
                            ))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    None => break,
                }
            }
        })
    }
}
