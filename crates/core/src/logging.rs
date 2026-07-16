//! Logging bootstrap helpers.
//!
//! Prefer [`try_init_tracing`] over `tracing_subscriber::fmt().init()` so
//! embedding hosts / tests that already installed a subscriber do not panic.

/// Install a default stderr subscriber if none is set.
///
/// Returns `true` when this call installed the global subscriber, `false`
/// when one was already present (or installation failed for another reason).
/// Never panics (P2-6 / upstream c17d1aa).
pub fn try_init_tracing() -> bool {
    // ===== P2-6 BEGIN =====
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .try_init()
        .is_ok()
    // ===== P2-6 END =====
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== P2-6 BEGIN =====
    #[test]
    fn test_p2_6_try_init_does_not_panic_when_called_twice() {
        // First call may or may not install (another test may have beaten us).
        let _first = try_init_tracing();
        // Second call must never panic — that is the whole point of try_init.
        let second = try_init_tracing();
        assert!(
            !second,
            "second try_init must report already-initialized (false)"
        );
    }
    // ===== P2-6 END =====
}
