// =====================================================================
// Telemetry integration tests — TDD RED phase
// =====================================================================
// These tests verify the metrics pipeline: counter/histogram registration,
// recording helpers, and Prometheus text format rendering.
// Run with: cargo test --features telemetry -p mempalace-core --test telemetry_test
//
// Architecture:
//   Metrics::test_init()  → in-process Prometheus handle, no global state
//   Metrics::init()       → real startup path (OnceLock singleton)
//   record_* helpers      → no-ops when telemetry feature is OFF (compile-time cfg)
// =====================================================================

#[cfg(feature = "telemetry")]
mod telemetry_integration {
    use mempalace_core::telemetry::{Metrics, HistogramEntry, CounterEntry};

    // ---------------------------------------------------------------------------
    // Helper: parse a Prometheus line like "mempalace_foo_total 42"
    // ---------------------------------------------------------------------------
    fn parse_counter(s: &str) -> Option<f64> {
        s.split_whitespace()
            .nth(1)
            .and_then(|v| v.parse().ok())
    }

    fn parse_histogram_count(s: &str) -> Option<u64> {
        s.split_whitespace()
            .nth(1)
            .and_then(|v| v.parse().ok())
    }

    // ---------------------------------------------------------------------------
    // Test: search counter increments and exports correct label
    // ---------------------------------------------------------------------------
    #[test]
    fn test_search_counter_increments() {
        let metrics = Metrics::test_init();
        metrics.record_search("ok", 42);
        let snapshot = metrics.handle().render();
        assert!(
            snapshot.contains(r#"mempalace_observations_searched_total{status="ok"} 1"#),
            "expected searched counter with ok label, got:\n{}", snapshot
        );
        assert!(
            snapshot.contains("mempalace_search_latency_ms_count 1"),
            "expected search latency histogram count, got:\n{}", snapshot
        );
    }

    // ---------------------------------------------------------------------------
    // Test: embed counter + histogram
    // ---------------------------------------------------------------------------
    #[test]
    fn test_embed_counter_and_histogram() {
        let metrics = Metrics::test_init();
        metrics.record_embed("fastembed", 15);
        let snap = metrics.handle().render();
        assert!(
            snap.contains(r#"mempalace_observations_embedded_total{provider="fastembed"} 1"#),
            "embed counter missing:\n{}", snap
        );
        assert!(
            snap.contains("mempalace_embed_latency_ms_count 1"),
            "embed latency histogram count missing:\n{}", snap
        );
    }

    // ---------------------------------------------------------------------------
    // Test: compression counter + histogram
    // ---------------------------------------------------------------------------
    #[test]
    fn test_compression_counter_and_histogram() {
        let metrics = Metrics::test_init();
        metrics.record_compress(88);
        let snap = metrics.handle().render();
        assert!(
            snap.contains(r#"mempalace_observations_compressed_total{} 1"#),
            "compressed counter missing:\n{}", snap
        );
        assert!(
            snap.contains("mempalace_compression_latency_ms_count 1"),
            "compression latency histogram count missing:\n{}", snap
        );
    }

    // ---------------------------------------------------------------------------
    // Test: LLM counter with provider + model labels
    // ---------------------------------------------------------------------------
    #[test]
    fn test_llm_counter_labels() {
        let metrics = Metrics::test_init();
        metrics.record_llm("anthropic", "claude-sonnet-4-20250514", 210);
        let snap = metrics.handle().render();
        assert!(
            snap.contains(r#"mempalace_llm_total{provider="anthropic",model="claude-sonnet-4-20250514"} 1"#),
            "llm counter missing:\n{}", snap
        );
        assert!(
            snap.contains("mempalace_llm_latency_ms_count 1"),
            "llm latency histogram count missing:\n{}", snap
        );
    }

    // ---------------------------------------------------------------------------
    // Test: insert counter
    // ---------------------------------------------------------------------------
    #[test]
    fn test_insert_counter() {
        let metrics = Metrics::test_init();
        metrics.record_insert("ok");
        let snap = metrics.handle().render();
        assert!(
            snap.contains(r#"mempalace_observations_inserted_total{status="ok"} 1"#),
            "inserted counter missing:\n{}", snap
        );
    }

    // ---------------------------------------------------------------------------
    // Test: consolidate counter
    // ---------------------------------------------------------------------------
    #[test]
    fn test_consolidate_counter() {
        let metrics = Metrics::test_init();
        metrics.record_consolidate();
        let snap = metrics.handle().render();
        assert!(
            snap.contains(r#"mempalace_observations_consolidated_total{} 1"#),
            "consolidated counter missing:\n{}", snap
        );
    }

    // ---------------------------------------------------------------------------
    // Test: evict counter
    // ---------------------------------------------------------------------------
    #[test]
    fn test_evict_counter() {
        let metrics = Metrics::test_init();
        metrics.record_evict();
        let snap = metrics.handle().render();
        assert!(
            snap.contains(r#"mempalace_observations_evicted_total{} 1"#),
            "evicted counter missing:\n{}", snap
        );
    }

    // ---------------------------------------------------------------------------
    // Test: kgraph added counter
    // ---------------------------------------------------------------------------
    #[test]
    fn test_kgraph_added_counter() {
        let metrics = Metrics::test_init();
        metrics.record_kgraph_added();
        let snap = metrics.handle().render();
        assert!(
            snap.contains(r#"mempalace_observations_kgraph_added_total{} 1"#),
            "kgraph_added counter missing:\n{}", snap
        );
    }

    // ---------------------------------------------------------------------------
    // Test: dedup counter
    // ---------------------------------------------------------------------------
    #[test]
    fn test_dedup_counter() {
        let metrics = Metrics::test_init();
        metrics.record_dedup();
        let snap = metrics.handle().render();
        assert!(
            snap.contains(r#"mempalace_observations_dedup_total{} 1"#),
            "dedup counter missing:\n{}", snap
        );
    }

    // ---------------------------------------------------------------------------
    // Test: summarize counter
    // ---------------------------------------------------------------------------
    #[test]
    fn test_summarize_counter() {
        let metrics = Metrics::test_init();
        metrics.record_summarize();
        let snap = metrics.handle().render();
        assert!(
            snap.contains(r#"mempalace_observations_summarized_total{} 1"#),
            "summarized counter missing:\n{}", snap
        );
    }

    // ---------------------------------------------------------------------------
    // Test: all counters + histograms are pre-registered (no lazy registration)
    // ---------------------------------------------------------------------------
    #[test]
    fn test_all_metrics_pre_registered() {
        let metrics = Metrics::test_init();
        let snap = metrics.handle().render();

        // Core counters
        let counters = &[
            "mempalace_observations_searched_total",
            "mempalace_observations_embedded_total",
            "mempalace_observations_compressed_total",
            "mempalace_observations_consolidated_total",
            "mempalace_observations_evicted_total",
            "mempalace_observations_inserted_total",
            "mempalace_observations_kgraph_added_total",
            "mempalace_observations_dedup_total",
            "mempalace_observations_summarized_total",
            "mempalace_llm_total",
        ];

        for name in counters {
            assert!(
                snap.contains(name),
                "missing pre-registered counter: {} in:\n{}", name, snap
            );
        }

        // Histograms
        let histograms = &[
            "mempalace_search_latency_ms",
            "mempalace_embed_latency_ms",
            "mempalace_llm_latency_ms",
            "mempalace_compression_latency_ms",
            "mempalace_query_expansion_latency_ms",
            "mempalace_evals_score",
        ];

        for name in histograms {
            assert!(
                snap.contains(name),
                "missing pre-registered histogram: {} in:\n{}", name, snap
            );
        }
    }

    // ---------------------------------------------------------------------------
    // Test: handle() returns the Prometheus handle for /metrics rendering
    // ---------------------------------------------------------------------------
    #[test]
    fn test_handle_returns_prometheus_handle() {
        let metrics = Metrics::test_init();
        let _handle = metrics.handle();
        // The handle should be usable for rendering
        let rendered = _handle.render();
        assert!(rendered.contains("mempalace_"));
    }
}

// =====================================================================
// Compile-time gate: metrics module must not exist when feature is OFF
// =====================================================================
#[cfg(not(feature = "telemetry"))]
#[test]
fn test_telemetry_feature_gated() {
    // When telemetry is OFF, the telemetry module is not compiled.
    // This test is a no-op placeholder that always passes.
    // Real verification is cargo build --no-default-features (must succeed).
    let _ = true;
}