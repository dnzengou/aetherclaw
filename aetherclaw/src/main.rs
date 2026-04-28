use anyhow::Result;
use std::sync::Arc;
use tokio::signal;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

mod agent;
mod bus;
mod channels;
mod config;
mod health;
mod heartbeat;
mod llm;
mod tools;
mod tui;
mod web;

use config::Config;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "aetherclaw=info,tokio=warn".to_string()),
        )
        .with_target(false)
        .init();

    info!("AetherClaw initializing...");

    let config = if Config::exists() {
        Config::load()?
    } else {
        info!("First run detected. Launching TUI wizard...");
        tui::init_wizard::run().await?
    };

    info!("Config loaded. Workspace: {}", config.workspace.display());

    let cancellation_token = CancellationToken::new();

    let mut bus = bus::MessageBus::new(256);
    let db = Arc::new(tokio::sync::RwLock::new(
        tools::persistence::Database::new(&config.workspace.join("memory.db"))?,
    ));

    // Shared WebSocket client map: session_key → unbounded sender
    let ws_clients: web::WsClients = Arc::new(dashmap::DashMap::new());

    let llm_router = llm::ModelRouter::new(config.llm.clone()).await?;

    let mut handles = vec![];

    // 1. Health server
    handles.push(tokio::spawn({
        let token = cancellation_token.clone();
        let cfg = config.gateway.clone();
        async move {
            if let Err(e) = health::serve(cfg, token).await {
                warn!("Health server error: {}", e);
            }
        }
    }));

    // 2. Web dashboard
    handles.push(tokio::spawn({
        let token = cancellation_token.clone();
        let cfg = config.clone();
        let bus_tx = bus.inbound_tx.clone();
        let db_ref = db.clone();
        let ws = ws_clients.clone();
        async move {
            if let Err(e) = web::serve(cfg, bus_tx, db_ref, ws, token).await {
                warn!("Web server error: {}", e);
            }
        }
    }));

    // 3. Channel manager (Telegram, Discord, …)
    handles.push(tokio::spawn({
        let token = cancellation_token.clone();
        let cfg = config.channels.clone();
        let bus_tx = bus.inbound_tx.clone();
        async move {
            channels::manager::run(cfg, bus_tx, token).await;
        }
    }));

    // 4. Agent loop (CoT ReAct engine)
    handles.push(tokio::spawn({
        let token = cancellation_token.clone();
        let mut bus_rx = bus.take_inbound_rx().expect("inbound_rx already consumed");
        let bus_tx = bus.outbound_tx.clone();
        let llm = llm_router;
        let tools = tools::ToolKit::new(&config.workspace, config.security.restrict_to_workspace);
        let workspace = config.workspace.clone();
        let db_ref = db.clone();
        async move {
            agent::AgentLoop::new(llm, tools, bus_tx, &workspace, db_ref)
                .run(&mut bus_rx, token)
                .await;
        }
    }));

    // 5. Heartbeat / cron
    if config.heartbeat.enabled {
        handles.push(tokio::spawn({
            let token = cancellation_token.clone();
            let interval = config.heartbeat.interval;
            let workspace = config.workspace.clone();
            async move {
                heartbeat::service::run(interval, workspace, token).await;
            }
        }));
    }

    // 6. Outbound message router: agent responses → WebSocket clients
    handles.push(tokio::spawn({
        let ws = ws_clients.clone();
        let mut outbound_rx = bus.take_outbound_rx().expect("outbound_rx already consumed");
        async move {
            while let Some(msg) = outbound_rx.recv().await {
                if msg.channel == "web" {
                    if let Some(client) = ws.get(&msg.session_key) {
                        let event = serde_json::json!({
                            "role": "assistant",
                            "content": msg.content,
                            "session_key": &msg.session_key,
                            "metadata": { "local": false }
                        });
                        let _ = client.send(event.to_string());
                    }
                }
                // Telegram / Discord outbound is handled by their own channel tasks
                // TODO: add routing for other channels here
            }
        }
    }));

    tokio::select! {
        _ = signal::ctrl_c() => {
            info!("Shutdown signal received...");
            cancellation_token.cancel();
        }
        _ = futures::future::join_all(handles) => {
            warn!("All services exited unexpectedly");
        }
    }

    info!("AetherClaw shutdown complete.");
    Ok(())
}
