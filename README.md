# ![Synapto](assets/synapto-text-100.svg)

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
- 🤖 **`robot`**: A hardware-focused bundle to run on edge devices like Raspberry Pi to control and interact with physical robot components and physical world.

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

That's it! You have a fully functional cognitive loop customized for your use case. For a more detailed step-by-step walkthrough, check out our guide on [How to Create a Custom Bundle](docs/CREATE_BUNDLE.md).

## 🔌 Available Plugins

Plugins operate asynchronously and securely, communicating with the core via strongly-typed JSON schemas. Our current integrations include:

- **`google-chat`**: Integrate seamlessly with Google Chat for organizational assistance.
- **`mumble`**: Connect to Mumble servers for low-latency VoIP communication.
- **`host-audio`**: Interface directly with local microphones and speakers for real-time text-to-speech and speech-to-text capabilities.

_(Have a new idea? Creating a plugin is easy—see below!)_

## 🧠 Architecture at a Glance

The Synapto project operates on a few core design principles:

- **Event-Driven Cognition:** It listens to a continuous stream of sensory input rather than a traditional request-response loop.
- **Native Tool Calling:** Uses standard LLM tool calling natively to fetch data and orchestrate tasks statelessly.
- **Hierarchical Memory:** Divides memory into temporal tiers (Interactions, Sessions, Progressions, Continuums) to simulate long-term retention without blowing up the context window.
- **Strict I/O Boundaries:** Enforces `!LLMSafe` boundaries to prevent prompt injection and strictly control what the LLM can read or write.

Read the complete [Architecture Overview](docs/ARCHITECTURE.md) to dive deeper into how the brain ticks. For a complete map of all documentation files, their respective scopes, and our metadata separation principles, refer to our [Documentation Guidelines](docs/DOCUMENTATION.md).

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

Ready to start hacking? Read the complete guide on [How to Create a Custom Plugin](docs/CREATE_PLUGIN.md) to get your boilerplate up and running in minutes.

## 🤝 Contributing

We welcome contributions through issues and pull requests! Please see our [Contribution Guidelines](CONTRIBUTION.md) for details on how to report issues, submit pull requests, and sign your commits (DCO).

### 🚷 Using AI for contribution

> [!IMPORTANT]
> It is not allowed to use LLMs to generate contributions other than RFCs. Treat all AI-generated code as legally "tainted" or untrusted because there is no assurance the code is not GPL / Proprietary / AGPL / Business Licensed.

## 📜 License

This project is licensed under the [Mozilla Public License 2.0 (MPL-2.0)](LICENSE).
