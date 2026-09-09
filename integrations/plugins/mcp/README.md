# Synapto MCP Plugin (`synapto-plugin-mcp`)

Integration plugin for connecting Synapto AI to third-party [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) servers using standard Claude-style `mcpServers` configuration JSON.

## Features

- **Claude Config Compatibility**: Native support for Claude Desktop `mcpServers` configuration schema.
- **Local Process Transport (`stdio`)**: Spawns asynchronous child processes with piped standard I/O handles using `command`, `args`, and `env`.
- **Remote HTTP Transport**: Supports remote MCP servers using `url` and optional `headers` (e.g., Bearer tokens).
- **Automatic Tool Namespacing**: Automatically namespaces discovered tools as `mcp__{server_name}__{tool_name}` to guarantee zero name collisions.
- **Async Actor Architecture**: Concurrently routes JSON-RPC 2.0 requests over non-blocking `mpsc` and `oneshot` Tokio channel actors.

## Configuration Syntax (Claude `mcpServers` Compatible)

Configure `synapto-plugin-mcp` in your `config.json` under plugin settings:

```json
{
  "plugins": {
    "mcp": {
      "mcpServers": {
        "filesystem": {
          "command": "npx",
          "args": [
            "-y",
            "@modelcontextprotocol/server-filesystem",
            "/path/to/directory"
          ],
          "env": {
            "DEBUG": "mcp:*"
          }
        },
        "memory": {
          "command": "uvx",
          "args": ["mcp-server-memory"]
        },
        "remote_service": {
          "url": "http://127.0.0.1:8000/sse",
          "headers": {
            "Authorization": "Bearer my-secret-token"
          }
        },
        "disabled_service": {
          "command": "some-executable",
          "disabled": true
        }
      }
    }
  }
}
```

## Architectural Notes & Lifecycle

1. **Plugin Initialization (`Plugin::create`)**:
   - Parses the `mcpServers` configuration map.
   - Ignores servers marked with `"disabled": true`.
   - Executes the MCP handshake (`initialize` -> `notifications/initialized` -> `tools/list`).
2. **Tool Namespacing**:
   - For a server named `"filesystem"` returning a tool named `"read_file"`, the tool is registered as `mcp__filesystem__read_file`.
3. **Lifetime Allocation**:
   - Discovered tool names and descriptions are dynamically boxed and leaked (`Box::leak`) to satisfy `synapto_interface::tool::ErasedTool`'s `&'static str` contract for long-lived runtime tools.
