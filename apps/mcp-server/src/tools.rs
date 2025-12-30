use crate::database::DatabaseHandle;
use crate::state::{expand_tilde, SharedState};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolCallParams {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolResultContent {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: Vec<ToolResultContent>,
    pub is_error: bool,
}

pub fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "silo_list_files",
            description: "Scans a local folder non-recursively.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "directory": { "type": "string", "description": "Directory path to list." }
                },
                "required": ["directory"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "silo_read_file",
            description: "Reads text content from a valid path.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to read." }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "silo_search_knowledge_base",
            description: "Searches the local knowledge base (LanceDB).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query." }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "silo_search",
            description: "Semantic search over indexed chunks (embed query + vector search).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "top_k": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10 }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "silo_get_config",
            description: "Returns the effective Silo configuration (including config file path).",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "silo_set_index_roots",
            description: "Sets filesystem indexing roots (MVP default is your home directory).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "roots": { "type": "array", "items": { "type": "string" }, "description": "Directories to index (supports ~/ prefix)." }
                },
                "required": ["roots"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "silo_validate_index_config",
            description: "Validates that configured indexing roots are accessible and sane.",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "silo_preview_index",
            description: "Scans configured roots and returns a deterministic preview of what would be indexed (no embeddings).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "max_sample_candidates": { "type": "integer", "minimum": 0, "maximum": 5000, "default": 200 },
                    "max_sample_skipped": { "type": "integer", "minimum": 0, "maximum": 5000, "default": 200 }
                },
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "silo_preview_extract",
            description: "Extracts text from a file (supports PDF via pdftotext) and returns a short preview (no embeddings).",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to extract (supports ~/ prefix)." },
                    "max_preview_chars": { "type": "integer", "minimum": 0, "maximum": 20000, "default": 2000 }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        },
        ToolDefinition {
            name: "silo_ingest_file",
            description: "Ingests a file: extract -> chunk (~500 tokens w/ overlap) -> (placeholder) embed -> store to LanceDB when enabled.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to ingest (supports ~/ prefix)." }
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        },
    ]
}

pub async fn call_tool(state: &SharedState, call: ToolCallParams) -> ToolResult {
    match call.name.as_str() {
        // New canonical names:
        "silo_list_files" |
        // Backward-compatible aliases:
        "list_files" => {
            let args: Result<ListFilesArgs, _> = serde_json::from_value(call.arguments);
            match args {
                Ok(args) => match list_files(args).await {
                    Ok(v) => ok_json(v),
                    Err(e) => err_text(e),
                },
                Err(e) => err_text(format!("Invalid arguments: {e}")),
            }
        }
        "silo_read_file" | "read_file" => {
            let args: Result<ReadFileArgs, _> = serde_json::from_value(call.arguments);
            match args {
                Ok(args) => match read_file(args).await {
                    Ok(v) => ok_json(v),
                    Err(e) => err_text(e),
                },
                Err(e) => err_text(format!("Invalid arguments: {e}")),
            }
        }
        "silo_search" | "silo_search_knowledge_base" | "search_knowledge_base" => {
            let args: Result<SearchKnowledgeBaseArgs, _> = serde_json::from_value(call.arguments);
            match args {
                Ok(args) => match silo_search(state, args.query, args.top_k).await {
                    Ok(v) => ok_json(v),
                    Err(e) => err_text(e),
                },
                Err(e) => err_text(format!("Invalid arguments: {e}")),
            }
        }
        "silo_get_config" => match state.get_config_json().await {
            v => ok_json(v),
        },
        "silo_set_index_roots" => {
            let args: Result<SetIndexRootsArgs, _> = serde_json::from_value(call.arguments);
            match args {
                Ok(args) => {
                    let roots: Vec<PathBuf> = args.roots.into_iter().map(|s| expand_tilde(&s)).collect();
                    match state.set_index_roots(roots).await {
                        Ok(v) => ok_json(v),
                        Err(e) => err_text(e),
                    }
                }
                Err(e) => err_text(format!("Invalid arguments: {e}")),
            }
        }
        "silo_validate_index_config" => ok_json(state.validate_index_config().await),
        "silo_preview_index" => {
            let args: Result<PreviewIndexArgs, _> = serde_json::from_value(call.arguments);
            match args {
                Ok(args) => {
                    let policy_opt = state.fs_policy.read().await;
                    let Some(policy) = policy_opt.as_ref() else {
                        return err_text("No filesystem policy configured".to_string());
                    };

                    let roots = state.filesystem_roots().await;
                    let opts = crate::filesystem::ScanOptions {
                        max_sample_candidates: args.max_sample_candidates.unwrap_or(200),
                        max_sample_skipped: args.max_sample_skipped.unwrap_or(200),
                    };

                    let summary = crate::filesystem::preview_index(roots, policy, opts).await;
                    ok_json(serde_json::to_value(summary).unwrap_or_else(|e| {
                        json!({"error": format!("failed to serialize scan summary: {e}")})
                    }))
                }
                Err(e) => err_text(format!("Invalid arguments: {e}")),
            }
        }
        "silo_preview_extract" => {
            let args: Result<PreviewExtractArgs, _> = serde_json::from_value(call.arguments);
            match args {
                Ok(args) => {
                    let path = expand_tilde(&args.path);
                    if let Err(e) = validate_safe_path(&path) {
                        return err_text(e);
                    }

                    // Use configured max_text_bytes when available.
                    let max_text_bytes = state
                        .fs_policy
                        .read()
                        .await
                        .as_ref()
                        .map(|p| p.max_text_bytes)
                        .unwrap_or(2 * 1024 * 1024);

                    let extracted = match crate::extract::extract_text(&path, max_text_bytes).await {
                        Ok(v) => v,
                        Err(e) => return err_text(e),
                    };

                    let max_preview_chars = args.max_preview_chars.unwrap_or(2000);
                    let preview = extracted.text.chars().take(max_preview_chars).collect::<String>();
                    let preview_truncated = extracted.text.chars().count() > max_preview_chars;

                    ok_json(json!({
                        "path": path.to_string_lossy(),
                        "kind": format!("{:?}", extracted.kind).to_lowercase(),
                        "text_len_chars": extracted.text.chars().count(),
                        "truncated_to_max_text_bytes": extracted.truncated,
                        "preview_truncated": preview_truncated,
                        "preview": preview
                    }))
                }
                Err(e) => err_text(format!("Invalid arguments: {e}")),
            }
        }
        "silo_ingest_file" => {
            let args: Result<IngestFileArgs, _> = serde_json::from_value(call.arguments);
            match args {
                Ok(args) => {
                    let fs_cfg = match state.filesystem_config().await {
                        Some(c) => c,
                        None => return err_text("No filesystem source configured".to_string()),
                    };

                    let max_text_bytes = state
                        .fs_policy
                        .read()
                        .await
                        .as_ref()
                        .map(|p| p.max_text_bytes)
                        .unwrap_or(2 * 1024 * 1024);

                    let res = crate::ingest::process_file(
                        &state.db,
                        &state.embedder,
                        &args.path,
                        max_text_bytes,
                        fs_cfg.chunk_tokens,
                        fs_cfg.chunk_overlap_tokens,
                    )
                    .await;

                    match res {
                        Ok(stats) => ok_json(serde_json::to_value(stats).unwrap_or_else(|e| {
                            json!({"error": format!("failed to serialize ingest stats: {e}")})
                        })),
                        Err(e) => err_text(e),
                    }
                }
                Err(e) => err_text(format!("Invalid arguments: {e}")),
            }
        }
        other => err_text(format!("Unknown tool: {other}")),
    }
}

fn ok_json(value: Value) -> ToolResult {
    ToolResult {
        content: vec![ToolResultContent {
            kind: "text",
            text: value.to_string(),
        }],
        is_error: false,
    }
}

fn err_text(msg: String) -> ToolResult {
    ToolResult {
        content: vec![ToolResultContent {
            kind: "text",
            text: msg,
        }],
        is_error: true,
    }
}

#[derive(Debug, Deserialize)]
struct ListFilesArgs {
    directory: String,
}

#[derive(Debug, Deserialize)]
struct ReadFileArgs {
    path: String,
}

#[derive(Debug, Deserialize)]
struct SearchKnowledgeBaseArgs {
    query: String,
    #[serde(default)]
    top_k: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SetIndexRootsArgs {
    roots: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct PreviewIndexArgs {
    #[serde(default)]
    max_sample_candidates: Option<usize>,
    #[serde(default)]
    max_sample_skipped: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct PreviewExtractArgs {
    path: String,
    #[serde(default)]
    max_preview_chars: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct IngestFileArgs {
    path: String,
}

async fn list_files(args: ListFilesArgs) -> Result<Value, String> {
    let dir = expand_tilde(&args.directory);
    let mut entries = tokio::fs::read_dir(&dir)
        .await
        .map_err(|e| format!("Failed to read directory {}: {e}", dir.display()))?;

    let mut out = Vec::<Value>::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("Failed to read directory entry: {e}"))?
    {
        let ft = entry
            .file_type()
            .await
            .map_err(|e| format!("Failed to stat directory entry: {e}"))?;

        out.push(json!({
            "name": entry.file_name().to_string_lossy(),
            "path": entry.path().to_string_lossy(),
            "isFile": ft.is_file(),
            "isDir": ft.is_dir(),
        }));
    }

    Ok(json!({ "entries": out }))
}

async fn read_file(args: ReadFileArgs) -> Result<Value, String> {
    let path = expand_tilde(&args.path);
    validate_safe_path(&path)?;

    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed to read file {}: {e}", path.display()))?;

    Ok(json!({ "path": path.to_string_lossy(), "content": content }))
}

async fn silo_search(state: &SharedState, query: String, top_k: Option<usize>) -> Result<Value, String> {
    if !state.db.is_enabled() {
        let reason = state
            .db
            .disabled_reason()
            .unwrap_or("unknown reason")
            .to_string();
        return Err(format!("Knowledge base is disabled: {reason}"));
    }

    let k = top_k.unwrap_or(10).clamp(1, 50);
    let qvec = state
        .embedder
        .embed_query(query)
        .await
        .map_err(|e| format!("Embedding failed: {e}"))?;

    let hits = state
        .db
        .search_chunks_by_vector(&qvec, k)
        .await
        .map_err(|e| format!("DB search failed: {e}"))?;

    Ok(json!({ "hits": hits }))
}

fn validate_safe_path(path: &Path) -> Result<(), String> {
    // Light "safety" check: reject obviously weird inputs; you can tighten this later.
    if path.as_os_str().is_empty() {
        return Err("Path must not be empty".to_string());
    }
    if path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return Err("Path must not contain '..'".to_string());
    }
    Ok(())
}


