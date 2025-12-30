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
            const TABLE_NAME: &str = "silo_chunks_v1";
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
                    file_mtime_epoch_secs: None,
                    file_size_bytes: None,
                    file_hash: None,
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
                    file_mtime_epoch_secs: None,
                    file_size_bytes: None,
                    file_hash: None,
                    content: content.to_string(),
                    embedding: embedding.to_vec(),
                },
            )
            .await?;
        }
        Ok(())
    }

    /// Replace all chunks for a given file path:
    /// 1) delete existing rows for that path
    /// 2) batch-insert new rows
    pub async fn replace_file_chunks(
        &self,
        path: &str,
        file_mtime_epoch_secs: Option<i64>,
        file_size_bytes: Option<i64>,
        file_hash: Option<String>,
        rows: Vec<(usize, usize, usize, String, Vec<f32>)>, // (chunk_index, start_token, end_token, content, embedding)
    ) -> Result<(), DbError> {
        #[cfg(not(feature = "lancedb"))]
        {
            let _ = (
                path,
                file_mtime_epoch_secs,
                file_size_bytes,
                &file_hash,
                &rows,
            );
            return Ok(());
        }
        #[cfg(feature = "lancedb")]
        {
            let Database::Enabled(db) = self else {
                return Ok(());
            };

            let mut table = db.table.lock().await;
            delete_by_path(&mut table, path).await?;

            let mut out_rows: Vec<Row> = Vec::with_capacity(rows.len());
            for (chunk_index, start_token, end_token, content, embedding) in rows {
                let id = blake3::hash(
                    format!("{path}\n{chunk_index}\n{}", blake3::hash(content.as_bytes()).to_hex())
                        .as_bytes(),
                )
                .to_hex()
                .to_string();

                out_rows.push(Row {
                    id,
                    path: path.to_string(),
                    chunk_index,
                    start_token,
                    end_token,
                    file_mtime_epoch_secs,
                    file_size_bytes,
                    file_hash: file_hash.clone(),
                    content,
                    embedding,
                });
            }

            add_rows(&mut table, out_rows).await?;
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
            Ok(hits)
        }

        #[cfg(not(feature = "lancedb"))]
        {
            let _ = query;
            Ok(vec![])
        }
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
    file_mtime_epoch_secs: Option<i64>,
    file_size_bytes: Option<i64>,
    file_hash: Option<String>,
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
        Field::new("file_mtime_epoch_secs", DataType::Int64, true),
        Field::new("file_size_bytes", DataType::Int64, true),
        Field::new("file_hash", DataType::Utf8, true),
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
    let file_mtime_arr = Arc::new(Int64Array::from(vec![row.file_mtime_epoch_secs]));
    let file_size_arr = Arc::new(Int64Array::from(vec![row.file_size_bytes]));
    let file_hash_arr = Arc::new(StringArray::from(vec![row.file_hash]));
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
            file_mtime_arr,
            file_size_arr,
            file_hash_arr,
            content_arr,
            emb_arr,
        ],
    )?;

    let batches = RecordBatchIterator::new(vec![Ok(batch)].into_iter(), schema);
    table.add(Box::new(batches)).execute().await?;
    Ok(())
}

#[cfg(feature = "lancedb")]
async fn add_rows(table: &mut lancedb::Table, rows: Vec<Row>) -> Result<(), DbError> {
    use arrow_array::{
        types::Float32Type, FixedSizeListArray, Int64Array, RecordBatch, RecordBatchIterator,
        StringArray,
    };

    if rows.is_empty() {
        return Ok(());
    }

    let schema = documents_schema();

    let id_arr = Arc::new(StringArray::from(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>()));
    let path_arr =
        Arc::new(StringArray::from(rows.iter().map(|r| r.path.as_str()).collect::<Vec<_>>()));
    let chunk_index_arr =
        Arc::new(Int64Array::from(rows.iter().map(|r| r.chunk_index as i64).collect::<Vec<_>>()));
    let start_token_arr =
        Arc::new(Int64Array::from(rows.iter().map(|r| r.start_token as i64).collect::<Vec<_>>()));
    let end_token_arr =
        Arc::new(Int64Array::from(rows.iter().map(|r| r.end_token as i64).collect::<Vec<_>>()));

    let file_mtime_arr =
        Arc::new(Int64Array::from(rows.iter().map(|r| r.file_mtime_epoch_secs).collect::<Vec<_>>()));
    let file_size_arr =
        Arc::new(Int64Array::from(rows.iter().map(|r| r.file_size_bytes).collect::<Vec<_>>()));
    let file_hash_arr = Arc::new(StringArray::from(
        rows.iter().map(|r| r.file_hash.as_deref()).collect::<Vec<_>>(),
    ));

    let content_arr =
        Arc::new(StringArray::from(rows.iter().map(|r| r.content.as_str()).collect::<Vec<_>>()));

    let emb_list = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
        rows.into_iter().map(|r| {
            Some(r.embedding.into_iter().map(Some).collect::<Vec<_>>())
        }),
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
            file_mtime_arr,
            file_size_arr,
            file_hash_arr,
            content_arr,
            emb_arr,
        ],
    )?;

    let batches = RecordBatchIterator::new(vec![Ok(batch)].into_iter(), schema);
    table.add(Box::new(batches)).execute().await?;
    Ok(())
}

#[cfg(feature = "lancedb")]
async fn delete_by_path(table: &mut lancedb::Table, path: &str) -> Result<(), DbError> {
    // NOTE: LanceDB expects SQL predicate strings.
    let escaped = path.replace('\'', "''");
    let predicate = format!("path = '{escaped}'");
    table.delete(&predicate).await?;
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


