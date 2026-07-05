//! Viewer SPA — embedded static assets.
//!
//! Three files are bundled at compile time via `include_str!` so the
//! MemPalace binary ships as a single file with no external asset
//! directory. The REST router in `rest_api.rs` serves them from
//! `/viewer/`, `/viewer/app.js`, `/viewer/styles.css`.
//!
//! The SPA provides a read-only dashboard with:
//! - Drawer / wing / room / KG entity counts
//! - Recent working-memory observations
//! - Search bar (entity search via `/graph/search`)
//! - Inline force-directed graph (no D3 dependency)
//! - SSE live stream from `/sse`
//!
//! Security: the viewer is read-only; it never sends POST mutations.

/// The SPA shell. Served at `GET /viewer/`.
pub const fn viewer_html() -> &'static str {
    include_str!("index.html")
}

/// The SPA stylesheet. Served at `GET /viewer/styles.css`.
pub const fn viewer_styles_css() -> &'static str {
    include_str!("styles.css")
}

/// The SPA logic. Served at `GET /viewer/app.js`.
pub const fn viewer_app_js() -> &'static str {
    include_str!("app.js")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time guarantee that the embedded assets are non-empty
    /// (a missing `include_str!` would fail to compile, but a typo
    /// path resolved to an empty file would silently produce a blank
    /// page — this test guards the latter).
    #[test]
    fn assets_embedded_nonempty() {
        assert!(viewer_html().contains("<title>"));
        assert!(viewer_styles_css().contains("--bg"));
        assert!(viewer_app_js().contains("EventSource"));
    }

    #[test]
    fn html_has_dashboard_cards() {
        let html = viewer_html();
        assert!(html.contains("card-drawers"));
        assert!(html.contains("card-wings"));
        assert!(html.contains("card-rooms"));
        assert!(html.contains("card-entities"));
    }

    #[test]
    fn js_has_search_and_graph() {
        let js = viewer_app_js();
        assert!(js.contains("fetchJson"));
        assert!(js.contains("force"));
        assert!(js.contains("/graph/search"));
        assert!(js.contains("/graph/stats"));
        assert!(js.contains("/kg/stats"));
        assert!(js.contains("/working_memory"));
    }

    #[test]
    fn js_is_readonly() {
        let js = viewer_app_js();
        // Ensure the SPA never sends POST to mutation endpoints
        assert!(!js.contains("/save"));
        assert!(!js.contains("/mempalace_save"));
        assert!(!js.contains("/diary/write"));
        assert!(!js.contains("/memories\", post"));
    }
}
