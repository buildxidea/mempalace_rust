use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use model2vec_rs::model::StaticModel;

use super::Embedder;

pub struct Model2VecEmbedder {
    model: Arc<StaticModel>,
    #[allow(dead_code)]
    model_name: &'static str,
    dim: usize,
    fingerprint: String,
}

impl Model2VecEmbedder {
    pub fn with_model(
        model_name: impl Into<String>,
        _cache_dir: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let model_name_owned = model_name.into();
        let model = StaticModel::from_pretrained(&model_name_owned, None, None, None)
            .with_context(|| format!("model2vec: failed to load {}", model_name_owned))?;

        let dim = model.encode(&[".".to_string()])[0].len();
        let fp = format!("model2vec:{}:{}", model_name_owned, dim);

        Ok(Self {
            model: Arc::new(model),
            model_name: Box::leak(model_name_owned.into_boxed_str()),
            dim,
            fingerprint: fp,
        })
    }
}

#[async_trait]
impl Embedder for Model2VecEmbedder {
    fn dim(&self) -> usize {
        self.dim
    }

    fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    async fn embed(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let mut out = self.embed_batch(&[text]).await?;
        Ok(out.pop().unwrap_or_default())
    }

    async fn embed_batch(&self, texts: &[&str]) -> anyhow::Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let owned: Vec<String> = texts.iter().map(|s| (*s).to_owned()).collect();
        let model = Arc::clone(&self.model);
        let vectors = tokio::task::spawn_blocking(move || model.encode(&owned))
            .await
            .context("model2vec: blocking task panicked")?;
        Ok(vectors)
    }
}
