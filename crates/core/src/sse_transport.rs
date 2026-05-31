//! SSE (Server-Sent Events) transport for MCP server.
//! Provides HTTP-based MCP transport via Server-Sent Events.

use axum::{
    extract::{Path, State, Query},
    http::{StatusCode, HeaderMap},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use tokio::sync::broadcast;
use std::sync::Arc;
use tokio::sync::Mutex;
use serde::{Deserialize, Serialize};
use futures_util::stream;
use rmcp::model::CallToolRequest;

#[derive(Clone)]
pub struct SseServerState {
    pub tx: broadcast::Sender<SseEvent>,
    pub read_only: bool,
}

#[derive(Debug, Clone)]
pub enum SseEvent {
    ToolResult { request_id: String, result: String },
    Error { request_id: String, error: String },
    Heartbeat,
}

#[derive(Deserialize)]
pub struct SseQueryParams {
    pub session_id: Option<String>,
    pub read_only: Option<bool>,
}

async fn sse_handler(
    State(state): State<Arc<Mutex<SseServerState>>>,
    Query(params): Query<SseQueryParams>,
) -> impl IntoResponse {
    let tx = state.lock().await.tx.clone();
    let event_stream = stream::repeat(()).map(move |_| {
        Event::default().retry(std::time::Duration::from_secs(30)).keep_alive()
    });
    use axum::response::sse::{Sse, Event};
    Sse::new(event_stream).into_response()
}

async fn sse_tool_handler(
    Path(tool_name): Path<String>,
    State(state): State<Arc<Mutex<SseServerState>>>,
    Json(payload): Json<CallToolRequest>,
) -> impl IntoResponse {
    let request_id = payload.id.clone().unwrap_or_default();
    let _ = state.lock().await.tx.send(SseEvent::ToolResult {
        request_id: request_id.clone(),
        result: format!("Calling tool: {}", tool_name),
    });
    (StatusCode::OK, Json(serde_json::json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "result": { "content": [{ "type": "text", "text": "SSE tool call received" }] }
    }))).into_response()
}

async fn health_handler() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({"status": "ok", "transport": "sse"}))).into_response()
}

pub fn create_sse_router() -> Router {
    Router::new()
        .route("/sse", get(sse_handler))
        .route("/sse/tools/:tool_name", post(sse_tool_handler))
        .route("/health", get(health_handler))
}

pub async fn run_sse_server(port: u16) -> anyhow::Result<()> {
    use std::net::SocketAddr;
    let (tx, _rx) = broadcast::channel::<SseEvent>(100);
    let state = Arc::new(Mutex::new(SseServerState { tx, read_only: false }));
    let app = create_sse_router().with_state(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    log("SSE server listening on port " + port);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}