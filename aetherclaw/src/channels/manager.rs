use crate::bus::InboundMessage;
use crate::config::ChannelsConfig;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::info;

pub async fn run(config: ChannelsConfig, bus_tx: mpsc::Sender<InboundMessage>, cancel: CancellationToken) {
    let mut handles = vec![];

    if let Some(tg) = config.telegram {
        if tg.enabled {
            let tx = bus_tx.clone();
            let token = cancel.clone();
            handles.push(tokio::spawn(async move {
                telegram::run(tg, tx, token).await;
            }));
        }
    }

    // Future: Discord, Slack, etc.

    futures::future::join_all(handles).await;
}

pub mod telegram {
    use super::*;
    use teloxide::prelude::*;
    use teloxide::types::ParseMode;

    pub async fn run(config: super::super::config::telegram::TelegramConfig, bus_tx: mpsc::Sender<InboundMessage>, cancel: CancellationToken) {
        info!("Starting Telegram channel...");

        let bot = Bot::new(&config.token);
        
        // Allowlist check
        let allowlist = config.allow_from.clone();

        let handler = Update::filter_message().branch(dptree::endpoint(move |msg: Message, bot: Bot| {
            let bus_tx = bus_tx.clone();
            let allowed = allowlist.clone();
            
            async move {
                let user_id = msg.from.as_ref().map(|u| u.id.to_string()).unwrap_or_default();
                let username = msg.from.as_ref().and_then(|u| u.username.clone());

                // Check allowlist
                if !allowed.is_empty() {
                    let allowed_str = allowed.join(",");
                    if !allowed_str.contains(&user_id) && !allowed.iter().any(|a| username.as_ref().map(|u| a.contains(u)).unwrap_or(false)) {
                        return Ok::<(), teloxide::RequestError>(());
                    }
                }

                let content = msg.text().unwrap_or("").to_string();
                let session_key = format!("tg:{}", msg.chat.id);

                let inbound = InboundMessage {
                    channel: "telegram".to_string(),
                    sender: user_id,
                    content,
                    session_key,
                    metadata: None,
                };

                let _ = bus_tx.send(inbound).await;

                // Send placeholder "thinking"
                let _ = bot.send_message(msg.chat.id, "🤔 AetherClaw is thinking...")
                    .parse_mode(ParseMode::Html)
                    .await;

                Ok(())
            }
        }));

        Dispatcher::builder(bot, handler)
            .enable_ctrlc_handler()
            .build()
            .dispatch()
            .await;
    }
}
