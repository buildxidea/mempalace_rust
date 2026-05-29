/// Search module: RRF fusion, session diversification, and query expansion.
///
/// Provides triple-stream hybrid search (BM25 + Vector + Graph) fused via
/// Reciprocal Rank Fusion (RRF), with optional LLM-based query expansion
/// and session-based result diversification.
pub mod diversify;
pub mod query_expansion;
pub mod rrf;

pub use diversify::{diversify_by_session, DiversifiableResult, DEFAULT_MAX_PER_SESSION};
pub use query_expansion::{
    build_search_entities, build_search_queries, expand_query, extract_entities_from_query,
    parse_expansion_xml, QueryExpansion,
};
pub use rrf::{
    fuse_results, normalize_weights, rrf_score, FusedResult, RrfConfig, SearchStream, StreamResult,
    RRF_K, DEFAULT_BM25_WEIGHT, DEFAULT_GRAPH_WEIGHT, DEFAULT_VECTOR_WEIGHT,
};
