//! Background loops that keep escrows in step with the node.
//!
//! `run_event_loop` watches every open escrow's invoice, on whichever node serves that escrow's
//! seller, and feeds each state change into the service as it happens. It also runs `reconcile` at start and periodically, so anything missed
//! while disconnected is caught. `run_scheduler` calls `tick` so holds are cancelled at their
//! deadline with no human action. `run_payouts` settles withdrawals from what the paying node reports
//! and moves naira ramp orders on.

use std::{
    collections::HashMap,
    future::Future,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use escrow_core::{
    adapter::NodeDirectory,
    model::{InvoiceEvent, NodeInvoiceState, PaymentHash},
    payout::PayoutService,
    ramp::RampService,
    service::EscrowService,
    store::{PayoutStore, Store},
};
use tokio::{sync::mpsc, task::JoinHandle, time::MissedTickBehavior};

use crate::LndAdapter;

pub fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Debug, Clone)]
pub struct EventLoopConfig {
    /// How often to look for newly opened escrows that need a watcher.
    pub discover_every: Duration,
    /// How often to reconcile every open escrow against the node, as a safety net.
    pub reconcile_every: Duration,
}

impl Default for EventLoopConfig {
    fn default() -> Self {
        Self { discover_every: Duration::from_secs(1), reconcile_every: Duration::from_secs(30) }
    }
}

const MIN_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Follow one invoice until it settles or is cancelled, resubscribing if the stream drops.
async fn watch_invoice(adapter: LndAdapter, hash: PaymentHash, tx: mpsc::Sender<InvoiceEvent>) {
    let mut backoff = MIN_BACKOFF;
    loop {
        match adapter.subscribe_invoice(&hash).await {
            Ok(mut events) => loop {
                match events.next().await {
                    Ok(Some(ev)) => {
                        backoff = MIN_BACKOFF;
                        let done = matches!(ev.state, NodeInvoiceState::Settled | NodeInvoiceState::Canceled);
                        if tx.send(ev).await.is_err() || done {
                            return;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!(error = %e, "invoice stream failed");
                        break;
                    }
                }
            },
            Err(e) => tracing::warn!(error = %e, "could not subscribe to invoice"),
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// How long to wait before trying again to reach a node that could not be found or connected to.
const NODE_RETRY: Duration = Duration::from_secs(10);

/// Runs until `shutdown` completes.
pub async fn run_event_loop<D, S>(
    service: &EscrowService<D, S>,
    config: EventLoopConfig,
    shutdown: impl Future<Output = ()>,
) where
    D: NodeDirectory<Node = LndAdapter>,
    S: Store,
{
    tokio::pin!(shutdown);
    let (tx, mut rx) = mpsc::channel::<InvoiceEvent>(256);
    let mut watchers: HashMap<PaymentHash, JoinHandle<()>> = HashMap::new();
    let mut unreachable: HashMap<PaymentHash, Instant> = HashMap::new();

    let mut discover = tokio::time::interval(config.discover_every);
    discover.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut reconcile = tokio::time::interval(config.reconcile_every);
    reconcile.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let result = async {
        loop {
            tokio::select! {
                _ = &mut shutdown => return,
                Some(ev) = rx.recv() => {
                    if let Err(e) = service.apply_event(ev, unix_now()).await {
                        tracing::error!(error = %e, "failed to apply node event");
                    }
                }
                _ = discover.tick() => {
                    watchers.retain(|_, h| !h.is_finished());
                    match service.watch_targets().await {
                        Ok(targets) => for t in targets {
                            if watchers.contains_key(&t.payment_hash) {
                                continue;
                            }
                            if unreachable.get(&t.payment_hash).is_some_and(|at| at.elapsed() < NODE_RETRY) {
                                continue;
                            }
                            match service.directory().node(&t.seller_id, t.node).await {
                                Ok(node) => {
                                    unreachable.remove(&t.payment_hash);
                                    watchers.insert(t.payment_hash, tokio::spawn(watch_invoice(node, t.payment_hash, tx.clone())));
                                }
                                Err(e) => {
                                    // One seller's node being down must not stop the others being watched.
                                    tracing::warn!(error = %e, seller = %t.seller_id, "cannot reach the node for an open escrow");
                                    unreachable.insert(t.payment_hash, Instant::now());
                                }
                            }
                        },
                        Err(e) => tracing::error!(error = %e, "could not list open escrows"),
                    }
                }
                // The first tick fires immediately, which gives the startup reconciliation.
                _ = reconcile.tick() => {
                    match service.reconcile(unix_now()).await {
                        Ok(r) if !r.errors.is_empty() => tracing::warn!(errors = r.errors.len(), "reconcile had errors"),
                        Ok(r) => tracing::info!(checked = r.checked, changed = r.changed.len(), "reconciled with node"),
                        Err(e) => tracing::warn!(error = %e, "reconcile failed"),
                    }
                }
            }
        }
    }
    .await;

    for (_, h) in watchers {
        h.abort();
    }
    result
}

/// Runs until `shutdown` completes.
pub async fn run_scheduler<D: NodeDirectory, S: Store>(
    service: &EscrowService<D, S>,
    every: Duration,
    shutdown: impl Future<Output = ()>,
) {
    tokio::pin!(shutdown);
    let mut ticker = tokio::time::interval(every);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = &mut shutdown => return,
            _ = ticker.tick() => {
                match service.tick(unix_now()).await {
                    Ok(r) => {
                        if !r.cancelled.is_empty() {
                            tracing::info!(cancelled = r.cancelled.len(), "scheduler cancelled expired holds");
                        }
                        for (id, err) in r.errors {
                            tracing::error!(escrow = %id, %err, "scheduler could not cancel");
                        }
                    }
                    Err(e) => tracing::error!(error = %e, "scheduler pass failed"),
                }
            }
        }
    }
}

/// Keep withdrawals and ramp orders in step with the nodes and the ramp provider. Withdrawals are
/// synced first, so an off-ramp sees its sats arrive in the same pass. Runs until `shutdown` completes.
pub async fn run_payouts<S: Store + PayoutStore>(
    payouts: Option<&PayoutService<S>>,
    ramp: Option<&RampService<S>>,
    every: Duration,
    shutdown: impl Future<Output = ()>,
) {
    tokio::pin!(shutdown);
    let mut ticker = tokio::time::interval(every);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = &mut shutdown => return,
            _ = ticker.tick() => {
                if let Some(p) = payouts {
                    match p.sync(unix_now()).await {
                        Ok(r) => {
                            if !r.finished.is_empty() {
                                tracing::info!(finished = r.finished.len(), "withdrawals settled");
                            }
                            for (id, err) in r.errors {
                                tracing::warn!(withdrawal = %id, %err, "could not check a withdrawal");
                            }
                        }
                        Err(e) => tracing::error!(error = %e, "withdrawal sync failed"),
                    }
                }
                if let Some(r) = ramp {
                    match r.tick(unix_now()).await {
                        Ok(report) => {
                            for (id, state) in report.advanced {
                                tracing::info!(order = %id, state = state.as_str(), "ramp order moved on");
                            }
                            for (id, err) in report.errors {
                                tracing::warn!(order = %id, %err, "ramp order could not move on");
                            }
                        }
                        Err(e) => tracing::error!(error = %e, "ramp pass failed"),
                    }
                }
            }
        }
    }
}
