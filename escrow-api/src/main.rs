//! toshi-server: the API, the node event loop and the deadline scheduler in one process.
//!
//! Configuration is by environment variable:
//!   OPERATOR_TOKEN   required, token for the operator account
//!   DATABASE_PATH    SQLite file (default ./toshi.db)
//!   BIND             listen address (default 127.0.0.1:3000)
//!   LND_ADDR         default https://localhost:10003
//!   LND_TLS_CERT     path to tls.cert
//!   LND_MACAROON     path to a macaroon limited to invoice operations
//!   RUST_LOG         log filter (default info)

use std::{net::SocketAddr, sync::Arc, time::Duration};

use escrow_api::{ensure_operator, router, ApiConfig, AppState};
use escrow_core::{service::EscrowService, timing::TimingConfig};
use escrow_lnd::{runner, LndAdapter};
use escrow_sqlite::SqliteStore;

fn env(name: &str, default: Option<&str>) -> String {
    match std::env::var(name) {
        Ok(v) if !v.is_empty() => v,
        _ => default.map(str::to_string).unwrap_or_else(|| {
            eprintln!("missing required environment variable {name}");
            std::process::exit(2);
        }),
    }
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let operator_token = env("OPERATOR_TOKEN", None);
    if operator_token.len() < 24 {
        eprintln!("OPERATOR_TOKEN must be at least 24 characters");
        std::process::exit(2);
    }
    let bind: SocketAddr = env("BIND", Some("127.0.0.1:3000")).parse()?;

    let store = Arc::new(SqliteStore::open(&env("DATABASE_PATH", Some("toshi.db"))).await?);
    let lnd = LndAdapter::connect(
        &env("LND_ADDR", Some("https://localhost:10003")),
        env("LND_TLS_CERT", Some("regtest/creds/seller/tls.cert")),
        env("LND_MACAROON", Some("regtest/creds/seller/admin.macaroon")),
        Duration::from_secs(10),
    )
    .await?;

    let service = Arc::new(EscrowService::new(lnd.clone(), store.clone(), TimingConfig::default()));
    ensure_operator(&store, &operator_token, runner::unix_now()).await.map_err(|e| e.message)?;

    let state = AppState::new(service.clone(), store, ApiConfig::default(), Arc::new(runner::unix_now));
    let app = router(state);

    // Keep escrows in step with the node, and cancel holds at their deadline.
    let events = tokio::spawn({
        let (service, lnd) = (service.clone(), lnd.clone());
        async move {
            runner::run_event_loop(&service, &lnd, runner::EventLoopConfig::default(), std::future::pending()).await
        }
    });
    let scheduler = tokio::spawn({
        let service = service.clone();
        async move { runner::run_scheduler(&service, Duration::from_secs(1), std::future::pending()).await }
    });

    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "toshi-server listening");
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    events.abort();
    scheduler.abort();
    Ok(())
}
