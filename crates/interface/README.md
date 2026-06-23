# Interface Crate Guideline

The `interface` crate serves as the Single Source of Truth (SSoT) for all data models, traits, and utilities shared between `ai-core` and the plugin ecosystem.

## Modular Structure

The crate is organized into logical domain modules. Each module follows the modern Rust module style (`module.rs` + `module/` directory).

- **Constraint**: Domain modules MUST NOT re-export objects from their submodules (`models.rs`, `utils.rs`). Consumers MUST use full paths (e.g., `crate::audio::models::PeerInputAudio`).
- **Constraint**: The crate root (`lib.rs`) MUST NOT re-export objects from domain modules.
- **Constraint**: The naming of modules MUST follow naming of channels (ie. `cognitive_output_audio/`, `peer_input_audio/`, ...).

- **`types.rs`**: General system types (Enveloped, CognitiveState). This is a leaf module and does not follow the domain module split.
- **`utils.rs`**: General system utils. This is a leaf module and does not follow the domain module split.

## Module Internal Layout

Each domain module MUST be split into the following components where applicable:

### `types.rs`

Contains pure data structures, enums, and constants.

- **Constraint**: Logic should be kept to a minimum (simple getters or `new` methods).

### `utils.rs`

Contains domain-specific logic and transformation functions.

- **Constraint**: MUST be free of 3rd-party dependencies (workspace members like `interface` itself are allowed).
- **Escalation**: If a 3rd-party dependency is inevitable for a utility, Lead Architect approval is required.

## Communication Patterns

- **Base `Plugin` Trait**: All plugins must implement the core `Plugin` trait defined in the crate root. It governs initialization (`new`), configuration typing (`type Config`), and self-registration.
- **Specialized Role Traits**: Depending on what roles a plugin plays (e.g. `ChatPlugin`, `STTPlugin`, `TTSPlugin`, `DocumentsPlugin`), it implements one or more specialized traits. The asynchronous `start` methods on these traits receive strongly typed, direct channels (`mpsc`, `broadcast`, `watch`) representing exact boundaries.
- **`PluginRegistry`**: Plugins use this interface trait during registration (`plugin.register(registry)`) to hook themselves into specific functional slots.
- **`Enveloped<T>`**: An optional standard wrapper for message routing if metadata/tagging is explicitly desired, ensuring events are tagged with source identifiers. For most direct interactions (like chat text or speech synthesis), the core avoids unnecessary wrapping or forwarding and uses direct channel coupling.

## Naming Convention (MANDATORY)

Follow the strict ternary naming structure defined in `docs/ARCHITECTURE.md`:
`{Subject}{Direction}{Type}`

- _Example:_ `PeerInputText`, `CognitiveOutputAudio`.

### Service Boundary Exception
For internal services that process data within the core (e.g., Speech-to-Text, Text-to-Speech), the ternary naming `{Subject}{Direction}{Type}` is replaced with a functional binary naming `{Purpose}{Direction}`. This applies when both the source and destination are effectively the internal system core.

- _Example:_ `core_voice_audio_rx` (Core sends audio to service), `speech_transcript_tx` (Service sends transcript to core).
