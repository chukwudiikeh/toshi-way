"use client";

import { useRouter } from "next/navigation";
import Link from "next/link";
import { useEffect, useRef, useState, type FormEvent } from "react";
import { api, ApiError, describeError } from "@/lib/api";
import { formatFiat, formatRate, valueOfSats } from "@/lib/fiat";
import { MAX_DESCRIPTION, NewEscrowForm, type NodeInfo, type RateQuote, type RatesInfo } from "@/lib/schemas";
import { useSession } from "@/lib/session";
import { SignIn } from "./Account";
import { Button, Card, Field, inputClass, Notice, Spinner } from "./ui";

type Values = {
  buyer_id: string;
  arbiter_id: string;
  fiat_currency: string;
  amount_sats: string;
  description: string;
  hold_amount: string;
  hold_unit: "minutes" | "hours";
};

const initial: Values = { buyer_id: "", arbiter_id: "", fiat_currency: "", amount_sats: "", description: "", hold_amount: "1", hold_unit: "hours" };

// Server error codes that belong to a particular field.
const fieldOfCode: Record<string, keyof Values> = {
  unknown_buyer: "buyer_id",
  unknown_arbiter: "arbiter_id",
  unsupported_currency: "fiat_currency",
  invalid_arbiter: "arbiter_id",
  arbiter_has_no_key: "arbiter_id",
  invalid_amount: "amount_sats",
  invalid_description: "description",
  invalid_deadline: "hold_amount",
};

export function NewEscrow() {
  const session = useSession();
  const router = useRouter();
  const [values, setValues] = useState<Values>(initial);
  const [errors, setErrors] = useState<Partial<Record<keyof Values, string>>>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // One key per submission: retrying the same values reuses it, so a lost response can't create a
  // second escrow. Editing the form starts a new submission.
  const idempotencyKey = useRef<string | null>(null);
  const [node, setNode] = useState<NodeInfo | null>(null);
  const [rates, setRates] = useState<RatesInfo | null>(null);
  const [quote, setQuote] = useState<RateQuote | null>(null);
  const [quoteError, setQuoteError] = useState<string | null>(null);

  useEffect(() => {
    if (!session || session.kind !== "user") return;
    let alive = true;
    // If this fails the server still answers "no_node" when it matters, so just carry on.
    api.node().then((n) => alive && setNode(n)).catch(() => {});
    // Whether fiat values are offered at all. If this fails the form simply has no currency choice.
    api.rates().then((r) => alive && setRates(r)).catch(() => {});
    return () => {
      alive = false;
    };
  }, [session]);

  // The price behind the preview, refreshed every minute while a currency is chosen. The server
  // takes its own rate when the escrow is created; this is only so people can see what to expect.
  const currency = values.fiat_currency;
  useEffect(() => {
    if (!currency || !session || session.kind !== "user") return;
    let alive = true;
    const load = () =>
      api
        .rate(currency)
        .then((q) => {
          if (!alive) return;
          setQuote(q);
          setQuoteError(null);
        })
        .catch((err) => alive && setQuoteError(describeError(err)));
    load();
    const t = setInterval(load, 60_000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [currency, session]);

  if (session === undefined) return <Spinner />;
  if (!session) return <SignIn note="Sign in as the seller to create an escrow." />;
  if (session.kind === "operator") {
    return <Notice tone="info">Operators can&apos;t be a party to an escrow. Sign in with a user token instead.</Notice>;
  }

  function set<K extends keyof Values>(key: K, value: Values[K]) {
    setValues((v) => ({ ...v, [key]: value }));
    setErrors((e) => ({ ...e, [key]: undefined }));
    idempotencyKey.current = null;
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    setFormError(null);
    const parsed = NewEscrowForm.safeParse(values);
    if (!parsed.success) {
      const next: Partial<Record<keyof Values, string>> = {};
      for (const issue of parsed.error.issues) {
        const key = issue.path[0] as keyof Values;
        next[key] ??= issue.message;
      }
      setErrors(next);
      return;
    }
    if (parsed.data.arbiter_id && parsed.data.arbiter_id === session?.userId) {
      setErrors({ arbiter_id: "You can't be your own arbiter." });
      return;
    }
    if (parsed.data.buyer_id && parsed.data.buyer_id === session?.userId) {
      setErrors({ buyer_id: "That's your own id. Enter the buyer's id." });
      return;
    }
    idempotencyKey.current ??= crypto.randomUUID();
    setBusy(true);
    try {
      const escrow = await api.create(parsed.data, idempotencyKey.current);
      router.push(`/escrows/${escrow.id}`);
    } catch (err) {
      const field = err instanceof ApiError ? fieldOfCode[err.code] : undefined;
      if (field) setErrors({ [field]: describeError(err) });
      else setFormError(describeError(err));
      if (err instanceof ApiError && err.code === "idempotency_key_reuse") idempotencyKey.current = null;
      setBusy(false);
    }
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-xl font-bold">New escrow</h1>
        <p className="text-sm text-neutral-600 dark:text-neutral-400">
          You are the seller. The buyer accepts the offer, pays, and the payment stays locked until they hand you
          the release secret, or until the hold time runs out and they are refunded.
        </p>
      </div>
      {node?.mode === "none" && (
        <Notice tone="warn">
          You need to <Link href="/node" className="font-semibold underline">connect your Lightning node</Link> before
          you can create an escrow: this service doesn&apos;t hold sellers&apos; money.
        </Notice>
      )}
      {node?.mode === "shared" && (
        <Notice tone="info">
          No node of yours is connected, so this escrow will use the service&apos;s shared node: the service receives
          the sats on release and owes them to you.{" "}
          <Link href="/node" className="font-semibold underline">
            Connect your own node
          </Link>{" "}
          to be paid directly.
        </Notice>
      )}
      <Card>
        <form onSubmit={submit} className="space-y-4" noValidate>
          <Field
            label="Buyer's user id (optional)"
            hint="Leave empty to get a link anyone can open: the first person to accept becomes the buyer. Fill it in to invite one specific person."
            error={errors.buyer_id}
          >
            <input
              value={values.buyer_id}
              onChange={(e) => set("buyer_id", e.target.value)}
              className={`${inputClass} font-mono`}
              placeholder="u_…"
              autoComplete="off"
              spellCheck={false}
            />
          </Field>
          <Field
            label="Arbiter's user id (optional)"
            hint="Someone you and the buyer both trust to decide a dispute. Without one, a dispute is just flagged for the operator and the buyer's refund is the default. They must have set up a key on the Arbiter page."
            error={errors.arbiter_id}
          >
            <input
              value={values.arbiter_id}
              onChange={(e) => set("arbiter_id", e.target.value)}
              className={`${inputClass} font-mono`}
              placeholder="u_…"
              autoComplete="off"
              spellCheck={false}
            />
          </Field>
          <Field label="Amount (sats)" hint="Between 1 and 1,000,000 sats." error={errors.amount_sats}>
            <input
              value={values.amount_sats}
              onChange={(e) => set("amount_sats", e.target.value)}
              className={inputClass}
              inputMode="numeric"
              placeholder="1000"
            />
          </Field>
          {rates?.enabled && (
            <Field
              label="Also show the value in (optional)"
              hint="Display only: the escrow is in sats. The value is worked out once, now, and never changes."
              error={errors.fiat_currency}
            >
              <select
                value={values.fiat_currency}
                onChange={(e) => {
                  setQuote(null);
                  setQuoteError(null);
                  set("fiat_currency", e.target.value);
                }}
                className={inputClass}
              >
                <option value="">No fiat value</option>
                {rates.currencies.map((c) => (
                  <option key={c.code} value={c.code}>
                    {c.code}
                  </option>
                ))}
              </select>
              {currency && <FiatPreview sats={values.amount_sats} quote={quote} error={quoteError} />}
            </Field>
          )}
          <Field
            label="Description"
            hint={`${values.description.length}/${MAX_DESCRIPTION} characters. Both parties see it.`}
            error={errors.description}
          >
            <textarea
              value={values.description}
              onChange={(e) => set("description", e.target.value)}
              className={inputClass}
              rows={3}
              maxLength={MAX_DESCRIPTION}
              placeholder="What is being sold or delivered"
            />
          </Field>
          <Field
            label="Hold time"
            hint="How long the payment stays locked once paid. If the buyer hasn't released by then, they are refunded. 1 minute to 6 hours."
            error={errors.hold_amount}
          >
            <div className="flex gap-2">
              <input
                aria-label="Hold time amount"
                value={values.hold_amount}
                onChange={(e) => set("hold_amount", e.target.value)}
                className={`${inputClass} max-w-32`}
                inputMode="decimal"
              />
              <select
                aria-label="Hold time unit"
                value={values.hold_unit}
                onChange={(e) => set("hold_unit", e.target.value as Values["hold_unit"])}
                className={`${inputClass} max-w-40`}
              >
                <option value="minutes">minutes</option>
                <option value="hours">hours</option>
              </select>
            </div>
          </Field>
          {formError && <Notice tone="error">{formError}</Notice>}
          <Button type="submit" busy={busy} disabled={node?.mode === "none"}>
            Create escrow
          </Button>
        </form>
      </Card>
    </div>
  );
}

/** What the sum is worth now, so the seller knows what to expect. The real value is fixed on creation. */
function FiatPreview({ sats, quote, error }: { sats: string; quote: RateQuote | null; error: string | null }) {
  if (error) return <p className="mt-2 text-xs text-red-700 dark:text-red-400">{error}</p>;
  if (!quote) return <p className="mt-2 text-xs text-neutral-500">Fetching the price…</p>;
  const amount = Number(sats);
  const valid = Number.isInteger(amount) && amount > 0 && amount <= 1_000_000;
  return (
    <p className="mt-2 text-sm">
      {valid ? (
        <>
          <strong>≈ {formatFiat(valueOfSats(amount, quote.rate_minor_per_btc), quote.currency, quote.exponent)}</strong>{" "}
          <span className="text-neutral-500">at</span>{" "}
        </>
      ) : null}
      <span className="text-neutral-500">
        {formatRate(quote.rate_minor_per_btc, quote.currency, quote.exponent)} per bitcoin ({quote.source})
      </span>
    </p>
  );
}
