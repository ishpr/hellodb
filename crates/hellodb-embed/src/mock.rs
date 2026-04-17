//! Deterministic mock embedder for tests and offline demos.
//!
//! Hashes the input text into a 384-dim pseudo-embedding. Same text →
//! same vector. Different texts → different vectors (with non-zero
//! cosine similarity when they share characters). Not semantically
//! meaningful — it's a mock — but stable and zero-cost.

use crate::embedder::Embedder;
use crate::error::EmbedError;

#[derive(Clone)]
pub struct MockEmbedder {
    dim: usize,
}

impl Default for MockEmbedder {
    fn default() -> Self {
        Self { dim: 384 }
    }
}

impl MockEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }

    fn hash_to_vec(&self, text: &str) -> Vec<f32> {
        // Seed a simple LCG from FNV-1a hash of the text. Output is
        // stable across runs (important for test determinism).
        let mut h: u64 = 0xcbf29ce484222325;
        for b in text.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        let mut state = h | 1;
        let mut out = Vec::with_capacity(self.dim);
        for _ in 0..self.dim {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let f = ((state >> 32) as u32 as f32) / (u32::MAX as f32); // [0,1)
            out.push(f * 2.0 - 1.0); // [-1, 1)
        }
        // Normalize to unit length so cosine similarity behaves.
        let n: f32 = out.iter().map(|x| x * x).sum::<f32>().sqrt();
        if n > 0.0 {
            for x in &mut out {
                *x /= n;
            }
        }
        out
    }
}

impl Embedder for MockEmbedder {
    fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        if text.is_empty() {
            return Err(EmbedError::EmptyInput);
        }
        Ok(self.hash_to_vec(text))
    }

    fn dim(&self) -> usize {
        self.dim
    }
    fn model_id(&self) -> &str {
        "mock-fnv-lcg-v1"
    }
    fn backend_name(&self) -> &'static str {
        "mock"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_text_same_vec() {
        let e = MockEmbedder::default();
        let a = e.embed_one("hello").unwrap();
        let b = e.embed_one("hello").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn different_text_different_vec() {
        let e = MockEmbedder::default();
        let a = e.embed_one("hello").unwrap();
        let b = e.embed_one("world").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn normalized_to_unit_length() {
        let e = MockEmbedder::default();
        let v = e.embed_one("hellodb is great").unwrap();
        let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((n - 1.0).abs() < 1e-3, "norm = {n}");
    }

    #[test]
    fn empty_rejected() {
        let e = MockEmbedder::default();
        assert!(matches!(e.embed_one(""), Err(EmbedError::EmptyInput)));
    }

    #[test]
    fn batch_matches_single() {
        let e = MockEmbedder::default();
        let batch = e
            .embed_batch(&["a".into(), "b".into(), "c".into()])
            .unwrap();
        assert_eq!(batch.len(), 3);
        assert_eq!(batch[0], e.embed_one("a").unwrap());
        assert_eq!(batch[2], e.embed_one("c").unwrap());
    }
}
