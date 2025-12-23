use crate::database::DatabaseHandle;
use crate::tools::{self, ToolCallParams, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::borrow::Cow;
use tokio::io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Runs a JSON-RPC 2.0 server over stdio, compatible with Anthropic MCP-style calls.
///
/// Expected methods:
/// - `initialize`: MCP handshake
/// - `tools/list`: returns tool definitions
/// - `tools/call`: executes a tool call
///
/// We also support `mcp.list_tools` / `mcp.call_tool` as aliases for convenience.
pub async fn run_stdio_server(db: DatabaseHandle) -> Result<(), ServerFatalError> {
    let stdin = io::stdin();
    let stdout = io::stdout();

    let mut reader = BufReader::new(stdin).lines();
    let mut writer = io::BufWriter::new(stdout);

    while let Some(line) = reader.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parsed: Result<JsonRpcRequest, _> = serde_json::from_str(line);
        let req = match parsed {
            Ok(r) => r,
            Err(e) => {
                // Can't extract an id reliably if JSON is invalid -> treat as a notification.
                write_json(
                    &mut writer,
                    &JsonRpcResponse::<Value>::error(
                        None,
                        JsonRpcError::invalid_request(format!("Invalid JSON: {e}")),
                    ),
                )
                .await?;
                continue;
            }
        };

        // Notifications (no id) are allowed; we do not respond per JSON-RPC.
        let Some(id) = req.id.clone() else {
            let _ = handle_request(req, &db).await;
            continue;
        };

        let resp = match handle_request(req, &db).await {
            Ok(result) => JsonRpcResponse::result(Some(id), result),
            Err(err) => JsonRpcResponse::<Value>::error(Some(id), err),
        };

        write_json(&mut writer, &resp).await?;
    }

    Ok(())
}

async fn handle_request(req: JsonRpcRequest, db: &DatabaseHandle) -> Result<Value, JsonRpcError> {
    if req.jsonrpc != "2.0" {
        return Err(JsonRpcError::invalid_request(
            "Only JSON-RPC 2.0 is supported".to_string(),
        ));
    }

    match req.method.as_str() {
        "initialize" => {
            let params = req
                .params
                .ok_or_else(|| JsonRpcError::invalid_params("Missing params".to_string()))?;
            let init: InitializeParams =
                serde_json::from_value(params).map_err(|e| {
                    JsonRpcError::invalid_params(format!("Invalid initialize params: {e}"))
                })?;

            // Echo the negotiated protocol version back (or fall back to the client's).
            let protocol_version = if init.protocol_version.is_empty() {
                "2024-11-05".to_string()
            } else {
                init.protocol_version
            };

            Ok(json!({
                "protocolVersion": protocol_version,
                "capabilities": {
                    "tools": { "listChanged": false }
                },
                "serverInfo": {
                    "name": "silo-mcp-server",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }))
        }
        "tools/list" | "mcp.list_tools" => {
            let tools = tools::tool_definitions();
            Ok(json!({ "tools": tools }))
        }
        "tools/call" | "mcp.call_tool" => {
            let params = req
                .params
                .ok_or_else(|| JsonRpcError::invalid_params("Missing params".to_string()))?;

            let call: ToolCallParams = serde_json::from_value(params).map_err(|e| {
                JsonRpcError::invalid_params(format!("Invalid mcp.call_tool params: {e}"))
            })?;

            let ToolResult { content, is_error } = tools::call_tool(db, call).await;
            Ok(json!({ "content": content, "isError": is_error }))
        }
        other => Err(JsonRpcError::method_not_found(format!(
            "Unknown method: {other}"
        ))),
    }
}

async fn write_json<W: AsyncWriteExt + Unpin, T: Serialize>(
    writer: &mut W,
    value: &T,
) -> Result<(), ServerFatalError> {
    let s = serde_json::to_string(value)?;
    writer.write_all(s.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<JsonRpcId>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum JsonRpcId {
    Number(i64),
    String(String),
}

#[derive(Debug, Deserialize)]
struct InitializeParams {
    #[serde(rename = "protocolVersion")]
    protocol_version: String,
    #[serde(default)]
    capabilities: Value,
    #[serde(rename = "clientInfo")]
    #[serde(default)]
    client_info: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse<T> {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<JsonRpcId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

impl<T> JsonRpcResponse<T> {
    fn result(id: Option<JsonRpcId>, result: T) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }
}

impl JsonRpcResponse<Value> {
    fn error(id: Option<JsonRpcId>, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: Cow<'static, str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

impl JsonRpcError {
    fn invalid_request(message: String) -> Self {
        Self {
            code: -32600,
            message: Cow::Borrowed("Invalid Request"),
            data: Some(json!({ "detail": message })),
        }
    }

    fn method_not_found(message: String) -> Self {
        Self {
            code: -32601,
            message: Cow::Borrowed("Method not found"),
            data: Some(json!({ "detail": message })),
        }
    }

    fn invalid_params(message: String) -> Self {
        Self {
            code: -32602,
            message: Cow::Borrowed("Invalid params"),
            data: Some(json!({ "detail": message })),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ServerFatalError {
    #[error("stdio I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json serialization error: {0}")]
    Json(#[from] serde_json::Error),
}


