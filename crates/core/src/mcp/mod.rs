//! MemPalace MCP server internals.

pub mod context_inject;
pub mod standalone;

/// HTTP MCP transport (feature-gated behind `http-server`).
///
/// Axum-based HTTP server that exposes `POST /mcp` for JSON-RPC over HTTP,
/// `GET /healthz` for liveness, optional bearer token auth, 16 MiB body cap,
/// DNS-rebinding guard (loopback-only), and read-only mode.
#[cfg(feature = "http-server")]
pub mod http_transport;
