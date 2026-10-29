//! A business's order number and link back, and the checkout link the API hands out.

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
}

async fn app(web_base_url: Option<&str>) -> App {
    let store = Arc::new(MemoryStore::new());
    let service = Arc::new(EscrowService::new(MockLightning::new(), store.clone(), TimingConfig::default()));
    let clock = Arc::new(AtomicI64::new(T0));
    let now = Arc::new(move || clock.load(Ordering::SeqCst));
    ensure_operator(&store, OPERATOR_TOKEN, T0).await.unwrap();
    let config = ApiConfig {
        rate_limit_per_min: 100_000,
        register_limit_per_min: 100_000,
        web_base_url: web_base_url.map(str::to_string),
        ..Default::default()
    };
    App { router: router(AppState::new(service, store, config, now)) }
}

impl App {
    async fn call(&self, method: Method, path: &str, token: &str, body: Option<Value>, idem: Option<&str>) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(path).header("authorization", format!("Bearer {token}"));
        if let Some(k) = idem {
            req = req.header("idempotency-key", k);
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

    async fn register(&self) -> (String, String) {
        let req = Request::builder().method(Method::POST).uri("/api/v1/users").body(Body::empty()).unwrap();
        let res = self.router.clone().oneshot(req).await.unwrap();
        let v: Value = serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
        (v["id"].as_str().unwrap().into(), v["token"].as_str().unwrap().into())
    }

    async fn offer(&self, token: &str, extra: Value, idem: Option<&str>) -> (StatusCode, Value) {
        let mut body = json!({"amount_sats": 5000, "description": "parcel to Ikeja", "hold_secs": 3600});
        for (k, v) in extra.as_object().unwrap() {
            body[k] = v.clone();
        }
        self.call(Method::POST, "/api/v1/escrows", token, Some(body), idem).await
    }
}

#[tokio::test]
async fn an_order_number_and_return_link_are_kept_and_shown_to_both_sides() {
    let app = app(None).await;
    let (_, shop) = app.register().await;
    let (buyer_id, buyer) = app.register().await;
    let (s, e) = app
        .offer(&shop, json!({"buyer_id": buyer_id, "reference": "  order #1001 ", "return_url": "https://shop.example.com/thanks?o=1001"}), None)
        .await;
    assert_eq!(s, StatusCode::CREATED, "{e}");
    assert_eq!(e["reference"], "order #1001");
    assert_eq!(e["return_url"], "https://shop.example.com/thanks?o=1001");

    let id = e["id"].as_str().unwrap();
    let (s, seen) = app.call(Method::GET, &format!("/api/v1/escrows/{id}"), &buyer, None, None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!((&seen["reference"], &seen["return_url"]), (&e["reference"], &e["return_url"]), "the buyer sees them too");
    let (_, list) = app.call(Method::GET, "/api/v1/escrows", &shop, None, None).await;
    assert_eq!(list["escrows"][0]["reference"], "order #1001");

    // Neither is required, and empty means none.
    let (_, plain) = app.offer(&shop, json!({"reference": "", "return_url": "  "}), None).await;
    assert_eq!((&plain["reference"], &plain["return_url"]), (&Value::Null, &Value::Null));
}

#[tokio::test]
async fn a_return_link_that_could_mislead_the_buyer_is_refused() {
    let app = app(None).await;
    let (_, shop) = app.register().await;
    for bad in [
        "http://shop.example.com/thanks",
        "javascript:alert(1)",
        "https://trusted.com@evil.example/",
        "https://shop.example.com\\@evil.example",
        "not a link",
    ] {
        let (s, v) = app.offer(&shop, json!({"return_url": bad}), None).await;
        assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_return_url")), "{bad}: {v}");
    }
    let (s, v) = app.offer(&shop, json!({"reference": "x".repeat(101)}), None).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_reference")), "{v}");
}

#[tokio::test]
async fn a_repeated_create_with_the_same_key_must_have_the_same_order_number() {
    let app = app(None).await;
    let (_, shop) = app.register().await;
    let (s, first) = app.offer(&shop, json!({"reference": "order 1"}), Some("k1")).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, again) = app.offer(&shop, json!({"reference": "order 1"}), Some("k1")).await;
    assert_eq!((s, &again["id"]), (StatusCode::CREATED, &first["id"]), "a retry returns the first escrow");
    let (s, v) = app.offer(&shop, json!({"reference": "order 2"}), Some("k1")).await;
    assert_eq!((s, v["error"]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("idempotency_key_reuse")));
}

#[tokio::test]
async fn the_checkout_link_appears_only_when_the_web_address_is_configured() {
    let off = app(None).await;
    let (_, shop) = off.register().await;
    let (_, e) = off.offer(&shop, json!({}), None).await;
    assert!(e.get("checkout_url").is_none(), "no address configured, no link: {e}");

    let on = app(Some("https://pay.example.com")).await;
    let (_, shop) = on.register().await;
    let (_, e) = on.offer(&shop, json!({"reference": "o1"}), None).await;
    let id = e["id"].as_str().unwrap();
    let link = format!("https://pay.example.com/escrows/{id}");
    assert_eq!(e["checkout_url"], link.as_str());
    let (_, got) = on.call(Method::GET, &format!("/api/v1/escrows/{id}"), &shop, None, None).await;
    assert_eq!(got["checkout_url"], link.as_str());
    let (_, list) = on.call(Method::GET, "/api/v1/escrows", &shop, None, None).await;
    assert_eq!(list["escrows"][0]["checkout_url"], link.as_str());
}
