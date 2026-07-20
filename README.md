# ![Synapto](assets/synapto-text-100.svg)

[![License](https://img.shields.io/badge/License-MPL--2.0-blue.svg)](https://github.com/synapto-ai/synapto#-license)
[![Crates.io](https://img.shields.io/crates/s/synapto.svg)](https://crates.io/crates/synapto)
[![Docs](https://docs.rs/synapto/badge.svg)](https://docs.rs/synapto/latest/synapto/)

> The Universal, Rust-Powered Cognitive Brain for Robots, Assistants, and Beyond.

Welcome to **Synapto**, an open-source, highly concurrent, and event-driven cognitive architecture built in Rust.

Whether you're looking to power a small desktop companion, run a robust organizational assistant in the cloud, or give life to a hardware robot on a Raspberry Pi Zero, **Synapto** provides a rock-solid, pluggable, and battery-optimized brain that just works and runs forever.

---

## 🌟 Why Synapto?

- 🦀 **Built in Rust (Hard to Break):** Designed to run indefinitely without memory leaks or unexpected crashes. It provides safe concurrency and a robust core execution engine.
- 🪫 **Extremely Resource Efficient:** The core has a tiny memory footprint and optimizations to prevent battery drain, meaning the base system can run on edge devices like a Raspberry Pi Zero, or you can scale up full-featured bundles in a cloud VM.
- 🧩 **Pluggable & Extensible:** Built on an open-core architecture, the core intelligence is decoupled from inputs/outputs. You can easily write plugins or compose them into custom bundles.
- 💸 **Free Forever:** Open-source (MPL-2.0) and freely available to use, modify, and distribute.

## 🎭 Roles (Composition Bundles)

The system is highly configurable via **Composition Bundles**. Instead of monolithic configurations, bundles specify the exact combination of plugins and core features required for a specific deployment type:

- 🏠 **`home-assistant`**: Automate your home and connect to physical sensors and actuators.
- 🏢 **`org-assistant`**: Your organizational companion for managing Google Meet, Chat, and scheduling.
- 🧳 **`personal-assistant`**: A localized, personal assistant to manage your day-to-day tasks.
- 🎓 **`teacher`**: An educational assistant with advanced speech-to-text integration for interactive learning.
- 🎲 **`rpg`**: A Game Master that maintains world state, manages story arcs, game state and interacts dynamically with players.

### 🧱 Create Your Own Custom Bundle

Need something specific? Creating a new custom bundle is ridiculously easy. A bundle is simply a lightweight Rust binary that boots the Synapto core and registers the exact plugins you want.

Here is what a complete, working custom bundle looks like:

```rust
use synapto::Synapto;
use host_audio::{HostAudioInputPlugin, HostAudioOutputPlugin};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    // 1. Initialize the core with the chosen configuration provider and profile
    Synapto::<
        datadir_local::DataLocalDir<"my-custom-assistant">,
        (synapto::config::ConfigJson, synapto::config::DotEnv, synapto::config::Env),
        prompt_file::FilePromptProvider
    >::run::<(
        HostAudioInputPlugin,
        HostAudioOutputPlugin,
        // MyCustomPlugin
    )>()
    // 2. Let it run forever!
    .await
}
```

That's it! You have a fully functional cognitive loop customized for your use case. For a more detailed step-by-step walkthrough, check out our guide on [How to Create a Custom Bundle](docs/src/creating_bundles.md).

## 🔌 Available Plugins

Plugins operate asynchronously and securely, communicating with the core via strongly-typed JSON schemas. The `synapto` repository is divided into workspaces: `crates` for the core architecture and `contrib` for the growing ecosystem of plugins and storage providers. Our current integrations in `contrib/plugins` include:

- **`mumble`**: Connect to Mumble servers for low-latency VoIP communication.
- **`linux-host-audio`**: Interface directly with local microphones and speakers for real-time text-to-speech and speech-to-text capabilities.
- **`clock`**: Provides time and alarm mechanisms.
- **Speech-to-Text (STT) & Text-to-Speech (TTS)**: Dedicated plugins for ElevenLabs, Google, and Speechmatics.

_(Have a new idea? Creating a plugin is easy—see below!)_

## 🧠 Architecture at a Glance

The Synapto project operates on a few core design principles:

- **Event-Driven Cognition:** It listens to a continuous stream of sensory input rather than a traditional request-response loop.
- **Native Tool Calling:** Uses standard LLM tool calling natively to fetch data and orchestrate tasks statelessly.
- **Hierarchical Memory:** Divides memory into temporal tiers (Interactions, Sessions, Progressions, Continuums) to simulate long-term retention without blowing up the context window.
- **Strict I/O Boundaries:** Enforces `!LLMSafe` boundaries to prevent prompt injection and strictly control what the LLM can read or write.

Read the complete [Architecture Overview](docs/src/ARCHITECTURE.md) to dive deeper into how the brain ticks. For a complete map of all documentation files, their respective scopes, and our metadata separation principles, refer to our [Documentation Guidelines](docs/src/DOCUMENTATION.md).

## 🚀 Getting Started

### Prerequisites

- [Rust toolchain](https://rustup.rs/) installed (a `rust-toolchain.toml` is included).
- Just (a command runner for executing project tasks).
- Required LLM API keys (e.g., OpenAI, Anthropic, or local provider equivalents).

### Running a Bundle

All project tasks are automated via `just`. Run `just` from the project root to see a complete list of available commands and their descriptions.

To start a pre-configured bundle, such as the Personal Assistant:

```bash
cargo run --bin personal-assistant
```

## 🛠️ Build Your Own Plugin

We designed **Synapto** to be incredibly welcoming for developers. You can extend the brain's capabilities by writing a standalone plugin.

Plugins exclusively own their external sourcing (API calls, web sockets, payload downloads) and communicate with the core via generic envelopes (`MessageChannel`). This means you don't need to touch the core cognitive loops to add new integrations!

Ready to start hacking? Read the complete guide on [How to Create a Custom Plugin](docs/src/plugin/basics.md) to get your boilerplate up and running in minutes.

## 🤝 Contributing

We welcome contributions through issues and pull requests! Please see our [Contribution Guidelines](CONTRIBUTION.md) for details on how to report issues, submit pull requests, and sign your commits (DCO).

### 🤖 Using AI for Contribution

This project allows the use of AI coding assistants, provided that the contributions are rigorously reviewed by the human contributor and not blindly submitted.

When using AI tools to assist with your development, you must follow the standard contribution process and adhere to the following guidelines:

#### Licensing and Legal Requirements
- All contributions must comply with the project's [MPL-2.0 License](LICENSE).
- The human contributor takes full responsibility for ensuring that any AI-generated code does not violate third-party licenses (e.g., GPL, AGPL, Proprietary) and is safe to use in this project.

#### Developer Certificate of Origin (DCO)
- AI agents **MUST NOT** add `Signed-off-by` tags. Only humans can legally certify the Developer Certificate of Origin (DCO).
- As the human submitter, you are exclusively responsible for:
  - Rigorously reviewing and understanding all AI-generated code.
  - Ensuring compliance with our licensing requirements.
  - Adding your own `Signed-off-by` tag to certify the DCO.
  - Taking full legal and technical responsibility for the contribution.

#### Attribution
To help us track the evolving role of AI in the development process, contributions assisted by AI should include an `Assisted-by` tag in your commit messages:

```
Assisted-by: AGENT_NAME:MODEL_VERSION [TOOL]
```

Where:
- `AGENT_NAME` is the name of the AI tool or framework (e.g., Copilot, Cursor, Zed).
- `MODEL_VERSION` is the specific model version used (e.g., claude-3-5-sonnet, gpt-4o).
- `[TOOL]` are optional specialized tools used.

Example:
```text
Assisted-by: Claude:claude-3-5-sonnet
```

## 📜 License

This project is licensed under the [Mozilla Public License 2.0 (MPL-2.0)](LICENSE).
