use std::sync::Arc;

pub const EMBEDDING_DIM: usize = 384;

pub type EmbedderHandle = Arc<dyn Embedder + Send + Sync>;

#[derive(Debug, Clone)]
pub enum EmbedderKind {
    Noop,
    #[cfg(feature = "embeddings")]
    FastEmbed,
}

#[async_trait::async_trait]
pub trait Embedder {
    fn kind(&self) -> EmbedderKind;
    fn dim(&self) -> usize {
        EMBEDDING_DIM
    }

    async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, String>;

    async fn embed_query(&self, query: String) -> Result<Vec<f32>, String> {
        let mut out = self.embed_texts(vec![query]).await?;
        out.pop().ok_or_else(|| "embedder returned no vectors".to_string())
    }
}

pub struct NoopEmbedder;

#[async_trait::async_trait]
impl Embedder for NoopEmbedder {
    fn kind(&self) -> EmbedderKind {
        EmbedderKind::Noop
    }

    async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, String> {
        Ok(texts.into_iter().map(|_| vec![0.0; EMBEDDING_DIM]).collect())
    }
}

#[cfg(feature = "embeddings")]
pub struct FastEmbedder {
    model: Arc<fastembed::TextEmbedding>,
}

#[cfg(feature = "embeddings")]
impl FastEmbedder {
    pub fn try_new_default() -> Result<Self, String> {
        // NOTE: fastembed API may differ slightly by version. If cargo reports a mismatch,
        // we will adjust the initialization accordingly.
        let opts = fastembed::InitOptions::new(fastembed::EmbeddingModel::BGESmallENV15);
        let model = fastembed::TextEmbedding::try_new(opts).map_err(|e| format!("{e}"))?;
        Ok(Self {
            model: Arc::new(model),
        })
    }
}

#[cfg(feature = "embeddings")]
#[async_trait::async_trait]
impl Embedder for FastEmbedder {
    fn kind(&self) -> EmbedderKind {
        EmbedderKind::FastEmbed
    }

    async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, String> {
        // fastembed is CPU-bound; run in blocking pool.
        let model = self.model.clone();
        tokio::task::spawn_blocking(move || {
            model
                .embed(texts, None)
                .map_err(|e| format!("{e}"))
        })
        .await
        .map_err(|e| format!("embed task failed: {e}"))?
    }
}


