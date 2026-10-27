"use client";

import Link from "next/link";
import type { ReactNode } from "react";
import { formatLocalTime, formatSats, serverNow, useServerNow } from "@/lib/clock";
import { nextLine, shortDuration, shortId, todoFor } from "@/lib/dashboard";
import { formatFiat, formatRate } from "@/lib/fiat";
import { lifecycle, splitDescription, type Step, type Tone } from "@/lib/lifecycle";
import type { Escrow } from "@/lib/schemas";
import { RoleBadge, panel } from "../dashboard/Widgets";
import { ArrowIcon, CheckIcon, ClockIcon, UndoIcon } from "../landing/icons";
import { CopyField, StateBadge } from "../ui";

// Header

/** Back link, the title, who you are in this deal, its status, and the amount. */
export function EscrowHeader({ escrow }: { escrow: Escrow }) {
  const { title } = splitDescription(escrow.description);
  return (
    <header className="space-y-4">
      <Link href="/" className="inline-flex items-center gap-1.5 text-sm font-medium text-neutral-500 hover:text-neutral-900 dark:hover:text-neutral-100">
        <ArrowIcon className="size-4 rotate-180" /> Dashboard
      </Link>
      <div className={`${panel} flex flex-wrap items-start justify-between gap-x-8 gap-y-5 p-6`}>
        <div className="min-w-0 flex-1 basis-72">
          <div className="flex flex-wrap items-center gap-2 text-xs text-neutral-500">
            <span>Escrow · you are the</span>
            <RoleBadge role={escrow.your_role} />
            <span className="font-mono text-neutral-400">#{shortId(escrow.id)}</span>
            {escrow.reference && (
              <span className="rounded-md bg-neutral-100 px-2 py-0.5 font-medium text-neutral-700 dark:bg-neutral-800 dark:text-neutral-300">
                Order {escrow.reference}
              </span>
            )}
          </div>
          <h1 className="mt-2 break-words text-2xl font-bold tracking-tight sm:text-3xl">{title}</h1>
          <div className="mt-3">
            <StateBadge state={escrow.state} />
          </div>
        </div>
        <div className="sm:text-right">
          <div className="text-xs font-semibold uppercase tracking-[0.12em] text-neutral-500">Amount</div>
          <div className="mt-1 font-mono text-3xl font-semibold tabular-nums tracking-tight sm:text-4xl">
            {formatSats(escrow.amount_sats)}
          </div>
          {escrow.fiat && (
            <div
              className="mt-1 text-sm text-neutral-600 dark:text-neutral-400"
              title="Display only. Worked out once when the offer was made and never updated; the price of bitcoin moves, so what this sum is worth now will differ."
            >
              ≈ {formatFiat(escrow.fiat.amount_minor, escrow.fiat.currency, escrow.fiat.exponent)} when offered
              <span className="block text-xs text-neutral-500">
                at {formatRate(escrow.fiat.rate_minor_per_btc, escrow.fiat.currency, escrow.fiat.exponent)} per bitcoin ·{" "}
                {escrow.fiat.source} · {formatLocalTime(escrow.fiat.quoted_at)} · display only
              </span>
            </div>
          )}
        </div>
      </div>
    </header>
  );
}

// Progress tracker

const toneColor: Record<Tone, { done: string; ended: string }> = {
  progress: { done: "bg-amber-500 text-black", ended: "bg-neutral-400 text-white dark:bg-neutral-600" },
  success: { done: "bg-emerald-500 text-black", ended: "bg-neutral-400 text-white" },
  refund: { done: "bg-amber-500 text-black", ended: "bg-violet-500 text-white" },
  stopped: { done: "bg-amber-500 text-black", ended: "bg-neutral-500 text-white" },
  attention: { done: "bg-amber-500 text-black", ended: "bg-red-500 text-white" },
};

function Dot({ step, index, tone, last }: { step: Step; index: number; tone: Tone; last: boolean }) {
  const base = "relative grid size-9 place-items-center rounded-full text-sm font-bold";
  if (step.status === "done") {
    // The final step turning green is what "paid" looks like; every earlier done step is amber.
    const color = last && tone === "success" ? toneColor.success.done : toneColor.progress.done;
    return (
      <span className={`${base} ${color}`}>
        <CheckIcon className="size-5" />
      </span>
    );
  }
  if (step.status === "current") {
    return (
      <span className={`${base} bg-white text-amber-700 ring-2 ring-amber-500 dark:bg-neutral-900 dark:text-amber-400`}>
        <span aria-hidden className="absolute inset-0 rounded-full bg-amber-500/30 motion-safe:animate-ping" />
        <span className="relative">{index + 1}</span>
      </span>
    );
  }
  if (step.status === "ended") {
    const color = toneColor[tone].ended;
    return (
      <span className={`${base} ${color}`}>
        {tone === "refund" ? <UndoIcon className="size-5" /> : <span className="text-base leading-none">×</span>}
      </span>
    );
  }
  return (
    <span className={`${base} border-2 border-neutral-300 bg-white text-neutral-400 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-600`}>
      {index + 1}
    </span>
  );
}

/** The four steps of an escrow, and where this one is. */
export function Progress({ escrow }: { escrow: Escrow }) {
  const { steps, tone } = lifecycle(escrow.state, escrow.payment_hash !== null);
  return (
    <nav aria-label="Escrow progress" className={`${panel} px-4 py-6 sm:px-8`}>
      <ol className="grid grid-cols-4">
        {steps.map((s, i) => {
          const reachedBefore = i > 0 && steps[i - 1].status === "done";
          const leaves = s.status === "done";
          return (
            <li key={s.key} aria-current={s.status === "current" ? "step" : undefined} className="relative flex flex-col items-center px-1 text-center">
              {i > 0 && (
                <span aria-hidden className={`absolute left-0 right-1/2 top-[1.0625rem] h-0.5 ${reachedBefore ? "bg-amber-500" : "bg-neutral-200 dark:bg-neutral-800"}`} />
              )}
              {i < steps.length - 1 && (
                <span aria-hidden className={`absolute left-1/2 right-0 top-[1.0625rem] h-0.5 ${leaves ? "bg-amber-500" : "bg-neutral-200 dark:bg-neutral-800"}`} />
              )}
              <Dot step={s} index={i} tone={tone} last={i === steps.length - 1} />
              <span
                className={`mt-2 text-xs font-semibold sm:text-sm ${
                  s.status === "upcoming" ? "text-neutral-400 dark:text-neutral-600" : "text-neutral-900 dark:text-neutral-100"
                }`}
              >
                {s.label}
              </span>
              {s.note && <span className="mt-0.5 text-[11px] font-medium text-orange-700 dark:text-orange-400">{s.note}</span>}
            </li>
          );
        })}
      </ol>
    </nav>
  );
}

// Your move

/** One line: what you need to do now, or what everyone is waiting on. Nothing once the deal is over. */
export function YourMove({ escrow }: { escrow: Escrow }) {
  const tick = useServerNow();
  const now = Math.floor(tick ?? serverNow());
  const todo = todoFor(escrow, now);

  if (todo) {
    const left = todo.by === null ? null : todo.by - now;
    return (
      <section
        aria-label="Your move"
        className="flex items-start gap-4 rounded-2xl border border-amber-400 bg-amber-50 p-5 ring-1 ring-amber-400/40 dark:border-amber-500/60 dark:bg-amber-500/10"
      >
        <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-amber-500 text-black">
          <ClockIcon className="size-5" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="text-xs font-semibold uppercase tracking-[0.12em] text-amber-800 dark:text-amber-300">Your move</div>
          <div className="mt-0.5 text-lg font-semibold">{todo.title}</div>
          <p className="text-sm text-neutral-700 dark:text-neutral-300">{todo.detail}</p>
        </div>
        {left !== null && left > 0 && (
          <div className="shrink-0 text-right">
            <div className={`font-mono text-lg font-semibold tabular-nums ${left < 600 ? "text-red-600 dark:text-red-400" : ""}`}>
              {shortDuration(left)}
            </div>
            <div className="text-xs text-neutral-500">left</div>
          </div>
        )}
      </section>
    );
  }

  // A finished deal already says how it ended, in the progress tracker and in the card below.
  if (!["CREATED", "INVOICE_ISSUED", "LOCKED", "DISPUTED"].includes(escrow.state)) return null;
  return (
    <section
      aria-label="Your move"
      className="rounded-2xl border border-neutral-200 bg-white px-5 py-4 text-sm font-medium text-neutral-700 dark:border-neutral-800 dark:bg-neutral-900/60 dark:text-neutral-300"
    >
      {nextLine(escrow)}
    </section>
  );
}

// The rail

function Rail({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className={`${panel} p-5`}>
      <h2 className="text-xs font-semibold uppercase tracking-[0.12em] text-neutral-500">{title}</h2>
      <div className="mt-3">{children}</div>
    </section>
  );
}

/** Where the funds go, and the dates that matter. */
export function DetailsCard({ escrow }: { escrow: Escrow }) {
  const ended = !["CREATED", "INVOICE_ISSUED", "LOCKED", "DISPUTED"].includes(escrow.state);
  return (
    <Rail title="Details">
      <dl className="space-y-3 text-sm">
        <div>
          <dt className="text-neutral-500">Funds go to</dt>
          <dd className="font-medium">
            {escrow.seller_node === "own" ? (
              <span className="text-emerald-700 dark:text-emerald-400">The seller&apos;s own node</span>
            ) : (
              <span className="text-amber-700 dark:text-amber-400" title="The service receives the sats and owes them to the seller">
                The service&apos;s shared node (custodial)
              </span>
            )}
          </dd>
        </div>
        {escrow.reference && (
          <div>
            <dt className="text-neutral-500">Seller&apos;s order reference</dt>
            <dd className="break-words font-medium">{escrow.reference}</dd>
          </div>
        )}
        <div>
          <dt className="text-neutral-500">{ended ? "Deadline was" : "Release deadline"}</dt>
          <dd>{formatLocalTime(escrow.deadline_at)}</dd>
        </div>
        <div>
          <dt className="text-neutral-500">Created</dt>
          <dd>{formatLocalTime(escrow.created_at)}</dd>
        </div>
      </dl>
    </Rail>
  );
}

/** The address of a link the seller set, or null when it is not a plain web link. */
function linkHost(url: string): string | null {
  try {
    const u = new URL(url);
    return u.protocol === "https:" || u.protocol === "http:" ? u.host : null;
  } catch {
    return null;
  }
}

/**
 * A way back to the seller's own site, for a buyer who came from a business's checkout. The seller
 * set it, so it is shown with its address spelled out and is only ever followed by a click.
 */
export function ReturnLink({ escrow }: { escrow: Escrow }) {
  const host = escrow.return_url ? linkHost(escrow.return_url) : null;
  if (!escrow.return_url || !host || escrow.your_role !== "buyer" || escrow.state === "CREATED") return null;
  return (
    <Rail title="Back to the seller">
      <div className="space-y-3 text-sm">
        <a
          href={escrow.return_url}
          rel="noopener noreferrer"
          className="inline-flex min-h-10 w-full items-center justify-center gap-2 rounded-lg bg-neutral-900 px-4 py-2 text-sm font-semibold text-white hover:bg-neutral-700 dark:bg-neutral-100 dark:text-neutral-900 dark:hover:bg-neutral-300"
        >
          Return to {host} <ArrowIcon className="size-4" />
        </a>
        <p className="text-xs text-neutral-500">
          The seller chose this link. Your escrow stays here and carries on whether or not you follow it. Check the
          address before you go.
        </p>
      </div>
    </Rail>
  );
}

/** Who is in the deal. */
export function PeopleCard({ escrow }: { escrow: Escrow }) {
  const you = (role: Escrow["your_role"]) =>
    escrow.your_role === role ? (
      <span className="ml-2 rounded bg-neutral-200 px-1.5 py-0.5 text-[10px] font-semibold uppercase text-neutral-700 dark:bg-neutral-800 dark:text-neutral-300">
        you
      </span>
    ) : null;
  return (
    <Rail title="People">
      <dl className="space-y-3 text-sm">
        <div>
          <dt className="text-neutral-500">Buyer{you("buyer")}</dt>
          <dd className="break-all font-mono text-xs">{escrow.buyer_id ?? "Not chosen yet (open offer)"}</dd>
        </div>
        <div>
          <dt className="text-neutral-500">Seller{you("seller")}</dt>
          <dd className="break-all font-mono text-xs">{escrow.seller_id}</dd>
        </div>
        {escrow.arbiter_id && (
          <div>
            <dt className="text-neutral-500">Arbiter{you("arbiter")}</dt>
            <dd className="text-sm">
              <span className="break-all font-mono text-xs">{escrow.arbiter_id}</span>
              {" · "}
              {escrow.arbiter_confirmed_at ? (
                <span className="font-medium text-emerald-700 dark:text-emerald-400">confirmed</span>
              ) : (
                <span className="font-medium text-amber-700 dark:text-amber-400">not confirmed yet</span>
              )}
              <div className="mt-0.5 break-words font-mono text-[11px] text-neutral-500">key {escrow.arbiter_fingerprint}</div>
            </dd>
          </div>
        )}
      </dl>
    </Rail>
  );
}

/** The long part of a description, when there is one. */
export function Terms({ escrow }: { escrow: Escrow }) {
  const { body } = splitDescription(escrow.description);
  if (!body) return null;
  return (
    <section className={`${panel} p-5`} aria-label="Terms">
      <h2 className="text-xs font-semibold uppercase tracking-[0.12em] text-neutral-500">Terms</h2>
      <p className="mt-3 whitespace-pre-wrap break-words text-sm leading-relaxed">{body}</p>
    </section>
  );
}

/** Ids and the payment hash, tucked away. */
export function TechnicalCard({ escrow }: { escrow: Escrow }) {
  return (
    <details className={`${panel} group p-5 text-sm`}>
      <summary className="flex cursor-pointer list-none items-center justify-between text-xs font-semibold uppercase tracking-[0.12em] text-neutral-500">
        Technical details
        <ArrowIcon aria-hidden className="size-4 rotate-90 transition-transform group-open:-rotate-90" />
      </summary>
      <div className="mt-3 space-y-3">
        <div>
          <div className="mb-1 text-neutral-500">Escrow id</div>
          <CopyField value={escrow.id} />
        </div>
        {escrow.payment_hash && (
          <div>
            <div className="mb-1 text-neutral-500">Payment hash</div>
            <CopyField value={escrow.payment_hash} />
          </div>
        )}
      </div>
    </details>
  );
}
