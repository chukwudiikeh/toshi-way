//! Behaviour every [`Store`] implementation must provide. Each backend's tests call
//! [`store_contract`] with a fresh, empty store so MemoryStore, SQLite and Postgres stay interchangeable.

use crate::{
    model::{ApiKey, AuditEntry, Escrow, NodeConnection, NodeEventRecord, NodeInvoiceState, NodeKind, Role, Scope, User},
    state::EscrowState,
    store::{EscrowUpdate, Store, StoreError, UserStore},
};

fn escrow(id: &str) -> Escrow {
    Escrow {
        id: id.into(),
        buyer_id: Some("buyer".into()),
        arbiter_id: None,
        arbiter_key: None,
        arbiter_sealed: None,
        arbiter_confirmed_at: None,
        node: NodeKind::Shared,
        fiat: None,
        reference: None,
        return_url: None,
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

    // A business's order number and link back survive storage (and are absent when not set).
    let mut shop = escrow("shop");
    shop.buyer_id = Some("shop-buyer".into());
    shop.seller_id = "shop-seller".into();
    shop.reference = Some("order #1001".into());
    shop.return_url = Some("https://shop.example.com/thanks?o=1001".into());
    store.insert(shop.clone()).await.unwrap();
    assert_eq!(store.get("shop").await.unwrap(), Some(shop));
    assert_eq!(store.get("a").await.unwrap().unwrap().reference, None);
    store.transition("shop", EscrowState::Created, EscrowUpdate::default(), audit("shop", EscrowState::Created, EscrowState::Failed, 150)).await.unwrap();
    assert_eq!(store.get("missing").await.unwrap(), None);
    assert_eq!(store.insert(e.clone()).await, Err(StoreError::Conflict));

    // A transition applies its update and appends its audit entry together.
    let hash = [7u8; 32];
    let update = EscrowUpdate {
        arbiter_sealed: None,
        arbiter_confirmed_at: None,
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

    // An arbitrated escrow keeps its arbiter, shows up in the arbiter's list, and accepts the sealed
    // secret and the arbiter's confirmation as updates.
    let mut arb = escrow("arb");
    arb.arbiter_id = Some("judge".into());
    arb.arbiter_key = Some("judge-key".into());
    arb.created_at = 45;
    store.insert(arb.clone()).await.unwrap();
    assert_eq!(store.get("arb").await.unwrap(), Some(arb));
    assert_eq!(ids(store.list_for_user("judge").await.unwrap()), ["arb"]);
    let sealed = EscrowUpdate { arbiter_sealed: Some("sealed-blob".into()), ..Default::default() };
    let e = store.transition("arb", Created, sealed, audit("arb", Created, InvoiceIssued, 46)).await.unwrap();
    assert_eq!(e.arbiter_sealed.as_deref(), Some("sealed-blob"));
    let confirm = EscrowUpdate { arbiter_confirmed_at: Some(47), ..Default::default() };
    let e = store.transition("arb", InvoiceIssued, confirm, audit("arb", InvoiceIssued, InvoiceIssued, 47)).await.unwrap();
    assert_eq!((e.state, e.arbiter_confirmed_at, e.arbiter_sealed.as_deref()), (InvoiceIssued, Some(47), Some("sealed-blob")));

    // A fiat quote is stored exactly as given, survives later transitions, and absent means absent.
    let mut priced = escrow("priced");
    priced.created_at = 40;
    priced.fiat = Some(crate::fiat::FiatQuote {
        currency: "NGN".into(),
        amount_minor: 7_312_500,
        rate_minor_per_btc: 9_750_000_000,
        quoted_at: 39,
        source: "demo rate".into(),
    });
    store.insert(priced.clone()).await.unwrap();
    assert_eq!(store.get("priced").await.unwrap(), Some(priced.clone()));
    let moved = store
        .transition("priced", Created, EscrowUpdate::default(), audit("priced", Created, InvoiceIssued, 41))
        .await
        .unwrap();
    assert_eq!(moved.fiat, priced.fiat, "the quote is locked: a transition does not touch it");
    assert_eq!(store.get("a").await.unwrap().unwrap().fiat, None);

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

/// A long history: the list holds everything in play plus the newest 100 that ended, while the
/// totals still add up over all of it.
pub async fn history_contract(store: &dyn Store) {
    use EscrowState::*;
    let mut n = 0;
    let mut add = |state: EscrowState, buyer: &str, seller: &str, amount: u64| {
        n += 1;
        let mut e = escrow(&format!("h{n:03}"));
        e.state = state;
        e.buyer_id = Some(buyer.into());
        e.seller_id = seller.into();
        e.amount_sats = amount;
        e.created_at = n;
        e
    };
    let mut all = Vec::new();
    // Oldest first: two still in play, then 70 settled and 40 refunded, with "me" as the buyer.
    all.push(add(Locked, "me", "them", 500));
    all.push(add(Created, "me", "them", 400));
    for _ in 0..70 {
        all.push(add(Settled, "me", "them", 10));
    }
    for _ in 0..40 {
        all.push(add(Refunded, "me", "them", 20));
    }
    // And a few where "me" is the seller, and one where "me" only judges.
    for _ in 0..3 {
        all.push(add(Settled, "them", "me", 100));
    }
    let mut judged = add(Settled, "them", "other", 9999);
    judged.arbiter_id = Some("me".into());
    all.push(judged);
    for e in all {
        store.insert(e).await.unwrap();
    }

    let list = store.list_for_user("me").await.unwrap();
    let in_play = list.iter().filter(|e| !e.state.is_terminal()).count();
    let ended = list.iter().filter(|e| e.state.is_terminal()).count();
    assert_eq!((in_play, ended), (2, 100), "everything in play, and the newest 100 that ended");
    assert_eq!(list.len(), 102);
    assert!(list.iter().any(|e| e.id == "h001"), "an old escrow still in play is not cut off");
    assert!(list.windows(2).all(|w| w[0].created_at >= w[1].created_at), "newest first");

    let t = store.totals_for_user("me").await.unwrap();
    assert_eq!(t.escrows, 116);
    assert_eq!((t.settled_count, t.settled_paid, t.settled_received), (74, 700, 300));
    assert_eq!((t.refunded_count, t.refunded_sats), (40, 800));
    assert_eq!(store.totals_for_user("nobody").await.unwrap(), Default::default());
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

    // API keys: found by hash, listed newest first per user, revoked once and only by their owner.
    let key = |id: &str, user: &str, scopes: Vec<Scope>, at: i64| ApiKey {
        id: id.into(),
        user_id: user.into(),
        name: format!("key {id}"),
        last_chars: "ab12".into(),
        scopes,
        created_at: at,
        last_used_at: None,
        revoked_at: None,
    };
    let k1 = key("k1", "alice", vec![Scope::EscrowsRead], 60);
    let k2 = key("k2", "alice", vec![Scope::EscrowsRead, Scope::EscrowsWrite], 70);
    store.insert_api_key(k1.clone(), "kh-1".into()).await.unwrap();
    store.insert_api_key(k2.clone(), "kh-2".into()).await.unwrap();
    assert_eq!(store.insert_api_key(k1.clone(), "kh-3".into()).await, Err(StoreError::Conflict), "id is unique");
    assert_eq!(
        store.insert_api_key(key("k3", "alice", vec![], 80), "kh-2".into()).await,
        Err(StoreError::Conflict),
        "hash is unique"
    );
    assert_eq!(store.api_key_by_hash("kh-2").await.unwrap(), Some(k2.clone()));
    assert_eq!(store.api_key_by_hash("missing").await.unwrap(), None);
    assert!(k2.allows(Scope::EscrowsWrite) && !k1.allows(Scope::EscrowsWrite));
    assert_eq!(store.list_api_keys("alice").await.unwrap(), vec![k2.clone(), k1.clone()]);
    assert!(store.list_api_keys("root").await.unwrap().is_empty());
    store.touch_api_key("k1", 90).await.unwrap();
    assert_eq!(store.api_key_by_hash("kh-1").await.unwrap().unwrap().last_used_at, Some(90));
    assert!(!store.revoke_api_key("root", "k1", 95).await.unwrap(), "not the owner's");
    assert!(store.revoke_api_key("alice", "k1", 96).await.unwrap());
    assert!(!store.revoke_api_key("alice", "k1", 97).await.unwrap(), "already revoked");
    let revoked = store.api_key_by_hash("kh-1").await.unwrap().unwrap();
    assert_eq!(revoked.revoked_at, Some(96), "the first revocation time stands");
    assert!(!revoked.allows(Scope::EscrowsRead), "a revoked key allows nothing");

    // An arbiter's public key: registered, replaced as a whole, kept per user.
    assert_eq!(store.arbiter_key("alice").await.unwrap(), None);
    store.put_arbiter_key("alice", "key-one", 40).await.unwrap();
    store.put_arbiter_key("root", "key-root", 41).await.unwrap();
    assert_eq!(store.arbiter_key("alice").await.unwrap().as_deref(), Some("key-one"));
    store.put_arbiter_key("alice", "key-two", 50).await.unwrap();
    assert_eq!(store.arbiter_key("alice").await.unwrap().as_deref(), Some("key-two"), "replaced");
    assert_eq!(store.arbiter_key("root").await.unwrap().as_deref(), Some("key-root"));

    // A seller's node connection: saved, replaced as a whole, deleted, kept per user.
    let conn = |user: &str, address: &str, at: i64| NodeConnection {
        user_id: user.into(),
        address: address.into(),
        tls_cert: "-----BEGIN CERTIFICATE-----\nabc\n-----END CERTIFICATE-----".into(),
        macaroon_enc: vec![1, 2, 3, 4],
        updated_at: at,
    };
    assert_eq!(store.node_connection("alice").await.unwrap(), None);
    store.put_node_connection(conn("alice", "https://alice.example:10009", 20)).await.unwrap();
    store.put_node_connection(conn("root", "https://root.example:10009", 21)).await.unwrap();
    assert_eq!(store.node_connection("alice").await.unwrap(), Some(conn("alice", "https://alice.example:10009", 20)));
    store.put_node_connection(conn("alice", "https://alice2.example:10009", 30)).await.unwrap();
    let saved = store.node_connection("alice").await.unwrap().unwrap();
    assert_eq!((saved.address.as_str(), saved.updated_at), ("https://alice2.example:10009", 30), "replaced");
    assert!(store.node_connection("root").await.unwrap().is_some(), "other users are untouched");
    assert!(store.delete_node_connection("alice").await.unwrap());
    assert!(!store.delete_node_connection("alice").await.unwrap(), "nothing left to delete");
    assert_eq!(store.node_connection("alice").await.unwrap(), None);
}
