"use client";

import { useEffect, useState } from "react";
import { api, describeError } from "@/lib/api";
import { formatFiat } from "@/lib/fiat";
import { NUBAN, type BankInfo, type RampOrder, type RampQuote, type RampState, type ResolvedAccount } from "@/lib/schemas";
import { Field, inputClass, Notice } from "./ui";

/** An amount in a ramp's currency, e.g. "₦65,000.00". */
export function money(amountMinor: number, o: { currency: string; exponent: number }): string {
  return formatFiat(amountMinor, o.currency, o.exponent);
}

const chip = {
  wait: "bg-sky-100 text-sky-900 dark:bg-sky-950 dark:text-sky-200",
  busy: "bg-amber-100 text-amber-900 dark:bg-amber-950 dark:text-amber-200",
  good: "bg-emerald-100 text-emerald-900 dark:bg-emerald-950 dark:text-emerald-200",
  back: "bg-violet-100 text-violet-900 dark:bg-violet-950 dark:text-violet-200",
  off: "bg-neutral-200 text-neutral-700 dark:bg-neutral-800 dark:text-neutral-300",
  bad: "bg-red-100 text-red-900 dark:bg-red-950 dark:text-red-200",
};

const states: Record<RampState, { label: string; tone: keyof typeof chip }> = {
  awaiting_deposit: { label: "Waiting for your transfer", tone: "wait" },
  deposit_received: { label: "Transfer received", tone: "busy" },
  paying: { label: "Paying the escrow", tone: "busy" },
  funded: { label: "Escrow paid", tone: "good" },
  complete: { label: "Done", tone: "good" },
  refunding: { label: "Refunding to your bank", tone: "back" },
  refunded: { label: "Refunded to your bank", tone: "back" },
  expired: { label: "No transfer arrived", tone: "off" },
  sending_sats: { label: "Converting", tone: "busy" },
  paying_out: { label: "Sending to your bank", tone: "busy" },
  paid_out: { label: "In your bank", tone: "good" },
  failed: { label: "Not sent", tone: "bad" },
};

const FINAL: ReadonlySet<RampState> = new Set(["complete", "refunded", "expired", "paid_out", "failed"]);

export const rampFinal = (o: RampOrder) => FINAL.has(o.state);

export function RampChip({ state }: { state: RampState }) {
  const s = states[state];
  return <span className={`rounded-full px-2 py-0.5 text-xs font-semibold whitespace-nowrap ${chip[s.tone]}`}>{s.label}</span>;
}

/** Shown wherever the provider is pretend, so nobody sends real money to it. */
export function DemoBankNotice() {
  return (
    <Notice tone="warn">
      <strong>Demo bank, no real money.</strong> This server uses a pretend payment provider for testing. Don&apos;t
      send a real transfer; use the demo button instead.
    </Notice>
  );
}

export type BankAccount = { bankCode: string; accountNumber: string; resolved: ResolvedAccount | null };

/**
 * A Nigerian bank and a 10-digit account number. As soon as both are filled in, the holder's name is
 * looked up so the person can check it is theirs before any money moves.
 */
export function BankAccountFields({
  banks,
  value,
  onChange,
  label = "Your bank",
  hint,
}: {
  banks: BankInfo[];
  value: BankAccount;
  onChange: (v: BankAccount) => void;
  label?: string;
  hint?: string;
}) {
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { bankCode, accountNumber, resolved } = value;
  const valid = NUBAN.safeParse(accountNumber);

  useEffect(() => {
    if (!bankCode || !valid.success || resolved) return;
    let alive = true;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- shows the lookup while it runs
    setChecking(true);
    setError(null);
    api
      .resolveAccount(bankCode, valid.data)
      .then((r) => alive && onChange({ bankCode, accountNumber, resolved: r }))
      .catch((err) => alive && setError(describeError(err)))
      .finally(() => alive && setChecking(false));
    return () => {
      alive = false;
    };
  }, [bankCode, accountNumber, valid.success, valid.data, resolved, onChange]);

  return (
    <div className="space-y-3">
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label={label} hint={hint}>
          <select
            value={bankCode}
            onChange={(e) => onChange({ bankCode: e.target.value, accountNumber, resolved: null })}
            className={inputClass}
          >
            <option value="">Choose a bank</option>
            {banks.map((b) => (
              <option key={b.code} value={b.code}>
                {b.name}
              </option>
            ))}
          </select>
        </Field>
        <Field
          label="Account number"
          error={accountNumber.length >= 10 && !valid.success ? valid.error.issues[0].message : undefined}
        >
          <input
            value={accountNumber}
            onChange={(e) =>
              onChange({ bankCode, accountNumber: e.target.value.replace(/\D/g, "").slice(0, 10), resolved: null })
            }
            inputMode="numeric"
            autoComplete="off"
            placeholder="0123456789"
            className={`${inputClass} font-mono tracking-wider`}
          />
        </Field>
      </div>
      {checking && <p className="text-sm text-neutral-500">Checking the account name…</p>}
      {error && <Notice tone="error">{error}</Notice>}
      {resolved && (
        <div className="flex items-center gap-2 rounded-lg bg-emerald-50 px-3 py-2 text-sm text-emerald-900 dark:bg-emerald-950/40 dark:text-emerald-200">
          <span className="size-2 rounded-full bg-emerald-500" />
          <span>
            <strong>{resolved.account_name}</strong> · {resolved.bank_name}
          </span>
        </div>
      )}
    </div>
  );
}

/** The live price for converting `sats`, refreshed as the amount changes. */
export function useRampQuote(direction: "onramp" | "offramp", sats: number | null) {
  const [quote, setQuote] = useState<RampQuote | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!sats || sats <= 0) {
      // eslint-disable-next-line react-hooks/set-state-in-effect -- no amount, no price
      setQuote(null);
      return;
    }
    let alive = true;
    const t = setTimeout(() => {
      api
        .rampQuote(direction, sats)
        .then((q) => {
          if (!alive) return;
          setQuote(q);
          setError(null);
        })
        .catch((err) => {
          if (!alive) return;
          setQuote(null);
          setError(describeError(err));
        });
    }, 300);
    return () => {
      alive = false;
      clearTimeout(t);
    };
  }, [direction, sats]);
  return { quote, error };
}

/** A converted amount with its fee and the rate, as a small receipt. */
export function QuoteLines({ quote, total }: { quote: RampQuote; total: string }) {
  return (
    <dl className="grid grid-cols-[1fr_auto] gap-x-4 gap-y-1 rounded-lg bg-neutral-100 px-4 py-3 text-sm dark:bg-neutral-800/60">
      <dt className="text-neutral-500">Rate</dt>
      <dd className="text-right tabular-nums">1 BTC = {money(quote.rate_minor_per_btc, quote)}</dd>
      <dt className="text-neutral-500">Fee</dt>
      <dd className="text-right tabular-nums">{money(quote.fee_minor, quote)}</dd>
      <dt className="font-semibold">{total}</dt>
      <dd className="text-right text-base font-bold tabular-nums">{money(quote.fiat_minor, quote)}</dd>
    </dl>
  );
}
