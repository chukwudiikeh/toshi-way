//! Webhooks: telling a business's software when one of its escrows changes.
//!
//! Delivery is driven by the audit log, the record of every real state change. Each endpoint keeps
//! the position of the last entry it was sent (`last_seq`), so a restart loses nothing, events go out
//! in order, and an endpoint that is down simply falls behind and catches up. A failed delivery is
//! retried with growing gaps, and an endpoint that has failed for days is switched off.
//!
//! What a receiver gets is a nudge: the escrow as it is now, plus the event's own state. It should
//! treat the signed message as "go and look", and read the escrow from the API before acting.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use async_trait::async_trait;
use escrow_core::{
    model::{AuditEntry, AuditRow, Escrow, Webhook, WebhookProgress},
    state::EscrowState,
    store::{Store, UserStore},
};
use escrow_lnd::{address, creds::CredentialKey};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde_json::{json, Value};
use sha2::Sha256;

use crate::routes::{escrow_view, with_checkout};
use crate::auth::Party;

/// A person may have this many endpoints at once.
pub const MAX_WEBHOOKS_PER_USER: usize = 5;
/// How long delivery may keep failing before the endpoint is switched off.
pub const DISABLE_AFTER_SECS: i64 = 3 * 24 * 3600;
/// The time allowed for a receiver to answer.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
const EVENTS_PER_ENDPOINT_PER_TICK: u32 = 20;

/// A signing secret: `whsec_` and 256 random bits. Shown once when the endpoint is made.
pub fn new_signing_secret() -> String {
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    format!("whsec_{}", hex::encode(b))
}

pub fn new_webhook_id() -> String {
    let mut b = [0u8; 8];
    rand::rngs::OsRng.fill_bytes(&mut b);
    format!("wh_{}", hex::encode(b))
}

/// `HMAC-SHA256(secret, "<timestamp>.<body>")` as hex. The timestamp is part of what is signed, so a
/// receiver can refuse old messages.
pub fn sign(secret: &str, timestamp: i64, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(format!("{timestamp}.{body}").as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// The `Toshi-Signature` header value: `t=<timestamp>,v1=<signature>`.
pub fn signature_header(secret: &str, timestamp: i64, body: &str) -> String {
    format!("t={timestamp},v1={}", sign(secret, timestamp, body))
}

/// The kind of event an audit entry is.
pub fn event_type(e: &AuditEntry) -> &'static str {
    use EscrowState::*;
    if e.from_state == e.to_state {
        return "escrow.updated";
    }
    match e.to_state {
        Created => "escrow.updated",
        InvoiceIssued => "escrow.accepted",
        Locked => "escrow.locked",
        Settled => "escrow.settled",
        Refunded => "escrow.refunded",
        ExpiredUnpaid => "escrow.expired",
        Disputed => "escrow.disputed",
        Failed => "escrow.failed",
    }
}

/// The JSON body for one audit entry.
pub fn build_event(row: &AuditRow, escrow: &Escrow, web_base_url: Option<&str>) -> Value {
    json!({
        "id": format!("evt_{}", row.seq),
        "type": event_type(&row.entry),
        "created_at": row.entry.created_at,
        "data": {
            // What the escrow's state became in this event; `escrow` is how it is now, which may be later.
            "state": row.entry.to_state.as_str(),
            "escrow": with_checkout(escrow_view(escrow, Party::Seller), web_base_url),
        },
    })
}

pub fn test_event(now: i64) -> Value {
    json!({
        "id": format!("evt_test_{now}"),
        "type": "webhook.test",
        "created_at": now,
        "data": { "message": "This is a test from toshi-way. Your endpoint is reachable and the signature can be checked." },
    })
}

/// How long to wait before trying again after `attempts` failures in a row.
pub fn backoff_secs(attempts: u32) -> i64 {
    match attempts {
        0 | 1 => 10,
        2 => 30,
        3 => 120,
        4 => 600,
        5 => 1800,
        _ => 3600,
    }
}

/// Sends one request. A trait so tests can stand in for the network.
#[async_trait]
pub trait Sender: Send + Sync {
    /// POST `body` with `headers` to `url`. `Ok(status)` is any answer received; `Err` is a short
    /// reason it could not be delivered at all (for example "timeout").
    async fn send(&self, url: &str, headers: Vec<(String, String)>, body: String) -> Result<u16, String>;
    /// Whether `url` may be used at all, checked when an endpoint is added.
    async fn check_url(&self, url: &str) -> Result<(), String>;
}

/// The real thing: HTTPS to a public address, no redirects, and the connection pinned to the address
/// that was checked, so the name cannot be pointed somewhere else between the check and the call.
pub struct HttpSender {
    /// Allow private and local addresses and plain `http://` for them. For a local test network only.
    pub allow_private: bool,
}

struct Target {
    host: String,
    port: u16,
}

fn parse_target(url: &str, allow_private: bool) -> Result<Target, String> {
    let u = reqwest::Url::parse(url.trim()).map_err(|_| "that is not a valid address".to_string())?;
    match u.scheme() {
        "https" => {}
        "http" if allow_private => {}
        _ => return Err("the address must start with https://".into()),
    }
    if !u.username().is_empty() || u.password().is_some() {
        return Err("the address must not contain a login".into());
    }
    if u.fragment().is_some() {
        return Err("the address must not contain a #".into());
    }
    let host = u.host_str().ok_or("the address needs a host")?.trim_matches(['[', ']']).to_string();
    let port = u.port_or_known_default().ok_or("the address needs a port")?;
    if url.chars().count() > 500 {
        return Err("the address is too long".into());
    }
    Ok(Target { host, port })
}

impl HttpSender {
    /// Resolve the host and refuse anything that is not an ordinary public address.
    async fn resolve(&self, t: &Target) -> Result<Vec<SocketAddr>, String> {
        let addrs: Vec<SocketAddr> = tokio::net::lookup_host((t.host.as_str(), t.port))
            .await
            .map_err(|_| "could not resolve that host".to_string())?
            .collect();
        if addrs.is_empty() {
            return Err("could not resolve that host".into());
        }
        if !self.allow_private && addrs.iter().any(|a| !address::is_public(a.ip())) {
            return Err("that address points at a private or local network".into());
        }
        Ok(addrs)
    }
}

#[async_trait]
impl Sender for HttpSender {
    async fn check_url(&self, url: &str) -> Result<(), String> {
        let t = parse_target(url, self.allow_private)?;
        self.resolve(&t).await.map(|_| ())
    }

    async fn send(&self, url: &str, headers: Vec<(String, String)>, body: String) -> Result<u16, String> {
        let t = parse_target(url, self.allow_private)?;
        let addrs = self.resolve(&t).await.map_err(|_| "blocked address".to_string())?;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(SEND_TIMEOUT)
            .resolve_to_addrs(&t.host, &addrs)
            .build()
            .map_err(|_| "client".to_string())?;
        let mut req = client.post(url.trim()).body(body);
        for (k, v) in headers {
            req = req.header(k, v);
        }
        match req.send().await {
            Ok(res) => Ok(res.status().as_u16()),
            Err(e) if e.is_timeout() => Err("timeout".into()),
            Err(e) if e.is_connect() => Err("connection failed".into()),
            Err(_) => Err("request failed".into()),
        }
    }
}

/// The headers every delivery carries.
pub fn delivery_headers(secret: &str, now: i64, event: &Value, body: &str) -> Vec<(String, String)> {
    vec![
        ("Content-Type".into(), "application/json".into()),
        ("User-Agent".into(), "toshi-way-webhooks/1".into()),
        ("Toshi-Signature".into(), signature_header(secret, now, body)),
        ("Toshi-Event-Id".into(), event["id"].as_str().unwrap_or_default().to_string()),
        ("Toshi-Event-Type".into(), event["type"].as_str().unwrap_or_default().to_string()),
    ]
}

/// A short, safe description of how a try went, for the person to read.
fn status_text(r: &Result<u16, String>) -> String {
    match r {
        Ok(code) => code.to_string(),
        Err(why) => why.clone(),
    }
}

pub struct Dispatcher<S> {
    store: S,
    key: CredentialKey,
    sender: Arc<dyn Sender>,
    web_base_url: Option<String>,
}

impl<S: Store + UserStore + Clone + 'static> Dispatcher<S> {
    pub fn new(store: S, key: CredentialKey, sender: Arc<dyn Sender>, web_base_url: Option<String>) -> Self {
        Self { store, key, sender, web_base_url }
    }

    /// Work through every endpoint that is due: send what it has not yet been sent, in order, until it
    /// is caught up, fails, or has had its share for this round. Returns how many events were delivered.
    pub async fn tick(&self, now: i64) -> usize {
        let due = match self.store.due_webhooks(now, 100).await {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(error = %e, "webhooks: could not read due endpoints");
                return 0;
            }
        };
        let mut delivered = 0;
        for hook in due {
            delivered += self.deliver(hook, now).await;
        }
        delivered
    }

    async fn deliver(&self, hook: Webhook, now: i64) -> usize {
        let Some(secret) = self.key.open(&hook.signing_key_enc).and_then(|b| String::from_utf8(b).ok()) else {
            // Sealed under another key: nothing can be signed. Say so and stop trying.
            tracing::error!(webhook = %hook.id, "webhooks: the signing secret cannot be opened; was NODE_CREDENTIAL_KEY changed?");
            self.record(&hook, WebhookProgress {
                last_seq: hook.last_seq,
                attempts: hook.attempts,
                next_attempt_at: now + 3600,
                failing_since: hook.failing_since.or(Some(now)),
                disabled_at: Some(now),
                last_attempt_at: Some(now),
                last_status: Some("secret unreadable".into()),
            })
            .await;
            return 0;
        };
        let rows = match self.store.audit_since(&hook.user_id, hook.last_seq, EVENTS_PER_ENDPOINT_PER_TICK).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "webhooks: could not read the audit log");
                return 0;
            }
        };
        let mut p = WebhookProgress {
            last_seq: hook.last_seq,
            attempts: hook.attempts,
            next_attempt_at: now,
            failing_since: hook.failing_since,
            disabled_at: None,
            last_attempt_at: hook.last_attempt_at,
            last_status: hook.last_status.clone(),
        };
        let mut delivered = 0;
        for row in rows {
            let escrow = match self.store.get(&row.entry.escrow_id).await {
                Ok(Some(e)) => e,
                // Cannot happen (the log references escrows), but never get stuck on it.
                Ok(None) => {
                    p.last_seq = row.seq;
                    continue;
                }
                Err(_) => break,
            };
            let event = build_event(&row, &escrow, self.web_base_url.as_deref());
            let body = event.to_string();
            let headers = delivery_headers(&secret, now, &event, &body);
            let result = self.sender.send(&hook.url, headers, body).await;
            p.last_attempt_at = Some(now);
            p.last_status = Some(status_text(&result));
            if matches!(result, Ok(200..=299)) {
                p.last_seq = row.seq;
                p.attempts = 0;
                p.failing_since = None;
                p.next_attempt_at = now;
                delivered += 1;
            } else {
                p.attempts += 1;
                p.failing_since = p.failing_since.or(Some(now));
                p.next_attempt_at = now + backoff_secs(p.attempts);
                if now - p.failing_since.unwrap_or(now) >= DISABLE_AFTER_SECS {
                    p.disabled_at = Some(now);
                    tracing::warn!(webhook = %hook.id, "webhooks: switched off after days of failed deliveries");
                }
                break;
            }
        }
        if p.last_seq != hook.last_seq || p.attempts != hook.attempts || p.disabled_at.is_some() {
            self.record(&hook, p).await;
        }
        delivered
    }

    async fn record(&self, hook: &Webhook, p: WebhookProgress) {
        if let Err(e) = self.store.record_webhook_progress(&hook.id, p).await {
            tracing::warn!(error = %e, webhook = %hook.id, "webhooks: could not record progress");
        }
    }

    /// Run until the task is dropped.
    pub async fn run(self, every: Duration, now: impl Fn() -> i64 + Send + 'static) {
        let mut interval = tokio::time::interval(every);
        loop {
            interval.tick().await;
            self.tick(now()).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use escrow_core::state::EscrowState::*;

    fn entry(from: EscrowState, to: EscrowState) -> AuditEntry {
        AuditEntry { escrow_id: "e".into(), actor: "a".into(), action: "x".into(), from_state: from, to_state: to, reason: "r".into(), created_at: 1 }
    }

    #[test]
    fn signatures_are_stable_and_depend_on_everything_signed() {
        let a = sign("whsec_a", 1000, "{}");
        assert_eq!(a, sign("whsec_a", 1000, "{}"));
        assert_eq!(a.len(), 64);
        assert_ne!(a, sign("whsec_b", 1000, "{}"), "the secret");
        assert_ne!(a, sign("whsec_a", 1001, "{}"), "the timestamp");
        assert_ne!(a, sign("whsec_a", 1000, "{ }"), "the body");
        assert_eq!(signature_header("whsec_a", 1000, "{}"), format!("t=1000,v1={a}"));
        // A known value, so a receiver written in any language can be checked against it.
        assert_eq!(
            sign("whsec_test", 1700000000, r#"{"id":"evt_1"}"#),
            "c89214b5b5da833daed6f0b8c5bb6bd58cea9022bd80ccc78230f3942d632925",
        );
    }

    #[test]
    fn event_types_follow_the_state() {
        assert_eq!(event_type(&entry(Created, InvoiceIssued)), "escrow.accepted");
        assert_eq!(event_type(&entry(InvoiceIssued, Locked)), "escrow.locked");
        assert_eq!(event_type(&entry(Locked, Settled)), "escrow.settled");
        assert_eq!(event_type(&entry(Locked, Refunded)), "escrow.refunded");
        assert_eq!(event_type(&entry(Locked, Disputed)), "escrow.disputed");
        assert_eq!(event_type(&entry(InvoiceIssued, ExpiredUnpaid)), "escrow.expired");
        assert_eq!(event_type(&entry(Locked, Failed)), "escrow.failed");
        assert_eq!(event_type(&entry(InvoiceIssued, InvoiceIssued)), "escrow.updated");
    }

    #[test]
    fn the_wait_between_tries_grows_and_stops_growing() {
        let waits: Vec<i64> = (1..=8).map(backoff_secs).collect();
        assert_eq!(waits, vec![10, 30, 120, 600, 1800, 3600, 3600, 3600]);
    }

    #[test]
    fn addresses_that_could_aim_the_server_inward_are_refused_up_front() {
        for bad in ["http://hooks.example.com/x", "ftp://a.example.com", "https://user:pw@a.example.com/", "https://a.example.com/x#frag", "not a url", "https://"] {
            assert!(parse_target(bad, false).is_err(), "{bad}");
        }
        assert!(parse_target("https://hooks.example.com/toshi?x=1", false).is_ok());
        assert!(parse_target("http://localhost:9000/hook", false).is_err(), "plain http only where private addresses are allowed");
        assert!(parse_target("http://localhost:9000/hook", true).is_ok());
    }
}
