use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use synapto_interface::context::ContextRequest;
use synapto_interface::plugin::{Plugin, PluginInitContext, PluginRegistry};
use synapto_interface::tool::{ErasedTool, ToolHandle};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpPluginConfig {
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    #[serde(default)]
    pub disabled: bool,
    #[serde(flatten)]
    pub target: McpServerTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum McpServerTarget {
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
    Sse {
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcNotification {
    jsonrpc: &'static str,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<u64>,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
    data: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct McpToolDescriptor {
    name: String,
    description: Option<String>,
    #[serde(rename = "inputSchema")]
    input_schema: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct McpToolsListResult {
    tools: Vec<McpToolDescriptor>,
}

enum McpActorCommand {
    CallTool {
        name: String,
        arguments: serde_json::Value,
        reply_tx: oneshot::Sender<Result<serde_json::Value, String>>,
    },
}

pub struct McpBridgedTool {
    name: &'static str,
    description: &'static str,
    schema: schemars::Schema,
    mcp_tool_name: String,
    actor_tx: mpsc::Sender<McpActorCommand>,
}

#[async_trait]
impl ErasedTool for McpBridgedTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn schema(&self) -> schemars::Schema {
        self.schema.clone()
    }

    async fn erased_is_available(
        &self,
        _ctx_request: &ContextRequest,
        _compiled_context: &serde_json::Value,
    ) -> Result<bool, String> {
        Ok(true)
    }

    async fn erased_execute(
        &self,
        _ctx_request: &ContextRequest,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.actor_tx
            .send(McpActorCommand::CallTool {
                name: self.mcp_tool_name.clone(),
                arguments: args,
                reply_tx,
            })
            .await
            .map_err(|e| format!("Failed to dispatch command to MCP actor: {}", e))?;

        match reply_rx.await {
            Ok(result) => result,
            Err(_) => Err("MCP server actor channel closed unexpectedly".to_string()),
        }
    }
}

pub struct McpPlugin {
    bridged_tools: Vec<ToolHandle>,
}

#[async_trait]
impl Plugin for McpPlugin {
    async fn create(context: &PluginInitContext<'_>) -> Result<Self, String> {
        let config: Option<McpPluginConfig> = context.optional_config()?;
        let mut bridged_tools: Vec<ToolHandle> = Vec::new();

        if let Some(cfg) = config {
            for (server_name, server_config) in cfg.mcp_servers {
                if server_config.disabled {
                    continue;
                }

                match server_config.target {
                    McpServerTarget::Stdio { command, args, env } => {
                        let tools =
                            spawn_stdio_mcp_server(&server_name, &command, &args, &env).await?;
                        bridged_tools.extend(tools);
                    }
                    McpServerTarget::Sse { url, headers } => {
                        let tools = connect_remote_mcp_server(&server_name, &url, &headers).await?;
                        bridged_tools.extend(tools);
                    }
                }
            }
        }

        Ok(Self { bridged_tools })
    }

    fn register<R: PluginRegistry + ?Sized>(self: Arc<Self>, registry: &mut R) {
        for tool in &self.bridged_tools {
            registry.register_erased_tool(tool.clone());
        }
    }
}

#[allow(clippy::collapsible_if)]
pub async fn spawn_stdio_mcp_server(
    server_name: &str,
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
) -> Result<Vec<ToolHandle>, String> {
    let mut child = tokio::process::Command::new(command)
        .args(args)
        .envs(env)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| {
            format!(
                "Failed to spawn MCP server process '{}' ({}) : {}",
                server_name, command, e
            )
        })?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| format!("Failed to capture stdin for MCP server '{}'", server_name))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| format!("Failed to capture stdout for MCP server '{}'", server_name))?;

    let (actor_tx, mut actor_rx) = mpsc::channel::<McpActorCommand>(32);
    let next_id = Arc::new(AtomicU64::new(1));

    let (init_tx, init_rx) = oneshot::channel();

    let server_name_owned = server_name.to_string();
    let next_id_clone = next_id.clone();

    tokio::spawn(async move {
        let mut writer = stdin;
        let mut lines = BufReader::new(stdout).lines();
        let mut pending_requests: HashMap<u64, oneshot::Sender<Result<serde_json::Value, String>>> =
            HashMap::new();

        let req_id = next_id_clone.fetch_add(1, Ordering::SeqCst);
        let init_req = JsonRpcRequest {
            jsonrpc: "2.0",
            id: req_id,
            method: "initialize".to_string(),
            params: Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "synapto-ai",
                    "version": "0.1.0"
                }
            })),
        };

        let init_json = match serde_json::to_string(&init_req) {
            Ok(j) => j,
            Err(e) => {
                drop(init_tx.send(Err(format!(
                    "Failed to serialize initialize request: {}",
                    e
                ))));
                return;
            }
        };

        if let Err(e) = writer
            .write_all(format!("{}\n", init_json).as_bytes())
            .await
        {
            drop(init_tx.send(Err(format!("Failed to write initialize request: {}", e))));
            return;
        }

        let mut init_tx_opt = Some(init_tx);
        let mut tools_req_id: Option<u64> = None;

        loop {
            tokio::select! {
                line_res = lines.next_line() => {
                    match line_res {
                        Ok(Some(line)) => {
                            if line.trim().is_empty() {
                                continue;
                            }
                            if let Ok(resp) = serde_json::from_str::<JsonRpcResponse>(&line) {
                                if let Some(id) = resp.id {
                                    if id == req_id {
                                        if let Some(err) = resp.error {
                                            if let Some(tx) = init_tx_opt.take() {
                                                drop(tx.send(Err(format!("MCP initialize returned error: {}", err.message))));
                                            }
                                            return;
                                        }

                                        let initialized_notif = JsonRpcNotification {
                                            jsonrpc: "2.0",
                                            method: "notifications/initialized".to_string(),
                                            params: None,
                                        };
                                        if let Ok(notif_json) = serde_json::to_string(&initialized_notif) {
                                            drop(writer.write_all(format!("{}\n", notif_json).as_bytes()).await);
                                        }

                                        let t_id = next_id_clone.fetch_add(1, Ordering::SeqCst);
                                        tools_req_id = Some(t_id);
                                        let list_tools_req = JsonRpcRequest {
                                            jsonrpc: "2.0",
                                            id: t_id,
                                            method: "tools/list".to_string(),
                                            params: None,
                                        };
                                        if let Ok(list_json) = serde_json::to_string(&list_tools_req) {
                                            drop(writer.write_all(format!("{}\n", list_json).as_bytes()).await);
                                        }
                                    } else if Some(id) == tools_req_id {
                                        if let Some(err) = resp.error {
                                            if let Some(tx) = init_tx_opt.take() {
                                                drop(tx.send(Err(format!("MCP tools/list returned error: {}", err.message))));
                                            }
                                            return;
                                        }

                                        let tools_result: Result<McpToolsListResult, String> = resp
                                            .result
                                            .ok_or_else(|| "Missing result".to_string())
                                            .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()));
                                        if let Some(tx) = init_tx_opt.take() {
                                            drop(tx.send(tools_result.map(|res| res.tools)));
                                        }
                                    } else if let Some(reply_tx) = pending_requests.remove(&id) {
                                        if let Some(err) = resp.error {
                                            drop(reply_tx.send(Err(format!("MCP tool error (code {}): {}", err.code, err.message))));
                                        } else if let Some(res) = resp.result {
                                            drop(reply_tx.send(Ok(res)));
                                        } else {
                                            drop(reply_tx.send(Ok(serde_json::Value::Null)));
                                        }
                                    }
                                }
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            tracing::error!("Error reading stdout from MCP server '{}': {}", server_name_owned, e);
                            break;
                        }
                    }
                }
                cmd_opt = actor_rx.recv() => {
                    match cmd_opt {
                        Some(McpActorCommand::CallTool { name, arguments, reply_tx }) => {
                            let id = next_id_clone.fetch_add(1, Ordering::SeqCst);
                            pending_requests.insert(id, reply_tx);
                            let req = JsonRpcRequest {
                                jsonrpc: "2.0",
                                id,
                                method: "tools/call".to_string(),
                                params: Some(serde_json::json!({
                                    "name": name,
                                    "arguments": arguments,
                                })),
                            };
                            if let Ok(req_json) = serde_json::to_string(&req) {
                                if let Err(e) = writer.write_all(format!("{}\n", req_json).as_bytes()).await {
                                    if let Some(tx) = pending_requests.remove(&id) {
                                        drop(tx.send(Err(format!("Failed to write to stdin of MCP server: {}", e))));
                                    }
                                }
                            }
                        }
                        None => break,
                    }
                }
            }
        }
    });

    let tool_descriptors = init_rx.await.map_err(|_| {
        format!(
            "MCP initialization task dropped prematurely for server '{}'",
            server_name
        )
    })??;

    let mut bridged_tools: Vec<ToolHandle> = Vec::new();

    for desc in tool_descriptors {
        let namespaced_name = format!("mcp__{}__{}", server_name, desc.name);
        let name_static: &'static str = Box::leak(namespaced_name.into_boxed_str());
        let desc_static: &'static str = Box::leak(
            desc.description
                .unwrap_or_else(|| format!("MCP tool {} from server {}", desc.name, server_name))
                .into_boxed_str(),
        );

        let schema: schemars::Schema = desc
            .input_schema
            .and_then(|val| serde_json::from_value(val).ok())
            .unwrap_or_else(|| schemars::schema_for!(serde_json::Value));

        let bridged = McpBridgedTool {
            name: name_static,
            description: desc_static,
            schema,
            mcp_tool_name: desc.name,
            actor_tx: actor_tx.clone(),
        };

        bridged_tools.push(ToolHandle::new(bridged));
    }

    Ok(bridged_tools)
}

pub async fn connect_remote_mcp_server(
    server_name: &str,
    url: &str,
    headers: &HashMap<String, String>,
) -> Result<Vec<ToolHandle>, String> {
    let mut header_map = reqwest::header::HeaderMap::new();
    header_map.insert(
        reqwest::header::CONTENT_TYPE,
        reqwest::header::HeaderValue::from_static("application/json"),
    );
    header_map.insert(
        reqwest::header::ACCEPT,
        reqwest::header::HeaderValue::from_static("application/json, text/event-stream, */*"),
    );
    for (k, v) in headers {
        if let (Ok(name), Ok(val)) = (
            reqwest::header::HeaderName::from_bytes(k.as_bytes()),
            reqwest::header::HeaderValue::from_str(v),
        ) {
            header_map.insert(name, val);
        }
    }

    let client = reqwest::Client::builder()
        .default_headers(header_map)
        .build()
        .map_err(|e| {
            format!(
                "Failed to create HTTP client for server '{}': {}",
                server_name, e
            )
        })?;

    let next_id = Arc::new(AtomicU64::new(1));
    let req_id = next_id.fetch_add(1, Ordering::SeqCst);

    let init_req = JsonRpcRequest {
        jsonrpc: "2.0",
        id: req_id,
        method: "initialize".to_string(),
        params: Some(serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "synapto-ai",
                "version": "0.1.0"
            }
        })),
    };

    let init_res = client.post(url).json(&init_req).send().await.map_err(|e| {
        format!(
            "Failed to send initialize request to remote MCP server '{}' ({}): {}",
            server_name, url, e
        )
    })?;

    let init_resp: JsonRpcResponse = init_res.json().await.map_err(|e| {
        format!(
            "Invalid JSON-RPC initialize response from remote MCP server '{}': {}",
            server_name, e
        )
    })?;

    if let Some(err) = init_resp.error {
        return Err(format!(
            "Remote MCP server '{}' initialize error: {}",
            server_name, err.message
        ));
    }

    let notif = JsonRpcNotification {
        jsonrpc: "2.0",
        method: "notifications/initialized".to_string(),
        params: None,
    };
    drop(client.post(url).json(&notif).send().await);

    let list_id = next_id.fetch_add(1, Ordering::SeqCst);
    let list_req = JsonRpcRequest {
        jsonrpc: "2.0",
        id: list_id,
        method: "tools/list".to_string(),
        params: None,
    };

    let list_res = client.post(url).json(&list_req).send().await.map_err(|e| {
        format!(
            "Failed to list tools from remote MCP server '{}': {}",
            server_name, e
        )
    })?;

    let list_resp: JsonRpcResponse = list_res.json().await.map_err(|e| {
        format!(
            "Invalid JSON-RPC tools/list response from remote MCP server '{}': {}",
            server_name, e
        )
    })?;

    if let Some(err) = list_resp.error {
        return Err(format!(
            "Remote MCP server '{}' tools/list error: {}",
            server_name, err.message
        ));
    }

    let tools_list: McpToolsListResult = list_resp
        .result
        .ok_or_else(|| {
            format!(
                "Missing tools result from remote MCP server '{}'",
                server_name
            )
        })
        .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))?;

    let (actor_tx, mut actor_rx) = mpsc::channel::<McpActorCommand>(32);
    let url_owned = url.to_string();
    let client_clone = client.clone();
    let next_id_clone = next_id.clone();

    tokio::spawn(async move {
        while let Some(cmd) = actor_rx.recv().await {
            match cmd {
                McpActorCommand::CallTool {
                    name,
                    arguments,
                    reply_tx,
                } => {
                    let call_id = next_id_clone.fetch_add(1, Ordering::SeqCst);
                    let call_req = JsonRpcRequest {
                        jsonrpc: "2.0",
                        id: call_id,
                        method: "tools/call".to_string(),
                        params: Some(serde_json::json!({
                            "name": name,
                            "arguments": arguments,
                        })),
                    };

                    let res_fut = client_clone.post(&url_owned).json(&call_req).send().await;
                    match res_fut {
                        Ok(res) => match res.json::<JsonRpcResponse>().await {
                            Ok(resp) => {
                                if let Some(err) = resp.error {
                                    drop(reply_tx.send(Err(format!(
                                        "MCP tool error (code {}): {}",
                                        err.code, err.message
                                    ))));
                                } else if let Some(val) = resp.result {
                                    drop(reply_tx.send(Ok(val)));
                                } else {
                                    drop(reply_tx.send(Ok(serde_json::Value::Null)));
                                }
                            }
                            Err(e) => {
                                drop(
                                    reply_tx
                                        .send(Err(format!("Failed to parse tool response: {}", e))),
                                );
                            }
                        },
                        Err(e) => {
                            drop(reply_tx.send(Err(format!(
                                "HTTP request failed to remote MCP server: {}",
                                e
                            ))));
                        }
                    }
                }
            }
        }
    });

    let mut bridged_tools: Vec<ToolHandle> = Vec::new();

    for desc in tools_list.tools {
        let namespaced_name = format!("mcp__{}__{}", server_name, desc.name);
        let name_static: &'static str = Box::leak(namespaced_name.into_boxed_str());
        let desc_static: &'static str = Box::leak(
            desc.description
                .unwrap_or_else(|| format!("MCP tool {} from server {}", desc.name, server_name))
                .into_boxed_str(),
        );

        let schema: schemars::Schema = desc
            .input_schema
            .and_then(|val| serde_json::from_value(val).ok())
            .unwrap_or_else(|| schemars::schema_for!(serde_json::Value));

        let bridged = McpBridgedTool {
            name: name_static,
            description: desc_static,
            schema,
            mcp_tool_name: desc.name,
            actor_tx: actor_tx.clone(),
        };

        bridged_tools.push(ToolHandle::new(bridged));
    }

    Ok(bridged_tools)
}
