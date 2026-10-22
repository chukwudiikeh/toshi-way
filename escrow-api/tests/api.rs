//! The HTTP API end to end, in-process, against the mock node and in-memory store.

use std::sync::{
    atomic::{AtomicI64, Ordering},
    Arc,
};

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
    Router,
};
use escrow_api::{ensure_operator, router, ApiConfig, AppState};
use escrow_core::{
    mock::MockLightning,
    model::{NodeInvoiceState, Preimage},
    service::EscrowService,
    store::{MemoryStore, UserStore},
    timing::TimingConfig,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const T0: i64 = 1_000_000;
const OPERATOR_TOKEN: &str = "operator-token-for-tests-0123456789";

type Svc = EscrowService<MockLightning, Arc<MemoryStore>>;

struct App {
    router: Router,
    node: MockLightning,
    service: Arc<Svc>,
    store: Arc<MemoryStore>,
    clock: Arc<AtomicI64>,
}

struct Account {
    id: String,
    token: String,
}

async fn app_with(config: ApiConfig) -> App {
    let node = MockLightning::new();
    let store = Arc::new(MemoryStore::new());
    let service = Arc::new(EscrowService::new(node.clone(), store.clone(), TimingConfig::default()));
    let clock = Arc::new(AtomicI64::new(T0));
    let now = {
        let clock = clock.clone();
        Arc::new(move || clock.load(Ordering::SeqCst))
    };
    ensure_operator(&store, OPERATOR_TOKEN, T0).await.unwrap();
    let state = AppState::new(service.clone(), store.clone(), config, now);
    App { router: router(state), node, service, store, clock }
}

async fn app() -> App {
    app_with(ApiConfig { rate_limit_per_min: 100_000, register_limit_per_min: 100_000, ..Default::default() }).await
}

impl App {
    async fn call(&self, method: Method, path: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
        let (s, _, v) = self.call_full(method, path, token, body, &[]).await;
        (s, v)
    }

    async fn call_full(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
        extra: &[(&str, &str)],
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        for (k, v) in extra {
            req = req.header(*k, *v);
        }
        let req = match body {
            Some(b) => req.header("content-type", "application/json").body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        let res = self.router.clone().oneshot(req).await.unwrap();
        let (status, headers) = (res.status(), res.headers().clone());
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, headers, value)
    }

    async fn register(&self) -> Account {
        let (s, v) = self.call(Method::POST, "/api/v1/users", None, None).await;
        assert_eq!(s, StatusCode::CREATED);
        Account { id: v["id"].as_str().unwrap().into(), token: v["token"].as_str().unwrap().into() }
    }

    /// Seller creates an escrow for `buyer`; returns its id.
    async fn create(&self, seller: &Account, buyer: &Account, hold_secs: i64) -> String {
        let (s, v) = self
            .call(
                Method::POST,
                "/api/v1/escrows",
                Some(&seller.token),
                Some(json!({"buyer_id": buyer.id, "amount_sats": 1000, "description": "parcel", "hold_secs": hold_secs})),
            )
            .await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
        v["id"].as_str().unwrap().into()
    }

    async fn accept(&self, buyer: &Account, id: &str, p: &Preimage) -> (StatusCode, Value) {
        self.call(
            Method::POST,
            &format!("/api/v1/escrows/{id}/accept"),
            Some(&buyer.token),
            Some(json!({"payment_hash": hex::encode(p.hash())})),
        )
        .await
    }

    /// The buyer pays and the node event reaches the service (as the runner would deliver it).
    async fn pay(&self, p: &Preimage) {
        let ev = self.node.pay(&p.hash());
        self.service.apply_event(ev, self.clock.load(Ordering::SeqCst)).await.unwrap();
    }

    async fn state(&self, token: &str, id: &str) -> String {
        let (_, v) = self.call(Method::GET, &format!("/api/v1/escrows/{id}"), Some(token), None).await;
        v["state"].as_str().unwrap().into()
    }

    /// Create, accept and lock an escrow.
    async fn locked(&self, seller: &Account, buyer: &Account, p: &Preimage) -> String {
        let id = self.create(seller, buyer, 3600).await;
        assert_eq!(self.accept(buyer, &id, p).await.0, StatusCode::OK);
        self.pay(p).await;
        id
    }
}

fn secret(n: u8) -> Preimage {
    Preimage::from_bytes([n; 32])
}

fn hex_of(p: &Preimage) -> String {
    hex::encode(p.as_bytes())
}

#[tokio::test]
async fn happy_path_over_http() {
    let app = app().await;
    let (seller, buyer) = (app.register().await, app.register().await);
    let p = secret(1);

    let id = app.create(&seller, &buyer, 3600).await;
    let (s, v) = app.accept(&buyer, &id, &p).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["state"], "INVOICE_ISSUED");
    assert_eq!(v["your_role"], "buyer");

    let (s, inv) = app.call(Method::GET, &format!("/api/v1/escrows/{id}/invoice"), Some(&buyer.token), None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(inv["payment_request"].as_str().unwrap().starts_with("lnbcrt"));

    // The API only reports LOCKED once the node has.
    assert_eq!(app.state(&seller.token, &id).await, "INVOICE_ISSUED");
    app.pay(&p).await;
    assert_eq!(app.state(&seller.token, &id).await, "LOCKED");

    let (s, v) = app
        .call(Method::POST, &format!("/api/v1/escrows/{id}/release"), Some(&seller.token), Some(json!({"preimage": hex_of(&p)})))
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["state"], "SETTLED");
    assert_eq!(app.node.state(&p.hash()), Some(NodeInvoiceState::Settled));

    let (s, ev) = app.call(Method::GET, &format!("/api/v1/escrows/{id}/events"), Some(&buyer.token), None).await;
    assert_eq!(s, StatusCode::OK);
    let states: Vec<_> = ev["audit"].as_array().unwrap().iter().map(|a| a["to_state"].as_str().unwrap()).collect();
    assert_eq!(states, ["INVOICE_ISSUED", "LOCKED", "SETTLED"]);
    assert_eq!(ev["node_events"][0]["node_invoice_state"], "ACCEPTED");
}

#[tokio::test]
async fn deadline_refund_over_http() {
    let app = app().await;
    let (seller, buyer) = (app.register().await, app.register().await);
    let p = secret(1);
    let id = app.locked(&seller, &buyer, &p).await;

    app.clock.store(T0 + 3600, Ordering::SeqCst);
    app.service.tick(T0 + 3600).await.unwrap();
    assert_eq!(app.state(&buyer.token, &id).await, "REFUNDED");

    // Too late to release now.
    let (s, v) = app
        .call(Method::POST, &format!("/api/v1/escrows/{id}/release"), Some(&seller.token), Some(json!({"preimage": hex_of(&p)})))
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(v["error"]["code"], "invalid_state");
}

#[tokio::test]
async fn authentication_is_required_and_checked() {
    let app = app().await;
    let (s, v) = app.call(Method::GET, "/api/v1/escrows/whatever", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert_eq!(v["error"]["code"], "unauthorized");
    assert_eq!(app.call(Method::GET, "/api/v1/escrows/x", Some("tw_forged"), None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(app.call(Method::POST, "/api/v1/escrows", None, Some(json!({}))).await.0, StatusCode::UNAUTHORIZED);
    // Health is public.
    assert_eq!(app.call(Method::GET, "/health", None, None).await.0, StatusCode::OK);
}

#[tokio::test]
async fn strangers_cannot_see_or_touch_an_escrow() {
    let app = app().await;
    let (seller, buyer, stranger) = (app.register().await, app.register().await, app.register().await);
    let p = secret(1);
    let id = app.locked(&seller, &buyer, &p).await;

    let t = Some(stranger.token.as_str());
    for (m, path, body) in [
        (Method::GET, format!("/api/v1/escrows/{id}"), None),
        (Method::GET, format!("/api/v1/escrows/{id}/events"), None),
        (Method::GET, format!("/api/v1/escrows/{id}/invoice"), None),
        (Method::POST, format!("/api/v1/escrows/{id}/release"), Some(json!({"preimage": hex_of(&p)}))),
        (Method::POST, format!("/api/v1/escrows/{id}/cancel"), None),
        (Method::POST, format!("/api/v1/escrows/{id}/dispute"), Some(json!({"reason": "x"}))),
        (Method::POST, format!("/api/v1/escrows/{id}/accept"), Some(json!({"payment_hash": hex_of(&p)}))),
    ] {
        let (s, v) = app.call(m.clone(), &path, t, body).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{m} {path}");
        assert_eq!(v["error"]["code"], "not_found");
    }
    // Nothing changed.
    assert_eq!(app.state(&seller.token, &id).await, "LOCKED");
    assert_eq!(app.node.settle_calls(), 0);
    assert_eq!(app.node.cancel_calls(), 0);
}

#[tokio::test]
async fn parties_are_limited_to_their_own_actions() {
    let app = app().await;
    let (seller, buyer) = (app.register().await, app.register().await);
    let p = secret(1);
    let id = app.create(&seller, &buyer, 3600).await;

    // The seller cannot accept on the buyer's behalf, or read the invoice.
    let (s, _) = app
        .call(Method::POST, &format!("/api/v1/escrows/{id}/accept"), Some(&seller.token), Some(json!({"payment_hash": hex::encode(p.hash())})))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    app.accept(&buyer, &id, &p).await;
    let (s, _) = app.call(Method::GET, &format!("/api/v1/escrows/{id}/invoice"), Some(&seller.token), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Once locked the invoice is no longer payable, and the buyer cannot cancel alone.
    app.pay(&p).await;
    let (s, v) = app.call(Method::GET, &format!("/api/v1/escrows/{id}/invoice"), Some(&buyer.token), None).await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(v["error"]["code"], "invalid_state");
    let (s, _) = app.call(Method::POST, &format!("/api/v1/escrows/{id}/cancel"), Some(&buyer.token), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(app.state(&buyer.token, &id).await, "LOCKED");

    // The seller can give the money back.
    let (s, v) = app.call(Method::POST, &format!("/api/v1/escrows/{id}/cancel"), Some(&seller.token), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["state"], "REFUNDED");
}

#[tokio::test]
async fn buyer_can_cancel_before_paying() {
    let app = app().await;
    let (seller, buyer) = (app.register().await, app.register().await);
    let id = app.create(&seller, &buyer, 3600).await;
    app.accept(&buyer, &id, &secret(1)).await;
    let (s, v) = app.call(Method::POST, &format!("/api/v1/escrows/{id}/cancel"), Some(&buyer.token), None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["state"], "EXPIRED_UNPAID");
}

#[tokio::test]
async fn operator_resolves_disputes_and_sees_the_queue() {
    let app = app().await;
    let (seller, buyer) = (app.register().await, app.register().await);
    let p = secret(1);
    let id = app.locked(&seller, &buyer, &p).await;

    let (s, v) = app
        .call(Method::POST, &format!("/api/v1/escrows/{id}/dispute"), Some(&buyer.token), Some(json!({"reason": "not delivered"})))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["state"], "DISPUTED");

    // Only the operator sees the queue.
    assert_eq!(app.call(Method::GET, "/api/v1/operator/escrows", Some(&buyer.token), None).await.0, StatusCode::FORBIDDEN);
    let (s, q) = app.call(Method::GET, "/api/v1/operator/escrows", Some(OPERATOR_TOKEN), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(q["escrows"][0]["id"], id.as_str());
    assert_eq!(q["escrows"][0]["state"], "DISPUTED");

    // The operator can read it, cancel it, but cannot release or open disputes.
    let op = Some(OPERATOR_TOKEN);
    assert_eq!(app.call(Method::GET, &format!("/api/v1/escrows/{id}"), op, None).await.0, StatusCode::OK);
    let (s, _) = app
        .call(Method::POST, &format!("/api/v1/escrows/{id}/release"), op, Some(json!({"preimage": hex_of(&p)})))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, v) = app.call(Method::POST, &format!("/api/v1/escrows/{id}/cancel"), op, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(v["state"], "REFUNDED");
    assert_eq!(v["your_role"], "operator");
}

#[tokio::test]
async fn wrong_and_short_preimages_are_rejected_without_state_change() {
    let app = app().await;
    let (seller, buyer) = (app.register().await, app.register().await);
    let p = secret(1);
    let id = app.locked(&seller, &buyer, &p).await;
    let path = format!("/api/v1/escrows/{id}/release");

    let (s, v) = app.call(Method::POST, &path, Some(&seller.token), Some(json!({"preimage": hex_of(&secret(2))}))).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["error"]["code"], "wrong_preimage");

    for bad in ["123456", "", &"ab".repeat(31), &"zz".repeat(32)] {
        let (s, v) = app.call(Method::POST, &path, Some(&seller.token), Some(json!({"preimage": bad}))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
        assert_eq!(v["error"]["code"], "invalid_preimage");
    }
    assert_eq!(app.state(&seller.token, &id).await, "LOCKED");
    assert_eq!(app.node.settle_calls(), 0);
}

#[tokio::test]
async fn duplicate_release_is_harmless() {
    let app = app().await;
    let (seller, buyer) = (app.register().await, app.register().await);
    let p = secret(1);
    let id = app.locked(&seller, &buyer, &p).await;
    let path = format!("/api/v1/escrows/{id}/release");
    let body = json!({"preimage": hex_of(&p)});

    let first = app.call(Method::POST, &path, Some(&seller.token), Some(body.clone())).await;
    let second = app.call(Method::POST, &path, Some(&buyer.token), Some(body)).await;
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(second.0, StatusCode::OK);
    assert_eq!(second.1["state"], "SETTLED");
    assert_eq!(app.node.settle_calls(), 1);
}

#[tokio::test]
async fn idempotency_key_prevents_duplicate_escrows() {
    let app = app().await;
    let (seller, buyer) = (app.register().await, app.register().await);
    let body = json!({"buyer_id": buyer.id, "amount_sats": 1000, "description": "parcel", "hold_secs": 3600});
    let key = [("idempotency-key", "create-abc")];

    let (s1, _, v1) = app.call_full(Method::POST, "/api/v1/escrows", Some(&seller.token), Some(body.clone()), &key).await;
    let (s2, _, v2) = app.call_full(Method::POST, "/api/v1/escrows", Some(&seller.token), Some(body.clone()), &key).await;
    assert_eq!((s1, s2), (StatusCode::CREATED, StatusCode::CREATED));
    assert_eq!(v1["id"], v2["id"]);
    let (_, q) = app.call(Method::GET, "/api/v1/operator/escrows", Some(OPERATOR_TOKEN), None).await;
    assert_eq!(q["escrows"].as_array().unwrap().len(), 1);

    // Same key, different request.
    let mut other = body.clone();
    other["amount_sats"] = json!(2000);
    let (s, _, v) = app.call_full(Method::POST, "/api/v1/escrows", Some(&seller.token), Some(other), &key).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(v["error"]["code"], "idempotency_key_reuse");

    // Keys are per user.
    let other_seller = app.register().await;
    let (s, _, v3) = app.call_full(Method::POST, "/api/v1/escrows", Some(&other_seller.token), Some(body), &key).await;
    assert_eq!(s, StatusCode::CREATED);
    assert_ne!(v3["id"], v1["id"]);
}

#[tokio::test]
async fn create_validates_its_input() {
    let app = app().await;
    let (seller, buyer) = (app.register().await, app.register().await);
    let post = |body: Value| app.call(Method::POST, "/api/v1/escrows", Some(&seller.token), Some(body));
    let ok = json!({"buyer_id": buyer.id, "amount_sats": 1000, "description": "x", "hold_secs": 3600});
    let with = |k: &str, v: Value| {
        let mut b = ok.clone();
        b[k] = v;
        b
    };

    assert_eq!(post(with("amount_sats", json!(0))).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(post(with("amount_sats", json!(10_000_000))).await.0, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(post(with("hold_secs", json!(7 * 3600))).await.1["error"]["code"], "invalid_deadline");
    assert_eq!(post(with("hold_secs", json!(5))).await.1["error"]["code"], "invalid_deadline");
    assert_eq!(post(with("hold_secs", json!(-100))).await.1["error"]["code"], "invalid_deadline");
    assert_eq!(post(with("description", json!("x".repeat(501)))).await.1["error"]["code"], "invalid_description");
    assert_eq!(post(with("buyer_id", json!("u_nobody"))).await.1["error"]["code"], "unknown_buyer");
    assert_eq!(post(with("buyer_id", json!(seller.id))).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(post(json!({"amount_sats": "lots"})).await.1["error"]["code"], "invalid_body");

    let (s, _) = app.call(Method::POST, "/api/v1/escrows", Some(OPERATOR_TOKEN), Some(ok)).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "operators cannot be parties");

    // Bad hashes on accept.
    let id = app.create(&seller, &buyer, 3600).await;
    let (s, v) = app
        .call(Method::POST, &format!("/api/v1/escrows/{id}/accept"), Some(&buyer.token), Some(json!({"payment_hash": "1234"})))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(v["error"]["code"], "invalid_hex");
}

#[tokio::test]
async fn escrow_views_never_expose_the_invoice_or_secrets() {
    let app = app().await;
    let (seller, buyer) = (app.register().await, app.register().await);
    let p = secret(7);
    let id = app.locked(&seller, &buyer, &p).await;

    for token in [&seller.token, &buyer.token, &OPERATOR_TOKEN.to_string()] {
        let (_, v) = app.call(Method::GET, &format!("/api/v1/escrows/{id}"), Some(token), None).await;
        assert!(v.get("invoice").is_none());
        assert!(!v.to_string().contains("lnbcrt"));
    }
    let (_, rel) = app
        .call(Method::POST, &format!("/api/v1/escrows/{id}/release"), Some(&seller.token), Some(json!({"preimage": hex_of(&p)})))
        .await;
    assert!(!rel.to_string().contains(&hex_of(&p)), "the preimage must never be echoed back");
}

#[tokio::test]
async fn tokens_are_stored_only_as_hashes() {
    let app = app().await;
    let a = app.register().await;
    assert!(a.token.starts_with("tw_") && a.token.len() == 67);
    assert_eq!(app.store.user_by_token_hash(&a.token).await.unwrap(), None, "raw token must not be a lookup key");
    assert!(app.store.get_user(&a.id).await.unwrap().is_some());
}

#[tokio::test]
async fn health_reports_node_connectivity() {
    let app = app().await;
    let (s, v) = app.call(Method::GET, "/health", None, None).await;
    assert_eq!((s, v["node"].as_str()), (StatusCode::OK, Some("up")));
    assert_eq!(v["server_time"], T0);

    app.node.set_offline(true);
    let (s, v) = app.call(Method::GET, "/health", None, None).await;
    assert_eq!((s, v["node"].as_str()), (StatusCode::SERVICE_UNAVAILABLE, Some("down")));
    assert_eq!(v["server_time"], T0, "clock is reported even when the node is down");
}

#[tokio::test]
async fn node_outage_surfaces_as_503_and_leaves_state_intact() {
    let app = app().await;
    let (seller, buyer) = (app.register().await, app.register().await);
    let p = secret(1);
    let id = app.locked(&seller, &buyer, &p).await;

    app.node.set_offline(true);
    let (s, v) = app
        .call(Method::POST, &format!("/api/v1/escrows/{id}/release"), Some(&seller.token), Some(json!({"preimage": hex_of(&p)})))
        .await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(v["error"]["code"], "node_unavailable");
    assert_eq!(app.state(&seller.token, &id).await, "LOCKED");

    app.node.set_offline(false);
    let (s, _) = app
        .call(Method::POST, &format!("/api/v1/escrows/{id}/release"), Some(&seller.token), Some(json!({"preimage": hex_of(&p)})))
        .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn rate_limits_apply_per_caller_and_to_registration() {
    let app = app_with(ApiConfig { rate_limit_per_min: 3, register_limit_per_min: 2, ..Default::default() }).await;

    for _ in 0..3 {
        assert_eq!(app.call(Method::GET, "/health", None, None).await.0, StatusCode::OK);
    }
    let (s, headers, v) = app.call_full(Method::GET, "/health", None, None, &[]).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(v["error"]["code"], "rate_limited");
    assert!(headers.contains_key("retry-after"));

    // Registration has its own, tighter budget.
    assert_eq!(app.call(Method::POST, "/api/v1/users", None, None).await.0, StatusCode::CREATED);
    assert_eq!(app.call(Method::POST, "/api/v1/users", None, None).await.0, StatusCode::CREATED);
    assert_eq!(app.call(Method::POST, "/api/v1/users", None, None).await.0, StatusCode::TOO_MANY_REQUESTS);

    // The next minute starts fresh.
    app.clock.store(T0 + 60, Ordering::SeqCst);
    assert_eq!(app.call(Method::GET, "/health", None, None).await.0, StatusCode::OK);
}

#[tokio::test]
async fn every_response_carries_a_request_id_and_bodies_are_size_limited() {
    let app = app().await;
    let (_, headers, _) = app.call_full(Method::GET, "/health", None, None, &[]).await;
    assert!(headers.contains_key("x-request-id"));
    let (_, headers, _) = app.call_full(Method::GET, "/health", None, None, &[("x-request-id", "trace-me")]).await;
    assert_eq!(headers["x-request-id"], "trace-me");

    let seller = app.register().await;
    let huge = json!({"buyer_id": "x", "amount_sats": 1, "description": "x".repeat(40_000), "hold_secs": 3600});
    let (s, _) = app.call(Method::POST, "/api/v1/escrows", Some(&seller.token), Some(huge)).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn cors_allows_only_configured_origins_and_skips_auth_for_preflight() {
    let app = app_with(ApiConfig {
        rate_limit_per_min: 100_000,
        register_limit_per_min: 100_000,
        cors_origins: vec!["http://localhost:3001".into()],
        ..Default::default()
    })
    .await;
    let preflight = |origin: &str| {
        Request::builder()
            .method(Method::OPTIONS)
            .uri("/api/v1/escrows")
            .header("origin", origin)
            .header("access-control-request-method", "POST")
            .header("access-control-request-headers", "authorization,content-type,idempotency-key")
            .body(Body::empty())
            .unwrap()
    };

    let ok = app.router.clone().oneshot(preflight("http://localhost:3001")).await.unwrap();
    assert!(ok.status().is_success(), "preflight needs no token: {}", ok.status());
    assert_eq!(ok.headers()["access-control-allow-origin"], "http://localhost:3001");
    let allowed = ok.headers()["access-control-allow-headers"].to_str().unwrap().to_lowercase();
    assert!(allowed.contains("authorization") && allowed.contains("idempotency-key"));

    let bad = app.router.clone().oneshot(preflight("http://evil.example")).await.unwrap();
    assert!(bad.headers().get("access-control-allow-origin").is_none());

    // Real responses carry the header too, and expose the request id to the page.
    let req = Request::builder()
        .uri("/health")
        .header("origin", "http://localhost:3001")
        .body(Body::empty())
        .unwrap();
    let res = app.router.clone().oneshot(req).await.unwrap();
    assert_eq!(res.headers()["access-control-allow-origin"], "http://localhost:3001");
    assert!(res.headers()["access-control-expose-headers"].to_str().unwrap().contains("x-request-id"));
}

#[tokio::test]
async fn users_can_list_their_own_escrows_and_only_those() {
    let app = app().await;
    let (seller, buyer, other) = (app.register().await, app.register().await, app.register().await);
    let one = app.create(&seller, &buyer, 3600).await;
    app.clock.store(T0 + 5, Ordering::SeqCst);
    let two = app.create(&seller, &buyer, 3600).await;
    let _unrelated = app.create(&other, &seller, 3600).await;

    let (s, v) = app.call(Method::GET, "/api/v1/escrows", Some(&buyer.token), None).await;
    assert_eq!(s, StatusCode::OK);
    let ids: Vec<_> = v["escrows"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [two.as_str(), one.as_str()], "newest first");
    assert!(v["escrows"].as_array().unwrap().iter().all(|e| e["your_role"] == "buyer"));

    let (_, v) = app.call(Method::GET, "/api/v1/escrows", Some(&seller.token), None).await;
    let roles: Vec<_> = v["escrows"].as_array().unwrap().iter().map(|e| e["your_role"].as_str().unwrap()).collect();
    assert_eq!(roles.len(), 3);
    assert_eq!(roles.iter().filter(|r| **r == "seller").count(), 2);

    let (_, v) = app.call(Method::GET, "/api/v1/escrows", Some(OPERATOR_TOKEN), None).await;
    assert!(v["escrows"].as_array().unwrap().is_empty(), "operators use the operator queue");
    assert_eq!(app.call(Method::GET, "/api/v1/escrows", None, None).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_view_exposes_invoice_expiry_for_countdowns() {
    let app = app().await;
    let (seller, buyer) = (app.register().await, app.register().await);
    let id = app.create(&seller, &buyer, 3600).await;
    app.accept(&buyer, &id, &secret(1)).await;
    let (_, v) = app.call(Method::GET, &format!("/api/v1/escrows/{id}"), Some(&buyer.token), None).await;
    assert_eq!(v["invoice_expires_at"], T0 + 600);
}
