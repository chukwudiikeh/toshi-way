//! In-memory stand-in for a Lightning node, modelled on what LND did in Phase 1:
//! `Open` -> `Accepted` once paid, then `Settled` or `Canceled`.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;

use crate::{
    adapter::{
        AdapterError, HoldInvoiceRequest, LightningAdapter, NodeConnectError, NodeCredentials, NodeDirectory,
        NodeSummary,
    },
    model::{InvoiceEvent, NodeInvoice, NodeInvoiceState, NodeKind, PaymentHash, Preimage},
};

#[derive(Default)]
struct Inner {
    invoices: HashMap<PaymentHash, NodeInvoice>,
    offline: bool,
    settle_calls: u32,
    cancel_calls: u32,
}

#[derive(Clone, Default)]
pub struct MockLightning(Arc<Mutex<Inner>>);

impl MockLightning {
    pub fn new() -> Self {
        Self::default()
    }

    /// Simulate the node being unreachable (or back).
    pub fn set_offline(&self, offline: bool) {
        self.0.lock().unwrap().offline = offline;
    }

    /// The buyer pays: the HTLC arrives and is held.
    pub fn pay(&self, hash: &PaymentHash) -> InvoiceEvent {
        let mut g = self.0.lock().unwrap();
        let inv = g.invoices.get_mut(hash).expect("no such invoice");
        assert_eq!(inv.state, NodeInvoiceState::Open, "invoice is not payable");
        inv.state = NodeInvoiceState::Accepted;
        inv.htlc_expiry_height = Some(1000);
        InvoiceEvent { payment_hash: *hash, state: inv.state }
    }

    /// The invoice expires unpaid.
    pub fn expire_unpaid(&self, hash: &PaymentHash) -> InvoiceEvent {
        let mut g = self.0.lock().unwrap();
        let inv = g.invoices.get_mut(hash).expect("no such invoice");
        assert_eq!(inv.state, NodeInvoiceState::Open);
        inv.state = NodeInvoiceState::Canceled;
        InvoiceEvent { payment_hash: *hash, state: inv.state }
    }

    /// LND's own auto-cancel of a held HTLC close to expiry.
    pub fn auto_cancel(&self, hash: &PaymentHash) -> InvoiceEvent {
        let mut g = self.0.lock().unwrap();
        let inv = g.invoices.get_mut(hash).expect("no such invoice");
        assert_eq!(inv.state, NodeInvoiceState::Accepted);
        inv.state = NodeInvoiceState::Canceled;
        InvoiceEvent { payment_hash: *hash, state: inv.state }
    }

    pub fn state(&self, hash: &PaymentHash) -> Option<NodeInvoiceState> {
        self.0.lock().unwrap().invoices.get(hash).map(|i| i.state)
    }

    pub fn settle_calls(&self) -> u32 {
        self.0.lock().unwrap().settle_calls
    }

    pub fn cancel_calls(&self) -> u32 {
        self.0.lock().unwrap().cancel_calls
    }
}

fn check_online(inner: &Inner) -> Result<(), AdapterError> {
    if inner.offline {
        Err(AdapterError::Unavailable("mock node offline".into()))
    } else {
        Ok(())
    }
}

#[async_trait]
impl LightningAdapter for MockLightning {
    async fn create_hold_invoice(&self, req: HoldInvoiceRequest) -> Result<String, AdapterError> {
        let mut g = self.0.lock().unwrap();
        check_online(&g)?;
        g.invoices
            .entry(req.payment_hash)
            .or_insert(NodeInvoice { state: NodeInvoiceState::Open, htlc_expiry_height: None });
        Ok(format!("lnbcrt-mock-{}", hex::encode(req.payment_hash)))
    }

    async fn settle_invoice(&self, preimage: &Preimage) -> Result<(), AdapterError> {
        let mut g = self.0.lock().unwrap();
        check_online(&g)?;
        g.settle_calls += 1;
        let inv = g
            .invoices
            .get_mut(&preimage.hash())
            .ok_or_else(|| AdapterError::Rejected("unknown invoice".into()))?;
        match inv.state {
            NodeInvoiceState::Accepted => inv.state = NodeInvoiceState::Settled,
            NodeInvoiceState::Settled => {}
            s => return Err(AdapterError::Rejected(format!("cannot settle a {s:?} invoice"))),
        }
        Ok(())
    }

    async fn cancel_invoice(&self, payment_hash: &PaymentHash) -> Result<(), AdapterError> {
        let mut g = self.0.lock().unwrap();
        check_online(&g)?;
        g.cancel_calls += 1;
        let inv = g
            .invoices
            .get_mut(payment_hash)
            .ok_or_else(|| AdapterError::Rejected("unknown invoice".into()))?;
        match inv.state {
            NodeInvoiceState::Open | NodeInvoiceState::Accepted => {
                inv.state = NodeInvoiceState::Canceled
            }
            NodeInvoiceState::Canceled => {}
            NodeInvoiceState::Settled => {
                return Err(AdapterError::Rejected("cannot cancel a settled invoice".into()))
            }
        }
        Ok(())
    }

    async fn ping(&self) -> Result<(), AdapterError> {
        check_online(&self.0.lock().unwrap())
    }

    async fn lookup_invoice(
        &self,
        payment_hash: &PaymentHash,
    ) -> Result<Option<NodeInvoice>, AdapterError> {
        let g = self.0.lock().unwrap();
        check_online(&g)?;
        Ok(g.invoices.get(payment_hash).cloned())
    }
}

/// One mock node serving everyone, as the service did before sellers could bring their own.
#[async_trait]
impl NodeDirectory for MockLightning {
    type Node = MockLightning;

    async fn kind_for_new(&self, _seller_id: &str) -> Result<NodeKind, AdapterError> {
        Ok(NodeKind::Shared)
    }

    async fn node(&self, _seller_id: &str, _kind: NodeKind) -> Result<MockLightning, AdapterError> {
        Ok(self.clone())
    }

    async fn ping(&self) -> Result<(), AdapterError> {
        LightningAdapter::ping(self).await
    }
}

/// Several mock nodes: each connected seller has their own, and an optional shared one serves the
/// rest. Lets tests prove that an escrow lands on its own seller's node and no one else's.
#[derive(Clone, Default)]
pub struct MockDirectory {
    inner: Arc<Mutex<DirectoryInner>>,
}

#[derive(Default)]
struct DirectoryInner {
    own: HashMap<String, MockLightning>,
    shared: Option<MockLightning>,
    summaries: HashMap<String, NodeSummary>,
}

impl MockDirectory {
    pub fn new(shared: Option<MockLightning>) -> Self {
        Self { inner: Arc::new(Mutex::new(DirectoryInner { own: HashMap::new(), shared, summaries: HashMap::new() })) }
    }

    /// The seller connects their own node.
    pub fn connect_seller(&self, seller_id: &str, node: MockLightning) {
        self.inner.lock().unwrap().own.insert(seller_id.to_string(), node);
    }

    pub fn disconnect_seller(&self, seller_id: &str) {
        self.inner.lock().unwrap().own.remove(seller_id);
    }

    /// The node created for a seller by `register_seller_node`.
    pub fn seller_node(&self, seller_id: &str) -> Option<MockLightning> {
        self.inner.lock().unwrap().own.get(seller_id).cloned()
    }
}

#[async_trait]
impl NodeDirectory for MockDirectory {
    type Node = MockLightning;

    async fn kind_for_new(&self, seller_id: &str) -> Result<NodeKind, AdapterError> {
        let g = self.inner.lock().unwrap();
        if g.own.contains_key(seller_id) {
            Ok(NodeKind::Own)
        } else if g.shared.is_some() {
            Ok(NodeKind::Shared)
        } else {
            Err(AdapterError::NoNode("connect your Lightning node first".into()))
        }
    }

    async fn node(&self, seller_id: &str, kind: NodeKind) -> Result<MockLightning, AdapterError> {
        let g = self.inner.lock().unwrap();
        let found = match kind {
            NodeKind::Own => g.own.get(seller_id),
            NodeKind::Shared => g.shared.as_ref(),
        };
        found.cloned().ok_or_else(|| AdapterError::NoNode(format!("no {} node for this seller", kind.as_str())))
    }

    fn offers_shared_node(&self) -> bool {
        self.inner.lock().unwrap().shared.is_some()
    }

    fn accepts_seller_nodes(&self) -> bool {
        true
    }

    /// Accepts any `https://` address and a hex macaroon, except hosts containing "unreachable" or
    /// "badperm", which fail the connection check the way a dead or over-restricted node would.
    async fn register_seller_node(
        &self,
        seller_id: &str,
        creds: NodeCredentials,
        now: i64,
    ) -> Result<NodeSummary, NodeConnectError> {
        if !creds.address.starts_with("https://") {
            return Err(NodeConnectError::Invalid("the node address must start with https://".into()));
        }
        if hex::decode(&creds.macaroon_hex).map(|b| b.is_empty()).unwrap_or(true) {
            return Err(NodeConnectError::Invalid("the macaroon must be hex".into()));
        }
        if creds.address.contains("unreachable") {
            return Err(NodeConnectError::Adapter(AdapterError::Unavailable("connection refused".into())));
        }
        if creds.address.contains("badperm") {
            return Err(NodeConnectError::CheckFailed("the macaroon cannot create invoices".into()));
        }
        let summary = NodeSummary { address: creds.address, updated_at: now };
        let mut g = self.inner.lock().unwrap();
        g.own.insert(seller_id.to_string(), MockLightning::new());
        g.summaries.insert(seller_id.to_string(), summary.clone());
        Ok(summary)
    }

    async fn remove_seller_node(&self, seller_id: &str) -> Result<bool, AdapterError> {
        let mut g = self.inner.lock().unwrap();
        g.summaries.remove(seller_id);
        Ok(g.own.remove(seller_id).is_some())
    }

    async fn seller_node_summary(&self, seller_id: &str) -> Result<Option<NodeSummary>, AdapterError> {
        Ok(self.inner.lock().unwrap().summaries.get(seller_id).cloned())
    }

    async fn ping(&self) -> Result<(), AdapterError> {
        let shared = self.inner.lock().unwrap().shared.clone();
        match shared {
            Some(n) => LightningAdapter::ping(&n).await,
            None => Ok(()),
        }
    }
}

#[derive(Default)]
struct WalletInner {
    payments: HashMap<PaymentHash, crate::payout::PaymentStatus>,
    /// What the next `pay` reports, instead of `InFlight`.
    next: Option<crate::payout::PaymentStatus>,
    /// The next `pay` gets no answer, as if the connection dropped mid-call.
    lose_next: bool,
    offline: bool,
    counter: u64,
}

/// In-memory stand-in for a node that pays and receives ordinary payments. Payments start in flight;
/// a test decides how they end with [`MockWallet::set_payment`].
#[derive(Clone, Default)]
pub struct MockWallet(Arc<Mutex<WalletInner>>);

impl MockWallet {
    pub fn new() -> Self {
        Self::default()
    }

    /// An invoice for `amount_sats`, payable until `expires_at`. Like a real one, any wallet can read it.
    pub fn invoice(&self, amount_sats: u64, expires_at: i64) -> String {
        let mut g = self.0.lock().unwrap();
        g.counter += 1;
        // Distinct per wallet and per call, so two wallets never make the same hash.
        let mut hash = [0u8; 32];
        hash[..8].copy_from_slice(&g.counter.to_be_bytes());
        hash[8..16].copy_from_slice(&(Arc::as_ptr(&self.0) as usize as u64).to_be_bytes());
        format!("lnbcrt{amount_sats}mock{}x{expires_at}", hex::encode(hash))
    }

    pub fn hash_of(&self, invoice: &str) -> Option<PaymentHash> {
        parse_mock_invoice(invoice).map(|d| d.payment_hash)
    }

    pub fn set_payment(&self, hash: &PaymentHash, status: crate::payout::PaymentStatus) {
        self.0.lock().unwrap().payments.insert(*hash, status);
    }

    /// Make the next `pay` report `status` straight away.
    pub fn next_payment(&self, status: crate::payout::PaymentStatus) {
        self.0.lock().unwrap().next = Some(status);
    }

    pub fn set_offline(&self, offline: bool) {
        self.0.lock().unwrap().offline = offline;
    }

    /// Make the next `pay` fail as unreachable without the payment being recorded, so the caller
    /// cannot know whether it started.
    pub fn lose_next_payment(&self) {
        self.0.lock().unwrap().lose_next = true;
    }

    /// Hashes of every payment started, in no particular order.
    pub fn paid(&self) -> Vec<PaymentHash> {
        self.0.lock().unwrap().payments.keys().copied().collect()
    }
}

/// Read an invoice made by [`MockWallet::invoice`].
fn parse_mock_invoice(invoice: &str) -> Option<crate::payout::DecodedInvoice> {
    let rest = invoice.strip_prefix("lnbcrt")?;
    let (amount, rest) = rest.split_once("mock")?;
    let (hash, expires) = rest.split_once('x')?;
    Some(crate::payout::DecodedInvoice {
        payment_hash: hex::decode(hash).ok()?.try_into().ok()?,
        amount_sats: Some(amount.parse().ok()?),
        expires_at: expires.parse().ok()?,
    })
}

/// The hash of an invoice from [`MockLightning`] or [`MockWallet`].
fn mock_invoice_hash(invoice: &str) -> Option<PaymentHash> {
    if let Some(d) = parse_mock_invoice(invoice) {
        return Some(d.payment_hash);
    }
    hex::decode(invoice.strip_prefix("lnbcrt-mock-")?).ok()?.try_into().ok()
}

#[async_trait]
impl crate::payout::Wallet for MockWallet {
    async fn decode(&self, invoice: &str) -> Result<crate::payout::DecodedInvoice, AdapterError> {
        let g = self.0.lock().unwrap();
        if g.offline {
            return Err(AdapterError::Unavailable("mock wallet offline".into()));
        }
        parse_mock_invoice(invoice).ok_or_else(|| AdapterError::Rejected("invalid invoice".into()))
    }

    async fn pay(&self, invoice: &str, _max_fee_sats: u64) -> Result<crate::payout::PaymentStatus, AdapterError> {
        let mut g = self.0.lock().unwrap();
        if g.offline {
            return Err(AdapterError::Unavailable("mock wallet offline".into()));
        }
        let hash = mock_invoice_hash(invoice).ok_or_else(|| AdapterError::Rejected("invalid invoice".into()))?;
        if std::mem::take(&mut g.lose_next) {
            return Err(AdapterError::Unavailable("connection lost".into()));
        }
        let status = g.next.take().unwrap_or(crate::payout::PaymentStatus::InFlight);
        g.payments.insert(hash, status.clone());
        Ok(status)
    }

    async fn payment_status(&self, hash: &PaymentHash) -> Result<crate::payout::PaymentStatus, AdapterError> {
        let g = self.0.lock().unwrap();
        if g.offline {
            return Err(AdapterError::Unavailable("mock wallet offline".into()));
        }
        Ok(g.payments.get(hash).cloned().unwrap_or(crate::payout::PaymentStatus::Unknown))
    }

    async fn create_invoice(&self, amount_sats: u64, _memo: &str, expiry_secs: i64) -> Result<String, AdapterError> {
        if self.0.lock().unwrap().offline {
            return Err(AdapterError::Unavailable("mock wallet offline".into()));
        }
        // Far enough ahead for any test clock.
        Ok(self.invoice(amount_sats, 1 << 40 | expiry_secs))
    }
}
