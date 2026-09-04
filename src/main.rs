mod api;
mod game;
mod state;
mod watcher;

use anyhow::Context;
use pubky_watcher::WatcherClient;
use state::AppState;
use tokio::sync::watch;
use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=debug".into()),
        )
        .init();

    let client = WatcherClient::mainnet().context("create Pubky watcher client")?;
    let state = AppState::new(client);
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let watcher_task = tokio::spawn(watcher::run(state.clone(), shutdown_rx));

    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:3001".to_owned());
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "web/dist".to_owned());
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("bind app to {bind_addr}"))?;
    info!(%bind_addr, %static_dir, "app listening");

    axum::serve(listener, api::router(state, &static_dir))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve API")?;

    let _ = shutdown_tx.send(true);
    let _ = watcher_task.await;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
