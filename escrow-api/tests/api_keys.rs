//! API keys: software acting for an account with only the scopes it was given.

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
use escrow_core::{mock::MockLightning, service::EscrowService, store::MemoryStore, timing::TimingConfig};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

const T0: i64 = 1_000_000;
const OPERATOR_TOKEN: &str = "operator-token-for-tests-0123456789";

struct App {
    router: Router,
    clock: Arc<AtomicI64>,
}

struct Account {
    id: String,
    token: String,
}

async fn app() -> App {
    let node = MockLightning::new();
    let store = Arc::new(MemoryStore::new());
    let service = Arc::new(EscrowService::new(node, store.clone(), TimingConfig::default()));
    let clock = Arc::new(AtomicI64::new(T0));
    let now = {
        let clock = clock.clone();
        Arc::new(move || clock.load(Ordering::SeqCst))
    };
    ensure_operator(&store, OPERATOR_TOKEN, T0).await.unwrap();
    let config = ApiConfig { rate_limit_per_min: 100_000, register_limit_per_min: 100_000, ..Default::default() };
    App { router: router(AppState::new(service, store, config, now)), clock }
}

impl App {
    async fn call(&self, method: Method, path: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
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
        let (s, v) = self.call(Method::POST, "/api/v1/users", None, None).await;
        assert_eq!(s, StatusCode::CREATED);
        Account { id: v["id"].as_str().unwrap().into(), token: v["token"].as_str().unwrap().into() }
    }

    /// Make a key with `scopes`; returns (key id, the key itself).
    async fn key(&self, owner: &Account, scopes: &[&str]) -> (String, String) {
        let (s, v) = self
            .call(Method::POST, "/api/v1/me/api-keys", Some(&owner.token), Some(json!({"name": "shop", "scopes": scopes})))
            .await;
        assert_eq!(s, StatusCode::CREATED, "{v}");
        (v["id"].as_str().unwrap().into(), v["key"].as_str().unwrap().into())
    }

    async fn offer(&self, token: &str) -> (StatusCode, Value) {
        self.call(
            Method::POST,
            "/api/v1/escrows",
            Some(token),
            Some(json!({"amount_sats": 1000, "description": "order 1001", "hold_secs": 3600})),
        )
        .await
    }
}

#[tokio::test]
async fn a_key_is_shown_once_and_listed_without_its_secret() {
    let app = app().await;
    let shop = app.register().await;
    let (s, v) = app
        .call(
            Method::POST,
            "/api/v1/me/api-keys",
            Some(&shop.token),
            Some(json!({"name": "  checkout  ", "scopes": ["escrows:read", "escrows:write", "escrows:read"]})),
        )
        .await;
    assert_eq!(s, StatusCode::CREATED, "{v}");
    let key = v["key"].as_str().unwrap();
    assert!(key.starts_with("tk_") && key.len() == 67, "{key}");
    assert_eq!(v["name"], "checkout");
    assert_eq!(v["scopes"], json!(["escrows:read", "escrows:write"]), "duplicates dropped");
    assert_eq!(v["last_chars"], key[key.len() - 4..]);

    let (s, list) = app.call(Method::GET, "/api/v1/me/api-keys", Some(&shop.token), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(list["api_keys"].as_array().unwrap().len(), 1);
    assert!(!list.to_string().contains(key), "the key is not shown again");
}

#[tokio::test]
async fn bad_requests_for_a_key_are_refused() {
    let app = app().await;
    let shop = app.register().await;
    for (body, code) in [
        (json!({"name": "", "scopes": ["escrows:read"]}), "invalid_name"),
        (json!({"name": "x".repeat(61), "scopes": ["escrows:read"]}), "invalid_name"),
        (json!({"name": "a", "scopes": []}), "invalid_scope"),
        (json!({"name": "a", "scopes": ["escrows:delete"]}), "invalid_scope"),
    ] {
        let (s, v) = app.call(Method::POST, "/api/v1/me/api-keys", Some(&shop.token), Some(body)).await;
        assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some(code)), "{v}");
    }
    // An operator is not a party to escrows, so has no use for a key.
    let (s, _) = app
        .call(Method::POST, "/api/v1/me/api-keys", Some(OPERATOR_TOKEN), Some(json!({"name": "a", "scopes": ["escrows:read"]})))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_key_with_the_scopes_can_create_and_read_escrows() {
    let app = app().await;
    let shop = app.register().await;
    let (_, key) = app.key(&shop, &["escrows:read", "escrows:write"]).await;

    let (s, me) = app.call(Method::GET, "/api/v1/me", Some(&key), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(me["id"], shop.id);
    assert_eq!(me["api_key"]["scopes"], json!(["escrows:read", "escrows:write"]));

    let (s, e) = app.offer(&key).await;
    assert_eq!(s, StatusCode::CREATED, "{e}");
    assert_eq!(e["seller_id"], shop.id, "the escrow belongs to the account the key acts for");
    let id = e["id"].as_str().unwrap();

    let (s, list) = app.call(Method::GET, "/api/v1/escrows", Some(&key), None).await;
    assert_eq!((s, list["escrows"].as_array().unwrap().len()), (StatusCode::OK, 1));
    assert_eq!(app.call(Method::GET, &format!("/api/v1/escrows/{id}"), Some(&key), None).await.0, StatusCode::OK);
    assert_eq!(app.call(Method::GET, &format!("/api/v1/escrows/{id}/events"), Some(&key), None).await.0, StatusCode::OK);

    // And it can cancel an offer nobody has accepted.
    assert_eq!(app.call(Method::POST, &format!("/api/v1/escrows/{id}/cancel"), Some(&key), None).await.0, StatusCode::OK);
}

#[tokio::test]
async fn a_key_cannot_go_beyond_its_scopes() {
    let app = app().await;
    let shop = app.register().await;
    let (_, read_only) = app.key(&shop, &["escrows:read"]).await;
    let (_, write_only) = app.key(&shop, &["escrows:write"]).await;

    let (s, v) = app.offer(&read_only).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::FORBIDDEN, Some("forbidden")), "{v}");
    assert_eq!(app.call(Method::GET, "/api/v1/escrows", Some(&write_only), None).await.0, StatusCode::FORBIDDEN);

    let (s, e) = app.offer(&write_only).await;
    assert_eq!(s, StatusCode::CREATED, "{e}");
    let id = e["id"].as_str().unwrap();
    assert_eq!(app.call(Method::POST, &format!("/api/v1/escrows/{id}/cancel"), Some(&read_only), None).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_key_is_closed_out_of_everything_else() {
    let app = app().await;
    let shop = app.register().await;
    let (_, key) = app.key(&shop, &["escrows:read", "escrows:write"]).await;
    let (_, e) = app.offer(&key).await;
    let id = e["id"].as_str().unwrap();

    for (method, path, body) in [
        (Method::GET, "/api/v1/me/api-keys".to_string(), None),
        (Method::POST, "/api/v1/me/api-keys".to_string(), Some(json!({"name": "a", "scopes": ["escrows:read"]}))),
        (Method::GET, "/api/v1/me/node".to_string(), None),
        (Method::DELETE, "/api/v1/me/node".to_string(), None),
        (Method::GET, "/api/v1/me/arbiter-key".to_string(), None),
        (Method::POST, format!("/api/v1/escrows/{id}/accept"), Some(json!({"payment_hash": "00".repeat(32)}))),
        (Method::POST, format!("/api/v1/escrows/{id}/release"), Some(json!({"preimage": "00".repeat(32)}))),
        (Method::POST, format!("/api/v1/escrows/{id}/dispute"), Some(json!({}))),
        (Method::GET, format!("/api/v1/escrows/{id}/invoice"), None),
    ] {
        let (s, v) = app.call(method.clone(), &path, Some(&key), body).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{method} {path} should be closed to keys: {v}");
    }
}

#[tokio::test]
async fn a_key_sees_only_its_own_accounts_escrows() {
    let app = app().await;
    let shop = app.register().await;
    let other = app.register().await;
    let (_, shop_key) = app.key(&shop, &["escrows:read", "escrows:write"]).await;
    let (_, theirs) = app.offer(&other.token).await;
    let their_id = theirs["id"].as_str().unwrap();
    // An open offer (no buyer yet) can be read by anyone who has its id, with a token or a key alike.
    let (s, _) = app.call(Method::GET, &format!("/api/v1/escrows/{their_id}"), Some(&shop_key), None).await;
    assert_eq!(s, StatusCode::OK);
    // Once it has a named buyer it is theirs alone: another account, and so its keys, get "not found".
    let buyer = app.register().await;
    let (_, named) = app
        .call(
            Method::POST,
            "/api/v1/escrows",
            Some(&other.token),
            Some(json!({"buyer_id": buyer.id, "amount_sats": 1000, "description": "private", "hold_secs": 3600})),
        )
        .await;
    let (s, _) = app.call(Method::GET, &format!("/api/v1/escrows/{}", named["id"].as_str().unwrap()), Some(&shop_key), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, list) = app.call(Method::GET, "/api/v1/escrows", Some(&shop_key), None).await;
    assert_eq!((s, list["escrows"].as_array().unwrap().len()), (StatusCode::OK, 0), "the list holds only the shop's own");
}

#[tokio::test]
async fn revoking_stops_a_key_and_only_its_owner_can_revoke() {
    let app = app().await;
    let shop = app.register().await;
    let other = app.register().await;
    let (id, key) = app.key(&shop, &["escrows:read"]).await;
    assert_eq!(app.call(Method::GET, "/api/v1/escrows", Some(&key), None).await.0, StatusCode::OK);

    let path = format!("/api/v1/me/api-keys/{id}");
    assert_eq!(app.call(Method::DELETE, &path, Some(&other.token), None).await.0, StatusCode::NOT_FOUND, "not theirs");
    assert_eq!(app.call(Method::GET, "/api/v1/escrows", Some(&key), None).await.0, StatusCode::OK, "still works");

    assert_eq!(app.call(Method::DELETE, &path, Some(&shop.token), None).await.0, StatusCode::OK);
    assert_eq!(app.call(Method::GET, "/api/v1/escrows", Some(&key), None).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(app.call(Method::DELETE, &path, Some(&shop.token), None).await.0, StatusCode::NOT_FOUND, "already revoked");

    let (_, list) = app.call(Method::GET, "/api/v1/me/api-keys", Some(&shop.token), None).await;
    assert!(list["api_keys"][0]["revoked_at"].is_i64(), "the list still shows it, as revoked");
}

#[tokio::test]
async fn last_used_is_recorded_but_not_on_every_call() {
    let app = app().await;
    let shop = app.register().await;
    let (_, key) = app.key(&shop, &["escrows:read"]).await;
    let last_used = |v: &Value| v["api_keys"][0]["last_used_at"].as_i64();
    let (_, list) = app.call(Method::GET, "/api/v1/me/api-keys", Some(&shop.token), None).await;
    assert_eq!(last_used(&list), None);

    app.call(Method::GET, "/api/v1/escrows", Some(&key), None).await;
    let (_, list) = app.call(Method::GET, "/api/v1/me/api-keys", Some(&shop.token), None).await;
    assert_eq!(last_used(&list), Some(T0));

    app.clock.store(T0 + 10, Ordering::SeqCst);
    app.call(Method::GET, "/api/v1/escrows", Some(&key), None).await;
    let (_, list) = app.call(Method::GET, "/api/v1/me/api-keys", Some(&shop.token), None).await;
    assert_eq!(last_used(&list), Some(T0), "within a minute, not written again");

    app.clock.store(T0 + 120, Ordering::SeqCst);
    app.call(Method::GET, "/api/v1/escrows", Some(&key), None).await;
    let (_, list) = app.call(Method::GET, "/api/v1/me/api-keys", Some(&shop.token), None).await;
    assert_eq!(last_used(&list), Some(T0 + 120));
}

#[tokio::test]
async fn there_is_a_limit_on_working_keys() {
    let app = app().await;
    let shop = app.register().await;
    let mut first = String::new();
    for i in 0..20 {
        let (id, _) = app.key(&shop, &["escrows:read"]).await;
        if i == 0 {
            first = id;
        }
    }
    let (s, v) = app
        .call(Method::POST, "/api/v1/me/api-keys", Some(&shop.token), Some(json!({"name": "one more", "scopes": ["escrows:read"]})))
        .await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("too_many_keys")), "{v}");
    // Revoking one makes room.
    app.call(Method::DELETE, &format!("/api/v1/me/api-keys/{first}"), Some(&shop.token), None).await;
    app.key(&shop, &["escrows:read"]).await;
}
