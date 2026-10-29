//! End-to-end behaviour of the escrow core against the mock node.

use std::sync::Arc;

use escrow_core::{
    mock::MockLightning,
    model::{Escrow, NodeInvoiceState, Preimage},
    service::{EscrowService, NewEscrow, ServiceError},
    state::EscrowState,
    store::MemoryStore,
    timing::{TimingConfig, TimingError},
};

const T0: i64 = 1_000_000;
const HOUR: i64 = 3600;

type Svc = EscrowService<MockLightning, Arc<MemoryStore>>;

struct Fixture {
    svc: Svc,
    node: MockLightning,
    store: Arc<MemoryStore>,
}

fn fixture() -> Fixture {
    let node = MockLightning::new();
    let store = Arc::new(MemoryStore::new());
    let svc = EscrowService::new(node.clone(), store.clone(), TimingConfig::default());
    Fixture { svc, node, store }
}

fn secret(n: u8) -> Preimage {
    Preimage::from_bytes([n; 32])
}

fn offer(deadline_at: i64) -> NewEscrow {
    NewEscrow {
        buyer_id: Some("buyer".into()),
        seller_id: "seller".into(),
        amount_sats: 1000,
        description: "deliver the parcel".into(),
        deadline_at,
    }
}

/// Create an escrow and issue its invoice.
async fn issued(f: &Fixture, p: &Preimage) -> Escrow {
    let e = f.svc.create(offer(T0 + HOUR), T0).await.unwrap();
    f.svc.accept(&e.id, p.hash(), "buyer", T0).await.unwrap()
}

/// Create, issue and lock an escrow (the buyer pays and the node reports it held).
async fn locked(f: &Fixture, p: &Preimage) -> Escrow {
    let e = issued(f, p).await;
    let ev = f.node.pay(&p.hash());
    f.svc.apply_event(ev, T0 + 5).await.unwrap().unwrap();
    f.svc.get(&e.id).await.unwrap()
}

#[tokio::test]
async fn happy_path_lock_then_settle() {
    let f = fixture();
    let p = secret(1);
    let e = issued(&f, &p).await;
    assert_eq!(e.state, EscrowState::InvoiceIssued);
    assert!(e.invoice.is_some());

    // Not locked until the node says so.
    assert_eq!(f.svc.get(&e.id).await.unwrap().state, EscrowState::InvoiceIssued);
    let ev = f.node.pay(&p.hash());
    let e = f.svc.apply_event(ev, T0 + 5).await.unwrap().unwrap();
    assert_eq!(e.state, EscrowState::Locked);

    let e = f.svc.release(&e.id, &p, "seller", T0 + 600).await.unwrap();
    assert_eq!(e.state, EscrowState::Settled);
    assert_eq!(f.node.state(&p.hash()), Some(NodeInvoiceState::Settled));

    let log = f.svc.audit_log(&e.id).await.unwrap();
    let path: Vec<_> = log.iter().map(|a| (a.from_state, a.to_state)).collect();
    assert_eq!(
        path,
        vec![
            (EscrowState::Created, EscrowState::InvoiceIssued),
            (EscrowState::InvoiceIssued, EscrowState::Locked),
            (EscrowState::Locked, EscrowState::Settled),
        ]
    );
}

#[tokio::test]
async fn wrong_preimage_is_rejected_without_touching_state_or_node() {
    let f = fixture();
    let p = secret(1);
    let e = locked(&f, &p).await;

    let err = f.svc.release(&e.id, &secret(2), "seller", T0 + 10).await.unwrap_err();
    assert!(matches!(err, ServiceError::WrongPreimage));
    assert_eq!(f.svc.get(&e.id).await.unwrap().state, EscrowState::Locked);
    assert_eq!(f.node.settle_calls(), 0);
    assert_eq!(f.svc.audit_log(&e.id).await.unwrap().len(), 2);
}

#[tokio::test]
async fn short_release_codes_cannot_be_parsed_as_preimages() {
    assert!(Preimage::from_hex("123456").is_none());
    assert!(Preimage::from_hex(&"ab".repeat(31)).is_none());
    assert!(Preimage::from_hex(&"ab".repeat(33)).is_none());
    assert!(Preimage::from_hex(&"zz".repeat(32)).is_none());
    assert!(Preimage::from_hex(&"ab".repeat(32)).is_some());
}

#[tokio::test]
async fn duplicate_release_does_not_settle_twice() {
    let f = fixture();
    let p = secret(1);
    let e = locked(&f, &p).await;

    f.svc.release(&e.id, &p, "seller", T0 + 10).await.unwrap();
    let again = f.svc.release(&e.id, &p, "seller", T0 + 11).await.unwrap();
    assert_eq!(again.state, EscrowState::Settled);
    assert_eq!(f.node.settle_calls(), 1);
    assert_eq!(f.svc.audit_log(&e.id).await.unwrap().len(), 3);
}

#[tokio::test]
async fn deadline_passes_and_the_scheduler_refunds() {
    let f = fixture();
    let p = secret(1);
    let e = locked(&f, &p).await;

    let early = f.svc.tick(T0 + HOUR - 1).await.unwrap();
    assert!(early.cancelled.is_empty());
    assert_eq!(f.svc.get(&e.id).await.unwrap().state, EscrowState::Locked);

    let report = f.svc.tick(T0 + HOUR).await.unwrap();
    assert_eq!(report.cancelled, vec![e.id.clone()]);
    assert_eq!(f.svc.get(&e.id).await.unwrap().state, EscrowState::Refunded);
    assert_eq!(f.node.state(&p.hash()), Some(NodeInvoiceState::Canceled));

    // A second pass does nothing more.
    let again = f.svc.tick(T0 + HOUR + 60).await.unwrap();
    assert!(again.cancelled.is_empty());
    assert_eq!(f.node.cancel_calls(), 1);
}

#[tokio::test]
async fn release_after_the_deadline_is_too_late() {
    let f = fixture();
    let p = secret(1);
    let e = locked(&f, &p).await;

    let err = f.svc.release(&e.id, &p, "seller", T0 + HOUR).await.unwrap_err();
    assert!(matches!(err, ServiceError::TooLate));
    assert_eq!(f.node.settle_calls(), 0);
    assert_eq!(f.svc.get(&e.id).await.unwrap().state, EscrowState::Locked);
}

#[tokio::test]
async fn node_auto_cancel_is_recorded_as_a_refund() {
    let f = fixture();
    let p = secret(1);
    let _ = locked(&f, &p).await;

    let ev = f.node.auto_cancel(&p.hash());
    let e = f.svc.apply_event(ev, T0 + 100).await.unwrap().unwrap();
    assert_eq!(e.state, EscrowState::Refunded);
}

#[tokio::test]
async fn unpaid_invoice_expires() {
    let f = fixture();
    let p = secret(1);
    let _ = issued(&f, &p).await;

    let ev = f.node.expire_unpaid(&p.hash());
    let e = f.svc.apply_event(ev, T0 + 601).await.unwrap().unwrap();
    assert_eq!(e.state, EscrowState::ExpiredUnpaid);
}

#[tokio::test]
async fn scheduler_expires_an_unpaid_invoice_the_node_never_reported() {
    let f = fixture();
    let p = secret(1);
    let e = issued(&f, &p).await;

    let report = f.svc.tick(T0 + 600).await.unwrap();
    assert_eq!(report.cancelled, vec![e.id.clone()]);
    assert_eq!(f.svc.get(&e.id).await.unwrap().state, EscrowState::ExpiredUnpaid);
}

#[tokio::test]
async fn node_offline_at_settle_time_leaves_the_escrow_locked_and_retryable() {
    let f = fixture();
    let p = secret(1);
    let e = locked(&f, &p).await;

    f.node.set_offline(true);
    let err = f.svc.release(&e.id, &p, "seller", T0 + 10).await.unwrap_err();
    assert!(matches!(err, ServiceError::Adapter(_)));
    assert_eq!(f.svc.get(&e.id).await.unwrap().state, EscrowState::Locked);

    f.node.set_offline(false);
    let e = f.svc.release(&e.id, &p, "seller", T0 + 20).await.unwrap();
    assert_eq!(e.state, EscrowState::Settled);
}

#[tokio::test]
async fn restart_mid_escrow_recovers_state_from_the_node() {
    let f = fixture();
    let p = secret(1);
    let e = issued(&f, &p).await;

    // The buyer pays while the backend is down, so the event is missed.
    f.node.pay(&p.hash());
    assert_eq!(f.svc.get(&e.id).await.unwrap().state, EscrowState::InvoiceIssued);

    // A fresh service over the same store and node reconciles on startup.
    let restarted: Svc = EscrowService::new(f.node.clone(), f.store.clone(), TimingConfig::default());
    let report = restarted.reconcile(T0 + 30).await.unwrap();
    assert_eq!(report.checked, 1);
    assert_eq!(report.changed, vec![e.id.clone()]);
    assert!(report.errors.is_empty());
    assert_eq!(restarted.get(&e.id).await.unwrap().state, EscrowState::Locked);

    // Reconciling again changes nothing.
    assert!(restarted.reconcile(T0 + 31).await.unwrap().changed.is_empty());
}

#[tokio::test]
async fn reconcile_adopts_a_settlement_that_happened_while_down() {
    let f = fixture();
    let p = secret(1);
    let e = locked(&f, &p).await;

    // Settled on the node, but the backend never heard.
    use escrow_core::adapter::LightningAdapter;
    f.node.settle_invoice(&p).await.unwrap();
    assert_eq!(f.svc.get(&e.id).await.unwrap().state, EscrowState::Locked);

    f.svc.reconcile(T0 + 30).await.unwrap();
    assert_eq!(f.svc.get(&e.id).await.unwrap().state, EscrowState::Settled);
}

#[tokio::test]
async fn deadline_reached_while_a_dispute_is_open_still_refunds() {
    let f = fixture();
    let p = secret(1);
    let e = locked(&f, &p).await;

    let e = f.svc.dispute(&e.id, "buyer", "goods not as described", T0 + 100).await.unwrap();
    assert_eq!(e.state, EscrowState::Disputed);
    // Repeating the flag is harmless.
    f.svc.dispute(&e.id, "buyer", "again", T0 + 101).await.unwrap();
    assert_eq!(f.svc.audit_log(&e.id).await.unwrap().len(), 3);

    f.svc.tick(T0 + HOUR).await.unwrap();
    assert_eq!(f.svc.get(&e.id).await.unwrap().state, EscrowState::Refunded);
}

#[tokio::test]
async fn a_dispute_cannot_be_raised_after_the_deadline() {
    let f = fixture();
    let p = secret(1);
    let e = locked(&f, &p).await;

    let err = f.svc.dispute(&e.id, "buyer", "late", T0 + HOUR).await.unwrap_err();
    assert!(matches!(err, ServiceError::TooLate));
}

#[tokio::test]
async fn actions_are_rejected_in_the_wrong_state() {
    let f = fixture();
    let p = secret(1);

    // Nothing to release before the funds are locked.
    let e = issued(&f, &p).await;
    let err = f.svc.release(&e.id, &p, "seller", T0 + 1).await.unwrap_err();
    assert!(matches!(err, ServiceError::InvalidState { state: EscrowState::InvoiceIssued, .. }));
    assert_eq!(f.node.settle_calls(), 0);

    // A settled escrow cannot be cancelled or disputed.
    let f = fixture();
    let e = locked(&f, &p).await;
    f.svc.release(&e.id, &p, "seller", T0 + 10).await.unwrap();
    let err = f.svc.cancel(&e.id, "buyer", "changed my mind", T0 + 11).await.unwrap_err();
    assert!(matches!(err, ServiceError::InvalidState { state: EscrowState::Settled, .. }));
    let err = f.svc.dispute(&e.id, "buyer", "too late", T0 + 12).await.unwrap_err();
    assert!(matches!(err, ServiceError::InvalidState { state: EscrowState::Settled, .. }));
}

#[tokio::test]
async fn late_node_events_cannot_move_a_terminal_escrow() {
    let f = fixture();
    let p = secret(1);
    let e = locked(&f, &p).await;
    f.svc.release(&e.id, &p, "seller", T0 + 10).await.unwrap();

    use escrow_core::model::InvoiceEvent;
    let stale = InvoiceEvent { payment_hash: p.hash(), state: NodeInvoiceState::Canceled };
    let e = f.svc.apply_event(stale, T0 + 20).await.unwrap().unwrap();
    assert_eq!(e.state, EscrowState::Settled);
}

#[tokio::test]
async fn impossible_node_state_sends_the_escrow_to_failed() {
    let f = fixture();
    let p = secret(1);
    let e = issued(&f, &p).await;

    use escrow_core::model::InvoiceEvent;
    let bogus = InvoiceEvent { payment_hash: p.hash(), state: NodeInvoiceState::Settled };
    let e2 = f.svc.apply_event(bogus, T0 + 1).await.unwrap().unwrap();
    assert_eq!(e2.id, e.id);
    assert_eq!(e2.state, EscrowState::Failed);
}

#[tokio::test]
async fn repeated_accept_is_idempotent_and_hash_reuse_is_refused() {
    let f = fixture();
    let p = secret(1);
    let e = issued(&f, &p).await;

    let again = f.svc.accept(&e.id, p.hash(), "buyer", T0 + 1).await.unwrap();
    assert_eq!(again, e);
    assert_eq!(f.svc.audit_log(&e.id).await.unwrap().len(), 1);

    // A different hash on an escrow that already has an invoice is refused.
    let err = f.svc.accept(&e.id, secret(2).hash(), "buyer", T0 + 2).await.unwrap_err();
    assert!(matches!(err, ServiceError::InvalidState { .. }));

    // Another escrow cannot reuse the same hash.
    let other = f.svc.create(offer(T0 + HOUR), T0).await.unwrap();
    let err = f.svc.accept(&other.id, p.hash(), "buyer", T0).await.unwrap_err();
    assert!(matches!(err, ServiceError::HashInUse));
}

#[tokio::test]
async fn creation_validates_amount_parties_and_deadline() {
    let f = fixture();

    let mut bad = offer(T0 + HOUR);
    bad.amount_sats = 0;
    assert!(matches!(f.svc.create(bad, T0).await, Err(ServiceError::Invalid(_))));

    let mut bad = offer(T0 + HOUR);
    bad.seller_id = bad.buyer_id.clone().unwrap();
    assert!(matches!(f.svc.create(bad, T0).await, Err(ServiceError::Invalid(_))));

    let err = f.svc.create(offer(T0 + 7 * HOUR), T0).await.unwrap_err();
    assert!(matches!(err, ServiceError::Timing(TimingError::TooLong { .. })));
    let err = f.svc.create(offer(T0 + 5), T0).await.unwrap_err();
    assert!(matches!(err, ServiceError::Timing(TimingError::TooSoon { .. })));
}

#[tokio::test]
async fn the_preimage_never_appears_in_debug_output() {
    let p = Preimage::from_bytes([0xab; 32]);
    let shown = format!("{p:?}");
    assert!(!shown.contains("abab"));
    assert!(shown.contains("redacted"));

    let f = fixture();
    let e = locked(&f, &p).await;
    f.svc.release(&e.id, &p, "seller", T0 + 10).await.unwrap();
    let dump = format!("{:?} {:?}", f.svc.get(&e.id).await.unwrap(), f.svc.audit_log(&e.id).await.unwrap());
    assert!(!dump.contains(&hex::encode([0xab; 32])));
}

#[tokio::test]
async fn memory_store_meets_the_store_contract() {
    escrow_core::testing::store_contract(&MemoryStore::new()).await;
}

#[tokio::test]
async fn memory_store_meets_the_user_store_contract() {
    escrow_core::testing::user_store_contract(&MemoryStore::new()).await;
}

#[tokio::test]
async fn node_events_are_recorded_for_the_audit_trail() {
    let f = fixture();
    let p = secret(1);
    let e = issued(&f, &p).await;
    let ev = f.node.pay(&p.hash());
    f.svc.apply_event(ev, T0 + 5).await.unwrap();

    use escrow_core::store::Store;
    let events = f.store.node_events(&e.id).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].node_invoice_state, NodeInvoiceState::Accepted);
    assert_eq!(events[0].received_at, T0 + 5);
}

#[tokio::test]
async fn the_first_user_to_accept_an_open_offer_becomes_its_buyer() {
    let f = fixture();
    let open = NewEscrow { buyer_id: None, ..offer(T0 + HOUR) };
    let e = f.svc.create(open, T0).await.unwrap();
    assert_eq!(e.buyer_id, None);

    // The seller cannot be the buyer of their own offer.
    assert!(matches!(f.svc.accept(&e.id, secret(1).hash(), "seller", T0).await, Err(ServiceError::Invalid(_))));
    assert_eq!(f.svc.get(&e.id).await.unwrap().state, EscrowState::Created);

    let e = f.svc.accept(&e.id, secret(1).hash(), "alice", T0).await.unwrap();
    assert_eq!((e.state, e.buyer_id.as_deref()), (EscrowState::InvoiceIssued, Some("alice")));

    // Alice repeating it is fine; someone else is turned away and nothing changes.
    f.svc.accept(&e.id, secret(1).hash(), "alice", T0).await.unwrap();
    assert!(matches!(f.svc.accept(&e.id, secret(2).hash(), "bob", T0).await, Err(ServiceError::Invalid(_))));
    assert_eq!(f.svc.get(&e.id).await.unwrap().buyer_id.as_deref(), Some("alice"));
}

#[tokio::test]
async fn a_named_buyer_cannot_be_replaced_by_someone_else_accepting() {
    let f = fixture();
    let e = f.svc.create(offer(T0 + HOUR), T0).await.unwrap();
    assert!(matches!(f.svc.accept(&e.id, secret(1).hash(), "mallory", T0).await, Err(ServiceError::Invalid(_))));
    assert_eq!(f.node.state(&secret(1).hash()), None, "no invoice was issued");
}
