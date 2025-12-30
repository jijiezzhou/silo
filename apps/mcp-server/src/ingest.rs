use crate::chunk::chunk_by_whitespace_tokens;
use crate::database::DatabaseHandle;
use crate::embed::EmbedderHandle;
use crate::extract::extract_text;
use crate::state::expand_tilde;
use blake3::Hash;
use serde::Serialize;

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
    embedder: &EmbedderHandle,
    path: &str,
    max_text_bytes: u64,
    chunk_tokens: usize,
    chunk_overlap_tokens: usize,
) -> Result<IngestStats, String> {
    let path = expand_tilde(path);
    let path_str = path.to_string_lossy().to_string();

    let file_meta = tokio::fs::metadata(&path)
        .await
        .ok();
    let file_size_bytes = file_meta.as_ref().map(|m| m.len() as i64);
    let file_mtime_epoch_secs = file_meta
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);

    let extracted = extract_text(&path, max_text_bytes).await?;
    let extracted_chars = extracted.text.chars().count();
    let file_hash = Some(blake3::hash(extracted.text.as_bytes()).to_hex().to_string());

    let chunks = chunk_by_whitespace_tokens(&extracted.text, chunk_tokens, chunk_overlap_tokens);

    let embeddings = embedder
        .embed_texts(chunks.iter().map(|c| c.text.clone()).collect())
        .await?;
    if embeddings.len() != chunks.len() {
        return Err(format!(
            "embedder returned {} vectors for {} chunks",
            embeddings.len(),
            chunks.len()
        ));
    }

    // Store only if DB is enabled (feature `lancedb` and initialization succeeded).
    let stored = if db.is_enabled() {
        let rows = chunks
            .iter()
            .zip(embeddings.iter())
            .map(|(ch, emb)| (ch.index, ch.start_token, ch.end_token, ch.text.clone(), emb.clone()))
            .collect::<Vec<_>>();

        db.replace_file_chunks(
            &path_str,
            file_mtime_epoch_secs,
            file_size_bytes,
            file_hash.clone(),
            rows,
        )
        .await
        .map_err(|e| format!("DB write failed: {e}"))?;
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


