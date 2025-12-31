use async_trait::async_trait;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

#[async_trait]
pub trait Llm: Send + Sync {
    async fn generate(&self, prompt: String) -> Result<String, String>;
}

pub type LlmHandle = std::sync::Arc<dyn Llm>;

/// Default LLM: disabled. Returns an actionable error message.
pub struct NoopLlm;

#[async_trait]
impl Llm for NoopLlm {
    async fn generate(&self, _prompt: String) -> Result<String, String> {
        Err(
            "Local LLM is not configured. Set SILO_LLM_BACKEND=ollama and SILO_LLM_MODEL (and optionally SILO_OLLAMA_PATH)".to_string(),
        )
    }
}

/// Local LLM via the `ollama` CLI (no network required).
///
/// Env vars (recommended for GUI apps with limited PATH):
/// - `SILO_OLLAMA_PATH`: absolute path to `ollama` binary (fallback: `ollama`)
/// - `SILO_LLM_MODEL`: model name (e.g. `llama3.2:3b`, `qwen2.5:7b`)
pub struct OllamaCliLlm {
    pub ollama_path: PathBuf,
    pub model: String,
}

#[async_trait]
impl Llm for OllamaCliLlm {
    async fn generate(&self, prompt: String) -> Result<String, String> {
        // `ollama run <model> "<prompt>"` prints completion to stdout.
        // Keep this dependency-light; we can switch to the HTTP API later if desired.
        let out = Command::new(&self.ollama_path)
            .arg("run")
            .arg(&self.model)
            .arg(prompt)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| format!("Failed to spawn ollama CLI at {}: {e}", self.ollama_path.display()))?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let hint = if stderr.to_ascii_lowercase().contains("could not connect to a running ollama instance")
                || stderr.to_ascii_lowercase().contains("could not connect")
            {
                " (hint: start the Ollama daemon: `ollama serve` or open the Ollama app)"
            } else {
                ""
            };
            return Err(if stderr.is_empty() {
                format!("ollama exited with status {}{hint}", out.status)
            } else {
                format!("ollama error: {stderr}{hint}")
            });
        }

        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}

pub fn llm_from_env() -> LlmHandle {
    let backend = std::env::var("SILO_LLM_BACKEND").unwrap_or_default().to_ascii_lowercase();
    if backend == "ollama" {
        let model = std::env::var("SILO_LLM_MODEL").unwrap_or_else(|_| "llama3.2:3b".to_string());
        let ollama_path = std::env::var_os("SILO_OLLAMA_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("ollama"));
        return std::sync::Arc::new(OllamaCliLlm { ollama_path, model });
    }

    std::sync::Arc::new(NoopLlm)
}


