//! The real sender against a real local receiver: what goes over the wire, and the safety rules.

use std::sync::{Arc, Mutex};

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
    Router,
};
use escrow_api::webhooks::{delivery_headers, sign, HttpSender, Sender};
use serde_json::json;

#[derive(Default, Clone)]
struct Seen {
    hits: Arc<Mutex<Vec<(String, HeaderMap, String)>>>,
}

async fn hook(State(s): State<Seen>, headers: HeaderMap, body: String) -> impl IntoResponse {
    s.hits.lock().unwrap().push(("/hook".into(), headers, body));
    StatusCode::NO_CONTENT
}

async fn redirect(State(s): State<Seen>, headers: HeaderMap, body: String) -> impl IntoResponse {
    s.hits.lock().unwrap().push(("/redirect".into(), headers, body));
    (StatusCode::FOUND, [(header::LOCATION, "/elsewhere")])
}

async fn elsewhere(State(s): State<Seen>, headers: HeaderMap, body: String) -> impl IntoResponse {
    s.hits.lock().unwrap().push(("/elsewhere".into(), headers, body));
    StatusCode::OK
}

async fn receiver() -> (String, Seen) {
    let seen = Seen::default();
    let app = Router::new()
        .route("/hook", post(hook))
        .route("/redirect", post(redirect))
        .route("/elsewhere", post(elsewhere))
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://127.0.0.1:{}", addr.port()), seen)
}

#[tokio::test]
async fn a_delivery_arrives_with_its_signature_and_can_be_verified_by_the_receiver() {
    let (base, seen) = receiver().await;
    let event = json!({"id": "evt_9", "type": "escrow.locked", "created_at": 5});
    let body = event.to_string();
    let headers = delivery_headers("whsec_demo", 1_700_000_000, &event, &body);

    let status = HttpSender { allow_private: true }.send(&format!("{base}/hook"), headers, body.clone()).await;
    assert_eq!(status, Ok(204));

    let hits = seen.hits.lock().unwrap();
    let (path, h, got) = &hits[0];
    assert_eq!((path.as_str(), got), ("/hook", &body));
    assert_eq!(h["content-type"], "application/json");
    assert_eq!(h["toshi-event-id"], "evt_9");
    assert_eq!(h["toshi-event-type"], "escrow.locked");
    // The receiver's check, written the way a business would: HMAC over "<t>.<body>".
    let sig = h["toshi-signature"].to_str().unwrap();
    let (t, v1) = sig.strip_prefix("t=").unwrap().split_once(",v1=").unwrap();
    assert_eq!(v1, sign("whsec_demo", t.parse().unwrap(), got));
}

#[tokio::test]
async fn private_and_local_addresses_are_refused_unless_the_operator_allows_them() {
    let (base, seen) = receiver().await;
    let strict = HttpSender { allow_private: false };
    for url in [format!("{base}/hook"), "https://127.0.0.1/hook".to_string(), "https://localhost/hook".to_string(), "https://169.254.169.254/latest".to_string(), "https://10.0.0.5/x".to_string(), "https://[::1]/x".to_string()] {
        assert!(strict.check_url(&url).await.is_err(), "{url} should be refused up front");
        assert!(strict.send(&url, vec![], "{}".into()).await.is_err(), "{url} should not be reached");
    }
    assert!(seen.hits.lock().unwrap().is_empty(), "the local receiver was never contacted");
    // The operator can allow them (a local test network), and then plain http works for them.
    assert!(HttpSender { allow_private: true }.check_url(&format!("{base}/hook")).await.is_ok());
}

#[tokio::test]
async fn a_redirect_is_not_followed() {
    let (base, seen) = receiver().await;
    let status = HttpSender { allow_private: true }.send(&format!("{base}/redirect"), vec![], "{}".into()).await;
    assert_eq!(status, Ok(302), "the redirect is reported as the answer, which is not a success");
    let paths: Vec<String> = seen.hits.lock().unwrap().iter().map(|h| h.0.clone()).collect();
    assert_eq!(paths, vec!["/redirect"], "nothing was sent on to where it pointed");
}

#[tokio::test]
async fn an_unreachable_receiver_is_reported_not_raised() {
    // A port nothing listens on.
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    let r = HttpSender { allow_private: true }.send(&format!("http://127.0.0.1:{port}/hook"), vec![], "{}".into()).await;
    assert_eq!(r, Err("connection failed".to_string()));
}
