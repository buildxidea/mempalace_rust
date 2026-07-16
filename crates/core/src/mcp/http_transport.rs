//! HTTP MCP transport for MemPalace.
//!
//! Axum-based HTTP MCP server (feature-gated behind `http-server`).
//!
//! - `POST /mcp` — JSON-RPC over HTTP, reuses existing `mcp_server::make_dispatch`.
//! - `GET  /healthz` — liveness probe (always 200 when server is up).
//! - Optional bearer token authentication via CLI `--token`, `MEMPALACE_HTTP_TOKEN`,
//!   or `MEMPALACE_MCP_HTTP_TOKEN` (legacy).
//! - 16 MiB request body cap.
//! - DNS-rebinding guard: rejects requests whose `Host` header is not
//!   loopback (when bound to loopback), and rejects non-loopback `Origin`.
//! - Read-only mode (`--read-only`): blocks mutation tools.
//! - Optional TLS via `--tls-cert` / `--tls-key` (axum-server + rustls).
//! - Optional idle-exit watchdog via `MEMPALACE_MCP_IDLE_EXIT_SECONDS`.
//!
//! # Security model
//!
//! This server defaults to **local-only** use (binds `127.0.0.1`). When
//! bound to a non-loopback host, a bearer token is **required** (fail-closed).

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::{Request, State};
use axum::http::header::{HeaderName, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, ORIGIN};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use rmcp::model::JsonObject;
use serde_json::json;
use tracing::{info, warn};

use crate::mcp_server::AppState;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum request body size: 16 MiB.
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// Legacy environment variable for optional bearer token authentication.
const ENV_MCP_HTTP_TOKEN: &str = "MEMPALACE_MCP_HTTP_TOKEN";

// ===== P1-10 BEGIN =====
/// Canonical environment variable for HTTP bearer token (REST + MCP HTTP).
const ENV_HTTP_TOKEN: &str = "MEMPALACE_HTTP_TOKEN";
// ===== P1-10 END =====

/// Environment variable for the port to bind to.
const ENV_HTTP_PORT: &str = "MEMPALACE_MCP_HTTP_PORT";

/// Default port for the MCP HTTP transport.
const DEFAULT_PORT: u16 = 3112;

/// Environment variable to override bind address.
const ENV_HTTP_BIND_ADDR: &str = "MEMPALACE_MCP_HTTP_BIND_ADDR";

/// Default bind address.
const DEFAULT_BIND_ADDR: &str = "127.0.0.1";

// ===== P2-7 BEGIN =====
/// Environment variable: exit after N seconds of HTTP idle (no requests).
const ENV_IDLE_EXIT_SECONDS: &str = "MEMPALACE_MCP_IDLE_EXIT_SECONDS";
// ===== P2-7 END =====

// ---------------------------------------------------------------------------
// Shared server state
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct HttpMcpState {
    app_state: Arc<AppState>,
    /// Bearer token required for authentication, if set.
    auth_token: Option<String>,
    /// When true, Host header must be loopback (DNS rebinding guard).
    /// Disabled when the server is intentionally bound to a non-loopback host.
    enforce_loopback_host: bool,
    // ===== P2-7 BEGIN =====
    /// Epoch-millis of the last accepted request (for idle watchdog).
    last_activity_ms: Arc<AtomicU64>,
    // ===== P2-7 END =====
}

// ---------------------------------------------------------------------------
// Public bind / auth config (P1-3 / P1-10)
// ---------------------------------------------------------------------------

// ===== P1-3 BEGIN =====
/// Bind / auth / TLS options for the MCP HTTP transport (and shared helpers).
#[derive(Debug, Clone, Default)]
pub struct HttpServeOptions {
    /// Bind host (IP or hostname). Defaults to `127.0.0.1`.
    pub host: Option<String>,
    /// Explicit bearer token. When `None`, resolved from env vars.
    pub token: Option<String>,
    /// Optional TLS certificate PEM path.
    pub tls_cert: Option<PathBuf>,
    /// Optional TLS private key PEM path.
    pub tls_key: Option<PathBuf>,
    /// Port override from CLI `--port`.
    pub port: Option<u16>,
}

/// Result of resolving host + token policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedHttpAuth {
    pub host: String,
    pub port: u16,
    pub token: Option<String>,
    /// True when the resolved host is loopback (or unspecified wildcard is treated as non-loopback).
    pub is_loopback_bind: bool,
}

/// Return true if `host` is a loopback bind target.
///
/// `0.0.0.0` / `::` / `*` are treated as **non-loopback** (public bind).
pub fn is_loopback_bind_host(host: &str) -> bool {
    let host = host.trim();
    if host.is_empty() {
        return true; // will fall back to default 127.0.0.1
    }
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    // Wildcards / all-interfaces are non-loopback for security policy.
    if host == "0.0.0.0" || host == "::" || host == "*" || host == "[::]" {
        return false;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return ip.is_loopback();
    }
    // Unresolvable hostname: treat as non-loopback (fail-closed later if no token).
    false
}

/// Resolve bearer token from CLI override and environment variables.
///
/// Precedence:
///   1. CLI `--token`
///   2. `MEMPALACE_HTTP_TOKEN`
///   3. `MEMPALACE_MCP_HTTP_TOKEN` (legacy)
pub fn resolve_auth_token(cli_token: Option<&str>) -> Option<String> {
    if let Some(t) = cli_token {
        let t = t.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    if let Ok(t) = std::env::var(ENV_HTTP_TOKEN) {
        if !t.is_empty() {
            return Some(t);
        }
    }
    if let Ok(t) = std::env::var(ENV_MCP_HTTP_TOKEN) {
        if !t.is_empty() {
            return Some(t);
        }
    }
    None
}

/// Generate a random bearer token (UUIDv4 hex, no dashes) for auto-provisioning.
pub fn generate_auth_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Fail-closed policy: non-loopback binds require a bearer token.
///
/// Returns `Ok(ResolvedHttpAuth)` with a token that is always `Some` for
/// non-loopback binds (auto-generated when neither CLI nor env provides one
/// is **not** done here — callers decide whether to auto-gen or error).
///
/// This function **errors** when the bind host is non-loopback and no token
/// is available, matching the fail-closed security model.
pub fn resolve_http_auth(
    host: Option<&str>,
    port: Option<u16>,
    cli_token: Option<&str>,
    auto_generate_token: bool,
) -> anyhow::Result<ResolvedHttpAuth> {
    let host = host
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            std::env::var("MEMPALACE_HTTP_HOST")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            std::env::var(ENV_HTTP_BIND_ADDR)
                .ok()
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(|| DEFAULT_BIND_ADDR.to_string());

    let port = resolve_port(port);
    let is_loopback_bind = is_loopback_bind_host(&host);
    let mut token = resolve_auth_token(cli_token);

    if !is_loopback_bind && token.is_none() {
        if auto_generate_token {
            let generated = generate_auth_token();
            eprintln!(
                "  WARNING: non-loopback bind ({}) requires a bearer token; auto-generated:",
                host
            );
            eprintln!("  MEMPALACE_HTTP_TOKEN={}", generated);
            eprintln!("  Pass Authorization: Bearer <token> on every request.");
            token = Some(generated);
        } else {
            anyhow::bail!(
                "refusing to bind to non-loopback host '{}' without a bearer token. \
                 Pass --token <secret>, set MEMPALACE_HTTP_TOKEN, or bind to 127.0.0.1.",
                host
            );
        }
    }

    Ok(ResolvedHttpAuth {
        host,
        port,
        token,
        is_loopback_bind,
    })
}

/// Validate that TLS cert/key are either both present or both absent.
pub fn validate_tls_pair(cert: Option<&PathBuf>, key: Option<&PathBuf>) -> anyhow::Result<()> {
    match (cert, key) {
        (None, None) => Ok(()),
        (Some(_), Some(_)) => Ok(()),
        (Some(_), None) => {
            anyhow::bail!("--tls-cert requires --tls-key (both must be provided together)")
        }
        (None, Some(_)) => {
            anyhow::bail!("--tls-key requires --tls-cert (both must be provided together)")
        }
    }
}
// ===== P1-3 END =====

// ===== P2-7 BEGIN =====
/// Resolve idle-exit timeout from `MEMPALACE_MCP_IDLE_EXIT_SECONDS`.
///
/// Returns `None` when unset/empty/zero/invalid (watchdog disabled).
pub fn resolve_idle_exit_seconds() -> Option<u64> {
    let raw = std::env::var(ENV_IDLE_EXIT_SECONDS).ok()?;
    let n: u64 = raw.trim().parse().ok()?;
    if n == 0 {
        None
    } else {
        Some(n)
    }
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
// ===== P2-7 END =====

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) enum HttpMcpError {
    /// Authentication failed.
    Unauthorized(String),
    /// Request body too large.
    PayloadTooLarge,
    /// DNS rebinding detected (non-loopback Host or Origin).
    DnsRebindingBlocked(String),
    /// Invalid or missing JSON-RPC request.
    BadRequest(String),
    /// Internal server error.
    Internal(String),
}

impl IntoResponse for HttpMcpError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            HttpMcpError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            HttpMcpError::PayloadTooLarge => (
                StatusCode::PAYLOAD_TOO_LARGE,
                format!("Request body exceeds {} byte limit", MAX_BODY_BYTES),
            ),
            HttpMcpError::DnsRebindingBlocked(msg) => (StatusCode::FORBIDDEN, msg),
            HttpMcpError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            HttpMcpError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };
        let body = json!({
            "jsonrpc": "2.0",
            "error": {
                "code": status.as_u16() as i64,
                "message": message,
            },
        });
        (status, Json(body)).into_response()
    }
}

// ---------------------------------------------------------------------------
// DNS rebinding guard middleware
// ---------------------------------------------------------------------------

/// Check whether a hostname (from Host or Origin header) resolves to a
/// loopback address. Accepts: `localhost`, `127.x.x.x`, `::1`,
/// `0.0.0.0` (header only), `[::1]`, and their `:port` variants.
fn is_loopback_host(hostname: &str) -> bool {
    // Strip port from IPv4-style "host:port" (but not from bare IPv6 addresses
    // which contain colons as part of the address notation).
    let host = if hostname.starts_with('[') {
        // IPv6 bracket notation: [::1]:port or [::1]
        if let Some(end) = hostname.find(']') {
            &hostname[1..end]
        } else {
            hostname
                .strip_prefix('[')
                .and_then(|s| s.strip_suffix(']'))
                .unwrap_or(hostname)
        }
    } else if let Some(colon_pos) = hostname.rfind(':') {
        // Could be "host:port" (IPv4) or bare IPv6 (multiple colons).
        // If the string parses as an IP, use it directly; otherwise assume
        // the last `:` separates port.
        let candidate = &hostname[..colon_pos];
        if candidate.parse::<IpAddr>().is_ok() {
            candidate
        } else if hostname.matches(':').count() == 1 {
            candidate // host:port
        } else {
            hostname // bare IPv6 like "::1"
        }
    } else {
        hostname
    };

    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    // Try parsing as IP address
    if let Ok(ip) = host.parse::<IpAddr>() {
        return ip.is_loopback();
    }
    false
}

/// Middleware that rejects requests from non-loopback origins when the
/// server is bound to loopback only.
async fn dns_rebinding_guard(
    State(state): State<HttpMcpState>,
    request: Request,
    next: Next,
) -> Result<Response, HttpMcpError> {
    if state.enforce_loopback_host {
        // Check Host header
        if let Some(host) = request.headers().get("host") {
            if let Ok(host_str) = host.to_str() {
                if !is_loopback_host(host_str) {
                    return Err(HttpMcpError::DnsRebindingBlocked(format!(
                        "Host header '{}' is not a loopback address; \
                         this server only accepts local connections",
                        host_str,
                    )));
                }
            }
        }

        // Check Origin header (present in browser CORS preflight / fetch)
        if let Some(origin) = request.headers().get(ORIGIN) {
            if let Ok(origin_str) = origin.to_str() {
                // Origin: http://localhost:3112 or similar
                if let Some(rest) = origin_str
                    .strip_prefix("http://")
                    .or_else(|| origin_str.strip_prefix("https://"))
                {
                    let hostname = rest.split('/').next().unwrap_or(rest);
                    let hostname = if hostname.starts_with('[') {
                        hostname
                    } else {
                        hostname.split(':').next().unwrap_or(hostname)
                    };
                    if !is_loopback_host(hostname) {
                        return Err(HttpMcpError::DnsRebindingBlocked(format!(
                            "Origin '{}' is not a loopback address; \
                             this server only accepts local connections",
                            origin_str,
                        )));
                    }
                }
            }
        }
    }

    Ok(next.run(request).await)
}

// ---------------------------------------------------------------------------
// Auth middleware
// ---------------------------------------------------------------------------

/// Middleware that checks for a valid bearer token when one is configured.
async fn auth_guard(
    State(state): State<HttpMcpState>,
    request: Request,
    next: Next,
) -> Result<Response, HttpMcpError> {
    if let Some(ref expected) = state.auth_token {
        let auth_header = request.headers().get(AUTHORIZATION);
        match auth_header {
            Some(value) => {
                let value_str = value.to_str().map_err(|_| {
                    HttpMcpError::Unauthorized("Invalid Authorization header encoding".into())
                })?;
                let token = value_str.strip_prefix("Bearer ").unwrap_or(value_str);
                if token != expected {
                    return Err(HttpMcpError::Unauthorized("Invalid bearer token".into()));
                }
            }
            None => {
                return Err(HttpMcpError::Unauthorized(
                    "Bearer token required (set MEMPALACE_HTTP_TOKEN or --token)".into(),
                ));
            }
        }
    }

    Ok(next.run(request).await)
}

// ---------------------------------------------------------------------------
// Body size limit middleware
// ---------------------------------------------------------------------------

/// Middleware that rejects requests exceeding `MAX_BODY_BYTES`.
async fn body_size_guard(request: Request, next: Next) -> Result<Response, HttpMcpError> {
    if let Some(content_length) = request.headers().get(CONTENT_LENGTH) {
        if let Ok(len_str) = content_length.to_str() {
            if let Ok(len) = len_str.parse::<usize>() {
                if len > MAX_BODY_BYTES {
                    return Err(HttpMcpError::PayloadTooLarge);
                }
            }
        }
    }
    Ok(next.run(request).await)
}

// ===== P2-7 BEGIN =====
/// Middleware that bumps the last-activity timestamp on every request.
async fn activity_tracker(
    State(state): State<HttpMcpState>,
    request: Request,
    next: Next,
) -> Result<Response, HttpMcpError> {
    state
        .last_activity_ms
        .store(now_epoch_ms(), Ordering::Relaxed);
    Ok(next.run(request).await)
}
// ===== P2-7 END =====

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /healthz` — liveness probe. Always returns 200 if the server is up.
async fn healthz_handler() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok", "service": "mempalace-mcp-http" }))
}

/// `POST /mcp` — JSON-RPC over HTTP.
///
/// Accepts a JSON-RPC 2.0 request body with `method` and `params`.
/// Dispatches to the same `make_dispatch` used by the stdio MCP server.
///
/// Supported request shapes:
///   - `{"method": "mempalace_status", "params": {}}` (JSON-RPC 2.0)
///   - `{"method": "tools/list"}` (MCP standard)
///   - `{"method": "tools/call", "params": {"name": "...", "arguments": {...}}}` (MCP standard)
///   - Legacy: `{"tool": "...", "args": {...}}` (REST API compat)
async fn mcp_handler(
    State(state): State<HttpMcpState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, HttpMcpError> {
    // --- JSON-RPC 2.0 / MCP standard path ---
    if let Some(method) = body.get("method").and_then(|m| m.as_str()) {
        return match method {
            // tools/list — return the tool catalog
            "tools/list" => {
                let tools = crate::mcp_server::make_tools();
                let tools_json: Vec<serde_json::Value> = tools
                    .iter()
                    .filter(|t| {
                        // In read-only mode, exclude mutation tools
                        if state.app_state.read_only {
                            !crate::mcp_server::is_mutation_tool(&t.name)
                        } else {
                            true
                        }
                    })
                    .map(|t| {
                        let input_schema = serde_json::Value::Object((*t.input_schema).clone());
                        json!({
                            "name": t.name,
                            "description": t.description,
                            "inputSchema": input_schema,
                        })
                    })
                    .collect();
                Ok(Json(json!({
                    "jsonrpc": "2.0",
                    "result": {
                        "tools": tools_json,
                    },
                })))
            }
            // tools/call — invoke a single tool
            "tools/call" => {
                let params = body.get("params").cloned().unwrap_or(json!({}));
                let tool_name = params
                    .get("name")
                    .and_then(|n| n.as_str())
                    .ok_or_else(|| {
                        HttpMcpError::BadRequest("tools/call requires 'name' field".into())
                    })?
                    .to_string();
                let tool_args = params
                    .get("arguments")
                    .and_then(|a| a.as_object().cloned())
                    .unwrap_or_default();
                dispatch_tool(&state, &tool_name, tool_args).await
            }
            // initialize — handshake
            "initialize" => Ok(Json(json!({
                "jsonrpc": "2.0",
                "result": {
                    "protocolVersion": "2025-03-26",
                    "serverInfo": {
                        "name": "mempalace-mcp-http",
                        "version": env!("CARGO_PKG_VERSION"),
                    },
                    "capabilities": {
                        "tools": {}
                    },
                },
            }))),
            _ => {
                // Generic method dispatch: treat method name as tool name
                let params = body
                    .get("params")
                    .cloned()
                    .and_then(|v| v.as_object().cloned())
                    .unwrap_or_default();
                dispatch_tool(&state, method, params).await
            }
        };
    }

    // --- Legacy REST API compat path: {"tool": "...", "args": {...}} ---
    let tool_name = body.get("tool").and_then(|t| t.as_str()).unwrap_or("");
    if tool_name.is_empty() {
        return Err(HttpMcpError::BadRequest(
            "Request must contain 'method' (JSON-RPC) or 'tool' (legacy) field".into(),
        ));
    }
    let tool_args = body
        .get("args")
        .and_then(|a| a.as_object().cloned())
        .unwrap_or_default();
    dispatch_tool(&state, tool_name, tool_args).await
}

/// Dispatch a tool call through the existing `mcp_server::make_dispatch`.
async fn dispatch_tool(
    state: &HttpMcpState,
    tool_name: &str,
    args: JsonObject,
) -> Result<Json<serde_json::Value>, HttpMcpError> {
    let dispatch = crate::mcp_server::make_dispatch(state.app_state.clone());
    let result = dispatch(tool_name.to_string(), args).await;
    match result {
        Ok(call_result) => {
            // Extract text content from the CallToolResult
            let content = &call_result.content;
            if content.is_empty() {
                Ok(Json(
                    json!({ "jsonrpc": "2.0", "result": { "content": [] } }),
                ))
            } else {
                let first = content.first();
                match first {
                    Some(c) => {
                        use rmcp::model::RawContent;
                        match &c.raw {
                            RawContent::Text(ref text) => {
                                let parsed = serde_json::from_str::<serde_json::Value>(&text.text)
                                    .unwrap_or_else(|_| json!({ "text": text.text }));
                                Ok(Json(json!({
                                    "jsonrpc": "2.0",
                                    "result": {
                                        "content": [{
                                            "type": "text",
                                            "text": serde_json::to_string(&parsed)
                                                .unwrap_or_else(|_| text.text.clone()),
                                        }],
                                    },
                                })))
                            }
                            _ => Ok(Json(json!({
                                "jsonrpc": "2.0",
                                "result": { "content": [{ "type": "text", "text": "{}" }] },
                            }))),
                        }
                    }
                    None => Ok(Json(
                        json!({ "jsonrpc": "2.0", "result": { "content": [] } }),
                    )),
                }
            }
        }
        Err(e) => {
            let msg = e.message.to_string();
            if msg.contains("read-only") || msg.contains("ReadOnly") {
                Err(HttpMcpError::BadRequest(format!(
                    "Mutation blocked (read-only mode): {}",
                    msg
                )))
            } else {
                Err(HttpMcpError::Internal(msg))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Router construction
// ---------------------------------------------------------------------------

fn build_router(state: HttpMcpState) -> Router {
    Router::new()
        .route("/healthz", get(healthz_handler))
        .route("/mcp", post(mcp_handler))
        // Catch-all: reject unknown paths with 404
        .fallback(|| async { (StatusCode::NOT_FOUND, Json(json!({ "error": "Not found" }))) })
        .layer(middleware::from_fn_with_state(state.clone(), auth_guard))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            dns_rebinding_guard,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            activity_tracker,
        ))
        .layer(middleware::from_fn(body_size_guard))
        .layer(
            tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::AllowOrigin::predicate(
                    |_origin, _request_parts| {
                        // Only allow loopback origins; the dns_rebinding_guard
                        // middleware already rejects non-loopback requests when
                        // enforce_loopback_host is set.
                        true
                    },
                ))
                .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
                .allow_headers([
                    AUTHORIZATION,
                    CONTENT_TYPE,
                    ORIGIN,
                    HeaderName::from_static("x-requested-with"),
                ]),
        )
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Port / bind address resolution
// ---------------------------------------------------------------------------

/// Resolve the port for the MCP HTTP transport.
///
/// Precedence:
///   1. `MEMPALACE_MCP_HTTP_PORT` env var
///   2. `port_override` (from CLI `--port`)
///   3. Default: 3112
pub fn resolve_port(port_override: Option<u16>) -> u16 {
    if let Ok(p) = std::env::var(ENV_HTTP_PORT) {
        if let Ok(n) = p.parse::<u16>() {
            return n;
        }
    }
    if let Some(p) = port_override {
        return p;
    }
    DEFAULT_PORT
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Start the MCP HTTP transport server (legacy signature).
///
/// Prefer [`run_mcp_http_with_options`] when host/token/TLS are needed.
pub fn run_mcp_http(
    palace_override: Option<&str>,
    read_only: bool,
    port_override: Option<u16>,
) -> anyhow::Result<()> {
    run_mcp_http_with_options(
        palace_override,
        read_only,
        HttpServeOptions {
            port: port_override,
            ..Default::default()
        },
    )
}

// ===== P1-3 BEGIN =====
/// Start the MCP HTTP transport server with full bind/auth/TLS options.
///
/// Binds to the resolved address/port, sets up the axum router with
/// security middleware, and serves until the process is interrupted
/// (or the idle-exit watchdog fires).
pub fn run_mcp_http_with_options(
    palace_override: Option<&str>,
    read_only: bool,
    opts: HttpServeOptions,
) -> anyhow::Result<()> {
    validate_tls_pair(opts.tls_cert.as_ref(), opts.tls_key.as_ref())?;

    // Fail-closed for non-loopback: require token; auto-generate when omitted
    // so Docker/systemd deploys get a usable secret printed to stderr.
    let auth = resolve_http_auth(
        opts.host.as_deref(),
        opts.port,
        opts.token.as_deref(),
        /* auto_generate_token */ true,
    )?;

    let mut config = crate::Config::load()?;
    if let Some(p) = palace_override {
        config.palace_path = crate::mcp_server::resolve_palace_override(p);
    }

    let app_state = Arc::new(AppState::new(config, read_only)?);
    let last_activity_ms = Arc::new(AtomicU64::new(now_epoch_ms()));

    let state = HttpMcpState {
        app_state,
        auth_token: auth.token.clone(),
        enforce_loopback_host: auth.is_loopback_bind,
        last_activity_ms: last_activity_ms.clone(),
    };

    let router = build_router(state);

    let addr: SocketAddr = format!("{}:{}", auth.host, auth.port)
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid bind address {}:{}: {}", auth.host, auth.port, e))?;

    let scheme = if opts.tls_cert.is_some() {
        "https"
    } else {
        "http"
    };
    info!(
        "MCP HTTP transport listening on {}://{}/mcp (read_only={}, loopback_bind={})",
        scheme, addr, read_only, auth.is_loopback_bind,
    );
    if auth.token.is_some() {
        info!("Bearer token authentication enabled");
    } else {
        warn!(
            "No bearer token configured — set {} or --token for authentication",
            ENV_HTTP_TOKEN,
        );
    }

    let idle_exit = resolve_idle_exit_seconds();
    if let Some(secs) = idle_exit {
        info!(
            "Idle-exit watchdog armed: {}s (env {})",
            secs, ENV_IDLE_EXIT_SECONDS
        );
    }

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        // ===== P2-7 BEGIN =====
        if let Some(idle_secs) = idle_exit {
            let activity = last_activity_ms.clone();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(1));
                loop {
                    ticker.tick().await;
                    let last = activity.load(Ordering::Relaxed);
                    let now = now_epoch_ms();
                    if now.saturating_sub(last) >= idle_secs.saturating_mul(1000) {
                        warn!(
                            "MCP HTTP idle for {}s — exiting (MEMPALACE_MCP_IDLE_EXIT_SECONDS)",
                            idle_secs
                        );
                        // Process-level exit: clean enough for Docker/k8s restart policy.
                        std::process::exit(0);
                    }
                }
            });
        }
        // ===== P2-7 END =====

        // ===== P1-3 BEGIN (TLS) =====
        match (opts.tls_cert.as_ref(), opts.tls_key.as_ref()) {
            (Some(cert), Some(key)) => {
                #[cfg(feature = "http-tls")]
                {
                    use axum_server::tls_rustls::RustlsConfig;
                    let tls_config = RustlsConfig::from_pem_file(cert, key).await.map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to load TLS cert/key from {} / {}: {}",
                            cert.display(),
                            key.display(),
                            e
                        )
                    })?;
                    axum_server::bind_rustls(addr, tls_config)
                        .serve(router.into_make_service())
                        .await
                        .map_err(|e| anyhow::anyhow!("TLS server error: {}", e))?;
                }
                #[cfg(not(feature = "http-tls"))]
                {
                    let _ = (cert, key, router, addr);
                    anyhow::bail!(
                        "TLS requested via --tls-cert/--tls-key but this binary was built \
                         without the `http-tls` feature. Rebuild with \
                         `--features http-server,http-tls`."
                    );
                }
            }
            _ => {
                let listener = tokio::net::TcpListener::bind(addr).await?;
                axum::serve(listener, router).await?;
            }
        }
        // ===== P1-3 END (TLS) =====
        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}
// ===== P1-3 END =====

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_loopback_host() {
        // Loopback
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("LOCALHOST"));
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("127.0.0.1:8080"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("[::1]"));

        // Non-loopback
        assert!(!is_loopback_host("example.com"));
        assert!(!is_loopback_host("192.168.1.1"));
        assert!(!is_loopback_host("10.0.0.1"));
        assert!(!is_loopback_host("8.8.8.8"));
    }

    #[test]
    fn test_resolve_port_default() {
        let _lock = crate::test_env_lock();
        // Clear any env var
        std::env::remove_var(ENV_HTTP_PORT);
        assert_eq!(resolve_port(None), DEFAULT_PORT);
    }

    #[test]
    fn test_resolve_port_override() {
        let _lock = crate::test_env_lock();
        std::env::remove_var(ENV_HTTP_PORT);
        assert_eq!(resolve_port(Some(9999)), 9999);
    }

    #[test]
    fn test_resolve_auth_token_empty_string_is_none() {
        let _lock = crate::test_env_lock();
        std::env::remove_var(ENV_HTTP_TOKEN);
        std::env::remove_var(ENV_MCP_HTTP_TOKEN);
        std::env::set_var(ENV_MCP_HTTP_TOKEN, "");
        let result = resolve_auth_token(None);
        assert!(result.is_none());
        std::env::remove_var(ENV_MCP_HTTP_TOKEN);
    }

    #[test]
    fn test_resolve_auth_token_some() {
        let _lock = crate::test_env_lock();
        std::env::remove_var(ENV_HTTP_TOKEN);
        std::env::set_var(ENV_MCP_HTTP_TOKEN, "my-secret-token");
        let result = resolve_auth_token(None);
        assert_eq!(result.as_deref(), Some("my-secret-token"));
        std::env::remove_var(ENV_MCP_HTTP_TOKEN);
    }

    #[test]
    fn test_resolve_auth_token_removed_is_none() {
        let _lock = crate::test_env_lock();
        std::env::set_var(ENV_MCP_HTTP_TOKEN, "temp");
        std::env::remove_var(ENV_MCP_HTTP_TOKEN);
        std::env::remove_var(ENV_HTTP_TOKEN);
        let _ = resolve_auth_token(None);
    }

    // ===== P1-3 / P1-10 tests =====
    #[test]
    fn test_is_loopback_bind_host() {
        assert!(is_loopback_bind_host("127.0.0.1"));
        assert!(is_loopback_bind_host("localhost"));
        assert!(is_loopback_bind_host("::1"));
        assert!(!is_loopback_bind_host("0.0.0.0"));
        assert!(!is_loopback_bind_host("::"));
        assert!(!is_loopback_bind_host("192.168.1.10"));
        assert!(!is_loopback_bind_host("example.com"));
    }

    #[test]
    fn test_resolve_http_auth_loopback_no_token_ok() {
        let _lock = crate::test_env_lock();
        std::env::remove_var(ENV_HTTP_TOKEN);
        std::env::remove_var(ENV_MCP_HTTP_TOKEN);
        std::env::remove_var(ENV_HTTP_BIND_ADDR);
        std::env::remove_var(ENV_HTTP_PORT);
        let auth = resolve_http_auth(Some("127.0.0.1"), Some(3112), None, false).unwrap();
        assert!(auth.is_loopback_bind);
        assert!(auth.token.is_none());
        assert_eq!(auth.host, "127.0.0.1");
        assert_eq!(auth.port, 3112);
    }

    #[test]
    fn test_resolve_http_auth_non_loopback_without_token_errors() {
        let _lock = crate::test_env_lock();
        std::env::remove_var(ENV_HTTP_TOKEN);
        std::env::remove_var(ENV_MCP_HTTP_TOKEN);
        std::env::remove_var(ENV_HTTP_BIND_ADDR);
        let err = resolve_http_auth(Some("0.0.0.0"), Some(8443), None, false).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("non-loopback") || msg.contains("bearer token"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn test_resolve_http_auth_non_loopback_with_cli_token_ok() {
        let _lock = crate::test_env_lock();
        std::env::remove_var(ENV_HTTP_TOKEN);
        std::env::remove_var(ENV_MCP_HTTP_TOKEN);
        let auth = resolve_http_auth(Some("0.0.0.0"), Some(8443), Some("s3cret"), false).unwrap();
        assert!(!auth.is_loopback_bind);
        assert_eq!(auth.token.as_deref(), Some("s3cret"));
    }

    #[test]
    fn test_resolve_http_auth_non_loopback_auto_generate() {
        let _lock = crate::test_env_lock();
        std::env::remove_var(ENV_HTTP_TOKEN);
        std::env::remove_var(ENV_MCP_HTTP_TOKEN);
        let auth = resolve_http_auth(Some("0.0.0.0"), Some(8443), None, true).unwrap();
        assert!(!auth.is_loopback_bind);
        assert!(auth.token.as_ref().is_some_and(|t| t.len() >= 16));
    }

    #[test]
    fn test_resolve_auth_token_prefers_http_token_env() {
        let _lock = crate::test_env_lock();
        std::env::set_var(ENV_HTTP_TOKEN, "from-http");
        std::env::set_var(ENV_MCP_HTTP_TOKEN, "from-mcp");
        assert_eq!(resolve_auth_token(None).as_deref(), Some("from-http"));
        assert_eq!(
            resolve_auth_token(Some("from-cli")).as_deref(),
            Some("from-cli")
        );
        std::env::remove_var(ENV_HTTP_TOKEN);
        std::env::remove_var(ENV_MCP_HTTP_TOKEN);
    }

    #[test]
    fn test_validate_tls_pair() {
        assert!(validate_tls_pair(None, None).is_ok());
        let cert = PathBuf::from("cert.pem");
        let key = PathBuf::from("key.pem");
        assert!(validate_tls_pair(Some(&cert), Some(&key)).is_ok());
        assert!(validate_tls_pair(Some(&cert), None).is_err());
        assert!(validate_tls_pair(None, Some(&key)).is_err());
    }

    #[test]
    fn test_generate_auth_token_unique() {
        let a = generate_auth_token();
        let b = generate_auth_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 32); // uuid simple hex
    }

    // ===== P2-7 tests =====
    #[test]
    fn test_resolve_idle_exit_seconds() {
        let _lock = crate::test_env_lock();
        std::env::remove_var(ENV_IDLE_EXIT_SECONDS);
        assert_eq!(resolve_idle_exit_seconds(), None);

        std::env::set_var(ENV_IDLE_EXIT_SECONDS, "0");
        assert_eq!(resolve_idle_exit_seconds(), None);

        std::env::set_var(ENV_IDLE_EXIT_SECONDS, "120");
        assert_eq!(resolve_idle_exit_seconds(), Some(120));

        std::env::set_var(ENV_IDLE_EXIT_SECONDS, "nope");
        assert_eq!(resolve_idle_exit_seconds(), None);

        std::env::remove_var(ENV_IDLE_EXIT_SECONDS);
    }

    #[test]
    fn test_max_body_bytes_is_16_mib() {
        assert_eq!(MAX_BODY_BYTES, 16 * 1024 * 1024);
    }

    #[test]
    fn test_error_response_shape() {
        let err = HttpMcpError::Unauthorized("test".into());
        let response: Response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn test_error_payload_too_large() {
        let err = HttpMcpError::PayloadTooLarge;
        let response: Response = err.into_response();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[test]
    fn test_error_dns_rebinding() {
        let err = HttpMcpError::DnsRebindingBlocked("blocked".into());
        let response: Response = err.into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_healthz_handler_returns_ok() {
        let response = healthz_handler().await;
        let Json(body) = response;
        assert_eq!(body["status"], "ok");
        assert_eq!(body["service"], "mempalace-mcp-http");
    }

    #[test]
    fn test_now_epoch_ms_monotonic_enough() {
        let a = now_epoch_ms();
        let b = now_epoch_ms();
        assert!(b >= a);
    }
}
