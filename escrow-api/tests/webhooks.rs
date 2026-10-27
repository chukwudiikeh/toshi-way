//! Webhooks over the HTTP API: setting up endpoints, and delivery of escrow changes, signed, in
//! order and retried (mock node, in-memory store, and a stand-in for the network).

use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, Mutex,
    },
};

use async_trait::async_trait;
use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use escrow_api::{
    ensure_operator, router,
    webhooks::{sign, Dispatcher, Sender, DISABLE_AFTER_SECS},
    ApiConfig, AppState, WebhookSupport,
};
use escrow_core::{
    mock::MockLightning,
    model::Preimage,
    service::EscrowService,
    store::{MemoryStore, UserStore},
    timing::TimingConfig,
};
use escrow_lnd::creds::CredentialKey;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const T0: i64 = 1_000_000;
const OPERATOR_TOKEN: &str = "operator-token-for-tests-0123456789";

#[derive(Clone, Debug)]
struct Sent {
    url: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Sent {
    fn header(&self, name: &str) -> &str {
        &self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).unwrap().1
    }
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap()
    }
}

/// Stands in for the network: records every request and answers from a script (200 when it runs out).
#[derive(Default)]
struct FakeNet {
    sent: Mutex<Vec<Sent>>,
    script: Mutex<VecDeque<Result<u16, String>>>,
}

#[async_trait]
impl Sender for FakeNet {
    async fn send(&self, url: &str, headers: Vec<(String, String)>, body: String) -> Result<u16, String> {
        self.sent.lock().unwrap().push(Sent { url: url.into(), headers, body });
        self.script.lock().unwrap().pop_front().unwrap_or(Ok(200))
    }
    async fn check_url(&self, url: &str) -> Result<(), String> {
        // The real sender's rules are tested in the library; here only what the routes pass on.
        if url.starts_with("https://") && !url.contains('@') {
            Ok(())
        } else {
            Err("the address must start with https://".into())
        }
    }
}

struct App {
    router: Router,
    node: MockLightning,
    service: Arc<EscrowService<MockLightning, Arc<MemoryStore>>>,
    store: Arc<MemoryStore>,
    net: Arc<FakeNet>,
    dispatcher: Dispatcher<Arc<MemoryStore>>,
    clock: Arc<AtomicI64>,
}

struct Account {
    id: String,
    token: String,
}

async fn app(with_key: bool) -> App {
    let node = MockLightning::new();
    let store = Arc::new(MemoryStore::new());
    let service = Arc::new(EscrowService::new(node.clone(), store.clone(), TimingConfig::default()));
    let clock = Arc::new(AtomicI64::new(T0));
    let now = {
        let clock = clock.clone();
        Arc::new(move || clock.load(Ordering::SeqCst))
    };
    ensure_operator(&store, OPERATOR_TOKEN, T0).await.unwrap();
    let config = ApiConfig {
        rate_limit_per_min: 100_000,
        register_limit_per_min: 100_000,
        web_base_url: Some("https://pay.example.com".into()),
        ..Default::default()
    };
    let net = Arc::new(FakeNet::default());
    let key = CredentialKey::from_hex(&"ab".repeat(32)).unwrap();
    let mut state = AppState::new(service.clone(), store.clone(), config, now);
    if with_key {
        state = state.with_webhooks(WebhookSupport { key: key.clone(), sender: net.clone() });
    }
    let dispatcher = Dispatcher::new(store.clone(), key, net.clone(), Some("https://pay.example.com".into()));
    App { router: router(state), node, service, store, net, dispatcher, clock }
}

impl App {
    async fn call(&self, method: Method, path: &str, token: &str, body: Option<Value>) -> (StatusCode, Value) {
        let req = Request::builder().method(method).uri(path).header("authorization", format!("Bearer {token}"));
        let req = match body {
            Some(b) => req.header("content-type", "application/json").body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        let res = self.router.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn register(&self) -> Account {
        let req = Request::builder().method(Method::POST).uri("/api/v1/users").body(Body::empty()).unwrap();
        let res = self.router.clone().oneshot(req).await.unwrap();
        let v: Value = serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
        Account { id: v["id"].as_str().unwrap().into(), token: v["token"].as_str().unwrap().into() }
    }

    fn now(&self) -> i64 {
        self.clock.load(Ordering::SeqCst)
    }

    fn advance(&self, secs: i64) {
        self.clock.fetch_add(secs, Ordering::SeqCst);
    }

    /// Add an endpoint; returns (id, signing secret).
    async fn hook(&self, owner: &Account, url: &str) -> (String, String) {
        let (s, v) = self.call(Method::POST, "/api/v1/me/webhooks", &owner.token, Some(json!({"url": url}))).await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
        (v["id"].as_str().unwrap().into(), v["secret"].as_str().unwrap().into())
    }

    async fn create(&self, seller: &Account, buyer: &Account) -> String {
        let (s, v) = self
            .call(
                Method::POST,
                "/api/v1/escrows",
                &seller.token,
                Some(json!({"buyer_id": buyer.id, "amount_sats": 1000, "description": "parcel", "hold_secs": 3600, "reference": "ORD-7"})),
            )
            .await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
        v["id"].as_str().unwrap().into()
    }

    async fn accept(&self, buyer: &Account, id: &str, p: &Preimage) {
        let (s, _) = self
            .call(Method::POST, &format!("/api/v1/escrows/{id}/accept"), &buyer.token, Some(json!({"payment_hash": hex::encode(p.hash())})))
            .await;
        assert_eq!(s, StatusCode::OK);
    }

    async fn pay(&self, p: &Preimage) {
        let ev = self.node.pay(&p.hash());
        self.service.apply_event(ev, self.now()).await.unwrap();
    }

    async fn tick(&self) -> usize {
        self.dispatcher.tick(self.now()).await
    }

    fn sent(&self) -> Vec<Sent> {
        self.net.sent.lock().unwrap().clone()
    }

    fn types(&self) -> Vec<String> {
        self.sent().iter().map(|s| s.json()["type"].as_str().unwrap().to_string()).collect()
    }
}

fn secret(n: u8) -> Preimage {
    Preimage::from_bytes([n; 32])
}

#[tokio::test]
async fn webhooks_need_the_credential_key_and_an_account_token() {
    let off = app(false).await;
    let shop = off.register().await;
    let (s, v) = off.call(Method::POST, "/api/v1/me/webhooks", &shop.token, Some(json!({"url": "https://a.example.com/h"}))).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("webhooks_unavailable")), "{v}");

    let app = app(true).await;
    let shop = app.register().await;
    // An API key cannot manage endpoints, nor can an operator have any.
    let (_, key) = app.call(Method::POST, "/api/v1/me/api-keys", &shop.token, Some(json!({"name": "k", "scopes": ["escrows:read", "escrows:write"]}))).await;
    let key = key["key"].as_str().unwrap();
    assert_eq!(app.call(Method::GET, "/api/v1/me/webhooks", key, None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(app.call(Method::POST, "/api/v1/me/webhooks", key, Some(json!({"url": "https://a.example.com/h"}))).await.0, StatusCode::FORBIDDEN);
    assert_eq!(app.call(Method::POST, "/api/v1/me/webhooks", OPERATOR_TOKEN, Some(json!({"url": "https://a.example.com/h"}))).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_endpoint_is_made_listed_without_its_secret_and_deleted_by_its_owner() {
    let app = app(true).await;
    let (shop, other) = (app.register().await, app.register().await);
    let (id, secret) = app.hook(&shop, "https://hooks.example.com/toshi").await;
    assert!(secret.starts_with("whsec_") && secret.len() == 70, "{secret}");

    let (s, list) = app.call(Method::GET, "/api/v1/me/webhooks", &shop.token, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(list["webhooks"][0]["url"], "https://hooks.example.com/toshi");
    assert_eq!(list["webhooks"][0]["status"], "ok");
    assert!(!list.to_string().contains(&secret), "the secret is shown once");
    // At rest it is sealed.
    let stored = app.store.list_webhooks(&shop.id).await.unwrap();
    assert!(!stored[0].signing_key_enc.windows(secret.len()).any(|w| w == secret.as_bytes()));

    for bad in ["http://hooks.example.com/x", "https://user@hooks.example.com/x", "nonsense"] {
        let (s, v) = app.call(Method::POST, "/api/v1/me/webhooks", &shop.token, Some(json!({"url": bad}))).await;
        assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_url")), "{bad}");
    }

    let path = format!("/api/v1/me/webhooks/{id}");
    assert_eq!(app.call(Method::DELETE, &path, &other.token, None).await.0, StatusCode::NOT_FOUND, "not theirs");
    assert_eq!(app.call(Method::POST, &format!("{path}/test"), &other.token, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(app.call(Method::DELETE, &path, &shop.token, None).await.0, StatusCode::OK);
    assert_eq!(app.call(Method::DELETE, &path, &shop.token, None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn there_is_a_limit_on_endpoints() {
    let app = app(true).await;
    let shop = app.register().await;
    for i in 0..5 {
        app.hook(&shop, &format!("https://h{i}.example.com/x")).await;
    }
    let (s, v) = app.call(Method::POST, "/api/v1/me/webhooks", &shop.token, Some(json!({"url": "https://h6.example.com/x"}))).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("too_many_webhooks")), "{v}");
}

#[tokio::test]
async fn every_change_is_delivered_once_in_order_and_signed() {
    let app = app(true).await;
    let (shop, buyer) = (app.register().await, app.register().await);
    let (_, whsec) = app.hook(&shop, "https://hooks.example.com/toshi").await;
    let p = secret(1);

    let id = app.create(&shop, &buyer).await;
    app.accept(&buyer, &id, &p).await;
    app.pay(&p).await;
    app.advance(5);
    let (s, _) = app.call(Method::POST, &format!("/api/v1/escrows/{id}/release"), &buyer.token, Some(json!({"preimage": hex::encode(p.as_bytes())}))).await;
    assert_eq!(s, StatusCode::OK);

    assert_eq!(app.tick().await, 3);
    assert_eq!(app.types(), vec!["escrow.accepted", "escrow.locked", "escrow.settled"]);

    let sent = app.sent();
    let now = app.now();
    for s in &sent {
        assert_eq!(s.url, "https://hooks.example.com/toshi");
        // A receiver checks: the signature over "<timestamp>.<body>" with the endpoint's secret.
        let header = s.header("toshi-signature");
        assert_eq!(header, format!("t={now},v1={}", sign(&whsec, now, &s.body)));
        assert_eq!(s.header("toshi-event-id"), s.json()["id"].as_str().unwrap());
        assert_eq!(s.header("toshi-event-type"), s.json()["type"].as_str().unwrap());
        assert_eq!(s.header("content-type"), "application/json");
    }

    // What is in an event: its state, and the escrow with the business's own order number and checkout link.
    let locked = sent[1].json();
    assert_eq!(locked["data"]["state"], "LOCKED");
    assert_eq!(locked["data"]["escrow"]["id"], id.as_str());
    assert_eq!(locked["data"]["escrow"]["reference"], "ORD-7");
    assert_eq!(locked["data"]["escrow"]["checkout_url"], format!("https://pay.example.com/escrows/{id}").as_str());
    assert!(!sent[1].body.contains("payment_request"), "no invoice or secret in an event");
    let ids: Vec<String> = sent.iter().map(|s| s.json()["id"].as_str().unwrap().to_string()).collect();
    assert!(ids.windows(2).all(|w| w[0] < w[1] || w[0].len() < w[1].len()), "ids rise: {ids:?}");

    // Caught up: nothing more, and nothing twice.
    assert_eq!(app.tick().await, 0);
    assert_eq!(app.sent().len(), 3);
}

#[tokio::test]
async fn only_what_happens_after_the_endpoint_exists_and_only_its_owners_escrows() {
    let app = app(true).await;
    let (shop, rival, buyer) = (app.register().await, app.register().await, app.register().await);
    // Something that happened before the endpoint existed is not sent.
    let early = app.create(&shop, &buyer).await;
    app.accept(&buyer, &early, &secret(1)).await;

    app.hook(&shop, "https://hooks.example.com/toshi").await;
    assert_eq!(app.tick().await, 0, "no past events");

    // A rival's escrow produces events, but not for this endpoint.
    let theirs = app.create(&rival, &buyer).await;
    app.accept(&buyer, &theirs, &secret(2)).await;
    assert_eq!(app.tick().await, 0);

    app.pay(&secret(1)).await;
    assert_eq!(app.tick().await, 1);
    assert_eq!(app.types(), vec!["escrow.locked"]);
    assert_eq!(app.sent()[0].json()["data"]["escrow"]["id"], early.as_str());
}

#[tokio::test]
async fn a_failed_delivery_is_retried_later_and_nothing_is_lost_or_reordered() {
    let app = app(true).await;
    let (shop, buyer) = (app.register().await, app.register().await);
    let (hook, _) = app.hook(&shop, "https://hooks.example.com/toshi").await;
    let p = secret(1);
    let id = app.create(&shop, &buyer).await;
    app.accept(&buyer, &id, &p).await;
    app.pay(&p).await;

    // The receiver is down: the first try fails, and nothing behind it jumps the queue.
    app.net.script.lock().unwrap().extend([Ok(500)]);
    assert_eq!(app.tick().await, 0);
    assert_eq!(app.types(), vec!["escrow.accepted"], "stops at the first failure");
    let status = |a: &App| {
        let store = a.store.clone();
        let id = hook.clone();
        async move { store.get_webhook(&id).await.unwrap().unwrap() }
    };
    let h = status(&app).await;
    assert_eq!((h.attempts, h.failing_since, h.last_status.as_deref()), (1, Some(T0), Some("500")));
    let (_, list) = app.call(Method::GET, "/api/v1/me/webhooks", &shop.token, None).await;
    assert_eq!(list["webhooks"][0]["status"], "failing");

    // Not before the wait is up.
    app.advance(5);
    assert_eq!(app.tick().await, 0);
    assert_eq!(app.sent().len(), 1, "no retry yet");

    // After the wait it tries the same event again (not a later one), then carries on in order.
    app.advance(10);
    assert_eq!(app.tick().await, 2);
    assert_eq!(app.types(), vec!["escrow.accepted", "escrow.accepted", "escrow.locked"]);
    let h = status(&app).await;
    assert_eq!((h.attempts, h.failing_since, h.last_status.as_deref()), (0, None, Some("200")));
    let (_, list) = app.call(Method::GET, "/api/v1/me/webhooks", &shop.token, None).await;
    assert_eq!(list["webhooks"][0]["status"], "ok");
}

#[tokio::test]
async fn an_endpoint_that_cannot_be_reached_is_switched_off_after_days() {
    let app = app(true).await;
    let (shop, buyer) = (app.register().await, app.register().await);
    let (hook, _) = app.hook(&shop, "https://hooks.example.com/toshi").await;
    let id = app.create(&shop, &buyer).await;
    app.accept(&buyer, &id, &secret(1)).await;

    // Every try fails (a network error, not even an answer), up to and past the limit.
    app.net.script.lock().unwrap().extend(std::iter::repeat_n(Err("timeout".to_string()), 200));
    let end = app.now() + DISABLE_AFTER_SECS;
    while app.now() <= end + 4000 {
        app.tick().await;
        app.advance(900);
    }
    let h = app.store.get_webhook(&hook).await.unwrap().unwrap();
    assert!(h.disabled_at.is_some(), "switched off");
    assert_eq!(h.last_status.as_deref(), Some("timeout"));
    let (_, list) = app.call(Method::GET, "/api/v1/me/webhooks", &shop.token, None).await;
    assert_eq!(list["webhooks"][0]["status"], "disabled");

    // Disabled means no more attempts.
    let before = app.sent().len();
    app.advance(10_000);
    app.tick().await;
    assert_eq!(app.sent().len(), before);
}

#[tokio::test]
async fn a_test_event_shows_the_endpoint_works_without_touching_its_progress() {
    let app = app(true).await;
    let shop = app.register().await;
    let (hook, whsec) = app.hook(&shop, "https://hooks.example.com/toshi").await;
    let (s, v) = app.call(Method::POST, &format!("/api/v1/me/webhooks/{hook}/test"), &shop.token, None).await;
    assert_eq!((s, &v["delivered"], &v["status"]), (StatusCode::OK, &json!(true), &json!("200")), "{v}");
    let sent = app.sent();
    assert_eq!(sent[0].json()["type"], "webhook.test");
    let now = app.now();
    assert_eq!(sent[0].header("toshi-signature"), format!("t={now},v1={}", sign(&whsec, now, &sent[0].body)));
    assert_eq!(app.store.get_webhook(&hook).await.unwrap().unwrap().last_attempt_at, None, "progress untouched");

    // A receiver that refuses is reported, not hidden.
    app.net.script.lock().unwrap().push_back(Ok(401));
    let (_, v) = app.call(Method::POST, &format!("/api/v1/me/webhooks/{hook}/test"), &shop.token, None).await;
    assert_eq!((&v["delivered"], &v["status"]), (&json!(false), &json!("401")));
    app.net.script.lock().unwrap().push_back(Err("timeout".into()));
    let (_, v) = app.call(Method::POST, &format!("/api/v1/me/webhooks/{hook}/test"), &shop.token, None).await;
    assert_eq!((&v["delivered"], &v["status"]), (&json!(false), &json!("timeout")));
}
