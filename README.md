## Silo

Local-first AI "Chief of Staff" desktop app.

### What is this repo?

Silo is a **local-first** desktop app (planned: **Tauri**) that acts like an AI “Chief of Staff” for your personal and project context.

This repository currently contains **Milestone 1** of the Data Layer: a standalone **MCP (Model Context Protocol) server** written in Rust that runs locally and exposes file/query tools to an LLM client (e.g. Claude Desktop).

### Current milestone status (Milestone 1: Handshake)

- **Handshake**: `initialize` over stdio JSON-RPC 2.0 ✅
- **Tools**: `tools/list` + `tools/call` ✅
- **Zero-panic**: errors return structured JSON / error strings (no crashing) ✅

### Configuration (Phase 2.0)

Silo stores a local config file to keep indexing policy safe and controllable:

- Default path: `~/.config/silo/config.json`
- Override: set `SILO_CONFIG_PATH`

By default, filesystem indexing roots are set to your **home directory** (`~`) with conservative exclusions (e.g. `.git/`, `node_modules/`, `target/`, secrets, caches).

### Repo layout

- `apps/mcp-server`: MCP server (Rust 2024, Tokio, stdio JSON-RPC, MCP tools)
- `apps/desktop-ui`: Tauri desktop app (planned)
- `crates/`: shared Rust crates (planned)

### Dev

#### Prerequisites

- **Rust toolchain**: install via `rustup`
- Optional (only if enabling LanceDB feature later): **`protoc`** (protobuf compiler)
- For PDF extraction (Phase 2.2): **`pdftotext`** via Poppler (`brew install poppler`)

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
 - Local embeddings (Phase 2.4) are also feature-gated:
   - Enable with `--features embeddings` (downloads model on first use)
   - Or use `--features mvp` to enable both `embeddings` + `lancedb`

### MCP tools (current)

- `silo_list_files`
- `silo_read_file`
- `silo_get_config`
- `silo_set_index_roots`
- `silo_validate_index_config`
- `silo_preview_index` (Phase 2.1: deterministic preview scan of what would be indexed)
- `silo_preview_extract` (Phase 2.2: extract text from file/PDF and return a preview)
- `silo_ingest_file` (Phase 2.3: extract + chunk + store chunks to DB when enabled)
- `silo_search` (Phase 2.6: semantic search over indexed chunks)
- `silo_search_knowledge_base` (disabled unless built with `--features lancedb`)


