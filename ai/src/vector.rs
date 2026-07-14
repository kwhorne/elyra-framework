//! Lightweight vector search for retrieval-augmented generation (RAG).
//!
//! A portable, in-memory store: hold `(embedding, payload)` pairs and rank them
//! by cosine similarity. Elyra's database layer uses the sqlx `Any` driver,
//! which has no native vector type (no pgvector), so this deliberately ranks in
//! Rust — a good fit for the small-to-medium datasets typical of desktop apps.
//! Persist embeddings as a JSON/text column and load candidates per query (see
//! `docs/ai.md`).

use crate::{client::Ai, error::Result};

/// Cosine similarity of two vectors in `[-1.0, 1.0]` (0.0 if either is empty or
/// zero-length).
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// A ranked search hit.
#[derive(Clone, Debug)]
pub struct Match<T> {
    /// Cosine similarity to the query (higher is closer).
    pub score: f32,
    /// The stored payload.
    pub payload: T,
}

/// An in-memory vector store keyed by an arbitrary payload `T` (e.g. a row id,
/// title, or the text itself).
pub struct VectorStore<T> {
    items: Vec<(Vec<f32>, T)>,
}

impl<T> Default for VectorStore<T> {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

impl<T: Clone> VectorStore<T> {
    /// A new, empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a pre-computed embedding with its payload.
    pub fn add(&mut self, embedding: Vec<f32>, payload: T) {
        self.items.push((embedding, payload));
    }

    /// Embed `texts` (one API call) and add each with its payload.
    pub async fn add_texts<S: Into<String>>(&mut self, ai: &Ai, texts: Vec<(S, T)>) -> Result<()> {
        let (strings, payloads): (Vec<String>, Vec<T>) =
            texts.into_iter().map(|(s, p)| (s.into(), p)).unzip();
        let embeddings = ai.embeddings(strings).generate().await?;
        for (embedding, payload) in embeddings.into_iter().zip(payloads) {
            self.items.push((embedding, payload));
        }
        Ok(())
    }

    /// Rank the store against a query vector, returning the top `k` matches
    /// (highest similarity first).
    pub fn search(&self, query: &[f32], k: usize) -> Vec<Match<T>> {
        let mut scored: Vec<Match<T>> = self
            .items
            .iter()
            .map(|(embedding, payload)| Match {
                score: cosine_similarity(query, embedding),
                payload: payload.clone(),
            })
            .collect();
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(k);
        scored
    }

    /// Embed `query` and return the top `k` matches.
    pub async fn search_text(&self, ai: &Ai, query: &str, k: usize) -> Result<Vec<Match<T>>> {
        let embeddings = ai.embeddings([query]).generate().await?;
        let query_vec = embeddings.into_iter().next().unwrap_or_default();
        Ok(self.search(&query_vec, k))
    }

    /// Number of stored vectors.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_of_identical_and_orthogonal() {
        assert!((cosine_similarity(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine_similarity(&[], &[1.0]), 0.0);
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }

    #[test]
    fn search_ranks_by_similarity() {
        let mut store = VectorStore::new();
        store.add(vec![1.0, 0.0, 0.0], "east");
        store.add(vec![0.0, 1.0, 0.0], "north");
        store.add(vec![0.9, 0.1, 0.0], "east-ish");
        let hits = store.search(&[1.0, 0.0, 0.0], 2);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].payload, "east");
        assert_eq!(hits[1].payload, "east-ish");
        assert!(hits[0].score >= hits[1].score);
    }
}
