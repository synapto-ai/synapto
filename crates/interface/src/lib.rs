#![doc = include_str!("../README.md")]

pub mod cognitive_output_audio;
pub mod cognitive_output_text;
pub mod peer_input_audio;
pub mod peer_input_text;
pub mod speech_to_text;
pub mod storage;

/// Instrumented synchronization primitives and re-exports of `tokio::sync`.
pub mod audio_recorder;
pub mod call;
pub mod camera;
pub mod chat;
pub mod cognitive;
pub mod command;
pub mod context;
pub mod document;
pub mod gui;
pub mod interaction;
/// Core data types used across the interface and core engine.
pub mod llm;
pub mod peer_input;
pub mod plugin;
pub mod rollout;
pub mod secrets;
pub mod sync;
pub mod tool;
