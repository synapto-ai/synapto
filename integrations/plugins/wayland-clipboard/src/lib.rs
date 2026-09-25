//! Wayland clipboard and primary selection plugin for the Synapto framework.

pub mod tools;

use async_trait::async_trait;
use std::sync::Arc;
use synapto_interface::plugin::{Plugin, PluginInitContext, PluginRegistry};

pub use tools::{
    GetClipboardArgs, GetClipboardTool, GetPrimarySelectionArgs, GetPrimarySelectionTool,
    SetClipboardArgs, SetClipboardTool,
};

#[derive(Default, Debug, Clone)]
pub struct WaylandClipboardPlugin;

impl WaylandClipboardPlugin {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Plugin for WaylandClipboardPlugin {
    const CAPABILITY: Option<&'static str> =
        Some("Wayland clipboard and primary selection management");

    async fn create(_context: &PluginInitContext<'_>) -> Result<Self, String> {
        Ok(Self::new())
    }

    fn register<R: PluginRegistry + ?Sized>(self: Arc<Self>, registry: &mut R) {
        registry.register_tool(GetPrimarySelectionTool);
        registry.register_tool(GetClipboardTool);
        registry.register_tool(SetClipboardTool);
    }
}
