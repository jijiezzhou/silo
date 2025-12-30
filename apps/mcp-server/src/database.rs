use serde::{Deserialize, Serialize};
use std::path::Path;

#[cfg(feature = "lancedb")]
use std::path::PathBuf;

#[cfg(feature = "lancedb")]
use std::sync::Arc;

pub type DatabaseHandle = std::sync::Arc<Database>;

const EMBEDDING_DIM: usize = crate::embed::EMBEDDING_DIM;

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
    #[cfg(feature = "lancedb")]
    #[error("arrow error: {0}")]
    Arrow(#[from] arrow_schema::ArrowError),
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
            // lancedb 0.4.x: connect(...) returns a builder; call execute().await to connect.
            let conn = lancedb::connect(data_dir.to_string_lossy().as_ref())
                .execute()
                .await?;
            let table = open_or_create_table(&conn, TABLE_NAME).await?;
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
            let Database::Enabled(db) = self else {
                return Ok(());
            };

            let mut table = db.table.lock().await;
            add_row(
                &mut table,
                Row {
                    id: blake3::hash(format!("{path}\n0").as_bytes()).to_hex().to_string(),
                    path: path.to_string(),
                    chunk_index: 0,
                    start_token: 0,
                    end_token: 0,
                    content: content.to_string(),
                    embedding: zero_embedding(),
                },
            )
            .await?;
        }
        Ok(())
    }

    /// Stores a chunk row (placeholder embedding).
    ///
    /// This is the Phase 2.3 ingestion target. When embeddings become real, this will store those.
    pub async fn add_chunk(
        &self,
        id: &str,
        path: &str,
        chunk_index: usize,
        start_token: usize,
        end_token: usize,
        content: &str,
        embedding: &[f32],
    ) -> Result<(), DbError> {
        let _ = (id, path, chunk_index, start_token, end_token, content, embedding);
        #[cfg(feature = "lancedb")]
        {
            let Database::Enabled(db) = self else {
                return Ok(());
            };

            let mut table = db.table.lock().await;
            add_row(
                &mut table,
                Row {
                    id: id.to_string(),
                    path: path.to_string(),
                    chunk_index,
                    start_token,
                    end_token,
                    content: content.to_string(),
                    embedding: embedding.to_vec(),
                },
            )
            .await?;
        }
        Ok(())
    }

    /// Searches documents (placeholder query embedding).
    pub async fn search_documents(&self, query: &str) -> Result<Vec<SearchHit>, DbError> {
        #[cfg(feature = "lancedb")]
        {
            use futures::TryStreamExt;
            use lancedb::query::{ExecutableQuery, QueryBase};
            let _ = query; // placeholder until real query embeddings
            let Database::Enabled(db) = self else {
                return Ok(vec![]);
            };

            let embedding = zero_embedding();
            let table = db.table.lock().await;
            let stream: lancedb::arrow::SendableRecordBatchStream = table
                .vector_search(embedding.as_slice())?
                .column("embedding")
                .limit(10)
                .execute()
                .await?;

            let batches = stream.try_collect::<Vec<arrow_array::RecordBatch>>().await?;
            let hits = batches_to_hits(batches);
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

// --- LanceDB integration (feature-gated) ---

#[cfg(feature = "lancedb")]
#[derive(Debug, Clone)]
struct Row {
    id: String,
    path: String,
    chunk_index: usize,
    start_token: usize,
    end_token: usize,
    content: String,
    embedding: Vec<f32>,
}

#[cfg(feature = "lancedb")]
fn documents_schema() -> arrow_schema::SchemaRef {
    use arrow_schema::{DataType, Field, Schema};
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Utf8, false),
        Field::new("path", DataType::Utf8, false),
        Field::new("chunk_index", DataType::Int64, false),
        Field::new("start_token", DataType::Int64, false),
        Field::new("end_token", DataType::Int64, false),
        Field::new("content", DataType::Utf8, false),
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                EMBEDDING_DIM as i32,
            ),
            true,
        ),
    ]))
}

#[cfg(feature = "lancedb")]
async fn open_or_create_table(conn: &lancedb::Connection, name: &str) -> Result<lancedb::Table, DbError> {
    match conn.open_table(name).execute().await {
        Ok(t) => Ok(t),
        Err(lancedb::Error::TableNotFound { .. }) => {
            let schema = documents_schema();
            Ok(conn.create_empty_table(name, schema).execute().await?)
        }
        Err(e) => Err(DbError::LanceDb(e)),
    }
}

#[cfg(feature = "lancedb")]
async fn add_row(table: &mut lancedb::Table, row: Row) -> Result<(), DbError> {
    use arrow_array::{
        types::Float32Type, FixedSizeListArray, Int64Array, RecordBatch, RecordBatchIterator,
        StringArray,
    };

    let schema = documents_schema();

    let id_arr = Arc::new(StringArray::from(vec![row.id]));
    let path_arr = Arc::new(StringArray::from(vec![row.path]));
    let chunk_index_arr = Arc::new(Int64Array::from(vec![row.chunk_index as i64]));
    let start_token_arr = Arc::new(Int64Array::from(vec![row.start_token as i64]));
    let end_token_arr = Arc::new(Int64Array::from(vec![row.end_token as i64]));
    let content_arr = Arc::new(StringArray::from(vec![row.content]));

    let emb_list = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
        std::iter::once(Some(row.embedding.into_iter().map(Some).collect::<Vec<_>>())),
        EMBEDDING_DIM as i32,
    );
    let emb_arr = Arc::new(emb_list);

    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            id_arr,
            path_arr,
            chunk_index_arr,
            start_token_arr,
            end_token_arr,
            content_arr,
            emb_arr,
        ],
    )?;

    let batches = RecordBatchIterator::new(vec![Ok(batch)].into_iter(), schema);
    table.add(Box::new(batches)).execute().await?;
    Ok(())
}

#[cfg(feature = "lancedb")]
fn batches_to_hits(batches: Vec<arrow_array::RecordBatch>) -> Vec<SearchHit> {
    use arrow_array::cast::AsArray;
    let mut hits = vec![];
    for b in batches {
        let Some(path_col) = b.column_by_name("path") else { continue };
        let paths = path_col.as_string::<i32>();

        let content_opt = b.column_by_name("content").map(|c| c.as_string::<i32>());
        let distance_opt = b.column_by_name("_distance").map(|c| c.as_primitive::<arrow_array::types::Float32Type>());

        for i in 0..b.num_rows() {
            let path = paths.value(i).to_string();
            let content_preview = content_opt
                .as_ref()
                .map(|c| preview(c.value(i), 240));
            let score = distance_opt.as_ref().map(|d| d.value(i));
            hits.push(SearchHit { path, score, content_preview });
        }
    }
    hits
}


