//! Sellers bringing their own Lightning node, over the HTTP API (mock nodes, in-memory store).

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
    mock::{MockDirectory, MockLightning},
    model::Preimage,
    service::EscrowService,
    store::MemoryStore,
    timing::TimingConfig,
};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const T0: i64 = 1_000_000;
const OPERATOR_TOKEN: &str = "operator-token-for-tests-0123456789";

struct App {
    router: Router,
    dir: MockDirectory,
    service: Arc<EscrowService<MockDirectory, Arc<MemoryStore>>>,
    clock: Arc<AtomicI64>,
}

struct Account {
    id: String,
    token: String,
}

async fn app(shared: Option<MockLightning>) -> App {
    let dir = MockDirectory::new(shared);
    let store = Arc::new(MemoryStore::new());
    let service = Arc::new(EscrowService::new(dir.clone(), store.clone(), TimingConfig::default()));
    let clock = Arc::new(AtomicI64::new(T0));
    let now = {
        let clock = clock.clone();
        Arc::new(move || clock.load(Ordering::SeqCst))
    };
    ensure_operator(&store, OPERATOR_TOKEN, T0).await.unwrap();
    let config = ApiConfig { rate_limit_per_min: 100_000, register_limit_per_min: 100_000, ..Default::default() };
    let state = AppState::new(service.clone(), store.clone(), config, now);
    App { router: router(state), dir, service, clock }
}

impl App {
    async fn call(&self, method: Method, path: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
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

    async fn connect(&self, who: &Account, address: &str) -> (StatusCode, Value) {
        self.call(
            Method::PUT,
            "/api/v1/me/node",
            Some(&who.token),
            Some(json!({
                "address": address,
                "tls_cert": "-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----",
                "macaroon": "0201036c6e6402f801030a10deadbeef",
            })),
        )
        .await
    }

    async fn create(&self, seller: &Account) -> (StatusCode, Value) {
        self.call(
            Method::POST,
            "/api/v1/escrows",
            Some(&seller.token),
            Some(json!({"amount_sats": 1000, "description": "parcel", "hold_secs": 3600})),
        )
        .await
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
}

fn secret(n: u8) -> Preimage {
    Preimage::from_bytes([n; 32])
}

#[tokio::test]
async fn without_a_node_and_without_a_shared_one_a_seller_cannot_create_escrows() {
    let app = app(None).await;
    let seller = app.register().await;
    let (s, v) = app.call(Method::GET, "/api/v1/me/node", Some(&seller.token), None).await;
    assert_eq!((s, v["mode"].as_str(), v["shared_available"].as_bool()), (StatusCode::OK, Some("none"), Some(false)));

    let (s, v) = app.create(&seller).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("no_node")), "{v}");
}

#[tokio::test]
async fn connecting_a_node_puts_the_sellers_escrows_on_it() {
    let app = app(None).await;
    let (seller, buyer) = (app.register().await, app.register().await);

    let (s, v) = app.connect(&seller, "https://seller.example.com:10009").await;
    assert_eq!((s, v["mode"].as_str(), v["address"].as_str()), (StatusCode::OK, Some("own"), Some("https://seller.example.com:10009")), "{v}");

    let (s, e) = app.create(&seller).await;
    assert_eq!((s, e["seller_node"].as_str()), (StatusCode::CREATED, Some("own")), "{e}");
    let id = e["id"].as_str().unwrap();

    // The hold invoice is made on the seller's own node.
    let p = secret(1);
    assert_eq!(app.accept(&buyer, id, &p).await.0, StatusCode::OK);
    let node = app.dir.seller_node(&seller.id).expect("the seller's node");
    assert!(node.state(&p.hash()).is_some(), "invoice exists on the seller's node");
}

#[tokio::test]
async fn the_response_never_contains_the_macaroon_or_certificate() {
    let app = app(None).await;
    let seller = app.register().await;
    let (_, put) = app.connect(&seller, "https://seller.example.com:10009").await;
    let (_, get) = app.call(Method::GET, "/api/v1/me/node", Some(&seller.token), None).await;
    for body in [put, get] {
        let text = body.to_string();
        assert!(!text.contains("deadbeef") && !text.contains("macaroon") && !text.contains("CERTIFICATE"), "{text}");
    }
}

#[tokio::test]
async fn bad_or_failing_connections_are_refused_and_nothing_is_saved() {
    let app = app(None).await;
    let seller = app.register().await;

    let (s, v) = app.connect(&seller, "http://seller.example.com:10009").await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_node")), "{v}");

    let (s, v) = app
        .call(
            Method::PUT,
            "/api/v1/me/node",
            Some(&seller.token),
            Some(json!({"address": "https://a.example.com:1", "tls_cert": "x", "macaroon": "not hex"})),
        )
        .await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_node")), "{v}");

    let (s, v) = app.connect(&seller, "https://unreachable.example.com:10009").await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::SERVICE_UNAVAILABLE, Some("node_unavailable")), "{v}");

    let (s, v) = app.connect(&seller, "https://badperm.example.com:10009").await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("node_check_failed")), "{v}");

    let (_, v) = app.call(Method::GET, "/api/v1/me/node", Some(&seller.token), None).await;
    assert_eq!(v["mode"], "none", "no failed attempt left a connection behind");
}

#[tokio::test]
async fn a_node_cannot_be_changed_while_an_escrow_is_in_flight_on_it() {
    let app = app(None).await;
    let (seller, buyer) = (app.register().await, app.register().await);
    app.connect(&seller, "https://one.example.com:10009").await;
    let (_, e) = app.create(&seller).await;
    let id = e["id"].as_str().unwrap().to_string();

    // An offer nobody has accepted has no invoice yet, so it does not block a change.
    let (s, _) = app.connect(&seller, "https://two.example.com:10009").await;
    assert_eq!(s, StatusCode::OK);

    // Once the invoice exists on the node, the node is locked in.
    let p = secret(1);
    app.accept(&buyer, &id, &p).await;
    let (s, v) = app.connect(&seller, "https://three.example.com:10009").await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("node_in_use")));
    let (s, v) = app.call(Method::DELETE, "/api/v1/me/node", Some(&seller.token), None).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("node_in_use")));
    let (_, v) = app.call(Method::GET, "/api/v1/me/node", Some(&seller.token), None).await;
    assert_eq!((v["mode"].as_str(), v["can_change"].as_bool()), (Some("own"), Some(false)));

    // When the escrow ends, the seller is free to change or remove it.
    app.call(Method::POST, &format!("/api/v1/escrows/{id}/cancel"), Some(&seller.token), None).await;
    let (s, v) = app.call(Method::DELETE, "/api/v1/me/node", Some(&seller.token), None).await;
    assert_eq!((s, v["mode"].as_str()), (StatusCode::OK, Some("none")), "{v}");
}

#[tokio::test]
async fn sellers_with_a_node_use_it_and_others_fall_back_to_the_shared_node() {
    let shared = MockLightning::new();
    let app = app(Some(shared.clone())).await;
    let (alice, bob, buyer) = (app.register().await, app.register().await, app.register().await);

    let (_, v) = app.call(Method::GET, "/api/v1/me/node", Some(&bob.token), None).await;
    assert_eq!((v["mode"].as_str(), v["shared_available"].as_bool()), (Some("shared"), Some(true)));

    app.connect(&alice, "https://alice.example.com:10009").await;
    let (_, ea) = app.create(&alice).await;
    let (_, eb) = app.create(&bob).await;
    assert_eq!((ea["seller_node"].as_str(), eb["seller_node"].as_str()), (Some("own"), Some("shared")));

    let (pa, pb) = (secret(1), secret(2));
    app.accept(&buyer, ea["id"].as_str().unwrap(), &pa).await;
    app.accept(&buyer, eb["id"].as_str().unwrap(), &pb).await;
    let alice_node = app.dir.seller_node(&alice.id).unwrap();
    assert!(alice_node.state(&pa.hash()).is_some() && shared.state(&pa.hash()).is_none());
    assert!(shared.state(&pb.hash()).is_some() && alice_node.state(&pb.hash()).is_none());
}

#[tokio::test]
async fn an_escrow_stays_on_the_node_it_was_made_for_even_if_the_seller_connects_one_later() {
    let shared = MockLightning::new();
    let app = app(Some(shared.clone())).await;
    let (seller, buyer) = (app.register().await, app.register().await);

    let (_, before) = app.create(&seller).await; // no node of their own yet: shared
    let before_id = before["id"].as_str().unwrap().to_string();
    let p = secret(1);
    app.accept(&buyer, &before_id, &p).await;

    // Connecting a node now is allowed (nothing is in flight on an *own* node)...
    let (s, _) = app.connect(&seller, "https://seller.example.com:10009").await;
    assert_eq!(s, StatusCode::OK);
    // ...and the escrow already issued stays on the shared node, where its invoice is.
    let ev = shared.pay(&p.hash());
    app.service.apply_event(ev, T0).await.unwrap();
    app.clock.store(T0 + 5, Ordering::SeqCst);
    let (s, v) = app
        .call(
            Method::POST,
            &format!("/api/v1/escrows/{before_id}/release"),
            Some(&seller.token),
            Some(json!({"preimage": hex::encode(p.as_bytes())})),
        )
        .await;
    assert_eq!((s, v["state"].as_str(), v["seller_node"].as_str()), (StatusCode::OK, Some("SETTLED"), Some("shared")), "{v}");
    assert_eq!(shared.settle_calls(), 1);

    // New escrows go to the seller's own node.
    let (_, after) = app.create(&seller).await;
    assert_eq!(after["seller_node"], "own");
}

#[tokio::test]
async fn operators_and_strangers_cannot_use_the_node_endpoints() {
    let app = app(None).await;
    let seller = app.register().await;
    let (s, _) = app.call(Method::GET, "/api/v1/me/node", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = app.call(Method::GET, "/api/v1/me/node", Some(OPERATOR_TOKEN), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = app.connect(&Account { id: "op".into(), token: OPERATOR_TOKEN.into() }, "https://x.example.com:1").await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // One seller's node is not visible to or affected by another.
    app.connect(&seller, "https://seller.example.com:10009").await;
    let other = app.register().await;
    let (_, v) = app.call(Method::GET, "/api/v1/me/node", Some(&other.token), None).await;
    assert_eq!(v["mode"], "none");
}
