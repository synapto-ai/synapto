use std::sync::Arc;
use synapto_interface::cognitive_output_audio::types::CognitiveOutputAudio;
use synapto_interface::sync::{OwnedSemaphorePermit, Semaphore};
use synapto_interface::sync::{broadcast, mpsc, watch};
use synapto_interface::types::CognitiveOutputSpeech;
use tracing::instrument;

use crate::cognitive::CognitiveDirectInterrupt;

#[instrument(skip_all)]
pub async fn start(
    interrupt_cognitive_direct: CognitiveDirectInterrupt,
    _cognitive_speech_tx: broadcast::Sender<CognitiveOutputSpeech>,
    mut cognitive_output_audio_rx_plugin: mpsc::Receiver<CognitiveOutputAudio>,
    cognitive_output_audio_tx: broadcast::Sender<CognitiveOutputAudio>,
) -> (watch::Receiver<bool>, Arc<Semaphore>) {
    let (ai_speaking_tx, ai_speaking_rx) = watch::channel(false);
    let ai_speaking_semaphore = Arc::new(Semaphore::new(1));
    let interrupt_rx = interrupt_cognitive_direct.inner().clone();

    let ai_speaking_semaphore_clone = ai_speaking_semaphore.clone();

    tokio::spawn(async move {
        use std::pin::pin;
        use tokio::time::{Duration, Instant, sleep};

        let mut permit: Option<OwnedSemaphorePermit> = None;
        let mut deadline = Instant::now();
        let mut sleep_timer = pin!(sleep(Duration::from_secs(365 * 24 * 60 * 60)));

        loop {
            better_tokio_select::tokio_select!(match .. {
                // If an audio packet is received
                .. if let Some(msg) = cognitive_output_audio_rx_plugin.recv() => {
                    // Forward the audio immediately
                    cognitive_output_audio_tx
                        .send(msg.clone())
                        .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                        .ok();

                    if let Ok(duration) = crate::utils::audio::get_ogg_opus_duration(&msg.0) {
                        // If we don't hold the permit, acquire it
                        if permit.is_none()
                            && let Ok(p) = ai_speaking_semaphore_clone.clone().acquire_owned().await
                        {
                            permit = Some(p);
                            ai_speaking_tx
                                .send(true)
                                .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                                .ok();
                            deadline = Instant::now();
                        }

                        // Extend the deadline by the audio duration
                        if permit.is_some() {
                            deadline = std::cmp::max(Instant::now(), deadline) + duration;
                            sleep_timer.as_mut().reset(deadline);
                        }
                    }
                }
                // If the speaking duration has elapsed
                .. if let _ = &mut sleep_timer
                    && permit.is_some() =>
                {
                    ai_speaking_tx
                        .send(false)
                        .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                        .ok();
                    permit.take();
                    sleep_timer
                        .as_mut()
                        .reset(Instant::now() + Duration::from_secs(365 * 24 * 60 * 60));
                }
                // If an interrupt is received
                .. if let _ = interrupt_rx.notified() => {
                    ai_speaking_tx
                        .send(false)
                        .inspect_err(|e| tracing::error!("Channel send failed: {:?}", e))
                        .ok();
                    permit.take();
                    sleep_timer
                        .as_mut()
                        .reset(Instant::now() + Duration::from_secs(365 * 24 * 60 * 60));
                }
            })
        }
    });

    (ai_speaking_rx, ai_speaking_semaphore)
}
