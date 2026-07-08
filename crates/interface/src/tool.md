## Dynamic Tools with `Tool`

The `Tool` interface is functionally similar to `Command`, but serves an entirely different purpose: it natively leverages the LLM's Function Calling mechanics to resolve external data _during_ the reasoning phase, rather than mutating the environment _after_ the reasoning phase.

### When to Use It

- You need the LLM to query an external database.
- You need to pull data from a URL, a specific document, or an external API into the LLM's context window.
- The LLM needs the result of the tool's execution to form its final response.

### How to Use It (Example)

Tools are defined via a schema struct using `schemars` and explicitly evaluated per-turn for availability.

#### Best Practice: State-Locked Availability

Tool availability (`is_available`) is dynamically evaluated on every turn using the fully compiled prompt context JSON. This eliminates race conditions.

**1. Same-Plugin State (Direct Schema Coupling)**
If your tool relies on context produced by a `ContextProvider` within the _same_ plugin, it is perfectly safe and encouraged to check that JSON structure directly.

```rust,ignore
    async fn is_available(
        &self,
        _ctx: &ContextRequest,
        compiled_context: &serde_json::Value
    ) -> Result<bool, String> {
        // Safe: Checking our own plugin's context
        let has_docs = compiled_context
            .get("available_documents")
            .and_then(|arr| arr.as_array())
            .is_some_and(|arr| !arr.is_empty());
        Ok(has_docs)
    }
```

**2. Cross-Plugin State (Fulltext Scan Anti-Coupling)**
Never tightly couple a tool's `is_available` check to the internal JSON schema of _another_ plugin. If your tool (e.g., `ReadUrlTool`) needs to activate when a URL is present—regardless of whether it was injected by the Chat plugin or the Memory plugin—serialize the global context to a string and perform a fast pattern scan.

```rust,ignore
    async fn is_available(
        &self,
        _ctx: &ContextRequest,
        compiled_context: &serde_json::Value
    ) -> Result<bool, String> {
        // Safe: Universal activation without cross-plugin schema coupling
        let full_context_str = serde_json::to_string(compiled_context).unwrap_or_default();
        let has_url = full_context_str.contains("http://") || full_context_str.contains("https://");
        Ok(has_url)
    }
```

To expose this tool, register it within your `Plugin` trait implementation:

```rust,ignore
impl Plugin for MyPlugin {
    fn register<R: synapto_interface::plugin::PluginRegistry + ?Sized>(self: Arc<Self>, registry: &mut R) {
        registry.register_tool(ReadDocumentPluginTool { ... });
    }
}
```