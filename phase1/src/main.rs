//! Phase 1: prove the hold-invoice cycle on the regtest network.
//!
//! Usage: `cargo run -p phase1 -- [settle|cancel|timeout|both]` (default: both)
//! `timeout` takes no action and mines blocks until the node cancels the hold itself.
//!
//! Plays all three roles against real LND nodes:
//!   buyer generates P and H = sha256(P)  ->  seller creates a hold invoice for H
//!   buyer pays through the router        ->  seller sees ACCEPTED (funds locked)
//!   seller settles with P (paid) or cancels (buyer refunded)

use std::{process::Command, time::Duration};

use anyhow::{bail, Context, Result};
use fedimint_tonic_lnd::{
    connect,
    invoicesrpc::{
        AddHoldInvoiceRequest, CancelInvoiceMsg, SettleInvoiceMsg, SubscribeSingleInvoiceRequest,
    },
    lnrpc::{invoice::InvoiceState, payment::PaymentStatus, ChannelBalanceRequest, GetInfoRequest},
    routerrpc::SendPaymentRequest,
    Client,
};
use rand::RngCore;
use sha2::{Digest, Sha256};
use tokio::time::timeout;

const AMOUNT_SATS: i64 = 1000;
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, PartialEq)]
enum Outcome {
    Settle,
    Cancel,
    /// Nobody acts; blocks are mined until the hold expires and the node cancels it.
    Timeout,
}

/// Final CLTV delta for the timeout scenario: small enough to reach by mining, above LND's minimum.
const TIMEOUT_CLTV_DELTA: u64 = 24;

fn mine(blocks: u32) -> Result<()> {
    let out = Command::new("docker")
        .args(["compose", "-f", "regtest/docker-compose.yml", "exec", "-T", "bitcoind"])
        .args(["bitcoin-cli", "-regtest", "-rpcuser=toshi", "-rpcpassword=toshi", "-rpcwallet=miner"])
        .arg(format!("-generate={blocks}"))
        .output()
        .context("running docker compose to mine blocks")?;
    if !out.status.success() {
        bail!("mining failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(())
}

async fn block_height(client: &mut Client) -> Result<u32> {
    Ok(client.lightning().get_info(GetInfoRequest {}).await?.into_inner().block_height)
}

async fn node(name: &str, port: u16) -> Result<Client> {
    let dir = std::env::var("CREDS_DIR").unwrap_or_else(|_| "regtest/creds".into());
    connect(
        format!("https://localhost:{port}"),
        format!("{dir}/{name}/tls.cert"),
        format!("{dir}/{name}/admin.macaroon"),
    )
    .await
    .with_context(|| format!("connecting to {name} (is ./regtest/setup.sh done?)"))
}

/// Sats we can spend / have on our side of our channels.
async fn local_balance(client: &mut Client) -> Result<u64> {
    let res = client
        .lightning()
        .channel_balance(ChannelBalanceRequest {})
        .await?
        .into_inner();
    Ok(res.local_balance.map(|a| a.sat).unwrap_or(0))
}

fn state_name(state: i32) -> String {
    InvoiceState::try_from(state)
        .map(|s| format!("{s:?}").to_uppercase())
        .unwrap_or_else(|_| format!("UNKNOWN({state})"))
}

async fn run(outcome: Outcome, buyer: &mut Client, seller: &mut Client) -> Result<()> {
    let label = match outcome {
        Outcome::Settle => "SETTLE",
        Outcome::Cancel => "CANCEL",
        Outcome::Timeout => "TIMEOUT",
    };
    println!("\n=== scenario: {label} ===");

    let buyer_before = local_balance(buyer).await?;
    let seller_before = local_balance(seller).await?;
    println!("balances before: buyer={buyer_before} seller={seller_before}");

    // Buyer side: secret P and its hash H. Only H leaves the buyer.
    let mut preimage = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut preimage);
    let hash = Sha256::digest(preimage).to_vec();
    println!("H = {}", hex::encode(&hash));

    // Seller side: hold invoice for H.
    let invoice = seller
        .invoices()
        .add_hold_invoice(AddHoldInvoiceRequest {
            memo: format!("phase1 {label}"),
            hash: hash.clone(),
            value: AMOUNT_SATS,
            expiry: 600,
            cltv_expiry: if outcome == Outcome::Timeout { TIMEOUT_CLTV_DELTA } else { 0 },
            ..Default::default()
        })
        .await?
        .into_inner();
    println!("seller created hold invoice");

    // Subscribe before paying so no state change is missed.
    let mut updates = seller
        .invoices()
        .subscribe_single_invoice(SubscribeSingleInvoiceRequest { r_hash: hash.clone() })
        .await?
        .into_inner();

    // Buyer pays in the background; the payment stays in flight while the hold is open.
    let mut payer = buyer.clone();
    let pay = tokio::spawn(async move {
        let mut stream = payer
            .router()
            .send_payment_v2(SendPaymentRequest {
                payment_request: invoice.payment_request,
                timeout_seconds: 60,
                fee_limit_sat: 100,
                no_inflight_updates: true,
                ..Default::default()
            })
            .await?
            .into_inner();
        let mut last = None;
        while let Some(p) = stream.message().await? {
            last = Some(p);
        }
        anyhow::Ok(last)
    });

    // Wait for the node to report ACCEPTED: only then are the funds locked.
    let accepted = timeout(STEP_TIMEOUT, async {
        while let Some(inv) = updates.message().await? {
            println!("seller invoice state: {}", state_name(inv.state));
            if inv.state == InvoiceState::Accepted as i32 {
                return anyhow::Ok(inv);
            }
        }
        bail!("invoice stream ended before ACCEPTED")
    })
    .await
    .context("timed out waiting for ACCEPTED")??;
    println!("LOCKED: {} sats held by seller's node, not yet settled", accepted.amt_paid_sat);
    println!(
        "balances while held: buyer={} seller={}",
        local_balance(buyer).await?,
        local_balance(seller).await?
    );

    match outcome {
        Outcome::Settle => {
            // Verify sha256(P) == H before settling, as the real service must.
            assert_eq!(Sha256::digest(preimage).to_vec(), hash);
            seller
                .invoices()
                .settle_invoice(SettleInvoiceMsg { preimage: preimage.to_vec() })
                .await?;
            println!("seller called SettleInvoice");
        }
        Outcome::Cancel => {
            seller
                .invoices()
                .cancel_invoice(CancelInvoiceMsg { payment_hash: hash.clone() })
                .await?;
            println!("seller called CancelInvoice");
        }
        Outcome::Timeout => {
            let expiry = accepted.htlcs.first().map(|h| h.expiry_height).unwrap_or(0);
            let start = block_height(seller).await?;
            println!("no action taken; HTLC expiry_height={expiry}, current height={start}");
            let mut cancelled_at = None;
            for _ in 0..60 {
                mine(1)?;
                // Give the node a moment to react to the new block.
                if let Ok(Some(inv)) =
                    timeout(Duration::from_millis(1500), updates.message()).await.map(|r| r.ok().flatten())
                {
                    println!("seller invoice state: {}", state_name(inv.state));
                    if inv.state == InvoiceState::Canceled as i32 {
                        cancelled_at = Some(block_height(seller).await?);
                        break;
                    }
                }
            }
            let at = cancelled_at.context("hold never expired after 60 blocks")?;
            println!(
                "node auto-cancelled at height {at}: {} blocks before HTLC expiry ({} blocks after lock)",
                expiry as i64 - at as i64,
                at - start
            );
        }
    }

    let want = if outcome == Outcome::Settle { InvoiceState::Settled } else { InvoiceState::Canceled };
    // The timeout scenario already observed CANCELED while mining.
    if outcome != Outcome::Timeout {
        timeout(STEP_TIMEOUT, async {
            while let Some(inv) = updates.message().await? {
                println!("seller invoice state: {}", state_name(inv.state));
                if inv.state == want as i32 {
                    return anyhow::Ok(());
                }
            }
            bail!("invoice stream ended before {want:?}")
        })
        .await
        .context("timed out waiting for final invoice state")??;
    }

    let payment = timeout(STEP_TIMEOUT, pay)
        .await
        .context("timed out waiting for buyer payment to resolve")???
        .context("no payment update received")?;
    let status = PaymentStatus::try_from(payment.status)?;
    println!("buyer payment status: {status:?} (failure_reason={})", payment.failure_reason);

    // Channel balances settle a moment after the HTLC resolves.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let buyer_after = local_balance(buyer).await?;
    let seller_after = local_balance(seller).await?;
    println!("balances after:  buyer={buyer_after} seller={seller_after}");

    match outcome {
        Outcome::Settle => {
            assert_eq!(status, PaymentStatus::Succeeded, "payment should succeed");
            assert_eq!(seller_after, seller_before + AMOUNT_SATS as u64, "seller should be paid");
            assert!(buyer_after < buyer_before, "buyer should have paid");
        }
        Outcome::Cancel | Outcome::Timeout => {
            assert_eq!(status, PaymentStatus::Failed, "payment should fail back");
            assert_eq!(seller_after, seller_before, "seller must not be paid");
            assert_eq!(buyer_after, buyer_before, "buyer must be refunded in full");
        }
    }
    println!("PASS: {label}");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .map_err(|_| anyhow::anyhow!("a rustls crypto provider was already installed"))?;
    let mode = std::env::args().nth(1).unwrap_or_else(|| "both".into());
    let mut buyer = node("buyer", 10001).await?;
    let mut seller = node("seller", 10003).await?;

    match mode.as_str() {
        "settle" => run(Outcome::Settle, &mut buyer, &mut seller).await?,
        "cancel" => run(Outcome::Cancel, &mut buyer, &mut seller).await?,
        "timeout" => run(Outcome::Timeout, &mut buyer, &mut seller).await?,
        "both" => {
            run(Outcome::Settle, &mut buyer, &mut seller).await?;
            run(Outcome::Cancel, &mut buyer, &mut seller).await?;
        }
        other => bail!("unknown mode '{other}', expected settle|cancel|timeout|both"),
    }
    Ok(())
}
