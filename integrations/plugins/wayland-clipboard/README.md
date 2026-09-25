# Synapto Wayland Clipboard Plugin

This plugin provides Wayland clipboard and primary selection management tools for the Synapto framework.

## Capabilities

The plugin connects directly to the Wayland display server through the Wayland data-control protocols (`ext-data-control` or `wlr-data-control`). It does not execute external shell commands.

It registers three tools:
1. `get_primary_selection`: Reads highlighted text from the primary selection buffer.
2. `get_clipboard`: Reads text from the standard clipboard (`Ctrl+C` buffer).
3. `set_clipboard`: Writes text to the standard clipboard (`Ctrl+V` buffer) and serves it through a background worker thread.

## Requirements

- Wayland compositor with support for `ext-data-control` or `wlr-data-control` (e.g. Sway, Hyprland, KDE Plasma 6+).
- The `WAYLAND_DISPLAY` environment variable must be set. Tools report unavailable when `WAYLAND_DISPLAY` is absent.

## Usage

Add the dependency to your bundle or assistant `Cargo.toml`:

```toml
[dependencies]
synapto-plugin-wayland-clipboard = { path = "../synapto/integrations/plugins/wayland-clipboard" }
```

Register the plugin in the `Synapto` builder:

```rust
use synapto_plugin_wayland_clipboard::WaylandClipboardPlugin;

Synapto::builder()
    // ...
    .plugins::<(
        // other plugins
        WaylandClipboardPlugin,
    )>()
    .run()
    .await
```
