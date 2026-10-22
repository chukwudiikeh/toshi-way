//! Background loops that keep escrows in step with the node.
//!
//! `run_event_loop` watches every open escrow's invoice and feeds each state change into the
//! service as it happens. It also runs `reconcile` at start and periodically, so anything missed
//! while disconnected is caught. `run_scheduler` calls `tick` so holds are cancelled at their
//! deadline with no human action.

use std::{
    collections::HashMap,
    future::Future,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use escrow_core::{
    model::{InvoiceEvent, NodeInvoiceState, PaymentHash},
    service::EscrowService,
    store::Store,
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

/// Runs until `shutdown` completes.
pub async fn run_event_loop<S: Store>(
    service: &EscrowService<LndAdapter, S>,
    adapter: &LndAdapter,
    config: EventLoopConfig,
    shutdown: impl Future<Output = ()>,
) {
    tokio::pin!(shutdown);
    let (tx, mut rx) = mpsc::channel::<InvoiceEvent>(256);
    let mut watchers: HashMap<PaymentHash, JoinHandle<()>> = HashMap::new();

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
                    match service.active_payment_hashes().await {
                        Ok(hashes) => for hash in hashes {
                            watchers.entry(hash).or_insert_with(|| {
                                tokio::spawn(watch_invoice(adapter.clone(), hash, tx.clone()))
                            });
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
pub async fn run_scheduler<S: Store>(
    service: &EscrowService<LndAdapter, S>,
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
