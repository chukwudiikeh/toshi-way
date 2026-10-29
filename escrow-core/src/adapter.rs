//! The only surface through which the core talks to a Lightning node.

use async_trait::async_trait;
use thiserror::Error;

use crate::model::{NodeInvoice, NodeKind, PaymentHash, Preimage};

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("node unreachable: {0}")]
    Unavailable(String),
    #[error("node rejected the request: {0}")]
    Rejected(String),
    /// The seller has no Lightning node connected and the service offers no shared one.
    #[error("no Lightning node is available for this seller: {0}")]
    NoNode(String),
}

#[derive(Debug, Clone)]
pub struct HoldInvoiceRequest {
    pub payment_hash: PaymentHash,
    pub amount_sats: u64,
    pub memo: String,
    /// How long the invoice can be paid before it expires.
    pub expiry_secs: i64,
    pub final_cltv_delta: u32,
}

/// Every call must carry its own timeout and surface failure explicitly. Implementations must
/// make `settle_invoice` and `cancel_invoice` idempotent.
#[async_trait]
pub trait LightningAdapter: Send + Sync {
    /// Returns the BOLT11 invoice to pay.
    async fn create_hold_invoice(&self, req: HoldInvoiceRequest) -> Result<String, AdapterError>;
    async fn settle_invoice(&self, preimage: &Preimage) -> Result<(), AdapterError>;
    async fn cancel_invoice(&self, payment_hash: &PaymentHash) -> Result<(), AdapterError>;
    /// Cheap reachability check for health reporting.
    async fn ping(&self) -> Result<(), AdapterError>;
    /// `None` if the node has no such invoice.
    async fn lookup_invoice(&self, payment_hash: &PaymentHash)
        -> Result<Option<NodeInvoice>, AdapterError>;
}

/// What a seller supplies to connect their own node.
#[derive(Clone)]
pub struct NodeCredentials {
    /// `https://host:port` of the node's gRPC interface.
    pub address: String,
    /// The node's TLS certificate, PEM.
    pub tls_cert: String,
    /// An invoice-only macaroon, as hex.
    pub macaroon_hex: String,
}

impl std::fmt::Debug for NodeCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeCredentials").field("address", &self.address).field("macaroon_hex", &"<redacted>").finish()
    }
}

/// A connected node as the seller may see it. Never contains the macaroon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSummary {
    pub address: String,
    pub updated_at: i64,
}

#[derive(Debug, Error)]
pub enum NodeConnectError {
    /// The input is not acceptable (malformed, or an address we refuse to connect to).
    #[error("{0}")]
    Invalid(String),
    /// The input looks right but the node would not do what escrows need.
    #[error("{0}")]
    CheckFailed(String),
    #[error(transparent)]
    Adapter(#[from] AdapterError),
}

/// Finds the node that serves a seller. The service asks this for every node call, so each seller's
/// escrows live on that seller's own node (or the service's shared node, if the operator allows it).
#[async_trait]
pub trait NodeDirectory: Send + Sync {
    type Node: LightningAdapter + Clone + 'static;

    /// The kind of node a new escrow for `seller_id` would use, or `NoNode` if there is none.
    async fn kind_for_new(&self, seller_id: &str) -> Result<NodeKind, AdapterError>;
    /// The node for an existing escrow of `seller_id` of the given kind.
    async fn node(&self, seller_id: &str, kind: NodeKind) -> Result<Self::Node, AdapterError>;
    /// Reachability of the service's own infrastructure, for `/health`. Sellers' nodes are their own.
    async fn ping(&self) -> Result<(), AdapterError>;

    /// Whether sellers may connect their own nodes at all on this server.
    fn accepts_seller_nodes(&self) -> bool {
        false
    }

    /// Whether sellers without a node of their own can still be served, by a shared node.
    fn offers_shared_node(&self) -> bool {
        false
    }

    /// Check `creds` really reach a node that can do escrows, then save them as the seller's node,
    /// replacing any earlier one. Implementations that cannot connect sellers refuse.
    async fn register_seller_node(
        &self,
        _seller_id: &str,
        _creds: NodeCredentials,
        _now: i64,
    ) -> Result<NodeSummary, NodeConnectError> {
        Err(NodeConnectError::Invalid("this service does not take sellers' own nodes".into()))
    }

    /// `true` if the seller had a node to remove.
    async fn remove_seller_node(&self, _seller_id: &str) -> Result<bool, AdapterError> {
        Ok(false)
    }

    async fn seller_node_summary(&self, _seller_id: &str) -> Result<Option<NodeSummary>, AdapterError> {
        Ok(None)
    }
}
