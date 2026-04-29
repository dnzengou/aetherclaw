use anyhow::Result;
use std::sync::Arc;
use tokio::signal;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

mod agent;
mod bus;
mod channels;
mod config;
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
    } else if std::env::var("NO_TUI").is_ok() || !atty::is(atty::Stream::Stdout) {
        // Headless (Docker, CI, systemd): build config from environment variables
        info!("No config found — building from environment variables (headless mode)");
        let cfg = build_config_from_env();
        cfg.save()?;
        cfg
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

    // 1. Web dashboard
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

fn build_config_from_env() -> Config {
    use config::{telegram, discord, ModelEntry};

    let workspace = std::env::var("WORKSPACE_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/workspace"));

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let mut cfg = Config {
        workspace,
        gateway: config::GatewayConfig {
            host: std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port,
            dashboard_enabled: true,
        },
        channels: config::ChannelsConfig {
            telegram: None,
            discord: None,
        },
        llm: config::LlmConfig {
            local_models_path: std::path::PathBuf::from("/data/models"),
            default_local_model: "phi-2-q4.gguf".to_string(),
            cloud_fallback: None,
            model_list: vec![],
        },
        heartbeat: config::HeartbeatConfig {
            enabled: true,
            interval: 30,
        },
        security: config::SecurityConfig {
            restrict_to_workspace: true,
            allow_exec: true,
        },
    };

    if let Ok(token) = std::env::var("TELEGRAM_BOT_TOKEN") {
        cfg.channels.telegram = Some(telegram::TelegramConfig {
            enabled: true,
            token,
            allow_from: vec![],
            proxy: None,
        });
    }

    if let Ok(token) = std::env::var("DISCORD_BOT_TOKEN") {
        cfg.channels.discord = Some(discord::DiscordConfig {
            enabled: true,
            token,
            allow_from: vec![],
        });
    }

    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        cfg.llm.model_list.push(ModelEntry {
            model_name: std::env::var("OPENAI_MODEL")
                .unwrap_or_else(|_| "gpt-4o-mini".to_string()),
            model: "openai/gpt-4o-mini".to_string(),
            api_key: Some(key),
            api_base: std::env::var("OPENAI_API_BASE").ok(),
        });
    }

    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        cfg.llm.model_list.push(ModelEntry {
            model_name: "claude-3-haiku".to_string(),
            model: "anthropic/claude-3-haiku-20240307".to_string(),
            api_key: Some(key),
            api_base: None,
        });
    }

    cfg
}
