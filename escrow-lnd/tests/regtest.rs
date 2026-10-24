//! Runs the escrow against the real regtest network from `regtest/setup.sh`.
//! Ignored by default; run with `cargo test -p escrow-lnd -- --ignored --test-threads=1`.

use std::{future::pending, sync::Arc, time::Duration};

use escrow_core::{
    adapter::{AdapterError, HoldInvoiceRequest, LightningAdapter, NodeConnectError, NodeCredentials, NodeDirectory},
    model::{NodeInvoiceState, NodeKind, Preimage, Role, User},
    service::{EscrowService, NewEscrow},
    state::EscrowState,
    store::UserStore,
    timing::TimingConfig,
};
use escrow_lnd::{creds::CredentialKey, directory::LndDirectory, install_crypto_provider, runner, LndAdapter};
use escrow_sqlite::SqliteStore;
use fedimint_tonic_lnd::{
    connect,
    lnrpc::{payment::PaymentStatus, ChannelBalanceRequest},
    routerrpc::SendPaymentRequest,
    Client,
};
use rand::RngCore;
use tokio::{task::JoinHandle, time::sleep};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);

fn creds(node: &str, file: &str) -> String {
    format!("{}/../regtest/creds/{node}/{file}", env!("CARGO_MANIFEST_DIR"))
}

/// The service's adapter. On the seller node it uses the restricted invoice-only macaroon the
/// real server uses, so these tests prove nothing else is needed; other nodes use admin.
async fn adapter(node: &str, port: u16) -> LndAdapter {
    let macaroon = if node == "seller" { "toshi.macaroon" } else { "admin.macaroon" };
    LndAdapter::connect(&format!("https://localhost:{port}"), creds(node, "tls.cert"), creds(node, macaroon), CALL_TIMEOUT)
        .await
        .expect("is ./regtest/setup.sh running?")
}

async fn buyer_client() -> Client {
    install_crypto_provider();
    connect("https://localhost:10001".into(), creds("buyer", "tls.cert"), creds("buyer", "admin.macaroon"))
        .await
        .expect("is ./regtest/setup.sh running?")
}

fn new_secret() -> Preimage {
    let mut b = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut b);
    Preimage::from_bytes(b)
}

/// The buyer pays `invoice` through the router; resolves with the payment's final status.
fn pay(invoice: String) -> JoinHandle<PaymentStatus> {
    tokio::spawn(async move {
        let mut buyer = buyer_client().await;
        let mut stream = buyer
            .router()
            .send_payment_v2(SendPaymentRequest {
                payment_request: invoice,
                timeout_seconds: 120,
                fee_limit_sat: 100,
                no_inflight_updates: true,
                ..Default::default()
            })
            .await
            .unwrap()
            .into_inner();
        let mut last = PaymentStatus::InFlight;
        while let Some(p) = stream.message().await.unwrap() {
            last = PaymentStatus::try_from(p.status).unwrap();
        }
        last
    })
}

async fn wait_until<F, Fut>(what: &str, secs: u64, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..secs * 4 {
        if check().await {
            return;
        }
        sleep(Duration::from_millis(250)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test]
#[ignore = "needs the regtest network"]
async fn adapter_settle_cancel_and_idempotency() {
    let seller = adapter("seller", 10003).await;
    let timing = TimingConfig::default();
    let req = |p: &Preimage| HoldInvoiceRequest {
        payment_hash: p.hash(),
        amount_sats: 1000,
        memo: "adapter test".into(),
        expiry_secs: 600,
        final_cltv_delta: timing.final_cltv_delta(0, 3600),
    };

    // Unknown invoice.
    assert_eq!(seller.lookup_invoice(&new_secret().hash()).await.unwrap(), None);

    // Settle path.
    let p = new_secret();
    let invoice = seller.create_hold_invoice(req(&p)).await.unwrap();
    assert!(invoice.starts_with("lnbcrt"));
    assert_eq!(seller.lookup_invoice(&p.hash()).await.unwrap().unwrap().state, NodeInvoiceState::Open);
    let payment = pay(invoice);
    wait_until("ACCEPTED", 20, || async {
        seller.lookup_invoice(&p.hash()).await.unwrap().is_some_and(|i| i.state == NodeInvoiceState::Accepted)
    })
    .await;
    let held = seller.lookup_invoice(&p.hash()).await.unwrap().unwrap();
    assert!(held.htlc_expiry_height.is_some(), "held HTLC should report its expiry height");
    seller.settle_invoice(&p).await.unwrap();
    seller.settle_invoice(&p).await.expect("settling twice must be harmless");
    assert_eq!(seller.lookup_invoice(&p.hash()).await.unwrap().unwrap().state, NodeInvoiceState::Settled);
    assert_eq!(payment.await.unwrap(), PaymentStatus::Succeeded);

    // A cancelled invoice cannot be settled.
    let q = new_secret();
    let invoice = seller.create_hold_invoice(req(&q)).await.unwrap();
    let payment = pay(invoice);
    wait_until("ACCEPTED", 20, || async {
        seller.lookup_invoice(&q.hash()).await.unwrap().is_some_and(|i| i.state == NodeInvoiceState::Accepted)
    })
    .await;
    seller.cancel_invoice(&q.hash()).await.unwrap();
    seller.cancel_invoice(&q.hash()).await.expect("cancelling twice must be harmless");
    assert_eq!(seller.lookup_invoice(&q.hash()).await.unwrap().unwrap().state, NodeInvoiceState::Canceled);
    assert_eq!(payment.await.unwrap(), PaymentStatus::Failed);
    assert!(matches!(seller.settle_invoice(&q).await, Err(AdapterError::Rejected(_))));
}

type Svc = EscrowService<LndAdapter, Arc<SqliteStore>>;

async fn service() -> (Arc<Svc>, LndAdapter) {
    let seller = adapter("seller", 10003).await;
    let store = Arc::new(SqliteStore::in_memory().await.unwrap());
    (Arc::new(EscrowService::new(seller.clone(), store, TimingConfig::default())), seller)
}

fn offer(hold_secs: i64) -> NewEscrow {
    NewEscrow {
        buyer_id: Some("buyer".into()),
        arbiter: None,
        seller_id: "seller".into(),
        amount_sats: 1000,
        description: "regtest escrow".into(),
        deadline_at: runner::unix_now() + hold_secs,
    }
}

async fn state_of<D: NodeDirectory>(svc: &EscrowService<D, Arc<SqliteStore>>, id: &str) -> EscrowState {
    svc.get(id).await.unwrap().state
}

#[tokio::test]
#[ignore = "needs the regtest network"]
async fn service_settles_and_refunds_driven_by_live_node_events() {
    let (svc, seller) = service().await;
    let events = tokio::spawn({
        let svc = svc.clone();
        async move { runner::run_event_loop(&svc, runner::EventLoopConfig::default(), pending()).await }
    });

    // Happy path: lock via the node event, then release with the preimage.
    let p = new_secret();
    let e = svc.create(offer(1800), runner::unix_now()).await.unwrap();
    let e = svc.accept(&e.id, p.hash(), "buyer", runner::unix_now()).await.unwrap();
    let payment = pay(e.invoice.clone().unwrap());
    wait_until("LOCKED", 20, || async { state_of(&svc, &e.id).await == EscrowState::Locked }).await;
    assert!(svc.release(&e.id, &new_secret(), "seller", runner::unix_now()).await.is_err(), "wrong preimage");
    assert_eq!(state_of(&svc, &e.id).await, EscrowState::Locked);
    let done = svc.release(&e.id, &p, "seller", runner::unix_now()).await.unwrap();
    assert_eq!(done.state, EscrowState::Settled);
    assert_eq!(payment.await.unwrap(), PaymentStatus::Succeeded);

    // Cancel path.
    let q = new_secret();
    let e = svc.create(offer(1800), runner::unix_now()).await.unwrap();
    let e = svc.accept(&e.id, q.hash(), "buyer", runner::unix_now()).await.unwrap();
    let payment = pay(e.invoice.clone().unwrap());
    wait_until("LOCKED", 20, || async { state_of(&svc, &e.id).await == EscrowState::Locked }).await;
    let done = svc.cancel(&e.id, "buyer", "changed my mind", runner::unix_now()).await.unwrap();
    assert_eq!(done.state, EscrowState::Refunded);
    assert_eq!(payment.await.unwrap(), PaymentStatus::Failed);

    // Every transition was recorded, and node events were kept.
    assert_eq!(svc.audit_log(&e.id).await.unwrap().len(), 3);

    events.abort();
}

#[tokio::test]
#[ignore = "needs the regtest network; takes about a minute"]
async fn scheduler_refunds_automatically_at_the_deadline() {
    let (svc, seller) = service().await;
    let events = tokio::spawn({
        let svc = svc.clone();
        async move { runner::run_event_loop(&svc, runner::EventLoopConfig::default(), pending()).await }
    });
    let scheduler = tokio::spawn({
        let svc = svc.clone();
        async move { runner::run_scheduler(&svc, Duration::from_secs(1), pending()).await }
    });

    let p = new_secret();
    let e = svc.create(offer(62), runner::unix_now()).await.unwrap();
    let e = svc.accept(&e.id, p.hash(), "buyer", runner::unix_now()).await.unwrap();
    let payment = pay(e.invoice.clone().unwrap());
    wait_until("LOCKED", 20, || async { state_of(&svc, &e.id).await == EscrowState::Locked }).await;

    // No human action from here on.
    wait_until("automatic refund", 90, || async { state_of(&svc, &e.id).await == EscrowState::Refunded }).await;
    assert_eq!(payment.await.unwrap(), PaymentStatus::Failed);
    let log = svc.audit_log(&e.id).await.unwrap();
    assert_eq!(log.last().unwrap().to_state, EscrowState::Refunded);

    events.abort();
    scheduler.abort();
}

/// Backend restart with payments in flight, against the real node and an on-disk database.
/// Three escrows are open when the "process" dies; while it is down the node moves on without
/// it. After restart, reconciliation must adopt the node's view and the escrows must still be
/// usable.
#[tokio::test]
#[ignore = "needs the regtest network"]
async fn restart_mid_escrow_reconciles_with_the_live_node() {
    let dir = std::env::temp_dir().join(format!("toshi-restart-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let db = dir.join("toshi.db");
    let db = db.to_str().unwrap();
    let seller = adapter("seller", 10003).await;
    let (a, b, c) = (new_secret(), new_secret(), new_secret());

    // First life: A and B are paid and locked, C has an invoice nobody has paid yet.
    let store = Arc::new(SqliteStore::open(db).await.unwrap());
    let svc = Arc::new(EscrowService::new(seller.clone(), store.clone(), TimingConfig::default()));
    let events = tokio::spawn({
        let svc = svc.clone();
        async move { runner::run_event_loop(&svc, runner::EventLoopConfig::default(), pending()).await }
    });
    let mut ids = Vec::new();
    let mut invoices = Vec::new();
    for p in [&a, &b, &c] {
        let e = svc.create(offer(1800), runner::unix_now()).await.unwrap();
        let e = svc.accept(&e.id, p.hash(), "buyer", runner::unix_now()).await.unwrap();
        ids.push(e.id);
        invoices.push(e.invoice.unwrap());
    }
    let pay_a = pay(invoices[0].clone());
    let pay_b = pay(invoices[1].clone());
    for id in &ids[..2] {
        wait_until("LOCKED", 20, || async { state_of(&svc, id).await == EscrowState::Locked }).await;
    }

    // The process dies.
    events.abort();
    drop(svc);
    drop(store);

    // While it is down: the node cancels B's hold, and the buyer pays C's invoice.
    seller.cancel_invoice(&b.hash()).await.unwrap();
    assert_eq!(pay_b.await.unwrap(), PaymentStatus::Failed);
    let pay_c = pay(invoices[2].clone());
    wait_until("C accepted by the node", 20, || async {
        seller.lookup_invoice(&c.hash()).await.unwrap().is_some_and(|i| i.state == NodeInvoiceState::Accepted)
    })
    .await;

    // Second life: same database file, fresh adapter, store and service.
    let seller = adapter("seller", 10003).await;
    let store = Arc::new(SqliteStore::open(db).await.unwrap());
    let svc = Arc::new(EscrowService::new(seller.clone(), store, TimingConfig::default()));
    assert_eq!(state_of(&svc, &ids[1]).await, EscrowState::Locked, "the database still has the old view");
    let report = svc.reconcile(runner::unix_now()).await.unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(state_of(&svc, &ids[0]).await, EscrowState::Locked, "A is untouched");
    assert_eq!(state_of(&svc, &ids[1]).await, EscrowState::Refunded, "B follows the node's cancel");
    assert_eq!(state_of(&svc, &ids[2]).await, EscrowState::Locked, "C follows the payment made while down");

    // The survivors still settle, and the cancelled one stays cancelled.
    assert!(svc.release(&ids[1], &b, "seller", runner::unix_now()).await.is_err());
    assert_eq!(svc.release(&ids[0], &a, "seller", runner::unix_now()).await.unwrap().state, EscrowState::Settled);
    assert_eq!(svc.release(&ids[2], &c, "seller", runner::unix_now()).await.unwrap().state, EscrowState::Settled);
    assert_eq!(pay_a.await.unwrap(), PaymentStatus::Succeeded);
    assert_eq!(pay_c.await.unwrap(), PaymentStatus::Succeeded);

    // Each transition is audited exactly once, restart or not.
    assert_eq!(svc.audit_log(&ids[0]).await.unwrap().len(), 3);
    std::fs::remove_dir_all(&dir).ok();
}

// --- Sellers who bring their own node ----------------------------------------------------------

fn creds_text(node: &str, file: &str) -> String {
    std::fs::read_to_string(creds(node, file)).unwrap()
}

fn creds_hex(node: &str, file: &str) -> String {
    hex::encode(std::fs::read(creds(node, file)).unwrap())
}

/// What a seller pastes into the app for their own node.
fn node_credentials(node: &str, port: u16) -> NodeCredentials {
    NodeCredentials {
        address: format!("https://localhost:{port}"),
        tls_cert: creds_text(node, "tls.cert"),
        macaroon_hex: creds_hex(node, "toshi.macaroon"),
    }
}

/// Spendable sats on a node's channels, read with that node's admin macaroon.
async fn local_balance(node: &str, port: u16) -> u64 {
    install_crypto_provider();
    let mut client = connect(format!("https://localhost:{port}"), creds(node, "tls.cert"), creds(node, "admin.macaroon"))
        .await
        .expect("is ./regtest/setup.sh running?");
    let bal = client.lightning().channel_balance(ChannelBalanceRequest {}).await.unwrap().into_inner();
    bal.local_balance.map(|a| a.sat).unwrap_or(0)
}

#[tokio::test]
#[ignore = "needs the regtest network (including seller2)"]
async fn each_seller_is_paid_on_their_own_node() {
    let store = Arc::new(SqliteStore::in_memory().await.unwrap());
    for id in ["alice", "bob"] {
        store.insert_user(User { id: id.into(), role: Role::User, created_at: 0 }, format!("hash-{id}")).await.unwrap();
    }
    let key = CredentialKey::from_hex(&CredentialKey::generate_hex()).unwrap();
    // No shared node at all: the service cannot receive anyone's money.
    let dir = LndDirectory::new(None, store.clone(), Some(key), CALL_TIMEOUT).allow_private_addresses(true);

    // Without a node, a seller cannot create an escrow.
    let svc = Arc::new(EscrowService::new(dir, store.clone(), TimingConfig::default()));
    let seller_of = |id: &str| NewEscrow { seller_id: id.into(), buyer_id: None, ..offer(1800) };
    assert!(matches!(
        svc.create(seller_of("alice"), runner::unix_now()).await,
        Err(escrow_core::service::ServiceError::Adapter(AdapterError::NoNode(_)))
    ));

    // A macaroon from a different node is refused, and so is a certificate that is not that node's.
    let mut wrong_macaroon = node_credentials("seller2", 10004);
    wrong_macaroon.macaroon_hex = creds_hex("seller", "toshi.macaroon");
    assert!(matches!(
        svc.directory().register_seller_node("alice", wrong_macaroon, 1).await,
        Err(NodeConnectError::CheckFailed(_))
    ));
    let mut wrong_cert = node_credentials("seller2", 10004);
    wrong_cert.tls_cert = creds_text("seller", "tls.cert");
    assert!(matches!(
        svc.directory().register_seller_node("alice", wrong_cert, 1).await,
        Err(NodeConnectError::CheckFailed(_))
    ));
    assert!(svc.directory().seller_node_summary("alice").await.unwrap().is_none(), "failed attempts save nothing");

    // Each seller connects their own node. The check makes and cancels a probe invoice on it.
    let alice_creds = node_credentials("seller", 10003);
    let macaroon_bytes = std::fs::read(creds("seller", "toshi.macaroon")).unwrap();
    svc.directory().register_seller_node("alice", alice_creds, runner::unix_now()).await.unwrap();
    svc.directory().register_seller_node("bob", node_credentials("seller2", 10004), runner::unix_now()).await.unwrap();

    // The macaroon is encrypted at rest.
    let saved = store.node_connection("alice").await.unwrap().unwrap();
    assert!(!saved.macaroon_enc.windows(macaroon_bytes.len()).any(|w| w == macaroon_bytes.as_slice()));

    let events = tokio::spawn({
        let svc = svc.clone();
        async move { runner::run_event_loop(&svc, runner::EventLoopConfig::default(), pending()).await }
    });

    let (before_alice, before_bob) = (local_balance("seller", 10003).await, local_balance("seller2", 10004).await);
    let (pa, pb) = (new_secret(), new_secret());
    let a = svc.create(seller_of("alice"), runner::unix_now()).await.unwrap();
    let b = svc.create(seller_of("bob"), runner::unix_now()).await.unwrap();
    assert_eq!((a.node, b.node), (NodeKind::Own, NodeKind::Own));
    let a = svc.accept(&a.id, pa.hash(), "buyer", runner::unix_now()).await.unwrap();
    let b = svc.accept(&b.id, pb.hash(), "buyer", runner::unix_now()).await.unwrap();

    // Each invoice exists on its own seller's node and not on the other's.
    let (node_a, node_b) = (adapter("seller", 10003).await, adapter("seller2", 10004).await);
    assert!(node_a.lookup_invoice(&pa.hash()).await.unwrap().is_some());
    assert!(node_b.lookup_invoice(&pa.hash()).await.unwrap().is_none(), "alice's invoice is not on bob's node");
    assert!(node_b.lookup_invoice(&pb.hash()).await.unwrap().is_some());
    assert!(node_a.lookup_invoice(&pb.hash()).await.unwrap().is_none(), "bob's invoice is not on alice's node");

    // The buyer pays both; both lock (each learned from its own node's events).
    let (pay_a, pay_b) = (pay(a.invoice.clone().unwrap()), pay(b.invoice.clone().unwrap()));
    wait_until("alice LOCKED", 30, || async { state_of(&svc, &a.id).await == EscrowState::Locked }).await;
    wait_until("bob LOCKED", 30, || async { state_of(&svc, &b.id).await == EscrowState::Locked }).await;

    // Releasing pays each seller on their own node.
    assert_eq!(svc.release(&a.id, &pa, "alice", runner::unix_now()).await.unwrap().state, EscrowState::Settled);
    assert_eq!(svc.release(&b.id, &pb, "bob", runner::unix_now()).await.unwrap().state, EscrowState::Settled);
    assert_eq!(pay_a.await.unwrap(), PaymentStatus::Succeeded);
    assert_eq!(pay_b.await.unwrap(), PaymentStatus::Succeeded);
    wait_until("alice's node balance", 15, || async { local_balance("seller", 10003).await == before_alice + 1000 }).await;
    wait_until("bob's node balance", 15, || async { local_balance("seller2", 10004).await == before_bob + 1000 }).await;

    // Replacing a seller's node is picked up: the cached connection is dropped and rebuilt.
    svc.directory().remove_seller_node("bob").await.unwrap();
    assert!(matches!(svc.create(seller_of("bob"), runner::unix_now()).await, Err(_)));

    events.abort();
}
