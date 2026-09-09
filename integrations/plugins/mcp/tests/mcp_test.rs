#![allow(clippy::disallowed_methods)]

use serde_json::json;
use synapto_plugin_mcp::{McpPluginConfig, McpServerTarget};

#[test]
fn test_claude_mcp_config_deserialization() {
    let config_json = json!({
        "mcpServers": {
            "filesystem": {
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
                "env": {
                    "DEBUG": "mcp:*"
                }
            },
            "remote": {
                "url": "http://localhost:8000/sse",
                "headers": {
                    "Authorization": "Bearer token123"
                },
                "disabled": true
            }
        }
    });

    let config: McpPluginConfig =
        serde_json::from_value(config_json).expect("Failed to parse Claude MCP config");
    assert_eq!(config.mcp_servers.len(), 2);

    let fs_server = config
        .mcp_servers
        .get("filesystem")
        .expect("filesystem server missing");
    assert!(!fs_server.disabled);
    match &fs_server.target {
        McpServerTarget::Stdio { command, args, env } => {
            assert_eq!(command, "npx");
            assert_eq!(
                args,
                &["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
            );
            assert_eq!(env.get("DEBUG").map(|s| s.as_str()), Some("mcp:*"));
        }
        McpServerTarget::Sse { .. } => panic!("Expected Stdio target"),
    }

    let remote_server = config
        .mcp_servers
        .get("remote")
        .expect("remote server missing");
    assert!(remote_server.disabled);
    match &remote_server.target {
        McpServerTarget::Sse { url, headers } => {
            assert_eq!(url, "http://localhost:8000/sse");
            assert_eq!(
                headers.get("Authorization").map(|s| s.as_str()),
                Some("Bearer token123")
            );
        }
        McpServerTarget::Stdio { .. } => panic!("Expected Sse target"),
    }
}

#[tokio::test]
async fn test_mcp_stdio_mock_server_handshake() {
    let script = r#"
while read line; do
    id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
    if echo "$line" | grep -q '"method":"initialize"'; then
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{}}}"
    elif echo "$line" | grep -q '"method":"tools/list"'; then
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"test_tool\",\"description\":\"A test tool\",\"inputSchema\":{\"type\":\"object\"}}]}}"
    elif echo "$line" | grep -q '"method":"tools/call"'; then
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"hello from mcp\"}]}}"
    fi
done
"#;

    let config_json = json!({
        "mcpServers": {
            "mock": {
                "command": "sh",
                "args": ["-c", script]
            }
        }
    });

    let config: McpPluginConfig =
        serde_json::from_value(config_json).expect("Failed to parse config");
    assert_eq!(config.mcp_servers.len(), 1);

    let server_cfg = config.mcp_servers.get("mock").unwrap();
    match &server_cfg.target {
        McpServerTarget::Stdio { command, args, env } => {
            let tools = synapto_plugin_mcp::spawn_stdio_mcp_server("mock", command, args, env)
                .await
                .expect("Failed to spawn mock MCP server");

            assert_eq!(tools.len(), 1);
            let tool = &tools[0];
            assert_eq!(tool.name(), "mcp__mock__test_tool");
            assert_eq!(tool.description(), "A test tool");

            let ctx_req = synapto_interface::context::ContextRequest::default();
            let result = tool
                .erased_execute(&ctx_req, json!({"msg": "hi"}))
                .await
                .expect("Failed to execute tool");

            assert_eq!(
                result,
                json!({
                    "content": [{"type": "text", "text": "hello from mcp"}]
                })
            );
        }
        _ => panic!("Expected Stdio target"),
    }
}
