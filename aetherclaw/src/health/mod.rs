use axum::{routing::get, Json, Router};
use std::net::SocketAddr;
use tokio_util::sync::CancellationToken;
use tracing::info;

pub async fn serve(config: crate::config::GatewayConfig, cancel: CancellationToken) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/health", get(|| async { Json(serde_json::json!({"status": "ok"})) }))
        .route("/ready", get(|| async { Json(serde_json::json!({"status": "ready"})) }));

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("Health server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;

    axum::serve(listener, app)
        .with_graceful_shutdown(async move { cancel.cancelled().await })
        .await?;

    Ok(())
}
