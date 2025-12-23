## Silo

Local-first AI "Chief of Staff" desktop app.

### What is this repo?

Silo is a **local-first** desktop app (planned: **Tauri**) that acts like an AI “Chief of Staff” for your personal and project context.

This repository currently contains **Milestone 1** of the Data Layer: a standalone **MCP (Model Context Protocol) server** written in Rust that runs locally and exposes file/query tools to an LLM client (e.g. Claude Desktop).

### Current milestone status (Milestone 1: Handshake)

- **Handshake**: `initialize` over stdio JSON-RPC 2.0 ✅
- **Tools**: `tools/list` + `tools/call` ✅
- **Zero-panic**: errors return structured JSON / error strings (no crashing) ✅

### Repo layout

- `apps/mcp-server`: MCP server (Rust 2024, Tokio, stdio JSON-RPC, MCP tools)
- `apps/desktop-ui`: Tauri desktop app (planned)
- `crates/`: shared Rust crates (planned)

### Dev

#### Prerequisites

- **Rust toolchain**: install via `rustup`
- Optional (only if enabling LanceDB feature later): **`protoc`** (protobuf compiler)

#### Build/run MCP server

```bash
cd apps/mcp-server
cargo run
```

#### Build only the MCP server crate (workspace)

```bash
cd /Users/zjzhou/Desktop/projects/silo
cargo build -p mcp-server
```

#### Handshake test (stdio JSON-RPC)

```bash
cd /Users/zjzhou/Desktop/projects/silo
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | cargo run -q -p mcp-server
```

#### Tool test (list directory)

```bash
cd /Users/zjzhou/Desktop/projects/silo
echo '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"silo_list_files","arguments":{"directory":"."}}}' | cargo run -q -p mcp-server
```

### Notes

- The knowledge base integration (LanceDB) is **feature-gated** for fast onboarding:
  - Default build: runs without external system deps like `protoc`
  - Later: enable with `--features lancedb` once you want vector search


