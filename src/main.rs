mod config;
mod proxy;
mod routes;
mod types;
mod util;

use crate::config::load_config;
use crate::proxy::{KeyPool, ProxyEngine, ProxyHandler, UpstreamClient};
use crate::routes::create_router;
use anyhow::{Context, Result};
use clap::Parser;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::signal;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Server bind address
    #[arg(short, long)]
    bind: Option<String>,

    /// Configuration file path
    #[arg(short, long)]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    init_tracing()?;

    info!("Starting KeyCycleProxy Rust server...");

    // Parse command line arguments
    let args = Args::parse();

    // Load configuration
    let (config, api_keys) = load_config().context("Failed to load configuration")?;
    
    if api_keys.is_empty() {
        anyhow::bail!("No API keys configured. Please set OPENAI_KEYS environment variable or create config.json");
    }

    info!("Loaded {} API keys", api_keys.len());

    // Determine bind address
    let bind_addr = args.bind.unwrap_or(config.server.bind_addr.clone());

    // Initialize components
    let key_pool = Arc::new(KeyPool::new(api_keys, &config.keys.rotation_strategy));
    let upstream_client = UpstreamClient::new(config.upstream.clone())
        .context("Failed to create upstream client")?;
    let engine = Arc::new(ProxyEngine::new(
        key_pool.clone(),
        upstream_client,
        config.upstream.max_retries,
    ));
    let handler = Arc::new(ProxyHandler::new(engine));

    // Create router with middleware
    let app = create_router(
        handler,
        config.server.request_body_limit_bytes,
        Duration::from_millis(config.upstream.request_timeout_ms),
    );

    // Start latency measurement task
    start_latency_updater(key_pool.clone());

    // Start server
    info!("Server starting on {}", bind_addr);
    let listener = TcpListener::bind(&bind_addr)
        .await
        .with_context(|| format!("Failed to bind to {}", bind_addr))?;

    info!("KeyCycleProxy server running at http://{}/", bind_addr);

    // Start server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(config.server.graceful_shutdown_duration()))
        .await
        .context("Server error")?;

    info!("Server shutdown complete");
    Ok(())
}

fn init_tracing() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "key_cycle_proxy=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    Ok(())
}

fn start_latency_updater(key_pool: Arc<KeyPool>) {
    tokio::spawn(async move {
        // Initial latency measurement
        key_pool.update_all_latencies().await;
        info!("Initial latency measurements complete");

        // Update latencies every 60 seconds
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            key_pool.update_all_latencies().await;
        }
    });
}

async fn shutdown_signal(grace_period: Duration) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C, starting graceful shutdown...");
        },
        _ = terminate => {
            info!("Received SIGTERM, starting graceful shutdown...");
        },
    }

    // Give the server some time to finish ongoing requests
    if grace_period > Duration::ZERO {
        info!("Waiting {}s for ongoing requests to complete...", grace_period.as_secs());
        tokio::time::sleep(grace_period).await;
    }
}