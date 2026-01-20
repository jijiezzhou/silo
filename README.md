## Silo — local-first AI chief of staff (WIP).

Local-first AI "Chief of Staff" desktop app.

### What is this repo?

Silo is a **local-first** desktop app (**Tauri**) that acts like an AI “Chief of Staff” for your personal and project context.

This repository currently contains **Milestone 1** of the Data Layer: a standalone **MCP (Model Context Protocol) server** written in Rust that runs locally and exposes file/query tools to an LLM client (e.g. Claude Desktop).

### Current milestone status (Milestone 1: Handshake)

- **Handshake**: `initialize` over stdio JSON-RPC 2.0 ✅
- **Tools**: `tools/list` + `tools/call` ✅
- **Zero-panic**: errors return structured JSON / error strings (no crashing) ✅

### Roadmap / TODO (Upcoming milestones)

#### Milestone 2 — “Useful agent loop” (local-first)

- [ ] Upgrade `silo_agent` from single-call routing to a multi-step loop (plan → tool calls → final answer)
- [ ] Add structured “thoughtless” planning format (JSON plan + tool calls + citations) and retries on invalid JSON
- [ ] Add guardrails: max steps, timeouts, tool allowlist per request, and audit log of tool calls

#### Milestone 3 — “Documents you can talk to” (PDF + filesystem)

- [ ] Add safe recursive file discovery tool (e.g. “find PDFs under ~/Downloads”)
- [ ] Add “summarize file / summarize folder” tools (extract → LLM summary), with size limits and redaction hooks
- [ ] Add incremental indexing (hash/mtime) + per-directory scoping (don’t force indexing all of `~`)

#### Milestone 4 — “Personal ops” (calendar/tasks)

- [ ] Calendar read tools: list events, find free slots, summarize upcoming week
- [ ] Calendar write tools: create/update events with confirmations
- [ ] Task integration (Reminders/Todoist/etc.) behind the same permission model

#### Milestone 5 — “Product-grade desktop”

- [ ] Tauri UI: “Ask” chat panel + tool trace + progress UI for long-running indexing
- [ ] Onboarding: pick folders to index, explain exclusions, show privacy guarantees
- [ ] Packaging: signed builds, auto-update, crash reporting (local-only by default)

### Configuration (Phase 2.0)

Silo stores a local config file to keep indexing policy safe and controllable:

- Default path: `~/.config/silo/config.json`
- Override: set `SILO_CONFIG_PATH`

By default, filesystem indexing roots are set to your **home directory** (`~`) with conservative exclusions (e.g. `.git/`, `node_modules/`, `target/`, secrets, caches).
For MVP bulk indexing, we also exclude app bundles and Photos libraries by default (to avoid huge/noisy folders and macOS privacy prompts):

- `**/*.app/**`
- `**/*.photoslibrary/**`

### Repo layout

- `apps/mcp-server`: MCP server (Rust 2024, Tokio, stdio JSON-RPC, MCP tools)
- `apps/desktop-ui`: Tauri desktop app (MVP UI)
- `crates/`: shared Rust crates (planned)

### Dev

#### Prerequisites

- **Rust toolchain**: install via `rustup`
- **Tauri CLI** (for `cargo tauri ...`): `cargo install tauri-cli`
- Required for `--features lancedb` / `--features mvp` (including building the desktop UI): **`protoc`** (protobuf compiler)
- For PDF extraction (Phase 2.2): **`pdftotext`** via Poppler (`brew install poppler`)
- For local LLM agent: **Ollama** (`brew install ollama`)

macOS quick install:

```bash
brew install protobuf poppler ollama
```

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
- `silo_index_home` (MVP: bulk index configured roots)
- `silo_search_knowledge_base` (disabled unless built with `--features lancedb`)

### MVP workflow

1) Build/run with full MVP features:

```bash
cd /Users/zjzhou/Desktop/projects/silo
cargo run -q -p mcp-server --features mvp
```

2) Bulk index (limit to a small number first):

```bash
cat <<'JSON' | cargo run -q -p mcp-server --features mvp
{"jsonrpc":"2.0","id":100,"method":"tools/call","params":{"name":"silo_index_home","arguments":{"max_files":200,"concurrency":2}}}
JSON
```

3) Search:

```bash
cat <<'JSON' | cargo run -q -p mcp-server --features mvp
{"jsonrpc":"2.0","id":101,"method":"tools/call","params":{"name":"silo_search","arguments":{"query":"chief of staff local-first","top_k":5}}}
JSON
```

### Tauri UI (Desktop)

From repo root:

```bash
cd /Users/zjzhou/Desktop/projects/silo
(cd apps/desktop-ui/src-tauri && cargo tauri dev)
```

Or run directly from the Tauri crate directory:

```bash
cd /Users/zjzhou/Desktop/projects/silo/apps/desktop-ui/src-tauri
cargo tauri dev
```

Note: the current UI buttons call the embedded Rust backend commands (`get_config`, `index_home`, `search`).
The Ollama-powered agent is exposed via MCP as `silo_agent` (see below); it is not wired into the UI yet.
If you open `apps/desktop-ui/ui/index.html` in a normal browser, Tauri IPC will not be available.

### Local LLM (Ollama) + Agent tool

Silo can use a **local LLM** (no cloud API) via the `ollama` CLI, and expose a simple agent tool
that picks one of Silo's tools and executes it.

#### 0) Start Ollama

```bash
ollama serve
```

#### 1) Install and pull a model

- Install Ollama (`brew install ollama` or download the app), then:

```bash
ollama pull llama3.2:3b
```

#### 2) Run the MCP server with the local LLM enabled

Set environment variables before launching (important for GUI apps that may not inherit your shell PATH):

```bash
export SILO_LLM_BACKEND=ollama
export SILO_LLM_MODEL=llama3.2:3b
# Optional if your GUI app can't find ollama in PATH:
# export SILO_OLLAMA_PATH=/opt/homebrew/bin/ollama
```

#### 3) Use the agent tool

Example:

```bash
cat <<'JSON' | cargo run -q -p mcp-server --features mvp
{"jsonrpc":"2.0","id":200,"method":"tools/call","params":{"name":"silo_agent","arguments":{"task":"search all pdfs I have"}}}
JSON
```

Troubleshooting:

- If you see “could not connect to a running Ollama instance”: run `ollama serve` and keep it running.
- If you see “Failed to spawn ollama CLI”: set `SILO_OLLAMA_PATH` to the absolute path (e.g. `/opt/homebrew/bin/ollama`).


