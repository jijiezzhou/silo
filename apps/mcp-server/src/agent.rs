use crate::state::SharedState;
use crate::tools::{call_tool_no_agent, tool_definitions, ToolCallParams};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct AgentArgs {
    task: String,
}

#[derive(Debug, Deserialize)]
struct AgentPlan {
    /// Tool name from `tools/list`.
    tool: String,
    /// JSON arguments for that tool.
    #[serde(default)]
    arguments: Value,
}

pub async fn agent_tool(state: &SharedState, args: Value) -> Result<Value, String> {
    let args: AgentArgs = serde_json::from_value(args).map_err(|e| format!("Invalid arguments: {e}"))?;

    // Expose *all* tools except the agent itself (avoid recursion).
    let tools = tool_definitions()
        .into_iter()
        .filter(|t| t.name != "silo_agent")
        .collect::<Vec<_>>();

    let prompt = format!(
        r#"You are a local-first desktop assistant for the Silo app.

You MUST respond with a SINGLE LINE of JSON only (no markdown, no explanation).
The JSON must have this exact shape:
{{"tool":"<tool_name>","arguments":{{...}}}}

Pick the ONE best tool to accomplish the user task. If the task cannot be done with the tools,
return: {{"tool":"none","arguments":{{"reason":"..."}}}}

Available tools (name + description + JSON schema):
{}

User task: {}
"#,
        serde_json::to_string_pretty(&tools).unwrap_or_else(|_| "[]".to_string()),
        args.task
    );

    let raw = state.llm.generate(prompt).await?;
    let raw = raw.trim();
    let plan: AgentPlan = serde_json::from_str(raw)
        .map_err(|e| format!("LLM returned non-JSON or invalid shape: {e}\nraw: {raw}"))?;

    if plan.tool == "none" {
        return Ok(json!({
            "ok": false,
            "reason": plan.arguments.get("reason").cloned().unwrap_or_else(|| json!("no reason provided")),
            "raw": raw,
        }));
    }

    let tool_for_debug = plan.tool.clone();
    let args_for_debug = plan.arguments.clone();

    let res = call_tool_no_agent(
        state,
        ToolCallParams {
            name: plan.tool,
            arguments: plan.arguments,
        },
    )
    .await;

    Ok(json!({
        "ok": !res.is_error,
        "content": res.content,
        "raw_plan": { "tool": tool_for_debug, "arguments": args_for_debug }
    }))
}


