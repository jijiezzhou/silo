use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TextChunk {
    pub index: usize,
    pub text: String,
    pub start_token: usize,
    pub end_token: usize,
}

/// MVP tokenization: whitespace tokens.
///
/// This matches the "500-token chunks with overlap" requirement approximately.
/// Later we can swap to a real tokenizer (e.g. tiktoken) without changing callers.
pub fn chunk_by_whitespace_tokens(text: &str, chunk_tokens: usize, overlap_tokens: usize) -> Vec<TextChunk> {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.is_empty() || chunk_tokens == 0 {
        return vec![];
    }

    let overlap = overlap_tokens.min(chunk_tokens.saturating_sub(1));
    let mut chunks = vec![];

    let mut start = 0usize;
    let mut idx = 0usize;
    while start < tokens.len() {
        let end = (start + chunk_tokens).min(tokens.len());
        let slice = &tokens[start..end];
        let chunk_text = slice.join(" ");

        chunks.push(TextChunk {
            index: idx,
            text: chunk_text,
            start_token: start,
            end_token: end,
        });

        idx += 1;
        if end == tokens.len() {
            break;
        }
        start = end.saturating_sub(overlap);
    }

    chunks
}


