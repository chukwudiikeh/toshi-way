//! The only surface through which the core talks to a Lightning node.

use async_trait::async_trait;
use thiserror::Error;

use crate::model::{NodeInvoice, PaymentHash, Preimage};

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("node unreachable: {0}")]
    Unavailable(String),
    #[error("node rejected the request: {0}")]
    Rejected(String),
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
