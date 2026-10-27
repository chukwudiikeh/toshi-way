//! toshi-server: the API, the node event loop and the deadline scheduler in one process.
//!
//! Configuration is by environment variable:
//!   OPERATOR_TOKEN   required, token for the operator account
//!   DATABASE_URL     postgres://user:pass@host/db to use PostgreSQL (takes precedence)
//!   DATABASE_PATH    SQLite file when DATABASE_URL is not set (default ./toshi.db)
//!   BIND             listen address (default 127.0.0.1:3000)
//!   SHARED_NODE      "off" to require every seller to bring their own node (default "on")
//!   NODE_CREDENTIAL_KEY  64 hex characters (openssl rand -hex 32); enables sellers' own nodes
//!   FIAT_PROVIDER    off (default), coingecko, mempool, or fixed:USD=65000,NGN=97500000 (demo rates)
//!                    Shows an approximate fiat value next to amounts, taken once at creation. Display only.
//!   RATE_LIMIT_PER_MIN       requests per minute per caller (default 120)
//!   REGISTER_LIMIT_PER_MIN   account registrations per minute per address (default 10; raise it for a demo
//!                            where many people share one network address)
//!   ALLOW_PRIVATE_NODE_ADDRESSES  "1" to let sellers use private network addresses (regtest only)
//!   LND_ADDR         shared node, default https://localhost:10003
//!   LND_TLS_CERT     path to tls.cert
//!   LND_MACAROON     path to a macaroon limited to invoice operations
//!   CORS_ORIGINS     comma-separated browser origins (default http://localhost:3001,http://127.0.0.1:3001)
//!   PUBLIC_WEB_URL   the web app's public address (for example https://pay.example.com); with it the API
//!                    returns a checkout link for each escrow. Off by default.
//!   RUST_LOG         log filter (default info)

use std::{net::SocketAddr, sync::Arc, time::Duration};

use escrow_api::{ensure_operator, router, ApiConfig, AppState};
use escrow_core::{
    service::EscrowService,
    store::{Store, UserStore},
    timing::TimingConfig,
};
use escrow_lnd::{creds::CredentialKey, directory::LndDirectory, runner, LndAdapter};
use escrow_postgres::PostgresStore;
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

    // The service's own node, for sellers who have none. Optional: `SHARED_NODE=off` makes every
    // seller bring their own, so the service never receives anyone's money.
    let shared = if env("SHARED_NODE", Some("on")) == "off" {
        tracing::info!("shared node disabled: every seller must connect their own");
        None
    } else {
        Some(
            LndAdapter::connect(
                &env("LND_ADDR", Some("https://localhost:10003")),
                env("LND_TLS_CERT", Some("regtest/creds/seller/tls.cert")),
                env("LND_MACAROON", Some("regtest/creds/seller/toshi.macaroon")),
                Duration::from_secs(10),
            )
            .await?,
        )
    };

    // Sellers' macaroons are encrypted at rest under this key; without it sellers cannot connect nodes.
    let key = match std::env::var("NODE_CREDENTIAL_KEY").ok().filter(|k| !k.is_empty()) {
        Some(k) => Some(CredentialKey::from_hex(&k).map_err(|e| format!("NODE_CREDENTIAL_KEY: {e}"))?),
        None => {
            tracing::warn!("NODE_CREDENTIAL_KEY is not set: sellers cannot connect their own nodes (generate one with `openssl rand -hex 32`)");
            None
        }
    };
    let allow_private = env("ALLOW_PRIVATE_NODE_ADDRESSES", Some("0")) == "1";
    if allow_private {
        tracing::warn!("ALLOW_PRIVATE_NODE_ADDRESSES is on: sellers can point the server at private networks (regtest only)");
    }

    // PostgreSQL when DATABASE_URL is set, otherwise a SQLite file. Both pass the same store contract.
    match std::env::var("DATABASE_URL").ok().filter(|u| !u.is_empty()) {
        Some(url) => {
            tracing::info!("using PostgreSQL");
            run(Arc::new(PostgresStore::connect(&url).await?), shared, key, allow_private, operator_token, bind).await
        }
        None => {
            let path = env("DATABASE_PATH", Some("toshi.db"));
            tracing::info!(%path, "using SQLite");
            run(Arc::new(SqliteStore::open(&path).await?), shared, key, allow_private, operator_token, bind).await
        }
    }
}

async fn run<S>(
    store: S,
    shared: Option<LndAdapter>,
    key: Option<CredentialKey>,
    allow_private: bool,
    operator_token: String,
    bind: SocketAddr,
) -> Result<(), Box<dyn std::error::Error>>
where
    S: Store + UserStore + Clone + 'static,
{
    let directory = LndDirectory::new(shared, store.clone(), key, Duration::from_secs(10))
        .allow_private_addresses(allow_private);
    let service = Arc::new(EscrowService::new(directory, store.clone(), TimingConfig::default()));
    ensure_operator(&store, &operator_token, runner::unix_now()).await.map_err(|e| e.message)?;

    let limit = |name: &str, default: u32| -> u32 {
        env(name, Some(&default.to_string())).parse().unwrap_or_else(|_| {
            eprintln!("{name} must be a whole number");
            std::process::exit(2);
        })
    };
    let config = ApiConfig {
        rate_limit_per_min: limit("RATE_LIMIT_PER_MIN", 120),
        register_limit_per_min: limit("REGISTER_LIMIT_PER_MIN", 10),
        cors_origins: env("CORS_ORIGINS", Some("http://localhost:3001,http://127.0.0.1:3001"))
            .split(',')
            .map(|o| o.trim().to_string())
            .filter(|o| !o.is_empty())
            .collect(),
        web_base_url: std::env::var("PUBLIC_WEB_URL")
            .ok()
            .map(|u| u.trim().trim_end_matches('/').to_string())
            .filter(|u| u.starts_with("https://") || u.starts_with("http://")),
        ..ApiConfig::default()
    };
    let rates = escrow_api::rates::from_setting(&env("FIAT_PROVIDER", Some("off"))).unwrap_or_else(|e| {
        eprintln!("FIAT_PROVIDER: {e}");
        std::process::exit(2);
    });
    if !rates.currencies().is_empty() {
        tracing::info!(source = %rates.name(), currencies = rates.currencies().len(), "fiat values on (display only)");
    }
    let state = AppState::new(service.clone(), store, config, Arc::new(runner::unix_now)).with_rates(rates);
    let app = router(state);

    // Keep escrows in step with the node, and cancel holds at their deadline.
    let events = tokio::spawn({
        let service = service.clone();
        async move { runner::run_event_loop(&service, runner::EventLoopConfig::default(), std::future::pending()).await }
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
