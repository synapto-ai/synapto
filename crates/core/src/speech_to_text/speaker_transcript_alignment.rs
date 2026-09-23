use std::collections::VecDeque;
use synapto_interface::peer_input_audio::PEER_INPUT_AUDIO_CHUNK_DURATION;
use synapto_interface::sync::{mpsc, watch};

use crate::cognitive::CognitiveDirectTrigger;
use crate::speech_to_text::SpeechTranscript;
use synapto_interface::peer_input::MessageText;
use synapto_interface::peer_input::PeerInputSpeech;
use synapto_interface::plugin::MessageChannel;
use synapto_interface::speech_to_text::SpeakerId;
use synapto_interface::speech_to_text::{InternalSpeaker, SpeakerSegment, Word};

pub(super) async fn start(
    mut transcript_rx: mpsc::Receiver<SpeechTranscript>,
    mut speaker_rx: Option<mpsc::Receiver<SpeakerSegment>>,
    heuristic_callback: Option<synapto_interface::speech_to_text::SpeakerHeuristicCallback>,
    peer_input_speech_tx: mpsc::Sender<PeerInputSpeech>,
    trigger_cognitive_direct: CognitiveDirectTrigger,
    last_voice_time_rx: watch::Receiver<std::time::Instant>,
) {
    let mut speaker_segments: VecDeque<SpeakerSegment> = VecDeque::new();
    let use_stt_diarization = speaker_rx.is_none();
    let heuristic = heuristic_callback.unwrap_or_else(|| {
        synapto_interface::speech_to_text::SpeakerHeuristicCallback::new(fallback_heuristic)
    });

    loop {
        better_tokio_select::tokio_select!(match .. {
            .. if let transcript_result = transcript_rx.recv() => {
                let transcript = match transcript_result {
                    Some(t) => t,
                    None => break, // Channel closed
                };

                let last_voice = *last_voice_time_rx.borrow();
                let lag_ms = last_voice.elapsed().as_secs_f64() * 1000.0;
                let chunk_count = transcript.end_index.saturating_sub(transcript.start_index) + 1;
                let chunk_duration_ms = PEER_INPUT_AUDIO_CHUNK_DURATION.as_secs_f64() * 1000.0;
                let audio_duration_ms = chunk_count as f64 * chunk_duration_ms;

                tracing::trace!(target: "telemetry", metric = "stt/perceived_lag_ms", value = lag_ms);
                tracing::trace!(target: "telemetry", metric = "stt/audio_duration_ms", value = audio_duration_ms);

                tracing::info!(
                    "STT perceived lag: {:.1}ms (audio: {:.1}ms) [chunks {}..={}]: \"{}\"",
                    lag_ms,
                    audio_duration_ms,
                    transcript.start_index,
                    transcript.end_index,
                    transcript.transcript.trim()
                );

                let span = tracing::trace_span!("heuristic", track_stats = true);
                let _enter = span.enter();

                // Clean up old segments. Keep segments that ended at most 50 chunks before the transcript started.
                while let Some(segment) = speaker_segments.front() {
                    if segment.end_index.saturating_add(50) < transcript.start_index {
                        speaker_segments.pop_front();
                    } else {
                        break;
                    }
                }

                struct Sentence {
                    start_index: u64,
                    end_index: u64,
                    words: Vec<Word>,
                }

                let words: Vec<Word> = match transcript.words {
                    Some(ref w) if !w.is_empty() => w.clone(),
                    _ => synthesize_words_from_transcript(
                        transcript.start_index,
                        transcript.end_index,
                        &transcript.transcript,
                    ),
                };

                let mut sentences: Vec<Sentence> = Vec::new();
                let mut grouped_messages: Vec<(InternalSpeaker, String)> = Vec::new();

                if use_stt_diarization {
                    // STT Diarization Fallback Path
                    if words.is_empty() {
                        if !transcript.transcript.trim().is_empty() {
                            sentences.push(Sentence {
                                start_index: transcript.start_index,
                                end_index: transcript.end_index,
                                words: Vec::new(),
                            });
                        }
                    } else {
                        let mut current_words: Vec<Word> = Vec::new();
                        let mut last_speaker_hint: Option<Option<String>> = None;

                        for word in &words {
                            let hint_changed = match &last_speaker_hint {
                                Some(last_hint) => *last_hint != word.speaker_hint,
                                None => false,
                            };

                            if hint_changed && !current_words.is_empty() {
                                let start_idx = current_words
                                    .iter()
                                    .find_map(|w| w.start_index)
                                    .unwrap_or(transcript.start_index);
                                let end_idx = current_words
                                    .iter()
                                    .rev()
                                    .find_map(|w| w.end_index)
                                    .unwrap_or(transcript.end_index);

                                sentences.push(Sentence {
                                    start_index: start_idx,
                                    end_index: end_idx,
                                    words: current_words.clone(),
                                });
                                current_words.clear();
                            }

                            current_words.push(word.clone());
                            last_speaker_hint = Some(word.speaker_hint.clone());
                            let w = word.word.trim();

                            if w.ends_with('.') || w.ends_with('?') || w.ends_with('!') {
                                let start_idx = current_words
                                    .iter()
                                    .find_map(|w| w.start_index)
                                    .unwrap_or(transcript.start_index);
                                let end_idx = current_words
                                    .iter()
                                    .rev()
                                    .find_map(|w| w.end_index)
                                    .unwrap_or(transcript.end_index);

                                sentences.push(Sentence {
                                    start_index: start_idx,
                                    end_index: end_idx,
                                    words: current_words.clone(),
                                });
                                current_words.clear();
                                last_speaker_hint = None;
                            }
                        }

                        if !current_words.is_empty() {
                            let start_idx = current_words
                                .iter()
                                .find_map(|w| w.start_index)
                                .unwrap_or(transcript.start_index);
                            let end_idx = current_words
                                .iter()
                                .rev()
                                .find_map(|w| w.end_index)
                                .unwrap_or(transcript.end_index);

                            sentences.push(Sentence {
                                start_index: start_idx,
                                end_index: end_idx,
                                words: current_words,
                            });
                        }
                    }

                    for sentence in sentences {
                        if sentence.words.is_empty() {
                            grouped_messages.push((
                                InternalSpeaker::Unknown(None),
                                transcript.transcript.trim().to_string(),
                            ));
                            continue;
                        }

                        let final_speaker = sentence.words[0]
                            .speaker_hint
                            .as_ref()
                            .map(|hint| {
                                InternalSpeaker::Recognized(SpeakerId(format!(
                                    "STT_Speaker_{}",
                                    hint
                                )))
                            })
                            .unwrap_or(InternalSpeaker::Unknown(None));

                        let sentence_text = sentence
                            .words
                            .iter()
                            .map(|w| w.word.trim())
                            .collect::<Vec<&str>>()
                            .join(" ");

                        if let Some((last_speaker, last_text)) = grouped_messages.last_mut()
                            && *last_speaker == final_speaker
                        {
                            last_text.push(' ');
                            last_text.push_str(&sentence_text);
                        } else {
                            grouped_messages.push((final_speaker, sentence_text));
                        }
                    }
                } else {
                    // Standard Local Diarization Path
                    // If the provider returned text but failed to provide word-level timestamps,
                    // we synthesize a single sentence spanning the entire transcript duration.
                    if words.is_empty() {
                        if !transcript.transcript.trim().is_empty() {
                            sentences.push(Sentence {
                                start_index: transcript.start_index,
                                end_index: transcript.end_index,
                                words: Vec::new(),
                            });
                        }
                    } else {
                        // Group raw words into logical sentences based on terminal punctuation.
                        let mut current_words = Vec::new();
                        for word in &words {
                            current_words.push(word.clone());
                            let w = word.word.trim();

                            // When we hit end-of-sentence punctuation, flush the buffer
                            if w.ends_with('.') || w.ends_with('?') || w.ends_with('!') {
                                let start_idx = current_words
                                    .iter()
                                    .find_map(|w| w.start_index)
                                    .unwrap_or(transcript.start_index);
                                let end_idx = current_words
                                    .iter()
                                    .rev()
                                    .find_map(|w| w.end_index)
                                    .unwrap_or(transcript.end_index);

                                sentences.push(Sentence {
                                    start_index: start_idx,
                                    end_index: end_idx,
                                    words: current_words.clone(),
                                });
                                current_words.clear();
                            }
                        }
                        // Flush any remaining words that didn't end with punctuation
                        if !current_words.is_empty() {
                            let start_idx = current_words
                                .iter()
                                .find_map(|w| w.start_index)
                                .unwrap_or(transcript.start_index);
                            let end_idx = current_words
                                .iter()
                                .rev()
                                .find_map(|w| w.end_index)
                                .unwrap_or(transcript.end_index);

                            sentences.push(Sentence {
                                start_index: start_idx,
                                end_index: end_idx,
                                words: current_words,
                            });
                        }
                    }

                    for sentence in sentences {
                        if sentence.words.is_empty() {
                            if !transcript.transcript.trim().is_empty() {
                                let mut s_overlaps: std::collections::HashMap<
                                    InternalSpeaker,
                                    u64,
                                > = std::collections::HashMap::new();

                                for segment in &speaker_segments {
                                    let overlap_start =
                                        std::cmp::max(sentence.start_index, segment.start_index);
                                    let overlap_end =
                                        std::cmp::min(sentence.end_index, segment.end_index);
                                    if overlap_end >= overlap_start {
                                        let overlap = overlap_end - overlap_start + 1;
                                        *s_overlaps.entry(segment.speaker.clone()).or_insert(0) +=
                                            overlap;
                                    }
                                }

                                let precomputed_overlaps =
                                    vec![synapto_interface::speech_to_text::WordOverlap {
                                        start_index: sentence.start_index,
                                        end_index: sentence.end_index,
                                        overlaps: s_overlaps,
                                        word: transcript.transcript.trim().to_string(),
                                    }];

                                let resolved_speakers = heuristic.evaluate(
                                    &precomputed_overlaps,
                                    speaker_segments.make_contiguous(),
                                );

                                let final_speaker = resolved_speakers
                                    .first()
                                    .and_then(|s| s.clone())
                                    .map(InternalSpeaker::Recognized)
                                    .unwrap_or(InternalSpeaker::Unknown(None));

                                grouped_messages.push((
                                    final_speaker,
                                    transcript.transcript.trim().to_string(),
                                ));
                            }
                            continue;
                        }

                        let mut precomputed_overlaps = Vec::new();
                        for word in &sentence.words {
                            let w_start = word.start_index.unwrap_or(sentence.start_index);
                            let w_end = word.end_index.unwrap_or(sentence.end_index);

                            let mut w_overlaps: std::collections::HashMap<InternalSpeaker, u64> =
                                std::collections::HashMap::new();

                            // Calculate overlap using mathematically inclusive closed bounds `[start, end]`.
                            // `overlap_end - overlap_start + 1` yields the precise number of discrete 80ms chunks.
                            for segment in &speaker_segments {
                                let overlap_start = std::cmp::max(w_start, segment.start_index);
                                let overlap_end = std::cmp::min(w_end, segment.end_index);
                                if overlap_end >= overlap_start {
                                    let overlap = overlap_end - overlap_start + 1;
                                    *w_overlaps.entry(segment.speaker.clone()).or_insert(0) +=
                                        overlap;
                                }
                            }

                            precomputed_overlaps.push(
                                synapto_interface::speech_to_text::WordOverlap {
                                    start_index: w_start,
                                    end_index: w_end,
                                    overlaps: w_overlaps,
                                    word: word.word.clone(),
                                },
                            );
                        }

                        let resolved_speakers = heuristic
                            .evaluate(&precomputed_overlaps, speaker_segments.make_contiguous());

                        let mut processed_words = Vec::new();
                        for (i, word_overlap) in precomputed_overlaps.into_iter().enumerate() {
                            let final_speaker = resolved_speakers[i]
                                .clone()
                                .map(InternalSpeaker::Recognized)
                                .unwrap_or(InternalSpeaker::Unknown(None));
                            processed_words.push((
                                word_overlap.start_index,
                                word_overlap.end_index,
                                word_overlap.overlaps,
                                word_overlap.word,
                                final_speaker,
                            ));
                        }

                        for (_, _, _, word_text, speaker) in processed_words {
                            // If the current word has the same speaker as the previous one,
                            // append its text to the last message instead of creating a new entry.
                            if let Some((last_speaker, last_text)) = grouped_messages.last_mut()
                                && *last_speaker == speaker
                            {
                                last_text.push(' ');
                                last_text.push_str(word_text.trim());
                                continue;
                            }

                            grouped_messages.push((speaker, word_text.trim().to_string()));
                        }
                    }
                }

                for (speaker, text) in grouped_messages {
                    let user_message = PeerInputSpeech {
                        channel: MessageChannel {
                            context: serde_json::Value::Null,
                        },
                        speaker: speaker.into(),
                        transcript: MessageText(text),
                    };

                    tracing::info!("\n{:?}", user_message);

                    peer_input_speech_tx
                        .send(user_message)
                        .await
                        .unwrap_or_else(|e| panic!("Failed to send peer input speech: {:?}", e));
                    trigger_cognitive_direct.trigger();
                }
            }
            .. if let result = async {
                match &mut speaker_rx {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } =>
            {
                match result {
                    Some(segment) => {
                        speaker_segments.push_back(segment);
                    }
                    None => {
                        break; // Channel closed
                    }
                }
            }
        })
    }
}

#[derive(PartialEq, Eq, Hash, Clone, Debug)]
enum InternalSpeakerCategory {
    Unknown,
    Recognized(SpeakerId),
}

fn fallback_heuristic(
    precomputed_overlaps: &[synapto_interface::speech_to_text::WordOverlap],
    speaker_segments: &[SpeakerSegment],
) -> Vec<Option<SpeakerId>> {
    let mut resolved_speakers = Vec::with_capacity(precomputed_overlaps.len());
    for word_overlap in precomputed_overlaps {
        resolved_speakers.push(fallback_word_heuristic(
            &word_overlap.overlaps,
            speaker_segments,
            word_overlap.start_index,
            word_overlap.end_index,
        ));
    }
    resolved_speakers
}

fn fallback_word_heuristic(
    w_overlaps: &std::collections::HashMap<InternalSpeaker, u64>,
    speaker_segments: &[SpeakerSegment],
    w_start: u64,
    w_end: u64,
) -> Option<SpeakerId> {
    if w_overlaps.is_empty() {
        return None;
    }

    // 1. Group and sum overlaps by speaker category: Recognized(id) or Unknown
    let mut recognized_overlaps: std::collections::HashMap<SpeakerId, u64> =
        std::collections::HashMap::new();
    let mut unknown_overlap = 0u64;

    for (speaker, &overlap) in w_overlaps {
        match speaker {
            InternalSpeaker::Recognized(id) => {
                *recognized_overlaps.entry(id.clone()).or_insert(0) += overlap;
            }
            InternalSpeaker::Unknown(_) => {
                unknown_overlap += overlap;
            }
        }
    }

    // 2. Find the maximum overlap value
    let mut max_overlap = unknown_overlap;
    for &overlap in recognized_overlaps.values() {
        if overlap > max_overlap {
            max_overlap = overlap;
        }
    }

    if max_overlap == 0 {
        return None;
    }

    // 3. Find which categories have this maximum overlap
    let mut max_candidates = std::collections::HashSet::new();
    if unknown_overlap == max_overlap {
        max_candidates.insert(InternalSpeakerCategory::Unknown);
    }
    for (id, &overlap) in &recognized_overlaps {
        if overlap == max_overlap {
            max_candidates.insert(InternalSpeakerCategory::Recognized(id.clone()));
        }
    }

    // If there is only one candidate, that is our winner!
    if max_candidates.len() == 1 {
        match max_candidates.into_iter().next().expect("Checked len == 1") {
            InternalSpeakerCategory::Recognized(id) => return Some(id),
            InternalSpeakerCategory::Unknown => return None,
        }
    }

    // 4. In case of a tie, "the first wins" chronologically.
    // We scan speaker_segments from front to back (earliest to latest).
    // The first segment that overlaps with [w_start, w_end] and whose speaker
    // belongs to one of the max_candidates is the winner!
    for segment in speaker_segments {
        let overlap_start = std::cmp::max(w_start, segment.start_index);
        let overlap_end = std::cmp::min(w_end, segment.end_index);
        if overlap_end >= overlap_start {
            let category = match &segment.speaker {
                InternalSpeaker::Recognized(id) => InternalSpeakerCategory::Recognized(id.clone()),
                InternalSpeaker::Unknown(_) => InternalSpeakerCategory::Unknown,
            };
            if max_candidates.contains(&category) {
                match category {
                    InternalSpeakerCategory::Recognized(id) => return Some(id),
                    InternalSpeakerCategory::Unknown => return None,
                }
            }
        }
    }

    None
}

fn synthesize_words_from_transcript(
    start_index: u64,
    end_index: u64,
    transcript: &str,
) -> Vec<Word> {
    let tokens: Vec<&str> = transcript.split_whitespace().collect();
    if tokens.is_empty() {
        return Vec::new();
    }

    let chunk_count = end_index.saturating_sub(start_index) + 1;
    let total_chars: usize = tokens.iter().map(|w| w.chars().count()).sum();

    let mut words = Vec::with_capacity(tokens.len());
    let mut cumulative_chars = 0usize;

    for (i, &token) in tokens.iter().enumerate() {
        let token_chars = token.chars().count();
        let word_start = if total_chars == 0 {
            start_index + (i as u64 * chunk_count / tokens.len() as u64)
        } else {
            start_index + (cumulative_chars as u64 * chunk_count / total_chars as u64)
        };
        cumulative_chars += token_chars;
        let word_end = if total_chars == 0 {
            start_index + ((i + 1) as u64 * chunk_count / tokens.len() as u64)
        } else {
            start_index + (cumulative_chars as u64 * chunk_count / total_chars as u64)
        };

        let clamped_start = word_start.min(end_index);
        let clamped_end = word_end.max(clamped_start).min(end_index);

        words.push(Word {
            start_index: Some(clamped_start),
            end_index: Some(clamped_end),
            word: token.to_string(),
            speaker_hint: None,
        });
    }

    words
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use synapto_interface::speech_to_text::SpeakerSegment;

    #[test]
    fn test_synthesize_words_empty_or_whitespace() {
        assert!(synthesize_words_from_transcript(100, 200, "").is_empty());
        assert!(synthesize_words_from_transcript(100, 200, "   \n\t  ").is_empty());
    }

    #[test]
    fn test_synthesize_words_interpolation_bounds() {
        let words = synthesize_words_from_transcript(100, 200, "What is the time?");
        assert_eq!(words.len(), 4);
        assert_eq!(words[0].word, "What");
        assert_eq!(words[3].word, "time?");

        assert_eq!(words[0].start_index, Some(100));
        assert_eq!(words[3].end_index, Some(200));

        for w in &words {
            let s = w.start_index.unwrap();
            let e = w.end_index.unwrap();
            assert!(s >= 100);
            assert!(e <= 200);
            assert!(e >= s);
        }
    }

    #[tokio::test]
    async fn test_alignment_with_none_words_and_local_diarization() {
        let (transcript_tx, transcript_rx) = mpsc::channel(10);
        let (speaker_tx, speaker_rx) = mpsc::channel(10);
        let (peer_input_tx, mut peer_input_rx) = mpsc::channel(10);
        let trigger = CognitiveDirectTrigger::default();
        let (_last_voice_tx, last_voice_rx) = watch::channel(std::time::Instant::now());

        tokio::spawn(start(
            transcript_rx,
            Some(speaker_rx),
            None,
            peer_input_tx,
            trigger.clone(),
            last_voice_rx,
        ));

        // Provide a speaker segment covering chunks 10..=30
        speaker_tx
            .send(SpeakerSegment {
                speaker: InternalSpeaker::Recognized(SpeakerId("alice".to_string())),
                start_index: 10,
                end_index: 30,
            })
            .await
            .unwrap();

        // Send a transcript with words: None
        transcript_tx
            .send(SpeechTranscript {
                start_index: 10,
                end_index: 30,
                transcript: "Hello world.".to_string(),
                words: None,
            })
            .await
            .unwrap();

        let received =
            tokio::time::timeout(std::time::Duration::from_secs(2), peer_input_rx.recv())
                .await
                .expect("Timed out waiting for PeerInputSpeech")
                .expect("PeerInputSpeech channel closed");

        assert_eq!(received.transcript.0, "Hello world.");
        assert_eq!(
            received.speaker,
            synapto_interface::peer_input::Speaker::Recognized(SpeakerId("alice".to_string()))
        );
    }
}
