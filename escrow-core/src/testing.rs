//! Behaviour every [`Store`] implementation must provide. Each backend's tests call
//! [`store_contract`] with a fresh, empty store so MemoryStore, SQLite and Postgres stay interchangeable.

use crate::{
    model::{AuditEntry, Escrow, NodeEventRecord, NodeInvoiceState, Role, User},
    state::EscrowState,
    store::{EscrowUpdate, Store, StoreError, UserStore},
};

fn escrow(id: &str) -> Escrow {
    Escrow {
        id: id.into(),
        buyer_id: Some("buyer".into()),
        seller_id: "seller".into(),
        amount_sats: 1000,
        description: "parcel".into(),
        payment_hash: None,
        invoice: None,
        invoice_expires_at: None,
        state: EscrowState::Created,
        deadline_at: 5000,
        created_at: 100,
        updated_at: 100,
    }
}

fn audit(id: &str, from: EscrowState, to: EscrowState, at: i64) -> AuditEntry {
    AuditEntry {
        escrow_id: id.into(),
        actor: "tester".into(),
        action: "act".into(),
        from_state: from,
        to_state: to,
        reason: "because".into(),
        created_at: at,
    }
}

pub async fn store_contract(store: &dyn Store) {
    use EscrowState::*;

    // Round trip and uniqueness.
    let e = escrow("a");
    store.insert(e.clone()).await.unwrap();
    assert_eq!(store.get("a").await.unwrap(), Some(e.clone()));
    assert_eq!(store.get("missing").await.unwrap(), None);
    assert_eq!(store.insert(e.clone()).await, Err(StoreError::Conflict));

    // A transition applies its update and appends its audit entry together.
    let hash = [7u8; 32];
    let update = EscrowUpdate {
        buyer_id: None,
        payment_hash: Some(hash),
        invoice: Some("lnbcrt1abc".into()),
        invoice_expires_at: Some(700),
    };
    let moved = store.transition("a", Created, update, audit("a", Created, InvoiceIssued, 200)).await.unwrap();
    assert_eq!(moved.state, InvoiceIssued);
    assert_eq!(moved.payment_hash, Some(hash));
    assert_eq!(moved.invoice.as_deref(), Some("lnbcrt1abc"));
    assert_eq!(moved.invoice_expires_at, Some(700));
    assert_eq!(moved.updated_at, 200);
    assert_eq!(store.get("a").await.unwrap(), Some(moved.clone()));
    assert_eq!(store.get_by_hash(&hash).await.unwrap(), Some(moved));
    assert_eq!(store.get_by_hash(&[9u8; 32]).await.unwrap(), None);

    // A transition from the wrong state fails and leaves no trace.
    let stale = store
        .transition("a", Created, EscrowUpdate::default(), audit("a", Created, Locked, 300))
        .await;
    assert_eq!(stale, Err(StoreError::Conflict));
    assert_eq!(store.get("a").await.unwrap().unwrap().state, InvoiceIssued);
    assert_eq!(store.audit_log("a").await.unwrap().len(), 1);

    // An empty update keeps the fields set earlier.
    let locked = store
        .transition("a", InvoiceIssued, EscrowUpdate::default(), audit("a", InvoiceIssued, Locked, 300))
        .await
        .unwrap();
    assert_eq!(locked.payment_hash, Some(hash));
    assert_eq!(locked.invoice.as_deref(), Some("lnbcrt1abc"));

    // Unknown escrow.
    let missing = store
        .transition("nope", Created, EscrowUpdate::default(), audit("nope", Created, InvoiceIssued, 1))
        .await;
    assert_eq!(missing, Err(StoreError::NotFound));

    // The audit log is ordered and scoped to its escrow.
    store.insert(escrow("b")).await.unwrap();
    store
        .transition("b", Created, EscrowUpdate::default(), audit("b", Created, Failed, 400))
        .await
        .unwrap();
    let log = store.audit_log("a").await.unwrap();
    assert_eq!(
        log.iter().map(|x| (x.from_state, x.to_state, x.created_at)).collect::<Vec<_>>(),
        vec![(Created, InvoiceIssued, 200), (InvoiceIssued, Locked, 300)]
    );
    assert_eq!(log[0], audit("a", Created, InvoiceIssued, 200));
    assert_eq!(store.audit_log("b").await.unwrap().len(), 1);

    // Active listing leaves out terminal escrows.
    let active: Vec<_> = store.list_active().await.unwrap().into_iter().map(|e| e.id).collect();
    assert_eq!(active, vec!["a".to_string()]);

    // Per-user listing: parties only, newest first, ties by id.
    let mut newer = escrow("c");
    newer.created_at = 200;
    newer.buyer_id = Some("someone-else".into());
    store.insert(newer).await.unwrap();
    let ids = |v: Vec<Escrow>| v.into_iter().map(|e| e.id).collect::<Vec<_>>();
    assert_eq!(ids(store.list_for_user("buyer").await.unwrap()), ["a", "b"]);
    assert_eq!(ids(store.list_for_user("seller").await.unwrap()), ["c", "a", "b"]);
    assert_eq!(ids(store.list_for_user("someone-else").await.unwrap()), ["c"]);
    assert!(store.list_for_user("nobody").await.unwrap().is_empty());

    // An open offer has no buyer until someone accepts; the claim is applied with the transition
    // and an update without a buyer leaves it alone.
    let mut open = escrow("open");
    open.buyer_id = None;
    open.created_at = 50;
    store.insert(open).await.unwrap();
    assert_eq!(store.get("open").await.unwrap().unwrap().buyer_id, None);
    assert!(store.list_for_user("claimer").await.unwrap().is_empty());
    let claim = EscrowUpdate { buyer_id: Some("claimer".into()), ..Default::default() };
    let claimed = store.transition("open", Created, claim, audit("open", Created, InvoiceIssued, 60)).await.unwrap();
    assert_eq!(claimed.buyer_id.as_deref(), Some("claimer"));
    assert_eq!(ids(store.list_for_user("claimer").await.unwrap()), ["open"]);
    let after = store
        .transition("open", InvoiceIssued, EscrowUpdate::default(), audit("open", InvoiceIssued, Locked, 70))
        .await
        .unwrap();
    assert_eq!(after.buyer_id.as_deref(), Some("claimer"), "later updates keep the buyer");
    // The active-listing and audit assertions above ran before this, so leave the rest unchanged.

    // Node events are kept per escrow, in order.
    for (state, at) in [(NodeInvoiceState::Accepted, 310), (NodeInvoiceState::Settled, 320)] {
        store
            .record_node_event(NodeEventRecord {
                escrow_id: "a".into(),
                node_invoice_state: state,
                raw_event_hash: format!("hash-{at}"),
                received_at: at,
            })
            .await
            .unwrap();
    }
    let events = store.node_events("a").await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].node_invoice_state, NodeInvoiceState::Accepted);
    assert_eq!(events[1].raw_event_hash, "hash-320");
    assert!(store.node_events("b").await.unwrap().is_empty());
}

pub async fn user_store_contract(store: &dyn UserStore) {
    let alice = User { id: "alice".into(), role: Role::User, created_at: 10 };
    let root = User { id: "root".into(), role: Role::Operator, created_at: 11 };
    store.insert_user(alice.clone(), "hash-a".into()).await.unwrap();
    store.insert_user(root.clone(), "hash-r".into()).await.unwrap();

    assert_eq!(store.user_by_token_hash("hash-a").await.unwrap(), Some(alice.clone()));
    assert_eq!(store.user_by_token_hash("hash-r").await.unwrap(), Some(root));
    assert_eq!(store.user_by_token_hash("nope").await.unwrap(), None);
    assert_eq!(store.get_user("alice").await.unwrap(), Some(alice.clone()));
    assert_eq!(store.get_user("ghost").await.unwrap(), None);

    // Ids and token hashes are unique.
    assert_eq!(store.insert_user(alice, "other".into()).await, Err(StoreError::Conflict));
    let bob = User { id: "bob".into(), role: Role::User, created_at: 12 };
    assert_eq!(store.insert_user(bob, "hash-a".into()).await, Err(StoreError::Conflict));
}
