use synapto_interface::sync::{broadcast, mpsc, watch};
use tracing::instrument;

use synapto_interface::peer_input_audio::{
    PEER_INPUT_AUDIO_CHUNK_SIZE, PEER_INPUT_AUDIO_SAMPLE_RATE, PeerInputAudio,
};
use synapto_interface::speech_to_text::{InputVoiceAudio, PeerInputAudioIndexed};

const _: () = assert!(PEER_INPUT_AUDIO_SAMPLE_RATE == 16_000);

const VAD_THRESHOLD: f32 = 0.5;

#[instrument(skip_all)]
pub(super) async fn detect_voice_task(
    mut audio_rx: mpsc::Receiver<PeerInputAudio>,
    voice_audio_tx: broadcast::Sender<InputVoiceAudio>,
    last_voice_time_tx: watch::Sender<std::time::Instant>,
) {
    let mut detector = earshot::Detector::default();
    let mut index = 1;

    let mut last_voice_time = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(30))
        .unwrap_or_else(std::time::Instant::now);

    while let Some(audio) = audio_rx.recv().await {
        let sum_score: f32 = audio
            .chunks_exact(256)
            .map(|frame| detector.predict_i16(frame))
            .sum();
        let avg_score = sum_score / (PEER_INPUT_AUDIO_CHUNK_SIZE as f32 / 256.0);

        tracing::trace!(target: "telemetry", metric = "vad", value = avg_score);

        if avg_score > VAD_THRESHOLD {
            last_voice_time = std::time::Instant::now();
        }

        if let Err(e) = last_voice_time_tx.send(last_voice_time) {
            tracing::error!("{}", e);
        }

        let result = if avg_score > VAD_THRESHOLD {
            InputVoiceAudio::Voice(PeerInputAudioIndexed { audio, index })
        } else {
            InputVoiceAudio::NoVoice(PeerInputAudioIndexed { audio, index })
        };

        if let Err(e) = voice_audio_tx.send(result) {
            tracing::error!("{}", e);
        }

        index += 1;
    }
}
