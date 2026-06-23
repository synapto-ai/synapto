# AI System Architecture

> **Architectural Documentation Rule:** This document defines the high-level system architecture, core design principles, and generic data boundaries.
> **Do NOT include** specific plugin implementations, concrete bundle names, tutorial-like code examples, or hardware-specific deployment requirements in this file. Information regarding specific integrations (e.g., `google-chat`, `mumble`) or concrete composition bundles belongs in the `README.md` or individual component documentation.

## Overview

The Synapto project is a highly concurrent, event-driven cognitive architecture built in Rust. It utilizes the `tokio` runtime for asynchronous task management and channel-based message passing. The core intelligence is driven by Large Language Models (LLMs) using strongly-typed JSON schemas to enforce structured inputs and outputs.

The system is built on an open-core architecture with loosely coupled plugins, configurable via composition bundles. These bundles compose the core intelligence with specific inputs and outputs to act in any desired capacity:

- **Hardware Agents**: Interacting with hardware sensors and actuators (CANBus, cameras, microphones).
- **Software Integrations**: Managing external software platforms, chat applications, and organizational routing.
- **Localized Companions**: Providing personal assistance and managing localized tasks without cloud dependencies.
- **Interactive Storytelling**: Maintaining world state, generating dynamic story arcs (Sagas, Chapters, Scenes), and interacting with users.

## Core Design Principles

1. **Event-Driven Cognition**: Instead of a simple request-response REST loop, the system listens to continuous streams of sensory input (speech, video, system state). Any significant change triggers a central `Notify` primitive, waking up the main cognitive loop.
2. **Actor-Model Inspiration**: Different components (Speech-to-Text, GUI, Memory Management, Vision) run as independent asynchronous tasks. They communicate state changes via `tokio::sync::watch` and emit discrete events via `tokio::sync::mpsc`.
3. **Strongly-Typed LLM Interfaces**: Interaction with LLMs (via crates like `genai`) is heavily structured. Inputs are serialized from Rust structs with explicit JSON schemas (`schemars`), and outputs are parsed directly into actionable Rust types (e.g., `CognitiveCommands`).
4. **Native Tool Calling and Stateless Context**: When the LLM requires external data (e.g., retrieving a document) or needs to defer execution for an asynchronous task (e.g., waiting for document parsing), it must use native LLM Tool Calling (Function Calling). The `LLMClient` intercepts the tool call, fetches the data or awaits the async event, and provides a `ToolResponse`. This maintains a stateless cognitive loop and prevents context window bloat caused by stateful "mounted" data.
5. **Generic Tool Architecture**: The core `LLMClient` remains agnostic to domain-specific tools. It leverages the non-generic `ToolExecutor` trait. Subsystems define their own tools and implement the executor (e.g., `CognitiveSideExecutor` executing document-retrieval tools), preserving a strict separation of concerns without generic boilerplate.
6. **Strict LLM I/O Boundaries (`!LLMSafe`)**: Internal system types explicitly opt out of LLM serialization using a negative trait bound (`impl !LLMSafe for Type {}`). This enforces the creation of specific view models (DTOs) for LLM prompts, reducing token overhead, stabilizing schemas, and mitigating hallucination risks by strictly bounding what the LLM is allowed to read and write.
7. **Strict Separation of I/O and Processing (Plugins vs. Subsystems)**: Plugins (e.g., chat platform integrations, VoIP clients) exclusively own external sourcing, API communication, and payload downloading. Core subsystems (e.g., `documents`) exclusively own data transformation and orchestration. The core operates as a generic router and must never import plugin-specific authentication clients or routing logic.
8. **Direct Channel Coupling**: The core avoids unnecessary routing layers or envelopes. Chat interactions utilize direct `PeerInputText` and `CognitiveOutputText` channels to connect a single active `ChatPlugin` directly to the Cognitive Core.
9. **Dual Streaming Modalities**: Input is separated into High-Latency Cognitive Side Tasks (e.g., `TextMessage`) and Low-Latency Cognitive Direct Tasks (e.g., `SpeechMessage`). This reflects distinct latency priorities while using a unified `MessageChannel` location envelope.
10. **Intent vs. State DTOs (Data Cohesion)**: Do not reuse internal state representations (e.g., a full `Document` struct) as message payloads across channels if the operation naturally restricts the schema. Use specific "Intent" structs (e.g., `DocumentRegistrationRequest`) containing only required input fields.
11. **Asynchronous Edge Ingestion**: Plugins must never block their primary ingestion loops with heavy I/O operations (like downloading files). Metadata must be registered with subsystems synchronously (fast path) to preserve context, while heavy payload transfers must be deferred to spawned background tasks.
12. **Prompt Injection Mitigation (JSON Sandboxing)**: All untrusted external data retrieved via tool calls (such as document contents or web scraping results) must be strictly serialized as structured JSON objects (e.g., `{"content": "..."}`) before being returned to the LLM as a `ToolResponse`. Passing raw, unescaped text or markdown directly into the conversational timeline is prohibited, as it exposes the cognitive loop to prompt injection attacks where malicious payload instructions could be interpreted as genuine system commands.
13. **Error Handling & Tracing Context**: Tracing automatically includes the path to the source code, so there is no need to manually prepend additional context strings to error logs. Use the following minimalist pattern:
    ```rs
    {
        Ok(output) => output,
        Err(e) => {
            tracing::error!("{}", e);
            continue;
        }
    };
    ```
14. **Variable Naming & Abbreviation Limits**: Maintain clear and descriptive variable names. The only broadly accepted single-letter abbreviation is `e` for `error`. Contextual shortening (e.g., shortening `saga_scenario_output` to `output`) is the maximum level of abbreviation allowed. Using the full variable name is always acceptable. Never use ambiguous or excessive abbreviations like `out`, `o`, or other unclear truncations.
15. **Strong Typing & Newtype Pattern Mandate**: Avoid "Primitive Obsession". Never use primitive types (`String`, `usize`, etc.) to represent domain concepts like identifiers, opaque contexts, or categories.
    - **Compile-Time Sets**: If the possible values are strictly bounded by internal core logic, use an `enum` (e.g., `enum MemoryTier` instead of `String`). However, for plugin-extensible concepts (like `provider`), use opaque strings.
    - **Dynamic Values**: If the value is dynamic, use the newtype pattern combined with `derive_more` macros to reduce boilerplate (e.g., `#[derive(Debug, Clone, derive_more::Display, derive_more::From, derive_more::Deref)] pub struct SenderId(pub String);`).
      This enforces self-documenting interfaces and prevents accidental swapping of parameters.

16. **Stream and Type Naming Convention (MANDATORY)**: All data types representing streams or events must follow a strict ternary naming structure: `{Subject}{Direction}{Type}`.
    - **Subject**: The source or entity (e.g., `Peer`, `Cognitive`, `System`).
    - **Direction**: The flow relative to the Synapto project (e.g., `Input`, `Output`).
    - **Type**: The content format (e.g., `Text`, `Audio`, `Speech`, `Event`).

    _Example:_ `PeerInputText`
    - `Peer`: Subject/Entity (the source).
    - `Input`: Direction (relative to the AI project).
    - `Text`: Content type.

    Existing examples in the codebase: `PeerInputAudio`, `CognitiveOutputText`, `CognitiveOutputSpeech`.

17. **Opaque Resource Identifier (ORI) Protocol**: Resource identifiers that act as references for the LLM (e.g., `DocumentId`) MUST be treated as opaque, unique strings (primary keys) to provide indirection and decoupling.
    - **Indirection over Redundancy**: ORI applies strictly to identifiers used as indirect lookups to abstract complex state away from the LLM. Do not apply the ORI protocol redundantly to every external API ID (like `SpaceId`, `ThreadId`, or `MessageId`) when they are grouped in an internal routing struct.
    - **No Metadata Leakage**: ORIs MUST NOT contain protocol information, source types (e.g., `file://`, `gdrive://`), or internal state details.
    - **Separation of Concerns**: All metadata regarding a resource's source, location, and lifecycle state MUST be stored in the internal state (e.g., `DocumentMemory`), indexed by the opaque identifier.
    - **Resolution Boundary**: Components needing to interact with the resource MUST use the opaque ID to look up the source of truth from internal state. The LLM view model remains clean and decoupled from infrastructure implementation details.

18. **Cross-Boundary RPC using the "Oneshot Return" Pattern**: When the core router or an isolated component needs to trigger a specific operation in a decoupled plugin/subsystem and await its result (e.g., tool execution, synchronous data fetching), you MUST use the **Oneshot Return Pattern** instead of global correlation IDs.
    - **Structure:** The invoking component sends a Request struct over a standard `mpsc` channel. This Request struct MUST contain a `reply_tx: tokio::sync::oneshot::Sender<Result<T, E>>`.
    - **Execution:** The target plugin consumes the request, performs the isolated heavy operation, and sends the result back via the `reply_tx`.
    - **Benefits:** This provides true actor-model decoupling. The calling task cleanly awaits the oneshot receiver (`reply_rx.await`), entirely eliminating the need to manage global demultiplexing loops, correlation IDs, or shared mutable state.

19. **Flat Code Structure & Chained Conditionals**: Prefer flat code over deeply nested blocks. Utilize Rust's `if let` chaining and early returns (`guard clauses`) to maintain readability and reduce cognitive load.
    - **Avoid Deep Nesting**: Instead of nesting `if` statements, chain them or return early if a condition is not met.

20. **Encapsulated Task Orchestration**: Modules are responsible for their own internal task lifecycle. Instead of spawning sub-component tasks directly in `main.rs`, provide a `start` or `init` function in the module's root (`mod.rs` or `lib.rs`).
    - **Self-Starting**: The `start` function should use `tokio::spawn` to launch all internal tasks (readers, writers, listeners).
    - **Clean Orchestration**: The orchestrator (`main.rs`) should only call the module's `start` function, receiving back any necessary communication channels (e.g., `mpsc::Sender`).
    - **Opaque Implementation**: Implementation details of how a module processes data (whether it uses one task or five) should be hidden from the caller.

21. **Principle of Locality**: When choosing between multiple architectural or implementation options of similar complexity, the solution that places logic, metadata, or configuration closest to the component it describes MUST win. This ensures that the component remains a Single Source of Truth (SSoT) and that refactoring, moving, or deleting the component automatically accounts for its associated context.
    - **Example**: Telemetry registrations (such as semantic channel naming via `register_channel_name!`) MUST reside immediately following the definition of the data type they describe.

22. **Anti-Patterns: Meaningless Containers**: Avoid creating "Context" or "Settings" structs that bundle unrelated dependencies (e.g., channels, state, handles) into a single argument. This violates the Law of Total Consumption (as defined in `AGENTS.md`) and creates "Data Mules." Favor explicit, atomic arguments in function signatures. This makes dependencies clear, simplifies testing, and adheres to the Single Responsibility Principle.

23. **Direct Wiring Principle (Zero-Intermediary Coupling)**: When two plugins or components are connected directly (such as an audio input plugin to an STT engine, or a chat plugin to the cognitive loop), they MUST use the same channel (opposite sender and receiver ends) with no intermediate forwarding tasks or translation loops. This ensures the system remains easy to understand and avoids unnecessary complexity. If any complexity or intermediate rewiring is introduced in a non-obvious way (e.g., due to backpressure constraints or type mismatches), it MUST be explicitly documented and justified in a preceding RFC, and a clear comment must be written in the code exactly where the rewiring is performed.

24. **Strict Domain Registries (No Generic Service Locators)**: The use of generic `dyn Any` type maps (Service Locators) to share arbitrary resources across plugins is strictly prohibited as it violates Domain-Driven Design (DDD) and compile-time safety.
    - **Typed Registries:** All registries must be strongly typed to their specific domain (e.g., `ToolRegistry`, `StorageRegistry`).
    - **Marker Traits:** If a registry must support multiple interchangeable backends (like different database pools), use a strict marker trait (e.g., `StorageProviderPool: Send + Sync + 'static`) instead of `dyn Any`.
    - **Scope Restrictions:** Never use a registry to pass arbitrary handles (like HTTP clients) just for convenience. Pass dependencies explicitly or define a dedicated, strictly-typed domain registry.

25. **Structured Output vs. Tool Calling (Avoiding Meaningless Roundtrips)**: Native LLM Tool Calling (Function Calling) MUST be strictly reserved for functions where the LLM requires a synchronous, immediate response from the system to progress its current thought cycle (e.g., retrieving file/document contents with `read_document` or fetching a website with `read_url`).
    Conversely, fire-and-forget actions and execution commands (e.g., speech/`say`, chat/`write`, movement/`GO FORWARD`/`TURN CAMERA`, or `state_changes`) MUST NOT be implemented as Tool calls. Doing so introduces:
    - **Meaningless Roundtrips**: Forces an extra API request/response loop to feed a dummy output (e.g., `{"success": true}`) back to the LLM before it can complete its cycle, adding 1-2 seconds of needless latency.
    - **Strict History Schema Violations**: Most LLM APIs require every tool call in the conversation history to be paired with a valid tool response message. Saving fire-and-forget actions in interaction history as tool calls would force us to serialize and re-send heavy, artificial tool thought/response structures in the conversation timeline.
      Therefore, all fire-and-forget commands must reside inside strongly-typed Structured Outputs (e.g., `CognitiveLLMOutput<CognitiveCommands>`), allowing the LLM to output reasoning, conversational text, and execution commands natively in a single, atomic API request.

### Cognitive Core (`src/cognitive.rs` and `src/cognitive/`)

The brain of the system. It is divided into direct (`src/cognitive/direct.rs`) and side (`src/cognitive/side.rs`) evaluation tasks. They run infinite loops waiting for notifications from input channels. When awakened, they snapshot the current state, memories, and sensor data, sending them to the LLM. They produce the unified `CognitiveLLMOutput<CognitiveCommands>` structure, which contains reasoning and the relevant command block (`CognitiveDirectCommands` or `CognitiveSideCommands`).

### Command Outputs

The standardized way the AI affects the world (`CognitiveDirectCommands` and `CognitiveSideCommands` in the `cognitive` module). They optionally include:

- `say`: Text to be spoken aloud via TTS (`CongnitiveOutputSpeech`).
- `write`: Text/links to be written into a specific plugin (`CognitiveOutputText`).
- `state_changes`: JSON patch operations to mutate the shared state.

### 5. Hardware, Peripherals, & Plugins

- **Audio Inputs**: Microphones capture audio streams, which are fed into Voice Activity Detection (`detect_voice.rs`), and then transcribed by STT Plugins (Google, Speechmatics, or ElevenLabs).
- **Audio Outputs**: Generated speech (`src/google_text_to_speech.rs`, `src/elevenlabs_text_to_speech.rs`) is piped for physical playback.
- **Sensors/Actuators**: CANBus bridge tasks read hardware states and publish them via `watch` channels.
- **Visuals/GUI**: `src/gui.rs` provides an `egui`-based interface that leverages generative AI models to render real-time visual representations of the current session and state.
- **Plugins**: Independent crates handle remote integration. They are self-orchestrated; the core spawns their `start` method as an asynchronous task, allowing them to maintain infinite loops or spawn internal sub-tasks to handle network events and command translation.

### 6. Decoupling & Plugin Boundaries

To maintain a clean architectural separation, the core must remain agnostic of plugin-specific states or control protocols.

- **State Leakage**: Avoid types like `ProcessingStateUpdate` that are specific to a single plugin's feedback requirements (e.g., visual "thinking" status).
- **Communication Boundaries (Actors vs. Providers)**: Recognize the difference between autonomous actors and linear data providers. The decision of which paradigm to use represents an architectural fork, and engineers must consult the Lead Architect when the boundary is grey.
  - **Actor Plugins (Channel Wiring)**: Plugins that manage autonomous infinite event loops (e.g., Audio, Chat, GUI) MUST ALWAYS be wired with the core via channels. When the core needs a synchronous response from an active background loop, use the **Request + Oneshot Channel** pattern to avoid locks.
  - **Provider Plugins (Inline Data Fetch)**: Plugins that act strictly as on-demand data providers during a linear control flow (e.g., context generation during prompt compilation) SHOULD expose direct `async fn` trait methods. Do not force an unnatural `mpsc` background loop onto simple, inline data fetches where the core must natively await the result anyway.
- **Internal Lifecycle**: Plugins must infer their own state transitions by observing the standard `peer_input_text_tx` and `cognitive_output_text_rx` streams.
- **LLMSafe Boundaries**: `LLMSafe` structs (data models specifically formatted for the LLM context) MUST NOT be part of the cross-boundary interface (`ai-interface`). They must remain internal to the core crate. Enforce this by restricting their visibility to `pub(crate)` instead of `pub`, and mapping from generic interface types (e.g., `ai_interface::types::Document`) to `LLMSafe` types via `From`/`Into` traits within the core.
- **Interface Purity**: The `ai-interface` crate MUST contain ONLY pure generic interface traits and universally shared DTOs (Data Transfer Objects). Never blindly move a highly specialized or domain-specific struct (e.g., `State`, `InteractionMemory`, `PluginInstruction`) from `ai-core` to `ai-interface` just to satisfy a dependency. If a plugin needs to communicate with the core using a core concept, define a simplified, generic abstraction in the interface, and implement `From`/`Into` translation boundaries inside `ai-core`.
- **Dynamic Tool Availability (State-Locked)**: When implementing dynamic LLM tools, the `is_available` method MUST NOT rely on internal structure of other plugins. It MUST evaluate its availability deterministically using fulltext serialization scans over the `compiled_context` JSON value, preserving zero cross-plugin schema coupling.
- **Opaque Resource Identifiers (ORI)**: The core treats all plugin-specific metadata as opaque JSON.
- **Inversion of Control (Channel Handover)**: Plugins must not attempt to "grab" or `take()` system resources or channels directly. The core is the sole creator and provider of channels.
  - **Type-Driven Exclusivity & Safe Handover:** Exclusivity constraints must be enforced naturally at the type level using Rust's standard ownership rules (non-`Copy` receivers) and `Option::take()` mechanisms, rather than relying on artificial assertions or boot-time registry tracking which are error-prone.
  - **Orchestrator Spawner Signatures:** The orchestrator/spawner closures in the core receive exclusive channel ends as `&mut Option<T>`. This allows safe reference-passing inside setup loop boundaries without compiler move errors. The closure itself must execute `.take().expect("resource already taken")` to transfer ownership.
  - **Plugin Trait Signatures:** The plugin-side trait signatures (e.g., `ChatPlugin::start`) accept these extracted resources as direct, bare, non-optional types. Only secondary optional features (like document upload senders) retain explicit `Option` wrappers in the trait methods.
- **Architectural Isolation (Switchboard Principle)**: Plugins never communicate with each other directly. All communication is mediated by the core, which acts as a "Switchboard." Even if two plugins are technically linked via a shared channel (one sender, one receiver), the core is the sole creator and provider of that channel. This ensures that any component (including Voice Detection) can be moved into a plugin without violating the system's structural integrity, as the core will simply "wire" the new plugin into the existing switchboard.
- **Atomic Traits, Composite Implementations (Interface Segregation)**: To maximize modularity, reusability, and testability, we enforce a strict separation between interface granularity and implementation composition:
  - **Atomic Traits:** All plugin traits defined in the `ai-interface` crate (such as `AudioInputPlugin`, `AudioOutputPlugin`, `ChatPlugin`, etc.) MUST be as small as possible. Each trait must represent the absolute smallest, non-decomposable unit of interface behavior, overarching exactly one logical set of channels/responsibilities.
  - **Platform-Agnostic Capabilites:** Plugin traits MUST be designed around generic, abstract capabilities (e.g., modeling a generic `VideoCall` or `Chat` capability) rather than being tailored to the API constraints, payload formats, or branding of a specific third-party service or platform (such as Google Meet, Discord, or Slack). This ensures that the core switchboard vocabulary remains stable, unified, and immune to external corporate API shifts.
  - **Composite Implementations:** A single concrete plugin implementation (e.g., a `MumblePlugin` or a local hardware peripheral controller) CAN implement multiple atomic plugin traits. This allows the implementation to natively and efficiently share socket connections, internal buffers, API clients, or hardware access handles without complexity or IPC overhead, while keeping the interface boundaries compile-time decoupled.

### 7. Configuration and Prompt Injections (`prompt-providers/` and `config-providers/`)

To keep the `ai-core` agnostic to external environments, we isolate static prompts and configuration loading mechanisms into dedicated, interchangeable modules provided during bootstrap.

- **Prompt Providers**: Implementation of the `CognitivePromptProvider` trait live in the `prompt-providers/` directory (e.g., `prompt-providers/empty`, `prompt-providers/file`). They dictate the static system instructions and dynamic runtime rules injected into the LLM context.
- **Config Providers**: Implementation of the `ConfigProvider` trait live in the `config-providers/` directory (e.g., `config-providers/memory`, `config-providers/file`). They manage how settings (API keys, models, system flags) are retrieved and parsed.
- **Strict Mock Isolation (No Mocks in Core)**: Under no circumstances should "Mock" or "Dummy" providers (such as dummy config providers, mock prompt providers, mock LLM executors, or mock plugins) be defined within the `ai-core` or `ai-interface` crates—even for testing purposes. All testing mocks and dummy implementations MUST reside in their respective modular workspace crates (e.g., `config-providers/memory`, `prompt-providers/empty`, or dedicated testing plugins like `plugins/dummy` and those defined inside `bundles/test-mode`).

## Source Code Map

- **`interface/`**: The stable domain vocabulary and generic envelope structures. It is logically modularized into `audio`, `text`, and `stt` domains. See [interface/README.md](../interface/README.md) for detailed implementation guidelines and modularity rules.
- **`core`**: The Core Execution Engine and router.
  - `cognitive/`: Core intelligence, direct and side LLM evaluation tasks.
  - `memories/`: Hierarchical memory system implementation (episodic, semantic, behavioral, state, tasks).
  - `scenarist/`: RPG story management (Saga, Chapter, Scene).
  - `telemetry/`: Tracing and profiling setup.
- **`plugins/*/`**: Independent integration crates (e.g., chat platform integrations, hardware device bridges).
- **`bundles/*/`**: The composition roots. These are binaries that boot the AI core and register specific plugins at runtime.
  - `main.rs`: Entry point, configuration loading, plugin registration.

## Composition Bundles

The system uses separate workspace crates as composition bundles to configure its operational mode. These replace compile-time feature flags. Each bundle specifies the combination of plugins, a specific `ConfigProvider`, and a `CognitivePromptProvider` required for a specific deployment type (e.g., a hardware-specific deployment, a cloud-based service, or a localized agent).

## Data Flow Pipeline

The **Task Memory** subsystem introduces an event-driven background pipeline. Task evaluators (`task_memory_task`, `goal_memory_task`, `mission_memory_task`) listen for updates in `InteractionMemory` and `State`. When a `Pending` task's triggers are met, it becomes `Active` and is injected into the context of the main Cognitive loop. Task completions cascade upwards to automatically re-evaluate Goal and Mission statuses using dedicated background LLM calls.

1. **Ingestion**: Raw data streams (Microphone, Camera frames, CANBus signals, Web APIs) are monitored by dedicated tasks.
2. **State Updates**: Processed events are sent over `watch` or `mpsc` channels (e.g., transcribed text, new system state).
3. **Notification**: Any significant channel update triggers a `tokio::sync::Notify` flag (`cognitive_direct_notifier`).
4. **Cognitive Evaluation**: The `cognitive_task` wakes up, aggregates the latest data across all channels (sensors, memories, scenarios), constructs a detailed context prompt, and queries the primary LLM model.
5. **Caller-Side Tool Orchestration**: If the LLM returns tool calls (e.g., for stateless document retrieval), the cognitive loop executes the tools asynchronously (such as awaiting document parsing), injects the results back into a clean prompt context, and re-queries the LLM. This explicitly prevents infinite loops and history pollution.
6. **Action Routing**: The final LLM output is parsed directly within the cognitive loop. If it contains commands, they are dispatched via dedicated `mpsc` channels to the respective actuator tasks (e.g., the TTS engine, state manager, or chat API).
7. **Memory Consolidation**: In parallel, background tasks progressively summarize older `Interactions` into `Sessions`, `Sessions` into `Progressions`, and so on, keeping the active cognitive context window clean and historically accurate. Semantic and behavioral insights are additionally extracted from sessions and interactions to capture long-term knowledge and communication preferences.

## AI Architecture Data Flow

The following diagram illustrates the relationships and data flow between the core AI components, sensor inputs, and memory modules. The implementation uses Tokio's asynchronous primitives for message passing. The diagram reflects the components and channels spanning the hardware integrations, the hierarchical memory system, and the `tokio::sync::Notify` pattern for triggering the main cognitive loop.

```mermaid
graph TD
    %% === Component Groups ===
    subgraph "Hardware & Peripherals"
        Microphone((fa:fa-microphone Microphone))
        Camera((fa:fa-video-camera Camera))
        CANBusIn((fa:fa-network-wired CANBus In))
    end

    subgraph "Plugins (Open-Core Trait Interfaces)"
        Plugin1[ChatPlugin (e.g. Chat)]
        STTPlugin[STTPlugin]
    end

    subgraph "Input Processing"
        VAD[Voice Activity Detection]
        Aligner[Transcript Aligner]
        Recognizer[Speaker Recognizer]
        CANBusBridge[CANBus Bridge Task]
    end

    subgraph "Cognitive Core"
        CognitiveSide[Cognitive Side Task\nHigh Latency / Text]
        CognitiveDirect[Cognitive Direct Task\nLow Latency / Speech]
    end

    subgraph "Hierarchical Memory System"
        Interactions[Interactions]
        Sessions[Sessions]
        Progressions[Progressions]
        Continuums[Continuums]
        SharedState[(State/JSON Patch)]
        Scenarist[Story Scenarist]
        Semantic[Semantic Insights]
        Behavioral[Behavioral Insights]
    end

    subgraph "Output Processing"
        TTS[TTSPlugin]
        GUI[GUI Renderer]
    end

    subgraph "Actuators & Outputs"
        AudioOutput((fa:fa-volume-up Audio Output))
        Display((fa:fa-desktop Display))
    end

    %% === Synchronization Primitives ===
    LLMNotifier(fa:fa-bell Notifier)

    %% Data Channels
    SpeechMessageTx(fa:fa-comments peer_input_speech_tx)
    TextMessageTx(fa:fa-comment-dots peer_input_text_tx)
    SensorDataWatch(fa:fa-tachometer-alt sensor_data_watch)
    VideoWatch(fa:fa-eye video_watch)
    MemoryWatch(fa:fa-brain memory_watch)

    %% === High-Level Flow ===

    %% 1. Ingestion & Processing
    Microphone --> VAD
    VAD --> STTPlugin
    VAD --> Recognizer
    STTPlugin -- "SpeechTranscript" --> Aligner
    STTPlugin -- "SpeechDetected" --> CognitiveDirect
    Recognizer -- "SpeakerSegment" --> Aligner
    Aligner -- "PeerInputSpeech" --> SpeechMessageTx
    Camera -- "watch::send" --> VideoWatch
    CANBusIn --> CANBusBridge
    CANBusBridge -- "watch::send" --> SensorDataWatch

    Plugin1 -- "PeerInputText" --> TextMessageTx

    %% 2. Notification Trigger
    SpeechMessageTx -- "notify_one()" ---> LLMNotifier
    TextMessageTx -- "notify_one()" ---> LLMNotifier
    SensorDataWatch -- "notify_one()" ---> LLMNotifier
    VideoWatch -- "notify_one()" ---> LLMNotifier
    MemoryWatch -- "notify_one()" ---> LLMNotifier

    %% 3. Cognitive Wakeup & Context Gathering
    LLMNotifier -- "notified().await" --> CognitiveSide
    LLMNotifier -- "notified().await" --> CognitiveDirect

    CognitiveDirect -- "Drain" --> SpeechMessageTx
    CognitiveSide -- "Drain" --> TextMessageTx

    CognitiveSide -- "Borrow" --> SensorDataWatch
    CognitiveSide -- "Borrow" --> VideoWatch
    CognitiveSide -- "Read" --> MemoryWatch
    CognitiveSide -- "Read" --> SharedState

    %% 4. Action Routing
    CognitiveDirect -- "CognitiveOutputSpeech" --> TTS
    CognitiveSide -- "CognitiveOutputText" --> Plugin1
    CognitiveSide -- "state_changes" --> SharedState

    TTS --> AudioOutput
    SharedState --> GUI
    GUI --> Display

    %% 5. Memory Consolidation
    CognitiveSide -- "Append" --> Interactions
    Interactions -- "Summarize" --> Sessions
    Sessions -- "Summarize" --> Progressions
    Progressions -- "Summarize" --> Continuums
    Scenarist -- "Evaluate/Update" --> Progressions
    Sessions -- "Extract" --> Semantic
    Interactions -- "Extract" --> Behavioral

    %% Feed memory updates back to watch
    Interactions -. "Update" .-> MemoryWatch
    Sessions -. "Update" .-> MemoryWatch
    Semantic -. "Update" .-> MemoryWatch
    Behavioral -. "Update" .-> MemoryWatch

    %% === Styling ===
    classDef component fill:#ececff,stroke:#999,stroke-width:2px,color:#333
    class CognitiveSide,CognitiveDirect,Aligner,Recognizer,VAD,TTS,CANBusBridge,GUI,Scenarist component

    classDef memory fill:#f9f2ec,stroke:#d2b48c,stroke-width:2px,color:#333
    class Interactions,Sessions,Progressions,Continuums,SharedState,Semantic,Behavioral memory

    classDef hardware fill:#d4edda,stroke:#5c9e6e,stroke-width:2px,color:#333
    class Microphone,Camera,CANBusIn,AudioOutput,Display hardware

    classDef plugin fill:#e1d5e7,stroke:#9673a6,stroke-width:2px,color:#333
    class Plugin1,STTPlugin plugin

    classDef mpscChannel fill:#fff2cc,stroke:#d6b656,stroke-width:2px,color:#333
    class SpeechMessageTx,TextMessageTx,ChangeStateChannel mpscChannel

    classDef watchChannel fill:#d4eaff,stroke:#4682b4,stroke-width:2px,color:#333
    class VideoWatch,SensorDataWatch,MemoryWatch watchChannel

    classDef notifier fill:#ffdddd,stroke:#c44,stroke-width:2px
    class LLMNotifier notifier
```

### Local Development and Testing

- **Justfile**: The `justfile` is the single source of truth for all project tasks. Run `just` in the root directory to see available build, run, and test commands.
- **Linting & Formatting**: Use `cargo clippy` and verify codebase correctness.
- **Test Mode**: The project includes a robust test-mode scenario runner that allows mocking inputs (like audio or API calls) to iterate on cognitive features locally without needing external sensors or live accounts.
- **Automated Tests**: To run all available scenarios in the `scenarios/` directory, use `just tests`.
- **Testing Guidelines**: For detailed instructions on running fast local unit tests, setting up live integration tests for individual plugins via `test_config.json`, and writing new tests, see the [Testing Guidelines](TESTING.md).
