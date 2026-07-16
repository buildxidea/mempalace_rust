use anyhow::Result;
use mempalace_core::cli;

fn main() -> Result<()> {
    // ===== P2-6 BEGIN =====
    // try_init (not init): embedding/MCP library hosts may already have a
    // global subscriber. Double-init would panic and take down the process.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .try_init();
    // ===== P2-6 END =====
    cli::run()
}
