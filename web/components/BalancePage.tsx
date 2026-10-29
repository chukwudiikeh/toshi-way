"use client";

import Link from "next/link";
import { useCallback, useEffect, useState, type FormEvent } from "react";
import { api, describeError } from "@/lib/api";
import { formatLocalTime, formatSats } from "@/lib/clock";
import type { Balance, NodeInfo, RampInfo, RampOrder, Withdrawal } from "@/lib/schemas";
import { useSession } from "@/lib/session";
import { SignIn } from "./Account";
import { demoInvoice, hasDemoWallet } from "./DemoWallet";
import { WalletIcon } from "./landing/icons";
import { BankAccountFields, DemoBankNotice, money, QuoteLines, RampChip, rampFinal, useRampQuote, type BankAccount } from "./naira";
import { PageHeader } from "./PageParts";
import { Button, Card, ConfirmButton, Field, inputClass, Notice, Spinner } from "./ui";

type Payable = Extract<Balance, { payouts: true }>;
type Ramp = Extract<RampInfo, { enabled: true }>;

/** The routing-fee allowance held back while a withdrawal of `sats` is paid (mirrors the server). */
function feeAllowance(sats: number, b: Payable): number {
  return Math.max(b.fee_allowance.min_sats, Math.floor((sats * b.fee_allowance.percent) / 100));
}

/** The most that can be withdrawn in one go, leaving room for the fee allowance. */
function maxWithdrawal(b: Payable): number {
  let a = Math.floor((b.available_sats * 100) / (100 + b.fee_allowance.percent));
  while (a > 0 && a + feeAllowance(a, b) > b.available_sats) a--;
  while (a + 1 + feeAllowance(a + 1, b) <= b.available_sats) a++;
  return Math.max(0, a);
}

/**
 * What the service owes a seller whose escrows settled on its shared node, and the ways to take it
 * out: to any Lightning wallet, or to a Nigerian bank account in naira.
 */
export function BalancePage() {
  const session = useSession();
  const [balance, setBalance] = useState<Balance | null>(null);
  const [withdrawals, setWithdrawals] = useState<Withdrawal[]>([]);
  const [orders, setOrders] = useState<RampOrder[]>([]);
  const [ramp, setRamp] = useState<RampInfo | null>(null);
  const [node, setNode] = useState<NodeInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const b = await api.balance();
      setBalance(b);
      if (b.payouts) setWithdrawals((await api.withdrawals()).withdrawals);
      setError(null);
    } catch (err) {
      setError(describeError(err));
    }
    // Bank payouts are optional: without them the page still shows the balance.
    api.rampInfo().then(setRamp).catch(() => setRamp({ enabled: false }));
    api
      .rampOrders()
      .then((o) => setOrders(o.orders.filter((x) => x.direction === "offramp")))
      .catch(() => {});
  }, []);

  const signedIn = session?.kind === "user";
  useEffect(() => {
    if (!signedIn) return;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- loads from the API on mount
    void load();
    api.node().then(setNode).catch(() => {});
  }, [signedIn, load]);

  // Follow withdrawals until they finish: each one ends only when the node says so.
  const moving = withdrawals.some((w) => w.state === "pending") || orders.some((o) => !rampFinal(o));
  useEffect(() => {
    if (!moving) return;
    const t = setInterval(load, 3000);
    return () => clearInterval(t);
  }, [moving, load]);

  if (session === undefined) return <Spinner />;
  if (!session) return <SignIn note="Sign in to see your balance." />;
  if (session.kind === "operator") return <Notice tone="info">The operator account has no balance.</Notice>;

  return (
    <div className="space-y-6">
      <PageHeader icon={<WalletIcon className="size-5" />} title="Balance">
        When a buyer releases an escrow that was paid on the shared node, the sats are kept here for you. Withdraw
        them whenever you like: to any Lightning wallet, or to your Nigerian bank account in naira.
      </PageHeader>
      {error && <Notice tone="error">{error}</Notice>}
      {!balance && !error && <Spinner />}
      {balance && !balance.payouts && (
        <Notice tone="info">This server doesn&apos;t hold balances, so released payments go straight to the seller&apos;s own node.</Notice>
      )}
      {balance?.payouts && (
        <>
          {node?.mode === "own" && (
            <Notice tone="info">
              Your own node is connected, so new escrows pay you there directly and don&apos;t add to this balance.
              Anything below is from escrows settled before you connected it.{" "}
              <Link href="/node" className="font-semibold underline">
                My node
              </Link>
            </Notice>
          )}
          <Totals balance={balance} />
          <div className="grid grid-cols-1 items-start gap-6 lg:grid-cols-2">
            <LightningWithdraw balance={balance} onDone={load} />
            {ramp?.enabled && ramp.offramp ? (
              <BankWithdraw balance={balance} ramp={ramp} onDone={load} />
            ) : (
              ramp && (
                <Card title="To a bank account">
                  <p className="text-sm text-neutral-500">Bank payouts in naira aren&apos;t offered on this server.</p>
                </Card>
              )
            )}
          </div>
          <History withdrawals={withdrawals} orders={orders} />
        </>
      )}
    </div>
  );
}

function Totals({ balance }: { balance: Payable }) {
  const rows = [
    { label: "Earned", value: balance.earned_sats, note: "From released escrows" },
    { label: "On the way", value: balance.held_sats, note: "Withdrawals still being paid" },
    { label: "Withdrawn", value: balance.withdrawn_sats, note: "Including routing fees" },
  ];
  return (
    <section className="grid gap-4 sm:grid-cols-[1.4fr_1fr_1fr_1fr]">
      <div className="rounded-2xl bg-neutral-950 p-5 text-white shadow-sm dark:bg-neutral-900">
        <div className="text-xs font-semibold uppercase tracking-[0.12em] text-amber-400">Available</div>
        <div className="mt-2 text-3xl font-bold tabular-nums">{formatSats(balance.available_sats)}</div>
        <div className="mt-1 text-xs text-neutral-400">Yours to withdraw at any time</div>
      </div>
      {rows.map((r) => (
        <div key={r.label} className="rounded-2xl border border-neutral-200 bg-white p-5 shadow-sm dark:border-neutral-800 dark:bg-neutral-900/60">
          <div className="text-xs font-semibold uppercase tracking-[0.12em] text-neutral-500">{r.label}</div>
          <div className="mt-2 text-xl font-semibold tabular-nums">{formatSats(r.value)}</div>
          <div className="mt-1 text-xs text-neutral-500">{r.note}</div>
        </div>
      ))}
    </section>
  );
}

function LightningWithdraw({ balance, onDone }: { balance: Payable; onDone: () => Promise<void> }) {
  const [invoice, setInvoice] = useState("");
  const [demoAmount, setDemoAmount] = useState("");
  const [busy, setBusy] = useState<"pay" | "demo" | null>(null);
  const [msg, setMsg] = useState<{ tone: "success" | "error"; text: string } | null>(null);
  const max = maxWithdrawal(balance);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!invoice.trim()) return setMsg({ tone: "error", text: "Paste a Lightning invoice from your wallet." });
    setBusy("pay");
    setMsg(null);
    try {
      const w = await api.withdraw(invoice);
      setInvoice("");
      setMsg({
        tone: "success",
        text:
          w.state === "succeeded"
            ? `${formatSats(w.amount_sats)} sent to your wallet.`
            : `Sending ${formatSats(w.amount_sats)}. It shows below as it goes through.`,
      });
      await onDone();
    } catch (err) {
      setMsg({ tone: "error", text: describeError(err) });
    } finally {
      setBusy(null);
    }
  }

  async function makeDemoInvoice() {
    const amount = Number(demoAmount || max);
    if (!Number.isInteger(amount) || amount < 1) return setMsg({ tone: "error", text: "Enter a whole number of sats." });
    setBusy("demo");
    setMsg(null);
    try {
      setInvoice(await demoInvoice(amount));
    } catch (err) {
      setMsg({ tone: "error", text: err instanceof Error ? err.message : "The demo wallet couldn't make an invoice." });
    } finally {
      setBusy(null);
    }
  }

  return (
    <Card title="To a Lightning wallet">
      <form onSubmit={submit} className="space-y-4" noValidate>
        <p className="text-sm text-neutral-600 dark:text-neutral-400">
          In your wallet, choose <em>Receive</em> and make an invoice for the amount you want, then paste it here. You
          can take up to <strong>{formatSats(max)}</strong> now; a routing-fee allowance of{" "}
          {balance.fee_allowance.percent}% (at least {balance.fee_allowance.min_sats} sats) is held while it is paid,
          and only what the payment actually cost is kept.
        </p>
        <Field label="Lightning invoice">
          <textarea
            value={invoice}
            onChange={(e) => setInvoice(e.target.value)}
            rows={3}
            spellCheck={false}
            autoComplete="off"
            placeholder="lnbc…"
            className={`${inputClass} font-mono text-xs`}
          />
        </Field>
        {hasDemoWallet && (
          <div className="flex flex-wrap items-end gap-2 rounded-lg border border-dashed border-neutral-300 p-3 dark:border-neutral-700">
            <Field label="Demo wallet: make an invoice for">
              <input
                value={demoAmount}
                onChange={(e) => setDemoAmount(e.target.value.replace(/\D/g, ""))}
                inputMode="numeric"
                placeholder={String(max)}
                className={`${inputClass} w-36`}
              />
            </Field>
            <Button variant="secondary" onClick={makeDemoInvoice} busy={busy === "demo"} disabled={max < 1}>
              Make invoice
            </Button>
          </div>
        )}
        {msg && <Notice tone={msg.tone}>{msg.text}</Notice>}
        <Button type="submit" busy={busy === "pay"} disabled={balance.available_sats <= 0}>
          Withdraw
        </Button>
      </form>
    </Card>
  );
}

function BankWithdraw({ balance, ramp, onDone }: { balance: Payable; ramp: Ramp; onDone: () => Promise<void> }) {
  const [account, setAccount] = useState<BankAccount>({ bankCode: "", accountNumber: "", resolved: null });
  const [amount, setAmount] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ tone: "success" | "error"; text: string } | null>(null);
  const max = maxWithdrawal(balance);
  const sats = Number(amount) || null;
  const { quote, error: quoteError } = useRampQuote("offramp", sats);
  const tooSmall = sats !== null && sats < ramp.min_sats;
  const tooBig = sats !== null && sats > max;

  async function send() {
    if (!sats || !account.resolved) return;
    setBusy(true);
    setMsg(null);
    try {
      const o = await api.startOfframp(sats, account.resolved.bank_code, account.resolved.account_number);
      setAmount("");
      setMsg({
        tone: "success",
        text: `${money(o.fiat_minor, o)} is on its way to ${o.bank.account_name}. Follow it below.`,
      });
      await onDone();
    } catch (err) {
      setMsg({ tone: "error", text: describeError(err) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card title="To a Nigerian bank account">
      <div className="space-y-4">
        {ramp.demo && <DemoBankNotice />}
        <BankAccountFields banks={ramp.banks} value={account} onChange={setAccount} />
        <Field
          label="Amount in sats"
          hint={`From ${formatSats(ramp.min_sats)} up to ${formatSats(max)}.`}
          error={tooSmall ? `The smallest bank payout is ${formatSats(ramp.min_sats)}.` : tooBig ? "That is more than you can withdraw now." : undefined}
        >
          <div className="flex gap-2">
            <input
              value={amount}
              onChange={(e) => setAmount(e.target.value.replace(/\D/g, ""))}
              inputMode="numeric"
              placeholder={String(max)}
              className={inputClass}
            />
            <Button variant="secondary" onClick={() => setAmount(String(max))} disabled={max < 1}>
              Max
            </Button>
          </div>
        </Field>
        {quote && !tooSmall && !tooBig && <QuoteLines quote={quote} total="You receive" />}
        {quoteError && !tooSmall && <Notice tone="error">{quoteError}</Notice>}
        {msg && <Notice tone={msg.tone}>{msg.text}</Notice>}
        <ConfirmButton
          variant="primary"
          busy={busy}
          disabled={!account.resolved || !quote || tooSmall || tooBig}
          confirmLabel="Yes, send it"
          confirmText={
            quote && account.resolved ? (
              <>
                Send <strong>{money(quote.fiat_minor, quote)}</strong> ({formatSats(quote.amount_sats)}) to{" "}
                <strong>{account.resolved.account_name}</strong> at {account.resolved.bank_name}? A bank transfer
                can&apos;t be taken back.
              </>
            ) : (
              ""
            )
          }
          onConfirm={send}
        >
          Withdraw to bank
        </ConfirmButton>
      </div>
    </Card>
  );
}

const withdrawalChip: Record<Withdrawal["state"], { label: string; cls: string }> = {
  pending: { label: "Sending", cls: "bg-amber-100 text-amber-900 dark:bg-amber-950 dark:text-amber-200" },
  succeeded: { label: "Sent", cls: "bg-emerald-100 text-emerald-900 dark:bg-emerald-950 dark:text-emerald-200" },
  failed: { label: "Failed, returned", cls: "bg-red-100 text-red-900 dark:bg-red-950 dark:text-red-200" },
};

/** Lightning withdrawals and bank payouts together, newest first. */
function History({ withdrawals, orders }: { withdrawals: Withdrawal[]; orders: RampOrder[] }) {
  const items = [
    ...withdrawals.filter((w) => w.kind === "lightning").map((w) => ({ at: w.created_at, key: w.id, w })),
    ...orders.map((o) => ({ at: o.created_at, key: o.id, o })),
  ].sort((a, b) => b.at - a.at);

  return (
    <Card title="History">
      {items.length === 0 ? (
        <p className="text-sm text-neutral-500">No withdrawals yet.</p>
      ) : (
        <ul className="divide-y divide-neutral-200 dark:divide-neutral-800">
          {items.map((i) => (
            <li key={i.key} className="flex flex-wrap items-start justify-between gap-3 py-3 text-sm">
              {"w" in i && i.w ? (
                <>
                  <div className="min-w-0">
                    <div className="font-medium">To a Lightning wallet</div>
                    <div className="text-xs text-neutral-500">
                      {formatLocalTime(i.w.created_at)}
                      {i.w.fee_sats !== null && i.w.state === "succeeded" && ` · routing fee ${formatSats(i.w.fee_sats)}`}
                    </div>
                    {i.w.failure && <div className="text-xs text-red-700 first-letter:uppercase dark:text-red-400">{i.w.failure}. The sats are back in your balance.</div>}
                  </div>
                  <div className="flex items-center gap-3">
                    <span className="font-semibold tabular-nums">{formatSats(i.w.amount_sats)}</span>
                    <span className={`rounded-full px-2 py-0.5 text-xs font-semibold ${withdrawalChip[i.w.state].cls}`}>
                      {withdrawalChip[i.w.state].label}
                    </span>
                  </div>
                </>
              ) : (
                "o" in i &&
                i.o && (
                  <>
                    <div className="min-w-0">
                      <div className="font-medium">
                        To {i.o.bank.account_name} · {i.o.bank.bank_name} {i.o.bank.account_number}
                      </div>
                      <div className="text-xs text-neutral-500">
                        {formatLocalTime(i.o.created_at)} · {formatSats(i.o.amount_sats)}
                        {i.o.fiat_reference && ` · ref ${i.o.fiat_reference}`}
                      </div>
                      {i.o.state === "failed" && i.o.note && (
                        <div className="text-xs text-red-700 first-letter:uppercase dark:text-red-400">{i.o.note}</div>
                      )}
                    </div>
                    <div className="flex items-center gap-3">
                      <span className="font-semibold tabular-nums">{money(i.o.fiat_minor, i.o)}</span>
                      <RampChip state={i.o.state} />
                    </div>
                  </>
                )
              )}
            </li>
          ))}
        </ul>
      )}
    </Card>
  );
}
