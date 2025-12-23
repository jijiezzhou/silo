use serde::{Deserialize, Serialize};
use std::path::Path;

#[cfg(feature = "lancedb")]
use std::path::PathBuf;

pub type DatabaseHandle = std::sync::Arc<Database>;

const EMBEDDING_DIM: usize = 384;

#[derive(Clone)]
pub enum Database {
    #[cfg(feature = "lancedb")]
    Enabled(EnabledDatabase),
    Disabled { reason: String },
}

#[cfg(feature = "lancedb")]
#[derive(Clone)]
pub struct EnabledDatabase {
    #[allow(dead_code)]
    data_dir: PathBuf,
    // We keep the table behind a mutex to avoid relying on Table's thread-safety guarantees.
    table: std::sync::Arc<tokio::sync::Mutex<lancedb::Table>>,
}

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[cfg(feature = "lancedb")]
    #[error("lancedb error: {0}")]
    LanceDb(#[from] lancedb::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchHit {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_preview: Option<String>,
}

impl Database {
    /// Create or open the local DB.
    ///
    /// - With feature `lancedb`: opens/creates a local LanceDB at `data_dir`.
    /// - Without: returns a disabled DB (so Milestone 1 handshake/tools still work).
    pub async fn new(data_dir: impl AsRef<Path>) -> Result<Self, DbError> {
        #[cfg(feature = "lancedb")]
        {
            const TABLE_NAME: &str = "documents";
            let data_dir = data_dir.as_ref().to_path_buf();
            tokio::fs::create_dir_all(&data_dir).await?;
            let conn = lancedb::connect(data_dir.to_string_lossy().as_ref()).await?;
            let table = conn.open_or_create_table(TABLE_NAME, &[]).await?;
            return Ok(Database::Enabled(EnabledDatabase {
                data_dir,
                table: std::sync::Arc::new(tokio::sync::Mutex::new(table)),
            }));
        }

        #[cfg(not(feature = "lancedb"))]
        {
            let _ = data_dir;
            Ok(Database::Disabled {
                reason: "LanceDB is not enabled. Rebuild with `--features lancedb`.".to_string(),
            })
        }
    }

    /// A safe fallback mode where DB-backed tools return a clear error string instead of crashing.
    pub fn disabled(reason: String) -> Self {
        Database::Disabled { reason }
    }

    pub fn is_enabled(&self) -> bool {
        #[cfg(feature = "lancedb")]
        {
            matches!(self, Database::Enabled(_))
        }
        #[cfg(not(feature = "lancedb"))]
        {
            false
        }
    }

    pub fn disabled_reason(&self) -> Option<&str> {
        match self {
            Database::Disabled { reason } => Some(reason.as_str()),
            #[cfg(feature = "lancedb")]
            Database::Enabled(_) => None,
        }
    }

    /// Stores a document (placeholder embedding).
    pub async fn add_document(&self, path: &str, content: &str) -> Result<(), DbError> {
        let _ = (path, content);
        #[cfg(feature = "lancedb")]
        {
            use serde_json::Value;
            let Database::Enabled(db) = self else {
                return Ok(());
            };

            let embedding = zero_embedding();
            let row = serde_json::json!({
                "path": path,
                "content": content,
                "embedding": embedding,
            });

            let mut table = db.table.lock().await;
            insert_rows(&mut table, vec![row]).await?;
        }
        Ok(())
    }

    /// Searches documents (placeholder query embedding).
    pub async fn search_documents(&self, query: &str) -> Result<Vec<SearchHit>, DbError> {
        let _ = query;
        #[cfg(feature = "lancedb")]
        {
            use serde_json::Value;
            let Database::Enabled(db) = self else {
                return Ok(vec![]);
            };

            let embedding = zero_embedding();
            let table = db.table.lock().await;
            let rows = search_rows(&table, embedding, 10).await?;

            let hits = rows
                .into_iter()
                .filter_map(|row| {
                    let path = row.get("path")?.as_str()?.to_string();
                    let score = row.get("_distance").and_then(|v| v.as_f64()).map(|f| f as f32);
                    let content_preview = row
                        .get("content")
                        .and_then(|v| v.as_str())
                        .map(|s| preview(s, 240));
                    Some(SearchHit {
                        path,
                        score,
                        content_preview,
                    })
                })
                .collect();

            return Ok(hits);
        }

        Ok(vec![])
    }
}

fn zero_embedding() -> Vec<f32> {
    vec![0.0; EMBEDDING_DIM]
}

fn preview(s: &str, max_chars: usize) -> String {
    let mut out = s.chars().take(max_chars).collect::<String>();
    if s.chars().count() > max_chars {
        out.push_str("…");
    }
    out
}

#[cfg(feature = "lancedb")]
async fn insert_rows(table: &mut lancedb::Table, rows: Vec<serde_json::Value>) -> Result<(), DbError> {
    table.insert(&rows).await?;
    Ok(())
}

#[cfg(feature = "lancedb")]
async fn search_rows(
    table: &lancedb::Table,
    embedding: Vec<f32>,
    limit: usize,
) -> Result<Vec<serde_json::Value>, DbError> {
    let rows: Vec<serde_json::Value> = table.search(&embedding).limit(limit).execute().await?;
    Ok(rows)
}


