mod config;
mod error;
mod model_cache;
mod models;
mod proxy;
mod rate_limiter;
mod tool_names;
mod transform;

use axum::{routing::post, Extension, Router};
use clap::Parser;
use config::{BackendProfile, CompatMode};
use proxy::{build_cors_layer, Config};
use reqwest::Client;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser, Debug)]
#[command(name = "anthmorph")]
#[command(about = "Anthropic to OpenAI-compatible proxy")]
#[command(version)]
struct Cli {
    #[arg(long)]
    port: Option<u16>,
    #[arg(long)]
    backend_url: Option<String>,
    #[arg(long)]
    model: Option<String>,
    #[arg(long)]
    reasoning_model: Option<String>,
    #[arg(long)]
    api_key: Option<String>,
    #[arg(long, value_enum)]
    backend_profile: Option<BackendProfile>,
    #[arg(long, value_enum)]
    compat_mode: Option<CompatMode>,
    #[arg(long)]
    ingress_api_key: Option<String>,
    #[arg(long)]
    allow_origin: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let mut config = Config::from_env();

    if let Some(port) = cli.port {
        config.port = port;
    }
    if let Some(url) = cli.backend_url {
        config.backend_url = url;
    }
    if let Some(m) = cli.model {
        config.primary_model = m;
    }
    if let Some(m) = cli.reasoning_model {
        config.reasoning_model = Some(m);
    }
    if let Some(k) = cli.api_key {
        config.api_key = Some(k);
    }
    if let Some(profile) = cli.backend_profile {
        config.backend_profile = profile;
    }
    if let Some(mode) = cli.compat_mode {
        config.compat_mode = mode;
    }
    if let Some(k) = cli.ingress_api_key {
        config.ingress_api_key = Some(k);
    }
    if !cli.allow_origin.is_empty() {
        config.allow_origins = cli.allow_origin;
    }

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "anthmorph=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("AnthMorph v{}", env!("CARGO_PKG_VERSION"));
    tracing::info!("Backend URL: {}", config.backend_url);
    tracing::info!("Backend Profile: {}", config.backend_profile.as_str());
    tracing::info!("Compat Mode: {}", config.compat_mode.as_str());
    tracing::info!("Primary Model: {}", config.primary_model);
    if let Some(ref m) = config.reasoning_model {
        tracing::info!("Reasoning Model: {}", m);
    }
    tracing::info!("Port: {}", config.port);

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .connect_timeout(std::time::Duration::from_secs(10))
        .pool_max_idle_per_host(10)
        .build()?;

    let config = Arc::new(config);
    let models_cache = model_cache::new_cache();

    let rate_limiter: Option<rate_limiter::SharedRateLimiter> = config
        .rate_limit_per_minute
        .map(|limit| {
            tracing::info!("Rate limiting enabled: {} requests/minute per client", limit);
            Arc::new(rate_limiter::RateLimiter::new(limit))
        });

    // Initial model cache load
    model_cache::refresh(&client, &config.models_url(), &models_cache).await;

    // Background refresh every 60s with graceful shutdown
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    {
        let client = client.clone();
        let models_url = config.models_url();
        let cache = models_cache.clone();
        let mut shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {
                        model_cache::refresh(&client, &models_url, &cache).await;
                    }
                    _ = shutdown.changed() => {
                        tracing::info!("Model cache refresh task shutting down");
                        break;
                    }
                }
            }
        });
    }

    // Rate limiter bucket cleanup every hour
    if let Some(limiter) = rate_limiter.clone() {
        let mut shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(3600)) => {
                        limiter.cleanup(3600).await;
                    }
                    _ = shutdown.changed() => { break; }
                }
            }
        });
    }

    let app = Router::new()
        .route("/v1/messages", post(proxy::proxy_handler))
        .route("/v1/responses", post(proxy::responses_handler))
        .route(
            "/v1/messages/count_tokens",
            post(proxy::count_tokens_handler),
        )
        .route("/v1/models", axum::routing::get(proxy::models_handler))
        .route("/health", axum::routing::get(health_handler))
        .layer(Extension(config.clone()))
        .layer(Extension(client))
        .layer(Extension(models_cache))
        .layer(Extension(rate_limiter))
        .layer(TraceLayer::new_for_http());

    let app = if let Some(cors) = build_cors_layer(&config)? {
        app.layer(cors)
    } else {
        app
    };

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!("Listening on {}", addr);
    tracing::info!("Proxy ready");

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            #[cfg(unix)]
            let sigterm = async {
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("failed to install SIGTERM handler")
                    .recv()
                    .await;
            };
            #[cfg(not(unix))]
            let sigterm = std::future::pending::<()>();
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = sigterm => {}
            }
            tracing::info!("Shutdown signal received");
            let _ = shutdown_tx.send(true);
        })
        .await?;

    Ok(())
}

async fn health_handler(
    Extension(config): Extension<Arc<Config>>,
) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "ok",
        "backend_profile": config.backend_profile.as_str(),
        "compat_mode": config.compat_mode.as_str(),
        "resolved_model": config.primary_model,
        "port": config.port,
    }))
}
