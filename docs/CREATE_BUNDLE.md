# Creating a Custom Bundle

A **Composition Bundle** is the actual executable binary that runs the AI. It acts as the "composition root," linking the `synapto` brain with specific `plugins` to create a customized agent (like a robot, a chat assistant, or a game master).

Because of our open-core architecture, creating a bundle is incredibly easy. You just create a tiny Rust binary that initializes the core and registers the plugins you want.

## Step-by-Step Guide

### 1. Create a New Binary Crate

Create a new binary crate inside the `bundles/` directory of the workspace:

```bash
cargo new bundles/my-custom-assistant --bin
```

### 2. Update Workspace Configuration

Ensure your new bundle is recognized by the workspace by checking the `Cargo.toml` in the project root. (Since it uses `bundles/*`, your new crate will be picked up automatically).

### 3. Add Dependencies

Edit your new bundle's `Cargo.toml` (`bundles/my-custom-assistant/Cargo.toml`) to include the `synapto` engine and any plugins you wish to use. You will also need `tokio` for the async runtime.

```toml
[package]
name = "my-custom-assistant"
version = "0.1.0"
edition = "2024"

[dependencies]
synapto = { path = "../../core" }
tokio.workspace = true

# Add the plugins you want to use
host-audio = { path = "../../plugins/host-audio" }
# my-chat-plugin = { path = "../../plugins/my-chat-plugin" }
```

### 4. Write the Bootstrapper

Edit `bundles/my-custom-assistant/src/main.rs`. All you need to do is initialize the core and snap in your plugins.

```rust
use synapto::Synapto;
use host_audio::{HostAudioInputPlugin, HostAudioOutputPlugin};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    // Initialize the core with the chosen configuration provider and profile
    Synapto::<
        datadir_local::DataLocalDir<"my-assistant">,
        (synapto::config::ConfigJson, synapto::config::DotEnv, synapto::config::Env),
        prompt_file::FilePromptProvider
    >::run::<(
        HostAudioInputPlugin,
        HostAudioOutputPlugin,
        // MyChatPlugin
    )>()
    .await
}
```

### 5. Run Your Bundle

You can now run your highly customized assistant from the project root!

```bash
cargo run --bin my-custom-assistant
```

## How It Works Under the Hood

When you call `Synapto::<...>::run::<...>()`, the engine prepares the cognitive loop, channel networks, and telemetry infrastructure. It instantiates the listed plugins by passing the loaded configurations to their setup methods, and registers their specialized traits with the system's registries.

During startup, the core maps these registered capabilities directly to the corresponding direct channel ends (e.g. passing text senders/receivers to `ChatPlugin` implementations) and boots their execution loops concurrently.
