//! Core data types.

use std::fmt;

use sha2::{Digest, Sha256};

use crate::state::EscrowState;

pub type PaymentHash = [u8; 32];

/// The release secret. Never stored and never logged: `Debug` is redacted and there is no
/// `Display`, so it cannot end up in a log line by accident.
pub struct Preimage([u8; 32]);

impl Preimage {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Parse the 64-hex-character form. Anything that is not exactly 32 bytes is rejected,
    /// so short codes can never be accepted as a release secret.
    pub fn from_hex(s: &str) -> Option<Self> {
        let bytes = hex::decode(s).ok()?;
        Some(Self(bytes.try_into().ok()?))
    }

    pub fn hash(&self) -> PaymentHash {
        Sha256::digest(self.0).into()
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Debug for Preimage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Preimage(<redacted>)")
    }
}

/// Whose Lightning node holds an escrow's invoice, and therefore receives the money on release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeKind {
    /// The seller's own node: on release the sats go straight to the seller. Non-custodial.
    Own,
    /// The service's shared node: the operator receives the sats and owes them to the seller.
    /// Custodial after release; offered only when the operator enables it.
    Shared,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::Shared => "shared",
        }
    }
}

impl std::str::FromStr for NodeKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "own" => Ok(Self::Own),
            "shared" => Ok(Self::Shared),
            other => Err(format!("unknown node kind '{other}'")),
        }
    }
}

/// How to reach a seller's own Lightning node. The macaroon is stored encrypted; the service
/// that encrypts it is the only one that can read it back.
#[derive(Clone, PartialEq, Eq)]
pub struct NodeConnection {
    pub user_id: String,
    /// `https://host:port` of the node's gRPC interface.
    pub address: String,
    /// The node's TLS certificate (PEM). It is public, and the connection is pinned to it.
    pub tls_cert: String,
    /// Nonce followed by ciphertext of the invoice macaroon.
    pub macaroon_enc: Vec<u8>,
    pub updated_at: i64,
}

impl fmt::Debug for NodeConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeConnection")
            .field("user_id", &self.user_id)
            .field("address", &self.address)
            .field("macaroon_enc", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Escrow {
    pub id: String,
    /// `None` for an open offer that nobody has accepted yet; accepting it sets the buyer.
    pub buyer_id: Option<String>,
    pub seller_id: String,
    /// A trusted third party both sides agreed on, who decides if there is a dispute.
    pub arbiter_id: Option<String>,
    /// The arbiter's public key (base64 SPKI) as it was when the escrow was created. Recorded
    /// here so the key the buyer encrypted to cannot be swapped afterwards.
    pub arbiter_key: Option<String>,
    /// The buyer's release secret encrypted to `arbiter_key`. Opaque to the server, which cannot
    /// read it; only the arbiter can. Present once the buyer has accepted.
    pub arbiter_sealed: Option<String>,
    /// When the arbiter confirmed they can decrypt `arbiter_sealed` and that it matches the
    /// payment hash. Until then the seller should not count on arbitration.
    pub arbiter_confirmed_at: Option<i64>,
    /// Which node holds this escrow's invoice. Fixed at creation, so changing the seller's node
    /// settings later cannot strand an escrow that is in flight.
    pub node: NodeKind,
    pub amount_sats: u64,
    pub description: String,
    pub payment_hash: Option<PaymentHash>,
    /// The BOLT11 string the buyer pays. Not for ordinary logs or public responses.
    pub invoice: Option<String>,
    pub invoice_expires_at: Option<i64>,
    pub state: EscrowState,
    /// Settle-by time (unix seconds). Releases are accepted before it; the scheduler cancels at it.
    pub deadline_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub escrow_id: String,
    pub actor: String,
    pub action: String,
    pub from_state: EscrowState,
    pub to_state: EscrowState,
    pub reason: String,
    pub created_at: i64,
}

/// State of a hold invoice as reported by the node. The node is authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeInvoiceState {
    Open,
    Accepted,
    Settled,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeInvoice {
    pub state: NodeInvoiceState,
    /// Block height at which the held HTLC expires, once a payment is held.
    pub htlc_expiry_height: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Operator,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Operator => "operator",
        }
    }
}

impl std::str::FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "user" => Ok(Self::User),
            "operator" => Ok(Self::Operator),
            other => Err(format!("unknown role '{other}'")),
        }
    }
}

/// Whether someone is the buyer or seller of an escrow is per escrow, not part of the account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub role: Role,
    pub created_at: i64,
}

/// A node report about an escrow's invoice, kept for the audit trail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeEventRecord {
    pub escrow_id: String,
    pub node_invoice_state: NodeInvoiceState,
    /// Hash of the raw event, so repeats can be recognised without storing the payload.
    pub raw_event_hash: String,
    pub received_at: i64,
}

impl NodeInvoiceState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "OPEN",
            Self::Accepted => "ACCEPTED",
            Self::Settled => "SETTLED",
            Self::Canceled => "CANCELED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvoiceEvent {
    pub payment_hash: PaymentHash,
    pub state: NodeInvoiceState,
}
