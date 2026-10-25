//! Display-only fiat values over the HTTP API (mock node, in-memory store).

use std::sync::{
    atomic::{AtomicI64, Ordering},
    Arc, Mutex,
};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use escrow_api::{
    ensure_operator,
    rates::{Disabled, FixedRates, Rate, RateError, RateProvider},
    router, ApiConfig, AppState,
};
use escrow_core::{mock::MockLightning, model::Preimage, service::EscrowService, store::MemoryStore, timing::TimingConfig};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const T0: i64 = 1_000_000;
const OPERATOR_TOKEN: &str = "operator-token-for-tests-0123456789";

struct App {
    router: Router,
    node: MockLightning,
    service: Arc<EscrowService<MockLightning, Arc<MemoryStore>>>,
}

struct Account {
    id: String,
    token: String,
}

async fn app_with(rates: Arc<dyn RateProvider>) -> App {
    let node = MockLightning::new();
    let store = Arc::new(MemoryStore::new());
    let service = Arc::new(EscrowService::new(node.clone(), store.clone(), TimingConfig::default()));
    let clock = Arc::new(AtomicI64::new(T0));
    let now = {
        let clock = clock.clone();
        Arc::new(move || clock.load(Ordering::SeqCst))
    };
    ensure_operator(&store, OPERATOR_TOKEN, T0).await.unwrap();
    let config = ApiConfig { rate_limit_per_min: 100_000, register_limit_per_min: 100_000, ..Default::default() };
    App { router: router(AppState::new(service.clone(), store, config, now).with_rates(rates)), node, service }
}

impl App {
    async fn call(&self, method: Method, path: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
        self.call_with(method, path, token, body, &[]).await
    }

    async fn call_with(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
        extra: &[(&str, &str)],
    ) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        for (k, v) in extra {
            req = req.header(*k, *v);
        }
        let body = match body {
            Some(b) => {
                req = req.header("content-type", "application/json");
                Body::from(b.to_string())
            }
            None => Body::empty(),
        };
        let res = self.router.clone().oneshot(req.body(body).unwrap()).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn register(&self) -> Account {
        let (s, v) = self.call(Method::POST, "/api/v1/users", None, None).await;
        assert_eq!(s, StatusCode::CREATED);
        Account { id: v["id"].as_str().unwrap().into(), token: v["token"].as_str().unwrap().into() }
    }

    async fn create(&self, seller: &Account, buyer: &Account, sats: u64, fiat: Option<&str>) -> (StatusCode, Value) {
        let mut body = json!({"buyer_id": buyer.id, "amount_sats": sats, "description": "parcel", "hold_secs": 3600});
        if let Some(c) = fiat {
            body["fiat_currency"] = json!(c);
        }
        self.call(Method::POST, "/api/v1/escrows", Some(&seller.token), Some(body)).await
    }
}

fn demo_rates() -> Arc<dyn RateProvider> {
    Arc::new(FixedRates::parse("USD=65000,NGN=97500000,JPY=9800000").unwrap())
}

/// A price feed whose price and health the test controls.
struct Market {
    state: Mutex<Result<i64, ()>>,
}

impl Market {
    fn at(rate_minor: i64) -> Arc<Self> {
        Arc::new(Self { state: Mutex::new(Ok(rate_minor)) })
    }
    fn set(&self, s: Result<i64, ()>) {
        *self.state.lock().unwrap() = s;
    }
}

#[async_trait]
impl RateProvider for Market {
    fn name(&self) -> String {
        "test market".into()
    }
    fn currencies(&self) -> Vec<String> {
        vec!["USD".into()]
    }
    async fn rate(&self, currency: &str, now: i64) -> Result<Rate, RateError> {
        if currency != "USD" {
            return Err(RateError::Unsupported(currency.into()));
        }
        let r = (*self.state.lock().unwrap()).map_err(|_| RateError::Unavailable)?;
        Ok(Rate { rate_minor_per_btc: r, as_of: now, source: self.name() })
    }
}

#[tokio::test]
async fn fiat_values_are_off_unless_configured() {
    let app = app_with(Arc::new(Disabled)).await;
    let (seller, buyer) = (app.register().await, app.register().await);

    let (s, v) = app.call(Method::GET, "/api/v1/rates", Some(&seller.token), None).await;
    assert_eq!((s, v["enabled"].as_bool(), v["currencies"].as_array().map(Vec::len)), (StatusCode::OK, Some(false), Some(0)));

    let (s, v) = app.create(&seller, &buyer, 5000, Some("USD")).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("fiat_disabled")), "{v}");
    let (s, v) = app.call(Method::GET, "/api/v1/rates/USD", Some(&seller.token), None).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("fiat_disabled")));

    // An escrow made without one is exactly as before: no fiat.
    let (s, v) = app.create(&seller, &buyer, 5000, None).await;
    assert_eq!((s, v["fiat"].is_null()), (StatusCode::CREATED, true), "{v}");
}

#[tokio::test]
async fn the_rates_endpoints_list_currencies_and_preview_a_price() {
    let app = app_with(demo_rates()).await;
    let seller = app.register().await;

    let (_, v) = app.call(Method::GET, "/api/v1/rates", Some(&seller.token), None).await;
    assert_eq!((v["enabled"].as_bool(), v["source"].as_str()), (Some(true), Some("demo rate")));
    let codes: Vec<_> = v["currencies"].as_array().unwrap().iter().map(|c| c["code"].as_str().unwrap()).collect();
    assert_eq!(codes, ["JPY", "NGN", "USD"]);
    let jpy = v["currencies"].as_array().unwrap().iter().find(|c| c["code"] == "JPY").unwrap();
    assert_eq!(jpy["exponent"], 0);

    // The code may be any case; the answer is in minor units per bitcoin.
    let (s, v) = app.call(Method::GET, "/api/v1/rates/usd", Some(&seller.token), None).await;
    assert_eq!((s, v["currency"].as_str(), v["rate_minor_per_btc"].as_i64(), v["source"].as_str()), (StatusCode::OK, Some("USD"), Some(6_500_000), Some("demo rate")), "{v}");
    assert_eq!(v["quoted_at"], T0);

    // Unknown and unconfigured currencies are refused.
    for c in ["XYZ", "EUR"] {
        let (s, v) = app.call(Method::GET, &format!("/api/v1/rates/{c}"), Some(&seller.token), None).await;
        assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("unsupported_currency")), "{c}");
    }
    // It needs an account.
    assert_eq!(app.call(Method::GET, "/api/v1/rates", None, None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(app.call(Method::GET, "/api/v1/rates/USD", None, None).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_escrow_carries_a_locked_fiat_value_that_everyone_sees() {
    let app = app_with(demo_rates()).await;
    let (seller, buyer) = (app.register().await, app.register().await);

    // 5,000 sats at 65,000.00 USD per bitcoin is 3.25 USD.
    let (s, v) = app.create(&seller, &buyer, 5000, Some("usd")).await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    assert_eq!(v["amount_sats"], 5000, "the escrow itself is still in sats");
    let f = &v["fiat"];
    assert_eq!(
        (f["currency"].as_str(), f["amount_minor"].as_i64(), f["rate_minor_per_btc"].as_i64(), f["exponent"].as_u64(), f["source"].as_str(), f["quoted_at"].as_i64()),
        (Some("USD"), Some(325), Some(6_500_000), Some(2), Some("demo rate"), Some(T0))
    );

    // Naira and yen use their own minor units: 1,000 sats at 97,500,000 NGN and at 9,800,000 JPY.
    let (_, ngn) = app.create(&seller, &buyer, 1000, Some("NGN")).await;
    assert_eq!(ngn["fiat"]["amount_minor"], 97_500, "975.00 NGN");
    let (_, jpy) = app.create(&seller, &buyer, 1000, Some("JPY")).await;
    assert_eq!((jpy["fiat"]["amount_minor"].as_i64(), jpy["fiat"]["exponent"].as_u64()), (Some(98), Some(0)), "98 yen");

    // Buyer, seller and the escrow list all show the same value.
    let id = v["id"].as_str().unwrap();
    for who in [&buyer, &seller] {
        let (_, e) = app.call(Method::GET, &format!("/api/v1/escrows/{id}"), Some(&who.token), None).await;
        assert_eq!(e["fiat"]["amount_minor"], 325);
    }
    let (_, list) = app.call(Method::GET, "/api/v1/escrows", Some(&buyer.token), None).await;
    assert!(list["escrows"].as_array().unwrap().iter().any(|e| e["id"] == id && e["fiat"]["amount_minor"] == 325));
}

#[tokio::test]
async fn the_value_is_locked_at_creation_whatever_the_market_does_afterwards() {
    let market = Market::at(6_500_000);
    let app = app_with(market.clone()).await;
    let (seller, buyer) = (app.register().await, app.register().await);
    let (_, v) = app.create(&seller, &buyer, 10_000, Some("USD")).await;
    let id = v["id"].as_str().unwrap().to_string();
    assert_eq!(v["fiat"]["amount_minor"], 650);

    // The price halves, then the feed dies. The escrow keeps what it was worth when it was offered,
    // through accepting, paying, and settling.
    market.set(Ok(3_250_000));
    let p = Preimage::from_bytes([1; 32]);
    let (s, _) = app
        .call(
            Method::POST,
            &format!("/api/v1/escrows/{id}/accept"),
            Some(&buyer.token),
            Some(json!({"payment_hash": hex::encode(p.hash())})),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let ev = app.node.pay(&p.hash());
    app.service.apply_event(ev, T0 + 1).await.unwrap();
    market.set(Err(()));
    let (s, done) = app
        .call(
            Method::POST,
            &format!("/api/v1/escrows/{id}/release"),
            Some(&seller.token),
            Some(json!({"preimage": hex::encode(p.as_bytes())})),
        )
        .await;
    assert_eq!((s, done["state"].as_str()), (StatusCode::OK, Some("SETTLED")), "a dead price feed never blocks a release");
    assert_eq!(done["fiat"]["amount_minor"], 650);
    assert_eq!(done["fiat"]["rate_minor_per_btc"], 6_500_000);
    assert_eq!(done["amount_sats"], 10_000);
}

#[tokio::test]
async fn a_requested_value_that_cannot_be_given_is_an_error_and_nothing_is_created() {
    let market = Market::at(6_500_000);
    let app = app_with(market.clone()).await;
    let (seller, buyer) = (app.register().await, app.register().await);

    market.set(Err(()));
    let (s, v) = app.create(&seller, &buyer, 5000, Some("USD")).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::SERVICE_UNAVAILABLE, Some("rate_unavailable")), "{v}");
    let (_, list) = app.call(Method::GET, "/api/v1/escrows", Some(&seller.token), None).await;
    assert!(list["escrows"].as_array().unwrap().is_empty(), "no escrow is created with a silently missing value");

    // Leaving the value out still works while the feed is down: it never blocks an escrow.
    let (s, v) = app.create(&seller, &buyer, 5000, None).await;
    assert_eq!((s, v["fiat"].is_null()), (StatusCode::CREATED, true));

    // A currency this feed does not price, and one nobody supports.
    market.set(Ok(6_500_000));
    let (s, v) = app.create(&seller, &buyer, 5000, Some("NGN")).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("unsupported_currency")));
    let (s, v) = app.create(&seller, &buyer, 5000, Some("XYZ")).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("unsupported_currency")));
    // A blank currency means none.
    let (s, v) = app.create(&seller, &buyer, 5000, Some("  ")).await;
    assert_eq!((s, v["fiat"].is_null()), (StatusCode::CREATED, true));
}

#[tokio::test]
async fn a_broken_price_cannot_produce_a_value_and_the_amount_rules_still_apply() {
    let market = Market::at(0);
    let app = app_with(market.clone()).await;
    let (seller, buyer) = (app.register().await, app.register().await);
    let (s, v) = app.create(&seller, &buyer, 5000, Some("USD")).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::SERVICE_UNAVAILABLE, Some("rate_unavailable")), "a zero rate is a broken feed: {v}");

    // The sats limits are unchanged by a fiat value; the fiat value is not a way around them.
    market.set(Ok(6_500_000));
    let (s, v) = app.create(&seller, &buyer, 1_000_001, Some("USD")).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("invalid_amount")));
    let (s, v) = app.create(&seller, &buyer, 1_000_000, Some("USD")).await;
    assert_eq!((s, v["fiat"]["amount_minor"].as_i64()), (StatusCode::CREATED, Some(65_000)), "650.00 USD");
}

#[tokio::test]
async fn the_currency_is_part_of_what_an_idempotency_key_protects() {
    let app = app_with(demo_rates()).await;
    let (seller, buyer) = (app.register().await, app.register().await);
    let body = |cur: &str| json!({"buyer_id": buyer.id, "amount_sats": 5000, "description": "p", "hold_secs": 3600, "fiat_currency": cur});
    let (s, first) = app.call_with(Method::POST, "/api/v1/escrows", Some(&seller.token), Some(body("USD")), &[("idempotency-key", "k1")]).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, again) = app.call_with(Method::POST, "/api/v1/escrows", Some(&seller.token), Some(body("USD")), &[("idempotency-key", "k1")]).await;
    assert_eq!((s, &again["id"]), (StatusCode::CREATED, &first["id"]), "a retry returns the first escrow");
    let (s, v) = app.call_with(Method::POST, "/api/v1/escrows", Some(&seller.token), Some(body("NGN")), &[("idempotency-key", "k1")]).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("idempotency_key_reuse")));
}
