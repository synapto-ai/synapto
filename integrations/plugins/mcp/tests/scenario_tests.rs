#![allow(clippy::disallowed_methods)]

use serde_json::json;
use synapto::Synapto;
use synapto::config::ConfigJson;
use synapto::config::{DotEnv, Env};
use synapto_plugin_mcp::{McpPlugin, McpPluginConfig, McpServerTarget};
use synapto_test::ephemeral_datadir::EphemeralDir;
use synapto_test::local_storage::LocalStorage;
use synapto_test::test_datadir::WorkspaceTestDir;
use synapto_test::{
    MockAudioInputPlugin, MockChatPlugin, MockDiarizationPlugin, MockDocumentsPlugin,
    MockSlowReadPlugin, MockSttPlugin, MockTtsPlugin, run_scenario,
};

async fn test_bundle() {
    Synapto::<(ConfigJson<WorkspaceTestDir>, DotEnv, Env), LocalStorage<EphemeralDir>>::run::<(
        MockAudioInputPlugin,
        MockDocumentsPlugin,
        MockChatPlugin,
        MockSlowReadPlugin,
        MockTtsPlugin,
        MockSttPlugin,
        MockDiarizationPlugin,
        McpPlugin,
    )>()
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn smoke_scenario() {
    run_scenario("tests/scenarios/smoke-test/scenario.yaml", test_bundle).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore]
async fn non_actionable_input_scenario() {
    run_scenario(
        "tests/scenarios/non-actionable-input/scenario.yaml",
        test_bundle,
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_multiple_mcp_servers_integration() {
    let script_fs = r#"
while read line; do
    id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
    if echo "$line" | grep -q '"method":"initialize"'; then
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{}}}"
    elif echo "$line" | grep -q '"method":"tools/list"'; then
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"read_file\",\"description\":\"Read contents of file\",\"inputSchema\":{\"type\":\"object\"}},{\"name\":\"write_file\",\"description\":\"Write contents to file\",\"inputSchema\":{\"type\":\"object\"}}]}}"
    elif echo "$line" | grep -q '"method":"tools/call"'; then
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"file action success\"}]}}"
    fi
done
"#;

    let script_mem = r#"
while read line; do
    id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
    if echo "$line" | grep -q '"method":"initialize"'; then
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{}}}"
    elif echo "$line" | grep -q '"method":"tools/list"'; then
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"create_graph\",\"description\":\"Create graph node\",\"inputSchema\":{\"type\":\"object\"}}]}}"
    elif echo "$line" | grep -q '"method":"tools/call"'; then
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"memory updated\"}]}}"
    fi
done
"#;

    let script_gh = r#"
while read line; do
    id=$(echo "$line" | grep -o '"id":[0-9]*' | cut -d: -f2)
    if echo "$line" | grep -q '"method":"initialize"'; then
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"protocolVersion\":\"2024-11-05\",\"capabilities\":{}}}"
    elif echo "$line" | grep -q '"method":"tools/list"'; then
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"tools\":[{\"name\":\"search_repositories\",\"description\":\"Search repos\",\"inputSchema\":{\"type\":\"object\"}}]}}"
    elif echo "$line" | grep -q '"method":"tools/call"'; then
        echo "{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"repositories list\"}]}}"
    fi
done
"#;

    let config_json = json!({
        "mcpServers": {
            "filesystem": {
                "command": "sh",
                "args": ["-c", script_fs]
            },
            "memory": {
                "command": "sh",
                "args": ["-c", script_mem]
            },
            "github": {
                "command": "sh",
                "args": ["-c", script_gh]
            }
        }
    });

    let config: McpPluginConfig =
        serde_json::from_value(config_json).expect("Failed to deserialize multi-server config");
    assert_eq!(config.mcp_servers.len(), 3);

    let ctx_req = synapto_interface::context::ContextRequest::default();

    // Spawn Filesystem MCP Server
    let fs_cfg = config.mcp_servers.get("filesystem").unwrap();
    if let McpServerTarget::Stdio { command, args, env } = &fs_cfg.target {
        let fs_tools = synapto_plugin_mcp::spawn_stdio_mcp_server("filesystem", command, args, env)
            .await
            .expect("Failed to spawn filesystem MCP server");
        assert_eq!(fs_tools.len(), 2);
        assert_eq!(fs_tools[0].name(), "mcp__filesystem__read_file");
        assert_eq!(fs_tools[1].name(), "mcp__filesystem__write_file");

        let res = fs_tools[0]
            .erased_execute(&ctx_req, json!({"path": "/tmp/test.txt"}))
            .await
            .expect("Execution failed");
        assert_eq!(
            res,
            json!({"content": [{"type": "text", "text": "file action success"}]})
        );
    }

    // Spawn Memory MCP Server
    let mem_cfg = config.mcp_servers.get("memory").unwrap();
    if let McpServerTarget::Stdio { command, args, env } = &mem_cfg.target {
        let mem_tools = synapto_plugin_mcp::spawn_stdio_mcp_server("memory", command, args, env)
            .await
            .expect("Failed to spawn memory MCP server");
        assert_eq!(mem_tools.len(), 1);
        assert_eq!(mem_tools[0].name(), "mcp__memory__create_graph");

        let res = mem_tools[0]
            .erased_execute(&ctx_req, json!({"entity": "Synapto"}))
            .await
            .expect("Execution failed");
        assert_eq!(
            res,
            json!({"content": [{"type": "text", "text": "memory updated"}]})
        );
    }

    // 3. Spawn GitHub MCP Server
    let gh_cfg = config.mcp_servers.get("github").unwrap();
    if let McpServerTarget::Stdio { command, args, env } = &gh_cfg.target {
        let gh_tools = synapto_plugin_mcp::spawn_stdio_mcp_server("github", command, args, env)
            .await
            .expect("Failed to spawn github MCP server");
        assert_eq!(gh_tools.len(), 1);
        assert_eq!(gh_tools[0].name(), "mcp__github__search_repositories");

        let res = gh_tools[0]
            .erased_execute(&ctx_req, json!({"query": "synapto"}))
            .await
            .expect("Execution failed");
        assert_eq!(
            res,
            json!({"content": [{"type": "text", "text": "repositories list"}]})
        );
    }
}
