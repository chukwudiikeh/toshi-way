//! In-memory stand-in for a Lightning node, modelled on what LND did in Phase 1:
//! `Open` -> `Accepted` once paid, then `Settled` or `Canceled`.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;

use crate::{
    adapter::{AdapterError, HoldInvoiceRequest, LightningAdapter},
    model::{InvoiceEvent, NodeInvoice, NodeInvoiceState, PaymentHash, Preimage},
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
