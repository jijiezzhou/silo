use crate::chunk::chunk_by_whitespace_tokens;
use crate::database::DatabaseHandle;
use crate::extract::extract_text;
use crate::state::expand_tilde;
use blake3::Hash;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct IngestStats {
    pub path: String,
    pub extracted_kind: String,
    pub extracted_chars: usize,
    pub chunk_tokens: usize,
    pub chunk_overlap_tokens: usize,
    pub chunks: usize,
    pub stored: bool,
}

/// Process a single file:
/// 1) extract text
/// 2) chunk into ~token windows (whitespace tokens)
/// 3) embed (placeholder zeros for now)
/// 4) store chunks into LanceDB when enabled
pub async fn process_file(
    db: &DatabaseHandle,
    path: &str,
    max_text_bytes: u64,
    chunk_tokens: usize,
    chunk_overlap_tokens: usize,
) -> Result<IngestStats, String> {
    let path = expand_tilde(path);
    let path_str = path.to_string_lossy().to_string();

    let extracted = extract_text(&path, max_text_bytes).await?;
    let extracted_chars = extracted.text.chars().count();

    let chunks = chunk_by_whitespace_tokens(&extracted.text, chunk_tokens, chunk_overlap_tokens);

    // Store only if DB is enabled (feature `lancedb` and initialization succeeded).
    let stored = if db.is_enabled() {
        for ch in &chunks {
            let id = chunk_id(&path_str, ch.index, &ch.text);
            db.add_chunk(&id, &path_str, ch.index, ch.start_token, ch.end_token, &ch.text)
                .await
                .map_err(|e| format!("DB insert failed: {e}"))?;
        }
        true
    } else {
        false
    };

    Ok(IngestStats {
        path: path_str,
        extracted_kind: format!("{:?}", extracted.kind).to_lowercase(),
        extracted_chars,
        chunk_tokens,
        chunk_overlap_tokens,
        chunks: chunks.len(),
        stored,
    })
}

fn chunk_id(path: &str, chunk_index: usize, text: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(path.as_bytes());
    hasher.update(b"\n");
    hasher.update(chunk_index.to_string().as_bytes());
    hasher.update(b"\n");
    hasher.update(text.as_bytes());
    let h: Hash = hasher.finalize();
    h.to_hex().to_string()
}


