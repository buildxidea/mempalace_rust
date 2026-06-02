// Telemetry — Prometheus metrics via the `metrics` façade (D1).
//
// Feature-gated behind `telemetry`. Exposes counters, histograms, and
// gauges for observability of search, LLM calls, KG operations, and
// errors. The `render()` function produces Prometheus text format.

use metrics_exporter_prometheus::PrometheusBuilder;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("prometheus builder error: {0}")]
    Builder(String),
    #[error("global metrics recorder already set")]
    AlreadyInitialized,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

static RECORDER_HANDLE: Mutex<Option<metrics_exporter_prometheus::PrometheusHandle>> =
    Mutex::new(None);
static INIT_CALLED: Mutex<bool> = Mutex::new(false);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init() -> Result<(), TelemetryError> {
    let mut called = INIT_CALLED.lock().expect("INIT_CALLED poisoned");
    if *called {
        return Err(TelemetryError::AlreadyInitialized);
    }

    let builder = PrometheusBuilder::new();

    let handle = builder
        .set_quantiles(&[0.0, 0.5, 0.9, 0.99, 1.0])
        .map_err(|e| TelemetryError::Builder(e.to_string()))?
        .install_recorder()
        .map_err(|e| TelemetryError::Builder(e.to_string()))?;

    let mut guard = RECORDER_HANDLE.lock().expect("RECORDER_HANDLE poisoned");
    *guard = Some(handle);
    *called = true;

    drop(called);
    drop(guard);

    describe_counters();
    describe_histograms();

    Ok(())
}

pub fn render() -> Option<String> {
    let guard = RECORDER_HANDLE.lock().ok()?;
    let handle = guard.as_ref()?;
    let output = handle.render();
    if output.is_empty() {
        return Some(String::new());
    }
    Some(output)
}

pub fn shutdown() {
    let mut handle_guard = RECORDER_HANDLE.lock().expect("RECORDER_HANDLE poisoned");
    *handle_guard = None;
    let mut called_guard = INIT_CALLED.lock().expect("INIT_CALLED poisoned");
    *called_guard = false;
}

// ---------------------------------------------------------------------------
// Façade re-exports
// ---------------------------------------------------------------------------

pub use metrics::{counter, describe_counter, describe_histogram, gauge, histogram};

pub fn register_counter(name: &'static str, desc: &'static str) -> metrics::Counter {
    metrics::describe_counter!(name, desc);
    let c = metrics::counter!(name);
    c.clone()
}

pub fn register_histogram(name: &'static str, desc: &'static str) -> metrics::Histogram {
    metrics::describe_histogram!(name, desc);
    metrics::histogram!(name)
}

// ---------------------------------------------------------------------------
// Pre-register all metrics
// ---------------------------------------------------------------------------

fn describe_counters() {
    metrics::describe_counter!(
        "mempalace_search_total",
        "Total number of search queries, labelled by status (success/error)."
    );
    metrics::describe_counter!(
        "mempalace_insert_total",
        "Total number of drawer insert operations."
    );
    metrics::describe_counter!(
        "mempalace_llm_total",
        "Total LLM calls, labelled by provider and model."
    );
    metrics::describe_counter!(
        "mempalace_kg_add_total",
        "Total knowledge graph triple additions."
    );
    metrics::describe_counter!(
        "mempalace_errors_total",
        "Total errors, labelled by error kind."
    );
}

fn describe_histograms() {
    metrics::describe_histogram!(
        "mempalace_search_latency_ms",
        "Search query round-trip latency in milliseconds."
    );
    metrics::describe_histogram!(
        "mempalace_embed_latency_ms",
        "Embedding generation latency in milliseconds."
    );
    metrics::describe_histogram!(
        "mempalace_llm_latency_ms",
        "LLM call round-trip latency in milliseconds."
    );
}

pub fn gauge_active_workers(count: f64) {
    metrics::gauge!("mempalace_active_workers").set(count);
}

pub fn gauge_db_size(bytes: f64) {
    metrics::gauge!("mempalace_db_size_bytes").set(bytes);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_idempotent() {
        let result1 = init();
        assert!(result1.is_ok(), "first init() should succeed");

        let result2 = init();
        assert!(
            result2.is_err(),
            "second init() should return AlreadyInitialized"
        );
        assert!(matches!(
            result2.unwrap_err(),
            TelemetryError::AlreadyInitialized
        ));
    }

    #[test]
    fn test_render_after_counter_increment() {
        let _ = init();

        metrics::counter!("mempalace_search_total", "status" => "success").increment(1);

        let output = render();
        assert!(output.is_some(), "render() should return Some after init");
        let rendered = output.expect("render() returned Some");
        assert!(
            rendered.contains("mempalace_search_total"),
            "rendered output should contain mempalace_search_total"
        );
        assert!(
            rendered.contains("status=\"success\""),
            "rendered output should contain the status label"
        );
    }

    #[test]
    fn test_shutdown_noop() {
        shutdown();
        shutdown();
    }
}
