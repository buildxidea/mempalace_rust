//! Live-graph viewer SPA — embedded static assets.
//!
//! Three files are bundled at compile time via `include_str!` so the
//! MemPalace binary ships as a single file with no external asset
//! directory. The REST router in `rest_api.rs` serves them from
//! `/viewer/`, `/viewer/app.js`, `/viewer/styles.css`.
//!
//! The HTML/JS/CSS files are intentionally tiny (~250 lines combined).
//! The full live-graph SPA (force layout, detail pane, SSE live
//! updates, search/expand) is the G5 follow-up tracked in
//! `REMAINING.md` — see that file for the design.

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
}
