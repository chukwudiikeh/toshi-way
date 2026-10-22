//! Persistence boundary. State changes are compare-and-set so concurrent actors (a node event
//! and the scheduler, say) cannot both win, and each change is written together with its audit entry.

use std::sync::Mutex;

use async_trait::async_trait;
use thiserror::Error;

use crate::{
    model::{AuditEntry, Escrow, NodeEventRecord, PaymentHash, User},
    state::EscrowState,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("escrow not found")]
    NotFound,
    #[error("escrow was changed concurrently")]
    Conflict,
    #[error("storage failure: {0}")]
    Backend(String),
}

/// What a transition changes besides the state itself.
#[derive(Debug, Clone, Default)]
pub struct EscrowUpdate {
    pub payment_hash: Option<PaymentHash>,
    pub invoice: Option<String>,
    pub invoice_expires_at: Option<i64>,
}

#[async_trait]
pub trait Store: Send + Sync {
    async fn insert(&self, escrow: Escrow) -> Result<(), StoreError>;
    async fn get(&self, id: &str) -> Result<Option<Escrow>, StoreError>;
    async fn get_by_hash(&self, hash: &PaymentHash) -> Result<Option<Escrow>, StoreError>;
    /// Escrows that are not in a terminal state.
    async fn list_active(&self) -> Result<Vec<Escrow>, StoreError>;
    /// Escrows where `user_id` is the buyer or the seller, newest first (ties by id), at most 100.
    async fn list_for_user(&self, user_id: &str) -> Result<Vec<Escrow>, StoreError>;
    /// Atomically move `id` from `expected` to `audit.to_state`, apply `update`, and append
    /// `audit`. Fails with `Conflict` if the escrow is no longer in `expected`.
    async fn transition(
        &self,
        id: &str,
        expected: EscrowState,
        update: EscrowUpdate,
        audit: AuditEntry,
    ) -> Result<Escrow, StoreError>;
    async fn audit_log(&self, id: &str) -> Result<Vec<AuditEntry>, StoreError>;
    async fn record_node_event(&self, event: NodeEventRecord) -> Result<(), StoreError>;
    async fn node_events(&self, id: &str) -> Result<Vec<NodeEventRecord>, StoreError>;
}

/// Accounts and their API tokens. Only a hash of the token is ever stored.
#[async_trait]
pub trait UserStore: Send + Sync {
    /// `Conflict` if the id or the token hash already exists.
    async fn insert_user(&self, user: User, token_hash: String) -> Result<(), StoreError>;
    async fn user_by_token_hash(&self, token_hash: &str) -> Result<Option<User>, StoreError>;
    async fn get_user(&self, id: &str) -> Result<Option<User>, StoreError>;
}

#[derive(Default)]
pub struct MemoryStore {
    inner: Mutex<MemoryInner>,
}

#[derive(Default)]
struct MemoryInner {
    escrows: Vec<Escrow>,
    audit: Vec<AuditEntry>,
    node_events: Vec<NodeEventRecord>,
    users: Vec<(User, String)>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Store for MemoryStore {
    async fn insert(&self, escrow: Escrow) -> Result<(), StoreError> {
        let mut g = self.inner.lock().unwrap();
        if g.escrows.iter().any(|e| e.id == escrow.id) {
            return Err(StoreError::Conflict);
        }
        g.escrows.push(escrow);
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<Escrow>, StoreError> {
        Ok(self.inner.lock().unwrap().escrows.iter().find(|e| e.id == id).cloned())
    }

    async fn get_by_hash(&self, hash: &PaymentHash) -> Result<Option<Escrow>, StoreError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .escrows
            .iter()
            .find(|e| e.payment_hash.as_ref() == Some(hash))
            .cloned())
    }

    async fn list_active(&self) -> Result<Vec<Escrow>, StoreError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .escrows
            .iter()
            .filter(|e| !e.state.is_terminal())
            .cloned()
            .collect())
    }

    async fn list_for_user(&self, user_id: &str) -> Result<Vec<Escrow>, StoreError> {
        let mut list: Vec<Escrow> = self
            .inner
            .lock()
            .unwrap()
            .escrows
            .iter()
            .filter(|e| e.buyer_id == user_id || e.seller_id == user_id)
            .cloned()
            .collect();
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at).then_with(|| a.id.cmp(&b.id)));
        list.truncate(100);
        Ok(list)
    }

    async fn transition(
        &self,
        id: &str,
        expected: EscrowState,
        update: EscrowUpdate,
        audit: AuditEntry,
    ) -> Result<Escrow, StoreError> {
        let mut g = self.inner.lock().unwrap();
        let e = g.escrows.iter_mut().find(|e| e.id == id).ok_or(StoreError::NotFound)?;
        if e.state != expected {
            return Err(StoreError::Conflict);
        }
        e.state = audit.to_state;
        e.updated_at = audit.created_at;
        if update.payment_hash.is_some() {
            e.payment_hash = update.payment_hash;
        }
        if update.invoice.is_some() {
            e.invoice = update.invoice;
        }
        if update.invoice_expires_at.is_some() {
            e.invoice_expires_at = update.invoice_expires_at;
        }
        let updated = e.clone();
        g.audit.push(audit);
        Ok(updated)
    }

    async fn audit_log(&self, id: &str) -> Result<Vec<AuditEntry>, StoreError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .audit
            .iter()
            .filter(|a| a.escrow_id == id)
            .cloned()
            .collect())
    }

    async fn record_node_event(&self, event: NodeEventRecord) -> Result<(), StoreError> {
        self.inner.lock().unwrap().node_events.push(event);
        Ok(())
    }

    async fn node_events(&self, id: &str) -> Result<Vec<NodeEventRecord>, StoreError> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .node_events
            .iter()
            .filter(|n| n.escrow_id == id)
            .cloned()
            .collect())
    }
}

/// Lets a service and a "restarted" service share one store.
#[async_trait]
impl<S: Store + ?Sized> Store for std::sync::Arc<S> {
    async fn insert(&self, escrow: Escrow) -> Result<(), StoreError> {
        (**self).insert(escrow).await
    }
    async fn get(&self, id: &str) -> Result<Option<Escrow>, StoreError> {
        (**self).get(id).await
    }
    async fn get_by_hash(&self, hash: &PaymentHash) -> Result<Option<Escrow>, StoreError> {
        (**self).get_by_hash(hash).await
    }
    async fn list_active(&self) -> Result<Vec<Escrow>, StoreError> {
        (**self).list_active().await
    }
    async fn list_for_user(&self, user_id: &str) -> Result<Vec<Escrow>, StoreError> {
        (**self).list_for_user(user_id).await
    }
    async fn transition(
        &self,
        id: &str,
        expected: EscrowState,
        update: EscrowUpdate,
        audit: AuditEntry,
    ) -> Result<Escrow, StoreError> {
        (**self).transition(id, expected, update, audit).await
    }
    async fn audit_log(&self, id: &str) -> Result<Vec<AuditEntry>, StoreError> {
        (**self).audit_log(id).await
    }
    async fn record_node_event(&self, event: NodeEventRecord) -> Result<(), StoreError> {
        (**self).record_node_event(event).await
    }
    async fn node_events(&self, id: &str) -> Result<Vec<NodeEventRecord>, StoreError> {
        (**self).node_events(id).await
    }
}

#[async_trait]
impl UserStore for MemoryStore {
    async fn insert_user(&self, user: User, token_hash: String) -> Result<(), StoreError> {
        let mut g = self.inner.lock().unwrap();
        if g.users.iter().any(|(u, h)| u.id == user.id || *h == token_hash) {
            return Err(StoreError::Conflict);
        }
        g.users.push((user, token_hash));
        Ok(())
    }

    async fn user_by_token_hash(&self, token_hash: &str) -> Result<Option<User>, StoreError> {
        Ok(self.inner.lock().unwrap().users.iter().find(|(_, h)| h == token_hash).map(|(u, _)| u.clone()))
    }

    async fn get_user(&self, id: &str) -> Result<Option<User>, StoreError> {
        Ok(self.inner.lock().unwrap().users.iter().find(|(u, _)| u.id == id).map(|(u, _)| u.clone()))
    }
}

#[async_trait]
impl<S: UserStore + ?Sized> UserStore for std::sync::Arc<S> {
    async fn insert_user(&self, user: User, token_hash: String) -> Result<(), StoreError> {
        (**self).insert_user(user, token_hash).await
    }
    async fn user_by_token_hash(&self, token_hash: &str) -> Result<Option<User>, StoreError> {
        (**self).user_by_token_hash(token_hash).await
    }
    async fn get_user(&self, id: &str) -> Result<Option<User>, StoreError> {
        (**self).get_user(id).await
    }
}
