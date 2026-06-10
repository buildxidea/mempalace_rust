/// Search module: RRF fusion, session diversification, query expansion, smart search, and reranking.
///
/// Provides triple-stream hybrid search (BM25 + Vector + Graph) fused via
/// Reciprocal Rank Fusion (RRF), with optional LLM-based query expansion,
/// session-based result diversification, smart search modes, and cross-encoder reranking.
pub mod cjk_segmenter;
pub mod diversify;
pub mod followup;
pub mod query_expansion;
pub mod reranker;
pub mod rrf;
pub mod smart_search;
pub mod synonyms;

pub use cjk_segmenter::{detect_script, has_cjk, segment_cjk, Script};
pub use diversify::{diversify_by_session, DiversifiableResult, DEFAULT_MAX_PER_SESSION};
pub use query_expansion::{
    build_search_entities, build_search_queries, expand_query, extract_entities_from_query,
    parse_expansion_xml, QueryExpansion,
};
pub use reranker::{
    format_rerank_input, mock_score_fn, rerank_with_scores, RerankInput, RerankResult,
    DEFAULT_TOP_K, MAX_INPUT_LENGTH,
};
pub use rrf::{
    fuse_results, normalize_weights, rrf_score, FusedResult, RrfConfig, SearchStream, StreamResult,
    DEFAULT_BM25_WEIGHT, DEFAULT_GRAPH_WEIGHT, DEFAULT_VECTOR_WEIGHT, RRF_K,
};
pub use smart_search::{
    build_expand_results, compact_limit, CompactSearchResult, ExpandedResult, SmartSearchParams,
    COMPACT_OVER_FETCH, MAX_COMPACT_RESULTS, MAX_EXPAND_IDS,
};
pub use synonyms::{
    expand_query as expand_query_synonyms, get_synonyms, SYNONYM_BM25_WEIGHT, SYNONYM_GROUPS,
};
