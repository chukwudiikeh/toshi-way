"use client";

import { useCallback, useEffect, useState, type ReactNode } from "react";
import { api, describeError } from "@/lib/api";
import { formatSats } from "@/lib/clock";
import type { Escrow, RampInfo, RampOrder } from "@/lib/schemas";
import { BankAccountFields, DemoBankNotice, money, QuoteLines, RampChip, rampFinal, useRampQuote, type BankAccount } from "../naira";
import { Button, Countdown, CopyField, Notice, Spinner } from "../ui";

type Ramp = Extract<RampInfo, { enabled: true }>;

/**
 * Paying an escrow by bank transfer in naira. The payment provider receives the transfer and pays the
 * escrow's Lightning invoice for the buyer, so the escrow works exactly as if they had paid in sats.
 * If it doesn't reach the seller, the money goes back to the buyer's own bank account.
 *
 * `canStart`: whether a new payment may be started (the escrow is waiting to be paid). Otherwise this
 * only shows how an earlier naira payment went, if there was one. `chosen`: the buyer already picked
 * naira, so go straight to the form.
 */
export function NairaPay({ escrow, canStart, chosen = false }: { escrow: Escrow; canStart: boolean; chosen?: boolean }) {
  const [ramp, setRamp] = useState<Ramp | null>(null);
  const [order, setOrder] = useState<RampOrder | null | undefined>(undefined);
  const [open, setOpen] = useState(chosen);

  const load = useCallback(async () => {
    try {
      setOrder((await api.onramp(escrow.id)).order);
    } catch {
      setOrder(null);
    }
  }, [escrow.id]);

  useEffect(() => {
    let alive = true;
    api
      .rampInfo()
      .then((r) => {
        if (!alive || !r.enabled) return;
        setRamp(r);
        void load();
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [load, escrow.state]);

  // Follow the payment until it settles into a final state.
  const live = order && !rampFinal(order);
  useEffect(() => {
    if (!live) return;
    const t = setInterval(load, 3000);
    return () => clearInterval(t);
  }, [live, load]);

  if (!ramp || order === undefined) return null;
  const restartable = canStart && (!order || order.state === "expired");

  if (order && !(restartable && open)) {
    return <OrderStatus order={order} ramp={ramp} onChange={setOrder} onRetry={restartable ? () => setOpen(true) : undefined} />;
  }
  if (!canStart) return null;

  if (!open) {
    return (
      <div className="rounded-xl border border-neutral-200 p-4 dark:border-neutral-800">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="text-sm">
            <div className="font-semibold">No Lightning wallet?</div>
            <div className="text-neutral-500">Pay in naira by bank transfer from any Nigerian bank.</div>
          </div>
          <Button variant="secondary" onClick={() => setOpen(true)}>
            Pay in naira (₦)
          </Button>
        </div>
      </div>
    );
  }
  return (
    <StartForm escrow={escrow} ramp={ramp} onStarted={setOrder} onCancel={chosen ? undefined : () => setOpen(false)} />
  );
}

/**
 * How the buyer wants to pay, side by side: sats over Lightning (`sats`, the invoice) or naira by bank
 * transfer. Without a naira provider on the server it is just the invoice.
 */
export function PayChoice({ escrow, sats }: { escrow: Escrow; sats: ReactNode }) {
  const [ramp, setRamp] = useState<boolean | null>(null);
  const [tab, setTab] = useState<"sats" | "naira">("sats");
  const { quote } = useRampQuote("onramp", ramp ? escrow.amount_sats : null);

  useEffect(() => {
    let alive = true;
    api
      .rampInfo()
      .then(async (r) => {
        if (!alive) return;
        setRamp(r.enabled);
        // A naira payment already under way opens on its own tab.
        if (r.enabled) {
          const { order } = await api.onramp(escrow.id);
          if (alive && order && !rampFinal(order)) setTab("naira");
        }
      })
      .catch(() => alive && setRamp(false));
    return () => {
      alive = false;
    };
  }, [escrow.id]);

  if (ramp === null) return <Spinner />;
  if (!ramp) return <>{sats}</>;

  const tabs = [
    { key: "sats" as const, title: "Pay in sats", sub: "Lightning wallet", amount: formatSats(escrow.amount_sats) },
    { key: "naira" as const, title: "Pay in naira", sub: "Bank transfer", amount: quote ? money(quote.fiat_minor, quote) : "₦…" },
  ];
  return (
    <div className="space-y-4">
      <div className="text-sm font-medium">How do you want to pay?</div>
      <div role="tablist" aria-label="How to pay" className="grid grid-cols-2 gap-2">
        {tabs.map((t) => (
          <button
            key={t.key}
            type="button"
            role="tab"
            aria-selected={tab === t.key}
            onClick={() => setTab(t.key)}
            className={`rounded-xl border-2 px-4 py-3 text-left transition-colors ${
              tab === t.key
                ? "border-amber-500 bg-amber-50 dark:bg-amber-950/30"
                : "border-neutral-200 hover:border-neutral-300 dark:border-neutral-800 dark:hover:border-neutral-700"
            }`}
          >
            <div className="text-sm font-semibold">{t.title}</div>
            <div className="text-xs text-neutral-500">{t.sub}</div>
            <div className="mt-1 text-lg font-bold tabular-nums">{t.amount}</div>
          </button>
        ))}
      </div>
      <div role="tabpanel">{tab === "sats" ? sats : <NairaPay escrow={escrow} canStart chosen />}</div>
    </div>
  );
}

function StartForm({
  escrow,
  ramp,
  onStarted,
  onCancel,
}: {
  escrow: Escrow;
  ramp: Ramp;
  onStarted: (o: RampOrder) => void;
  /** Absent when naira was picked from the tabs, which are the way back. */
  onCancel?: () => void;
}) {
  const [account, setAccount] = useState<BankAccount>({ bankCode: "", accountNumber: "", resolved: null });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { quote, error: quoteError } = useRampQuote("onramp", escrow.amount_sats);

  async function start() {
    if (!account.resolved) return;
    setBusy(true);
    setError(null);
    try {
      onStarted(await api.startOnramp(escrow.id, account.resolved.bank_code, account.resolved.account_number));
    } catch (err) {
      setError(describeError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-4 rounded-xl border border-neutral-200 p-4 dark:border-neutral-800">
      <div className="text-base font-semibold">Pay in naira</div>
      {ramp.demo && <DemoBankNotice />}
      {quote ? (
        <QuoteLines quote={quote} total={`You pay for ${formatSats(escrow.amount_sats)}`} />
      ) : quoteError ? (
        <Notice tone="error">{quoteError}</Notice>
      ) : (
        <Spinner label="Getting the price…" />
      )}
      <BankAccountFields
        banks={ramp.banks}
        value={account}
        onChange={setAccount}
        label="Your bank"
        hint="Where your money goes back if the seller isn't paid."
      />
      <p className="text-xs text-neutral-500">
        The escrow works the same as paying in sats: the seller is paid only when you release, and if the deal
        falls through, the full amount goes back to this account in naira.
      </p>
      {error && <Notice tone="error">{error}</Notice>}
      <div className="flex flex-wrap gap-2">
        <Button onClick={start} busy={busy} disabled={!account.resolved || !quote}>
          Get transfer details
        </Button>
        {onCancel && (
          <Button variant="ghost" onClick={onCancel}>
            Back
          </Button>
        )}
      </div>
    </div>
  );
}

function OrderStatus({
  order,
  ramp,
  onChange,
  onRetry,
}: {
  order: RampOrder;
  ramp: Ramp;
  onChange: (o: RampOrder) => void;
  onRetry?: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function demoSent() {
    setBusy(true);
    setError(null);
    try {
      onChange(await api.demoDeposit(order.id));
    } catch (err) {
      setError(describeError(err));
    } finally {
      setBusy(false);
    }
  }

  const amount = money(order.fiat_minor, order);
  return (
    <div className="space-y-4 rounded-xl border border-neutral-200 p-4 dark:border-neutral-800">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="text-base font-semibold">Paying in naira · {amount}</div>
        <RampChip state={order.state} />
      </div>
      {order.state === "awaiting_deposit" && order.deposit && (
        <>
          {ramp.demo && <DemoBankNotice />}
          <p className="text-sm">
            Transfer <strong>exactly {amount}</strong> from your bank app to this account. Put the reference in the
            narration so the payment is matched to you.
          </p>
          <dl className="space-y-3 text-sm">
            <div>
              <dt className="text-xs uppercase tracking-wide text-neutral-500">Amount</dt>
              <dd>
                <CopyField value={(order.fiat_minor / 10 ** order.exponent).toFixed(order.exponent)} />
              </dd>
            </div>
            <div>
              <dt className="text-xs uppercase tracking-wide text-neutral-500">
                {order.deposit.bank_name} · {order.deposit.account_name}
              </dt>
              <dd>
                <CopyField value={order.deposit.account_number} />
              </dd>
            </div>
            <div>
              <dt className="text-xs uppercase tracking-wide text-neutral-500">Reference</dt>
              <dd>
                <CopyField value={order.deposit.reference} />
              </dd>
            </div>
          </dl>
          <Countdown until={order.deposit.expires_at} label="Transfer within" endedText="Closed" />
          {ramp.demo && (
            <Button onClick={demoSent} busy={busy}>
              I&apos;ve sent the transfer (demo)
            </Button>
          )}
          {!ramp.demo && (
            <p className="flex items-center gap-2 text-sm text-neutral-500">
              <span className="size-2 animate-pulse rounded-full bg-sky-500" />
              This updates by itself when the transfer arrives.
            </p>
          )}
        </>
      )}
      {(order.state === "deposit_received" || order.state === "paying") && (
        <Spinner label="Transfer received. Paying the escrow's invoice for you…" />
      )}
      {(order.state === "funded" || order.state === "complete") && (
        <Notice tone="success">
          Your transfer paid the escrow. If the seller isn&apos;t paid in the end, {amount} goes back to{" "}
          {order.bank.account_name} ({order.bank.bank_name} {order.bank.account_number}).
        </Notice>
      )}
      {(order.state === "refunding" || order.state === "refunded") && (
        <Notice tone="info">
          {order.state === "refunding" ? "Sending" : "Sent"} {amount} back to {order.bank.account_name} at{" "}
          {order.bank.bank_name} ({order.bank.account_number}).
          {order.fiat_reference && <> Reference: <span className="font-mono">{order.fiat_reference}</span>.</>}
        </Notice>
      )}
      {order.state === "expired" && (
        <Notice tone="info">No transfer arrived before the window closed, so nothing was paid.</Notice>
      )}
      {error && <Notice tone="error">{error}</Notice>}
      {onRetry && (
        <Button variant="secondary" onClick={onRetry}>
          Start a new naira payment
        </Button>
      )}
    </div>
  );
}
