//! The escrow service. Payment state comes only from the node: every move to LOCKED, SETTLED,
//! REFUNDED or EXPIRED_UNPAID is driven by a node event or a node lookup, never by a caller's claim.

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{
    adapter::{AdapterError, HoldInvoiceRequest, LightningAdapter},
    model::{
        AuditEntry, Escrow, InvoiceEvent, NodeEventRecord, NodeInvoiceState, PaymentHash, Preimage,
    },
    state::EscrowState,
    store::{EscrowUpdate, Store, StoreError},
    timing::{TimingConfig, TimingError},
};

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("escrow not found")]
    NotFound,
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error(transparent)]
    Timing(#[from] TimingError),
    #[error("cannot {action} an escrow in state {state}")]
    InvalidState { action: &'static str, state: EscrowState },
    #[error("preimage does not match the payment hash")]
    WrongPreimage,
    #[error("the settle-by deadline has passed")]
    TooLate,
    #[error("payment hash is already used by another escrow")]
    HashInUse,
    #[error("node error: {0}")]
    Adapter(#[from] AdapterError),
    #[error("store error: {0}")]
    Store(StoreError),
}

impl From<StoreError> for ServiceError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::NotFound => ServiceError::NotFound,
            other => ServiceError::Store(other),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewEscrow {
    pub buyer_id: String,
    pub seller_id: String,
    pub amount_sats: u64,
    pub description: String,
    pub deadline_at: i64,
}

#[derive(Debug, Default)]
pub struct TickReport {
    pub cancelled: Vec<String>,
    pub errors: Vec<(String, String)>,
}

#[derive(Debug, Default)]
pub struct ReconcileReport {
    pub checked: usize,
    pub changed: Vec<String>,
    pub errors: Vec<(String, String)>,
}

pub struct EscrowService<A, S> {
    adapter: A,
    store: S,
    timing: TimingConfig,
}

/// What a node's invoice state means for an escrow currently in `state`.
/// `Ok(None)` = nothing to do; `Err(())` = the combination should be impossible.
fn target_state(state: EscrowState, node: NodeInvoiceState) -> Result<Option<EscrowState>, ()> {
    use EscrowState as E;
    use NodeInvoiceState as N;
    match (state, node) {
        (E::InvoiceIssued, N::Accepted) => Ok(Some(E::Locked)),
        (E::InvoiceIssued, N::Canceled) => Ok(Some(E::ExpiredUnpaid)),
        (E::Locked | E::Disputed, N::Settled) => Ok(Some(E::Settled)),
        (E::Locked | E::Disputed, N::Canceled) => Ok(Some(E::Refunded)),
        (E::InvoiceIssued, N::Open) | (E::Locked | E::Disputed, N::Accepted) => Ok(None),
        (E::InvoiceIssued, N::Settled) => Err(()),
        (E::Locked | E::Disputed, N::Open) => Err(()),
        // Created has no invoice; terminal states never move again.
        _ => Ok(None),
    }
}

impl<A: LightningAdapter, S: Store> EscrowService<A, S> {
    pub fn new(adapter: A, store: S, timing: TimingConfig) -> Self {
        Self { adapter, store, timing }
    }

    pub async fn get(&self, id: &str) -> Result<Escrow, ServiceError> {
        self.store.get(id).await?.ok_or(ServiceError::NotFound)
    }

    pub async fn list_active(&self) -> Result<Vec<Escrow>, ServiceError> {
        Ok(self.store.list_active().await?)
    }

    pub async fn list_for_user(&self, user_id: &str) -> Result<Vec<Escrow>, ServiceError> {
        Ok(self.store.list_for_user(user_id).await?)
    }

    pub async fn node_events(&self, id: &str) -> Result<Vec<NodeEventRecord>, ServiceError> {
        Ok(self.store.node_events(id).await?)
    }

    pub async fn node_health(&self) -> Result<(), ServiceError> {
        Ok(self.adapter.ping().await?)
    }

    /// Payment hashes of escrows whose invoice the node may still change.
    pub async fn active_payment_hashes(&self) -> Result<Vec<PaymentHash>, ServiceError> {
        Ok(self.store.list_active().await?.into_iter().filter_map(|e| e.payment_hash).collect())
    }

    pub async fn audit_log(&self, id: &str) -> Result<Vec<AuditEntry>, ServiceError> {
        Ok(self.store.audit_log(id).await?)
    }

    pub async fn create(&self, new: NewEscrow, now: i64) -> Result<Escrow, ServiceError> {
        if new.amount_sats == 0 {
            return Err(ServiceError::Invalid("amount must be greater than zero".into()));
        }
        if new.buyer_id == new.seller_id {
            return Err(ServiceError::Invalid("buyer and seller must differ".into()));
        }
        self.timing.validate_deadline(now, new.deadline_at)?;
        let escrow = Escrow {
            id: uuid::Uuid::new_v4().to_string(),
            buyer_id: new.buyer_id,
            seller_id: new.seller_id,
            amount_sats: new.amount_sats,
            description: new.description,
            payment_hash: None,
            invoice: None,
            invoice_expires_at: None,
            state: EscrowState::Created,
            deadline_at: new.deadline_at,
            created_at: now,
            updated_at: now,
        };
        self.store.insert(escrow.clone()).await?;
        Ok(escrow)
    }

    /// The buyer accepts and supplies `H`; the seller's node issues the hold invoice.
    /// Repeating the call with the same hash returns the same escrow.
    pub async fn accept(
        &self,
        id: &str,
        hash: PaymentHash,
        actor: &str,
        now: i64,
    ) -> Result<Escrow, ServiceError> {
        let e = self.get(id).await?;
        match e.state {
            EscrowState::InvoiceIssued if e.payment_hash == Some(hash) => return Ok(e),
            EscrowState::Created => {}
            state => return Err(ServiceError::InvalidState { action: "accept", state }),
        }
        if let Some(other) = self.store.get_by_hash(&hash).await? {
            if other.id != e.id {
                return Err(ServiceError::HashInUse);
            }
        }
        self.timing.validate_deadline(now, e.deadline_at)?;

        let expiry_secs = self.timing.invoice_expiry_secs.min(e.deadline_at - now);
        let invoice = self
            .adapter
            .create_hold_invoice(HoldInvoiceRequest {
                payment_hash: hash,
                amount_sats: e.amount_sats,
                memo: format!("escrow {}", e.id),
                expiry_secs,
                final_cltv_delta: self.timing.final_cltv_delta(now, e.deadline_at),
            })
            .await?;
        let update = EscrowUpdate {
            payment_hash: Some(hash),
            invoice: Some(invoice),
            invoice_expires_at: Some(now + expiry_secs),
        };
        self.move_to(&e, EscrowState::InvoiceIssued, update, actor, "accept", "hold invoice issued", now)
            .await
    }

    /// Apply an event from the node's invoice subscription. Unknown invoices are ignored.
    pub async fn apply_event(&self, ev: InvoiceEvent, now: i64) -> Result<Option<Escrow>, ServiceError> {
        let Some(e) = self.store.get_by_hash(&ev.payment_hash).await? else {
            return Ok(None);
        };
        let raw = format!("{}:{}", hex::encode(ev.payment_hash), ev.state.as_str());
        self.store
            .record_node_event(NodeEventRecord {
                escrow_id: e.id.clone(),
                node_invoice_state: ev.state,
                raw_event_hash: hex::encode(Sha256::digest(raw)),
                received_at: now,
            })
            .await?;
        Ok(Some(self.apply_node_state(e, ev.state, "node", "node invoice event", now).await?))
    }

    /// Submit the preimage. Verified against `H` before the node is touched; state changes only
    /// when the node reports the invoice settled.
    pub async fn release(
        &self,
        id: &str,
        preimage: &Preimage,
        actor: &str,
        now: i64,
    ) -> Result<Escrow, ServiceError> {
        let e = self.get(id).await?;
        let hash = e.payment_hash.ok_or(ServiceError::InvalidState { action: "release", state: e.state })?;
        if preimage.hash() != hash {
            return Err(ServiceError::WrongPreimage);
        }
        match e.state {
            EscrowState::Settled => return Ok(e),
            EscrowState::Locked | EscrowState::Disputed => {}
            state => return Err(ServiceError::InvalidState { action: "release", state }),
        }
        if now >= e.deadline_at {
            return Err(ServiceError::TooLate);
        }
        self.adapter.settle_invoice(preimage).await?;
        self.sync_from_node(e, actor, "release", now).await
    }

    /// Cancel the hold invoice. Repeating it on an already cancelled escrow is a no-op.
    pub async fn cancel(&self, id: &str, actor: &str, reason: &str, now: i64) -> Result<Escrow, ServiceError> {
        let e = self.get(id).await?;
        let hash = match (e.state, e.payment_hash) {
            (EscrowState::Refunded | EscrowState::ExpiredUnpaid, _) => return Ok(e),
            (EscrowState::InvoiceIssued | EscrowState::Locked | EscrowState::Disputed, Some(h)) => h,
            (state, _) => return Err(ServiceError::InvalidState { action: "cancel", state }),
        };
        self.adapter.cancel_invoice(&hash).await?;
        self.sync_from_node(e, actor, reason, now).await
    }

    /// Flag a dispute. Does not extend the hold: the deadline still applies.
    pub async fn dispute(&self, id: &str, actor: &str, reason: &str, now: i64) -> Result<Escrow, ServiceError> {
        let e = self.get(id).await?;
        match e.state {
            EscrowState::Disputed => return Ok(e),
            EscrowState::Locked => {}
            state => return Err(ServiceError::InvalidState { action: "dispute", state }),
        }
        if now >= e.deadline_at {
            return Err(ServiceError::TooLate);
        }
        self.move_to(&e, EscrowState::Disputed, EscrowUpdate::default(), actor, "dispute", reason, now)
            .await
    }

    /// The scheduler pass: cancel holds at their deadline and unpaid invoices once they expire.
    pub async fn tick(&self, now: i64) -> Result<TickReport, ServiceError> {
        let mut report = TickReport::default();
        for e in self.store.list_active().await? {
            let due = match e.state {
                EscrowState::Locked | EscrowState::Disputed => now >= e.deadline_at,
                EscrowState::InvoiceIssued => e.invoice_expires_at.is_some_and(|t| now >= t),
                _ => false,
            };
            if !due {
                continue;
            }
            match self.cancel(&e.id, "scheduler", "deadline reached", now).await {
                Ok(_) => report.cancelled.push(e.id),
                Err(err) => report.errors.push((e.id, err.to_string())),
            }
        }
        Ok(report)
    }

    /// Startup recovery: compare every open escrow with the node and adopt the node's view.
    pub async fn reconcile(&self, now: i64) -> Result<ReconcileReport, ServiceError> {
        let mut report = ReconcileReport::default();
        for e in self.store.list_active().await? {
            let Some(hash) = e.payment_hash else { continue };
            report.checked += 1;
            let before = e.state;
            let result = match self.adapter.lookup_invoice(&hash).await {
                Ok(Some(inv)) => self.apply_node_state(e.clone(), inv.state, "reconcile", "startup reconciliation", now).await,
                Ok(None) => {
                    self.move_to(&e, EscrowState::Failed, EscrowUpdate::default(), "reconcile", "reconcile", "node has no such invoice", now)
                        .await
                }
                Err(err) => Err(err.into()),
            };
            match result {
                Ok(after) if after.state != before => report.changed.push(after.id),
                Ok(_) => {}
                Err(err) => report.errors.push((e.id, err.to_string())),
            }
        }
        Ok(report)
    }

    async fn sync_from_node(
        &self,
        e: Escrow,
        actor: &str,
        reason: &str,
        now: i64,
    ) -> Result<Escrow, ServiceError> {
        let hash = e.payment_hash.expect("escrows with an invoice have a hash");
        match self.adapter.lookup_invoice(&hash).await? {
            Some(inv) => self.apply_node_state(e, inv.state, actor, reason, now).await,
            None => Ok(e),
        }
    }

    /// Move the escrow to whatever the node's invoice state implies, retrying if a concurrent
    /// actor got there first.
    async fn apply_node_state(
        &self,
        mut e: Escrow,
        node: NodeInvoiceState,
        actor: &str,
        reason: &str,
        now: i64,
    ) -> Result<Escrow, ServiceError> {
        for _ in 0..3 {
            let (to, why) = match target_state(e.state, node) {
                Ok(None) => return Ok(e),
                Ok(Some(to)) => (to, reason.to_string()),
                Err(()) => (EscrowState::Failed, format!("node reports {node:?} while escrow is {}", e.state)),
            };
            match self.move_to(&e, to, EscrowUpdate::default(), actor, "node_state", &why, now).await {
                Err(ServiceError::Store(StoreError::Conflict)) => e = self.get(&e.id).await?,
                other => return other,
            }
        }
        Err(ServiceError::Store(StoreError::Conflict))
    }

    #[allow(clippy::too_many_arguments)]
    async fn move_to(
        &self,
        e: &Escrow,
        to: EscrowState,
        update: EscrowUpdate,
        actor: &str,
        action: &str,
        reason: &str,
        now: i64,
    ) -> Result<Escrow, ServiceError> {
        if !e.state.can_transition_to(to) {
            return Err(ServiceError::InvalidState { action: "transition", state: e.state });
        }
        let audit = AuditEntry {
            escrow_id: e.id.clone(),
            actor: actor.to_string(),
            action: action.to_string(),
            from_state: e.state,
            to_state: to,
            reason: reason.to_string(),
            created_at: now,
        };
        Ok(self.store.transition(&e.id, e.state, update, audit).await?)
    }
}
