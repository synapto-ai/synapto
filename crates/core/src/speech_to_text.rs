use crate::cognitive::CognitiveDirectTrigger;
use synapto_interface::peer_input::PeerInputSpeech;
use synapto_interface::peer_input_audio::PeerInputAudio;
use synapto_interface::speech_to_text::{InputVoiceAudio, SpeechTranscript};
use synapto_interface::sync::{broadcast, mpsc, watch};

mod detect_voice;
mod speaker_transcript_alignment;

pub(super) async fn start(
    peer_input_audio_rx: mpsc::Receiver<PeerInputAudio>,
    peer_input_speech_tx: mpsc::Sender<PeerInputSpeech>,
    speaker_segment_rx: Option<mpsc::Receiver<synapto_interface::speech_to_text::SpeakerSegment>>,
    heuristic: Option<synapto_interface::speech_to_text::SpeakerHeuristicCallback>,
    trigger_cognitive_direct: CognitiveDirectTrigger,
    last_voice_time_tx: watch::Sender<std::time::Instant>,
) -> (
    mpsc::Sender<SpeechTranscript>,
    mpsc::Receiver<InputVoiceAudio>,
    broadcast::Sender<InputVoiceAudio>,
) {
    let (input_voice_audio_tx, mut _input_voice_audio_rx) =
        broadcast::channel::<InputVoiceAudio>(20);

    tokio::spawn(detect_voice::detect_voice_task(
        peer_input_audio_rx,
        input_voice_audio_tx.clone(),
        last_voice_time_tx.clone(),
    ));

    let (speech_transcript_tx, speech_transcript_rx) = mpsc::channel::<SpeechTranscript>(100);
    let (plugin_voice_audio_tx, plugin_voice_audio_rx) = mpsc::channel::<InputVoiceAudio>(100);

    let mut input_voice_audio_rx_bridge = input_voice_audio_tx.subscribe();
    tokio::spawn(async move {
        while let Ok(msg) = input_voice_audio_rx_bridge.recv().await {
            plugin_voice_audio_tx
                .send(msg)
                .await
                .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                .ok();
        }
    });

    tokio::spawn(speaker_transcript_alignment::start(
        speech_transcript_rx,
        speaker_segment_rx,
        heuristic,
        peer_input_speech_tx,
        trigger_cognitive_direct.clone(),
    ));

    (
        speech_transcript_tx,
        plugin_voice_audio_rx,
        input_voice_audio_tx,
    )
}
