//! LND as a [`Wallet`]: paying invoices out of a node and asking how those payments went.
//!
//! This needs more than the invoice-only macaroon escrows use: `offchain:read offchain:write
//! invoices:write info:read` (see `regtest/setup.sh`, which bakes `payout.macaroon`).

use async_trait::async_trait;
use escrow_core::{
    adapter::AdapterError,
    model::PaymentHash,
    payout::{DecodedInvoice, PaymentStatus, Wallet},
};
use fedimint_tonic_lnd::{
    lnrpc::{payment::PaymentStatus as LndStatus, Invoice, PayReqString, Payment, PaymentFailureReason},
    routerrpc::{SendPaymentRequest, TrackPaymentRequest},
    tonic::{Code, Status},
};

use crate::{map_status, LndAdapter};

/// How long LND may spend finding a route before it gives up on a payment.
const ROUTING_TIMEOUT_SECS: i32 = 60;

fn failure_text(reason: i32) -> String {
    match PaymentFailureReason::try_from(reason).unwrap_or(PaymentFailureReason::FailureReasonError) {
        PaymentFailureReason::FailureReasonTimeout => "no route was found in time",
        PaymentFailureReason::FailureReasonNoRoute => "no route to the receiver",
        PaymentFailureReason::FailureReasonIncorrectPaymentDetails => "the receiver did not accept the payment",
        PaymentFailureReason::FailureReasonInsufficientBalance => "not enough outbound liquidity on the paying node",
        PaymentFailureReason::FailureReasonCanceled => "the payment was cancelled",
        PaymentFailureReason::FailureReasonNone | PaymentFailureReason::FailureReasonError => "the payment failed",
    }
    .to_string()
}

fn status_of(p: &Payment) -> PaymentStatus {
    match LndStatus::try_from(p.status).unwrap_or(LndStatus::Unknown) {
        LndStatus::Succeeded => PaymentStatus::Succeeded { fee_sats: p.fee_sat.max(0) as u64 },
        LndStatus::Failed => PaymentStatus::Failed(failure_text(p.failure_reason)),
        LndStatus::InFlight | LndStatus::Initiated => PaymentStatus::InFlight,
        LndStatus::Unknown => PaymentStatus::Unknown,
    }
}

#[async_trait]
impl Wallet for LndAdapter {
    async fn decode(&self, invoice: &str) -> Result<DecodedInvoice, AdapterError> {
        let mut client = self.client.clone();
        let req = self
            .call(async { client.lightning().decode_pay_req(PayReqString { pay_req: invoice.to_string() }).await })
            .await?
            .into_inner();
        let payment_hash = hex::decode(&req.payment_hash)
            .ok()
            .and_then(|b| PaymentHash::try_from(b).ok())
            .ok_or_else(|| AdapterError::Rejected("the invoice has no valid payment hash".into()))?;
        Ok(DecodedInvoice {
            payment_hash,
            amount_sats: u64::try_from(req.num_satoshis).ok().filter(|&a| a > 0),
            expires_at: req.timestamp + req.expiry,
        })
    }

    async fn pay(&self, invoice: &str, max_fee_sats: u64) -> Result<PaymentStatus, AdapterError> {
        let mut client = self.client.clone();
        let mut updates = self
            .call(async {
                client
                    .router()
                    .send_payment_v2(SendPaymentRequest {
                        payment_request: invoice.to_string(),
                        fee_limit_sat: i64::try_from(max_fee_sats).unwrap_or(i64::MAX),
                        timeout_seconds: ROUTING_TIMEOUT_SECS,
                        ..Default::default()
                    })
                    .await
                    .map(|r| r.into_inner())
            })
            .await?;
        // The first update says the node has taken the payment on. It carries on routing after the
        // stream is dropped; how it ends is read later with `payment_status`.
        match self.call(updates.message()).await? {
            Some(p) => Ok(status_of(&p)),
            None => Ok(PaymentStatus::InFlight),
        }
    }

    async fn payment_status(&self, hash: &PaymentHash) -> Result<PaymentStatus, AdapterError> {
        let mut client = self.client.clone();
        let started = tokio::time::timeout(self.timeout, async {
            client
                .router()
                .track_payment_v2(TrackPaymentRequest { payment_hash: hash.to_vec(), no_inflight_updates: false })
                .await
        })
        .await
        .map_err(|_| AdapterError::Unavailable("node call timed out".into()))?;
        let mut updates = match started {
            Ok(r) => r.into_inner(),
            Err(s) if is_not_found(&s) => return Ok(PaymentStatus::Unknown),
            Err(s) => return Err(map_status(s)),
        };
        // LND sends the payment's current state first.
        match tokio::time::timeout(self.timeout, updates.message()).await {
            Err(_) => Err(AdapterError::Unavailable("node call timed out".into())),
            Ok(Err(s)) if is_not_found(&s) => Ok(PaymentStatus::Unknown),
            Ok(Err(s)) => Err(map_status(s)),
            Ok(Ok(Some(p))) => Ok(status_of(&p)),
            Ok(Ok(None)) => Ok(PaymentStatus::Unknown),
        }
    }

    async fn create_invoice(&self, amount_sats: u64, memo: &str, expiry_secs: i64) -> Result<String, AdapterError> {
        let mut client = self.client.clone();
        let res = self
            .call(async {
                client
                    .lightning()
                    .add_invoice(Invoice {
                        memo: memo.to_string(),
                        value: i64::try_from(amount_sats).unwrap_or(i64::MAX),
                        expiry: expiry_secs,
                        ..Default::default()
                    })
                    .await
            })
            .await?
            .into_inner();
        Ok(res.payment_request)
    }
}

/// LND's answer when it has never seen a payment for the hash.
fn is_not_found(s: &Status) -> bool {
    s.code() == Code::NotFound || s.message().contains("payment isn't initiated")
}
