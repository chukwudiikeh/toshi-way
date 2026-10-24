//! Finds the Lightning node that serves each seller.
//!
//! A seller who has connected their own node gets escrows on it, so a release pays them directly.
//! Otherwise, if the operator runs a shared node and allows it, escrows use that one instead (the
//! operator then receives the sats and owes them to the seller, so it is labelled custodial in
//! the API). Connections are made on first use, cached, and replaced when the seller changes them.

use std::{collections::HashMap, time::Duration};

use async_trait::async_trait;
use escrow_core::{
    adapter::{AdapterError, LightningAdapter, NodeConnectError, NodeCredentials, NodeDirectory, NodeSummary},
    model::{NodeConnection, NodeKind},
    store::{StoreError, UserStore},
};
use tokio::sync::Mutex;

use crate::{creds::CredentialKey, LndAdapter};

pub struct LndDirectory<U> {
    shared: Option<LndAdapter>,
    users: U,
    /// `None` disables sellers' own nodes: without a key their macaroons could not be stored safely.
    key: Option<CredentialKey>,
    timeout: Duration,
    /// Seller id -> (the connection's `updated_at` it was built from, the live client).
    cache: Mutex<HashMap<String, (i64, LndAdapter)>>,
    /// Allow node addresses on private or local networks. Only for a local regtest setup.
    allow_private_addresses: bool,
}

impl<U: UserStore> LndDirectory<U> {
    /// `shared` is the service's own node, or `None` to require every seller to bring their own.
    pub fn new(shared: Option<LndAdapter>, users: U, key: Option<CredentialKey>, timeout: Duration) -> Self {
        Self { shared, users, key, timeout, cache: Mutex::new(HashMap::new()), allow_private_addresses: false }
    }

    /// Permit node addresses on private networks (loopback, 10.x, ...). Needed to point sellers at
    /// the local regtest nodes; never enable it on a public deployment.
    pub fn allow_private_addresses(mut self, allow: bool) -> Self {
        self.allow_private_addresses = allow;
        self
    }

    pub fn has_shared_node(&self) -> bool {
        self.shared.is_some()
    }

    /// Drop a cached connection, for when a seller replaces or removes their node.
    pub async fn forget(&self, seller_id: &str) {
        self.cache.lock().await.remove(seller_id);
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    async fn own_node(&self, seller_id: &str) -> Result<LndAdapter, AdapterError> {
        let conn = self
            .users
            .node_connection(seller_id)
            .await
            .map_err(|e: StoreError| AdapterError::Unavailable(format!("could not load the node connection: {e}")))?
            .ok_or_else(|| AdapterError::NoNode("this seller has no node connected".into()))?;

        let mut cache = self.cache.lock().await;
        if let Some((version, node)) = cache.get(seller_id) {
            if *version == conn.updated_at {
                return Ok(node.clone());
            }
        }
        let macaroon = self
            .key
            .as_ref()
            .and_then(|k| k.open(&conn.macaroon_enc))
            .ok_or_else(|| AdapterError::Unavailable("the stored node credentials cannot be decrypted".into()))?;
        let node = LndAdapter::connect_from_memory(&conn.address, &conn.tls_cert, &macaroon, self.timeout).await?;
        cache.insert(seller_id.to_string(), (conn.updated_at, node.clone()));
        Ok(node)
    }
}

#[async_trait]
impl<U: UserStore> NodeDirectory for LndDirectory<U> {
    type Node = LndAdapter;

    async fn kind_for_new(&self, seller_id: &str) -> Result<NodeKind, AdapterError> {
        let has_own = self
            .users
            .node_connection(seller_id)
            .await
            .map_err(|e| AdapterError::Unavailable(format!("could not load the node connection: {e}")))?
            .is_some();
        if has_own {
            Ok(NodeKind::Own)
        } else if self.shared.is_some() {
            Ok(NodeKind::Shared)
        } else {
            Err(AdapterError::NoNode("connect your Lightning node before creating an escrow".into()))
        }
    }

    async fn node(&self, seller_id: &str, kind: NodeKind) -> Result<LndAdapter, AdapterError> {
        match kind {
            NodeKind::Own => self.own_node(seller_id).await,
            NodeKind::Shared => self
                .shared
                .clone()
                .ok_or_else(|| AdapterError::NoNode("the service's shared node is not configured".into())),
        }
    }

    /// Health of what the operator runs: the shared node, if there is one.
    async fn ping(&self) -> Result<(), AdapterError> {
        match &self.shared {
            Some(n) => LightningAdapter::ping(n).await,
            None => Ok(()),
        }
    }

    fn offers_shared_node(&self) -> bool {
        self.shared.is_some()
    }

    fn accepts_seller_nodes(&self) -> bool {
        self.key.is_some()
    }

    async fn register_seller_node(
        &self,
        seller_id: &str,
        creds: NodeCredentials,
        now: i64,
    ) -> Result<NodeSummary, NodeConnectError> {
        let key = self.key.as_ref().ok_or_else(|| {
            NodeConnectError::Invalid("sellers' own nodes are not enabled on this server".into())
        })?;
        let address = crate::address::check(&creds.address, self.allow_private_addresses)
            .await
            .map_err(NodeConnectError::Invalid)?;
        let cert = creds.tls_cert.trim();
        if !cert.starts_with("-----BEGIN CERTIFICATE-----") || cert.len() > 16 * 1024 {
            return Err(NodeConnectError::Invalid("the TLS certificate must be PEM text (-----BEGIN CERTIFICATE-----)".into()));
        }
        let macaroon = hex::decode(creds.macaroon_hex.trim())
            .ok()
            .filter(|m| !m.is_empty() && m.len() <= 4096)
            .ok_or_else(|| NodeConnectError::Invalid("the macaroon must be hex (for example from `xxd -p -c 1000 file`)".into()))?;

        // Prove it works before saving anything: connect, check sync, make and cancel a probe invoice.
        let normalized = format!("https://{}:{}", address.host, address.port);
        let node = LndAdapter::connect_from_memory(&normalized, cert, &macaroon, self.timeout)
            .await
            .map_err(|e| NodeConnectError::CheckFailed(format!("could not connect to that node: {e}")))?;
        node.check_invoice_access().await.map_err(|e| match e {
            AdapterError::Rejected(msg) => NodeConnectError::CheckFailed(format!(
                "connected, but the node refused: {msg}. The macaroon needs invoices:read, invoices:write and info:read."
            )),
            other => NodeConnectError::CheckFailed(format!("the node did not pass the check: {other}")),
        })?;

        self.users
            .put_node_connection(NodeConnection {
                user_id: seller_id.to_string(),
                address: normalized.clone(),
                tls_cert: cert.to_string(),
                macaroon_enc: key.seal(&macaroon),
                updated_at: now,
            })
            .await
            .map_err(|e| AdapterError::Unavailable(format!("could not save the node connection: {e}")))?;
        self.cache.lock().await.insert(seller_id.to_string(), (now, node));
        Ok(NodeSummary { address: normalized, updated_at: now })
    }

    async fn remove_seller_node(&self, seller_id: &str) -> Result<bool, AdapterError> {
        let removed = self
            .users
            .delete_node_connection(seller_id)
            .await
            .map_err(|e| AdapterError::Unavailable(format!("could not remove the node connection: {e}")))?;
        self.forget(seller_id).await;
        Ok(removed)
    }

    async fn seller_node_summary(&self, seller_id: &str) -> Result<Option<NodeSummary>, AdapterError> {
        Ok(self
            .users
            .node_connection(seller_id)
            .await
            .map_err(|e| AdapterError::Unavailable(format!("could not load the node connection: {e}")))?
            .map(|c| NodeSummary { address: c.address, updated_at: c.updated_at }))
    }
}
