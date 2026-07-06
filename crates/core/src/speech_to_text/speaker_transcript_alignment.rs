use std::collections::VecDeque;
use synapto_interface::sync::mpsc;

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
) {
    let mut speaker_segments: VecDeque<SpeakerSegment> = VecDeque::new();
    let use_stt_diarization = speaker_rx.is_none();
    let heuristic = heuristic_callback.unwrap_or_else(|| std::sync::Arc::new(fallback_heuristic));

    loop {
        better_tokio_select::tokio_select!(match .. {
            .. if let transcript_result = transcript_rx.recv() => {
                let transcript = match transcript_result {
                    Some(t) => t,
                    None => break, // Channel closed
                };
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

                let mut sentences: Vec<Sentence> = Vec::new();
                let mut grouped_messages: Vec<(InternalSpeaker, String)> = Vec::new();

                if use_stt_diarization {
                    // STT Diarization Fallback Path
                    if transcript.words.is_empty() {
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

                        for word in &transcript.words {
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
                    if transcript.words.is_empty() {
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
                        for word in &transcript.words {
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

                        let resolved_speakers =
                            heuristic(&precomputed_overlaps, speaker_segments.make_contiguous());

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
