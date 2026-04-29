use crate::bus::InboundMessage;
use crate::config::Config;
use axum::{
    extract::{Query, State, WebSocketUpgrade},
    extract::ws::{Message, WebSocket},
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tracing::info;

pub type WsClients = Arc<DashMap<String, mpsc::UnboundedSender<String>>>;

pub async fn serve(
    config: Config,
    bus_tx: mpsc::Sender<InboundMessage>,
    db: Arc<tokio::sync::RwLock<crate::tools::persistence::Database>>,
    ws_clients: WsClients,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    if !config.gateway.dashboard_enabled {
        return Ok(());
    }

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let state = AppState {
        bus_tx,
        db,
        ws_clients,
        start_time: std::time::Instant::now(),
    };

    let static_dir = std::env::var("STATIC_DIR").unwrap_or_default();
    let has_static = !static_dir.is_empty() && std::path::Path::new(&static_dir).exists();

    // API + WebSocket routes (shared by both modes)
    let api_router = Router::new()
        .route("/api/chat", post(chat_handler))
        .route("/ws", get(ws_handler))
        .route("/api/health", get(health_handler))
        .route("/api/status", get(status_handler))
        .route("/api/history", get(history_handler))
        .with_state(state.clone());

    let app = if has_static {
        // Production: serve built React SPA; API routes take priority
        info!("Serving frontend from: {}", static_dir);
        let index = format!("{}/index.html", static_dir);
        api_router.fallback_service(
            ServeDir::new(&static_dir)
                .not_found_service(ServeFile::new(index)),
        )
    } else {
        // Development fallback: serve embedded HTMX dashboard
        api_router.route("/", get(index_fallback))
    };

    let app = app.layer(cors);

    let addr = format!("{}:{}", config.gateway.host, config.gateway.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("Dashboard on http://{} (static={})", addr, has_static);

    axum::serve(listener, app)
        .with_graceful_shutdown(async move { cancel.cancelled().await })
        .await?;

    Ok(())
}

#[derive(Clone)]
struct AppState {
    bus_tx: mpsc::Sender<InboundMessage>,
    db: Arc<tokio::sync::RwLock<crate::tools::persistence::Database>>,
    ws_clients: WsClients,
    start_time: std::time::Instant,
}

async fn index_fallback() -> Html<&'static str> {
    Html(include_str!("static/index.html"))
}

async fn chat_handler(
    State(state): State<AppState>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let content = payload["message"].as_str().unwrap_or_default();
    let session_key = payload["session_key"]
        .as_str()
        .unwrap_or("web_session")
        .to_string();

    let msg = InboundMessage {
        channel: "web".to_string(),
        sender: "web_user".to_string(),
        content: content.to_string(),
        session_key,
        metadata: None,
    };
    let _ = state.bus_tx.send(msg).await;
    Json(serde_json::json!({ "status": "received" }))
}

async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn status_handler(State(state): State<AppState>) -> impl IntoResponse {
    let (memory_mb, cpu_pct) = process_metrics();
    let uptime = state.start_time.elapsed().as_secs();

    let (total_requests, total_tokens) = {
        let db = state.db.read().await;
        db.get_total_usage().unwrap_or((0, 0))
    };

    Json(serde_json::json!({
        "status": "running",
        "version": env!("CARGO_PKG_VERSION"),
        "uptime_secs": uptime,
        "connections": state.ws_clients.len(),
        "system": { "memory_mb": memory_mb, "cpu_percent": cpu_pct },
        "usage": { "total_requests": total_requests, "total_tokens": total_tokens }
    }))
}

async fn history_handler(
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let session_key = params.get("session").cloned().unwrap_or_default();
    if session_key.is_empty() {
        return Json(serde_json::json!({ "messages": [] }));
    }
    let history = {
        let db = state.db.read().await;
        db.get_history(&session_key, 100).unwrap_or_default()
    };
    let messages: Vec<_> = history
        .iter()
        .map(|(role, content)| serde_json::json!({ "role": role, "content": content }))
        .collect();
    Json(serde_json::json!({ "messages": messages }))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let session_key = params
        .get("session")
        .cloned()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    ws.on_upgrade(move |socket| handle_socket(socket, session_key, state))
}

async fn handle_socket(socket: WebSocket, session_key: String, state: AppState) {
    let (mut ws_tx, mut ws_rx) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    state.ws_clients.insert(session_key.clone(), tx);
    info!("WS connected: {}", &session_key[..8.min(session_key.len())]);

    // Send session init so client knows its key
    let init = serde_json::json!({ "type": "init", "session_key": &session_key }).to_string();
    if ws_tx.send(Message::Text(init.into())).await.is_err() {
        state.ws_clients.remove(&session_key);
        return;
    }

    // Outbound: agent messages → WS
    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_tx.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Inbound: WS messages → bus
    let bus_tx = state.bus_tx.clone();
    let sk = session_key.clone();
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                Message::Text(text) => {
                    if let Ok(json) = serde_json::from_str::<Value>(text.as_str()) {
                        let content = json["message"].as_str().unwrap_or("").to_string();
                        if content.is_empty() {
                            continue;
                        }
                        let _ = bus_tx
                            .send(InboundMessage {
                                channel: "web".to_string(),
                                sender: "web_user".to_string(),
                                content,
                                session_key: sk.clone(),
                                metadata: None,
                            })
                            .await;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    }

    state.ws_clients.remove(&session_key);
    info!("WS disconnected: {}", &session_key[..8.min(session_key.len())]);
}

fn process_metrics() -> (f64, f32) {
    linux_proc_metrics().unwrap_or_else(sysinfo_metrics)
}

#[cfg(target_os = "linux")]
fn linux_proc_metrics() -> Option<(f64, f32)> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb = status
        .lines()
        .find(|l| l.starts_with("VmRSS:"))?
        .split_whitespace()
        .nth(1)?
        .parse::<f64>()
        .ok()?;
    Some((kb / 1024.0, 0.0))
}

#[cfg(not(target_os = "linux"))]
fn linux_proc_metrics() -> Option<(f64, f32)> {
    None
}

fn sysinfo_metrics() -> (f64, f32) {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    let mem_mb = sys.used_memory() as f64 / 1_048_576.0;
    (mem_mb, 0.0)
}
