//! Local code embeddings: jina-embeddings-v2-base-code via fastembed/ONNX.
//! Model weights download once to the fastembed cache (~/.fastembed or
//! FASTEMBED_CACHE_PATH); everything runs offline after that.

use anyhow::{Context, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

/// Attention memory is batch × heads × seq² per layer, and ONNX Runtime's
/// arena keeps its peak forever — so cap sequence length hard (512 tokens is
/// plenty for similarity) and keep batches small. At 8 × 512² this stays in
/// the hundreds of MB instead of many GB.
const MAX_TOKENS: usize = 512;
const BATCH: usize = 16;

static MODEL: std::sync::OnceLock<TextEmbedding> = std::sync::OnceLock::new();

fn model() -> Result<&'static TextEmbedding> {
    if MODEL.get().is_none() {
        let m = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::JinaEmbeddingsV2BaseCode)
                .with_show_download_progress(true)
                .with_max_length(MAX_TOKENS),
        )
        .context("initialize jina-embeddings-v2-base-code (first run downloads the model)")?;
        let _ = MODEL.set(m);
    }
    Ok(MODEL.get().unwrap())
}

pub fn embed(texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
    let model = model()?;
    let total = texts.len();
    let mut out: Vec<Vec<f32>> = Vec::with_capacity(total);
    for (i, chunk) in texts.chunks(256).enumerate() {
        out.extend(model.embed(chunk.to_vec(), Some(BATCH)).context("embed")?);
        if total > 256 {
            eprintln!("[dupfinder] embedded {}/{}", (i * 256 + chunk.len()).min(total), total);
        }
    }
    for v in &mut out {
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            v.iter_mut().for_each(|x| *x /= norm);
        }
    }
    Ok(out)
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
