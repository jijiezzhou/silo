use crate::indexer::{index_roots, IndexOptions, IndexSummary};
use crate::state::AppState;
use crate::{database::Database, state::SharedState};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;

/// High-level API used by the desktop UI (Tauri) without going through MCP stdio.
pub struct SiloApp {
    pub state: SharedState,
}

impl SiloApp {
    pub async fn new() -> Result<Self, String> {
        let db = Database::new("./data")
            .await
            .map_err(|e| format!("db init failed: {e}"))?;
        let state = AppState::new(Arc::new(db))
            .await
            .map_err(|e| format!("state init failed: {e}"))?;
        Ok(Self { state })
    }

    pub async fn get_config(&self) -> serde_json::Value {
        self.state.get_config_json().await
    }

    pub async fn index_home(&self, max_files: Option<u64>, concurrency: Option<usize>) -> Result<IndexSummary, String> {
        let Some(policy) = self.state.filesystem_policy().await else {
            return Err("No filesystem policy configured".to_string());
        };
        let roots = self.state.filesystem_roots().await;
        let opts = IndexOptions {
            max_files,
            concurrency: concurrency.unwrap_or(2),
            max_sample_errors: 20,
        };
        Ok(index_roots(
            roots,
            Arc::new(policy),
            self.state.db.clone(),
            self.state.embedder.clone(),
            opts,
        )
        .await)
    }

    pub async fn search(&self, query: String, top_k: usize) -> Result<serde_json::Value, String> {
        let qvec = self
            .state
            .embedder
            .embed_query(query)
            .await
            .map_err(|e| format!("Embedding failed: {e}"))?;
        let hits = self
            .state
            .db
            .search_chunks_by_vector(&qvec, top_k.clamp(1, 50))
            .await
            .map_err(|e| format!("DB search failed: {e}"))?;
        Ok(serde_json::json!({ "hits": hits }))
    }
}


