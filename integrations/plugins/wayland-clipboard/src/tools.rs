use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::io::Read;
use synapto_interface::context::ContextRequest;
use synapto_interface::llm::LLMSafe;
use synapto_interface::tool::Tool;
use wl_clipboard_rs::copy::{
    ClipboardType as CopyClipboardType, MimeType as CopyMimeType, Options,
};
use wl_clipboard_rs::paste::{
    ClipboardType as PasteClipboardType, Error as PasteError, MimeType as PasteMimeType, Seat,
    get_contents,
};

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, Default, PartialEq, Eq, LLMSafe)]
pub struct GetPrimarySelectionArgs {}

#[derive(Clone, Copy, Debug, Default)]
pub struct GetPrimarySelectionTool;

#[async_trait]
impl Tool for GetPrimarySelectionTool {
    type Arguments = GetPrimarySelectionArgs;
    const NAME: &'static str = "get_primary_selection";
    const DESCRIPTION: &'static str = "Retrieves the currently selected (highlighted) text in the Wayland desktop environment (primary selection buffer).";

    async fn is_available(
        &self,
        _ctx_request: &ContextRequest,
        _compiled_context: &serde_json::Value,
    ) -> Result<bool, String> {
        Ok(std::env::var_os("WAYLAND_DISPLAY").is_some())
    }

    async fn execute(
        &self,
        _ctx_request: &ContextRequest,
        _args: Self::Arguments,
    ) -> Result<serde_json::Value, String> {
        let text = read_wayland_selection(PasteClipboardType::Primary).await?;
        if text.trim().is_empty() {
            Ok(serde_json::json!({
                "text": "",
                "empty": true,
                "message": "The primary selection buffer is currently empty."
            }))
        } else {
            Ok(serde_json::json!({
                "text": text,
                "empty": false
            }))
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, Default, PartialEq, Eq, LLMSafe)]
pub struct GetClipboardArgs {}

#[derive(Clone, Copy, Debug, Default)]
pub struct GetClipboardTool;

#[async_trait]
impl Tool for GetClipboardTool {
    type Arguments = GetClipboardArgs;
    const NAME: &'static str = "get_clipboard";
    const DESCRIPTION: &'static str = "Retrieves the text currently stored in the standard Wayland clipboard (the Ctrl+C buffer).";

    async fn is_available(
        &self,
        _ctx_request: &ContextRequest,
        _compiled_context: &serde_json::Value,
    ) -> Result<bool, String> {
        Ok(std::env::var_os("WAYLAND_DISPLAY").is_some())
    }

    async fn execute(
        &self,
        _ctx_request: &ContextRequest,
        _args: Self::Arguments,
    ) -> Result<serde_json::Value, String> {
        let text = read_wayland_selection(PasteClipboardType::Regular).await?;
        if text.trim().is_empty() {
            Ok(serde_json::json!({
                "text": "",
                "empty": true,
                "message": "The clipboard is currently empty."
            }))
        } else {
            Ok(serde_json::json!({
                "text": text,
                "empty": false
            }))
        }
    }
}

#[derive(Serialize, Deserialize, JsonSchema, Clone, Debug, PartialEq, Eq, LLMSafe)]
pub struct SetClipboardArgs {
    /// The text content to place into the clipboard for pasting with Ctrl+V.
    pub text: String,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SetClipboardTool;

#[async_trait]
impl Tool for SetClipboardTool {
    type Arguments = SetClipboardArgs;
    const NAME: &'static str = "set_clipboard";
    const DESCRIPTION: &'static str = "Sets the text in the standard Wayland clipboard so the user can paste it immediately with Ctrl+V.";

    async fn is_available(
        &self,
        _ctx_request: &ContextRequest,
        _compiled_context: &serde_json::Value,
    ) -> Result<bool, String> {
        Ok(std::env::var_os("WAYLAND_DISPLAY").is_some())
    }

    async fn execute(
        &self,
        _ctx_request: &ContextRequest,
        args: Self::Arguments,
    ) -> Result<serde_json::Value, String> {
        let text_len = args.text.len();
        tokio::task::spawn_blocking(move || {
            write_wayland_clipboard(args.text, CopyClipboardType::Regular)
        })
        .await
        .map_err(|e| format!("Failed to join clipboard task: {e}"))??;

        Ok(serde_json::json!({
            "success": true,
            "bytes_written": text_len,
            "message": "Text successfully set in Wayland clipboard. Ready for Ctrl+V."
        }))
    }
}

async fn read_wayland_selection(clipboard_type: PasteClipboardType) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let result = get_contents(clipboard_type, Seat::Unspecified, PasteMimeType::Text);
        match result {
            Ok((mut pipe, _mime_type)) => {
                let mut contents = Vec::new();
                pipe.read_to_end(&mut contents)
                    .map_err(|e| format!("Failed to read Wayland pipe: {e}"))?;
                Ok(String::from_utf8_lossy(&contents).into_owned())
            }
            Err(PasteError::ClipboardEmpty)
            | Err(PasteError::NoSeats)
            | Err(PasteError::NoMimeType) => Ok(String::new()),
            Err(PasteError::MissingProtocol { .. }) => Err(
                "Wayland compositor does not support the data-control protocol (ext-data-control or wlr-data-control)".to_string(),
            ),
            Err(e) => Err(format!("Wayland paste error: {e}")),
        }
    })
    .await
    .map_err(|e| format!("Failed to join Wayland read task: {e}"))?
}

fn write_wayland_clipboard(text: String, clipboard_type: CopyClipboardType) -> Result<(), String> {
    let mut opts = Options::new();
    opts.clipboard(clipboard_type);
    opts.foreground(true);

    let prepared_copy = opts
        .prepare_copy(
            wl_clipboard_rs::copy::Source::Bytes(text.into_bytes().into()),
            CopyMimeType::Autodetect,
        )
        .map_err(|e| format!("Failed to prepare Wayland clipboard copy: {e}"))?;

    std::thread::Builder::new()
        .name("wayland-clipboard-server".to_string())
        .spawn(move || {
            if let Err(e) = prepared_copy.serve() {
                tracing::debug!("Wayland clipboard server terminated: {e}");
            }
        })
        .map_err(|e| format!("Failed to spawn Wayland clipboard worker thread: {e}"))?;

    Ok(())
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    #[test]
    fn test_args_serialization() {
        let set_args = SetClipboardArgs {
            text: "Hello Wayland".to_string(),
        };
        let serialized = serde_json::to_string(&set_args).unwrap_or_default();
        assert!(serialized.contains("Hello Wayland"));

        let deserialized: Result<SetClipboardArgs, _> = serde_json::from_str(&serialized);
        assert!(deserialized.is_ok());

        let primary_args = GetPrimarySelectionArgs {};
        let serialized_primary = serde_json::to_string(&primary_args).unwrap_or_default();
        let deserialized_primary: Result<GetPrimarySelectionArgs, _> =
            serde_json::from_str(&serialized_primary);
        assert!(deserialized_primary.is_ok());

        let clip_args = GetClipboardArgs {};
        let serialized_clip = serde_json::to_string(&clip_args).unwrap_or_default();
        let deserialized_clip: Result<GetClipboardArgs, _> = serde_json::from_str(&serialized_clip);
        assert!(deserialized_clip.is_ok());
    }

    #[test]
    fn test_schemas() {
        let set_schema = schemars::schema_for!(SetClipboardArgs);
        assert!(serde_json::to_value(&set_schema).is_ok());

        let primary_schema = schemars::schema_for!(GetPrimarySelectionArgs);
        assert!(serde_json::to_value(&primary_schema).is_ok());

        let clip_schema = schemars::schema_for!(GetClipboardArgs);
        assert!(serde_json::to_value(&clip_schema).is_ok());
    }

    #[tokio::test]
    async fn test_tool_metadata() {
        assert_eq!(GetPrimarySelectionTool::NAME, "get_primary_selection");
        assert_eq!(GetClipboardTool::NAME, "get_clipboard");
        assert_eq!(SetClipboardTool::NAME, "set_clipboard");

        let primary_tool = GetPrimarySelectionTool;
        let ctx = ContextRequest {
            recent_interactions: Vec::new(),
            initial_run: false,
            resolved_tools_count: 0,
        };
        let val = serde_json::Value::Null;
        let available = primary_tool.is_available(&ctx, &val).await;
        assert!(available.is_ok());
    }
}
