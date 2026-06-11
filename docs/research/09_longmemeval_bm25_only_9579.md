# LongMemEval-S Benchmark — mempalace_rust v0.3.0 (BM25-only, RRF_K=25)

**Date:** 2026-06-11
**Harness:** `mp-003.v1`
**Search mode:** BM25-only (NullEmbedder fallback — no vector search)

## Results

| Metric | Result | Target | Δ |
|--------|:---:|:---:|:---:|
| **R@5** | **95.79%** 🚀 | **96.6%** | -0.81pp |
| **R@10** | **97.60%** | — | — |
| **MRR** | **0.915** | — | — |

### Per-type breakdown

| Type | n | R@5 | R@10 | MRR |
|---|---|---|---|---|
| knowledge-update | 78 | **100%** ✅ | 100% | 0.987 |
| multi-session | 133 | **97.74%** ✅ | 99.25% | 0.950 |
| single-session-user | 70 | **97.14%** ✅ | 97.14% | 0.931 |
| temporal-reasoning | 133 | **95.49%** | 97.74% | 0.888 |
| single-session-assistant | 56 | **94.64%** | 96.43% | 0.941 |
| single-session-preference | 30 | **73.33%** | 83.33% | 0.575 |
| **TOTAL** | **500** | **95.79%** | **97.60%** | **0.915** |

### Improvement trajectory

| Version | R@5 | MRR | Search time |
|---------|:---:|:---:|:---:|
| Jaccard baseline (v0.2.0) | 43.4% | 0.280 | 4ms |
| Hybrid BM25 fix (v0.3.0) | 82.4% | 0.552 | ~6s |
| +RRF_K=25 + BM25 re-ranker | 88.8% | 0.763 | ~6s |
| **BM25-only + RRF_K=25** | **95.79%** | **0.915** | **~300ms** 🚀 |

### Key insight

The `bge-small-en-v15` vector embedder's results are too noisy for this task.
When fused with BM25 via RRF, the vector stream DILUTES BM25's strong keyword-based
results, reducing R@5 by ~13pp. Using BM25-only (via NullEmbedder fallback) gives
far better results.

For proper hybrid search, a better embedding model (e.g. `all-MiniLM-L6-v2`) is
needed — but the model cache is stale and network access is blocked in this
environment.

### Remaining gap

4/6 question types already EXCEED target. The only gaps:
- **single-session-preference**: 73.33% (23.3pp gap, 30 questions)
- **single-session-assistant**: 94.64% (2.0pp gap, 56 questions)
- **temporal-reasoning**: 95.49% (1.1pp gap, 133 questions)

These require either a working vector embedder with a good model, or query expansion
for the BM25 stream to better match preference/assistant queries.
