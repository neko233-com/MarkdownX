mod api;
mod config;
mod git;
mod live;
mod store;
mod templates;

use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::net::TcpListener;
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

use crate::api::AppState;
use crate::config::Config;
use crate::git::Repo;
use crate::live::Hub;
use crate::store::Store;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("markdownx_server=info,tower_http=info")))
        .init();

    let mut cfg = Config::from_args()?;
    if cfg.token.is_empty() && !cfg.dev {
        cfg.token = Uuid::new_v4().simple().to_string();
        tracing::warn!(token = %cfg.token, "no token configured; generated one for this run");
    }
    let store = Arc::new(Store::open(&cfg.root).with_context(|| format!("open {}", cfg.root.display()))?);
    let repo = Arc::new(Repo::open(&cfg.root, &cfg.commit_name, &cfg.commit_mail).await?);
    let hub = Arc::new(Hub::new(store.clone(), repo.clone(), cfg.max_bytes));
    let state = AppState { cfg: cfg.clone(), store, repo, hub };
    let web = ServeDir::new(&cfg.web_dir).append_index_html_on_directories(true);
    let app = api::router(state)
        .fallback_service(web)
        .layer(TraceLayer::new_for_http());

    let listener = TcpListener::bind(&cfg.addr).await.with_context(|| format!("bind {}", cfg.addr))?;
    tracing::info!(addr = %cfg.addr, root = %cfg.root.display(), dev = cfg.dev, "markdownx-server listening");
    axum::serve(listener, app).with_graceful_shutdown(shutdown()).await?;
    Ok(())
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}
