"use client";

import { useEffect, useState } from "react";
import { CheckIcon, ClockIcon, LockIcon } from "./icons";

// A picture of the product: one escrow moving through the stages a real one does. It is only a
// picture (nothing here is a real escrow), so it says so.
const STAGES = [
  {
    key: "offer",
    badge: "Awaiting payment",
    tone: "bg-sky-400/15 text-sky-200 ring-sky-300/30",
    line: "The buyer accepts and pays. The payment is about to be locked.",
  },
  {
    key: "locked",
    badge: "Funds locked",
    tone: "bg-amber-400/15 text-amber-200 ring-amber-300/30",
    line: "Locked on the Lightning network. The seller can start work.",
  },
  {
    key: "settled",
    badge: "Settled",
    tone: "bg-emerald-400/15 text-emerald-200 ring-emerald-300/30",
    line: "The buyer showed the QR. The seller was paid, to their own node.",
  },
] as const;

export function EscrowCard() {
  const [stage, setStage] = useState(1);

  useEffect(() => {
    // Respect people who ask for less motion: they get the "locked" stage and no cycling.
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    const t = setInterval(() => setStage((s) => (s + 1) % STAGES.length), 3200);
    return () => clearInterval(t);
  }, []);

  const current = STAGES[stage];
  return (
    <div className="relative">
      {/* A soft glow behind the card. */}
      <div aria-hidden className="absolute -inset-6 rounded-[2rem] bg-amber-500/20 blur-3xl" />
      <div className="relative overflow-hidden rounded-2xl border border-white/10 bg-neutral-900/80 p-5 shadow-2xl shadow-black/40 ring-1 ring-white/5 backdrop-blur sm:p-6">
        <div className="flex items-start justify-between gap-3">
          <div>
            <div className="text-[11px] font-medium uppercase tracking-wider text-neutral-400">Escrow · sample</div>
            <div className="mt-1 text-lg font-semibold text-white">Parcel #4821 to Lekki, Lagos</div>
          </div>
          <span
            className={`inline-flex shrink-0 items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-semibold ring-1 transition-colors duration-500 ${current.tone}`}
          >
            <span className="size-1.5 rounded-full bg-current" />
            {current.badge}
          </span>
        </div>

        <div className="mt-5 flex items-end justify-between gap-4">
          <div>
            <div className="font-mono text-3xl font-semibold tabular-nums text-white">5,000 sats</div>
            <div className="mt-0.5 text-sm text-neutral-400">≈ ₦4,875.00 when offered · display only</div>
          </div>
          <div className="flex items-center gap-1.5 rounded-lg bg-white/5 px-3 py-2 text-sm text-neutral-200 ring-1 ring-white/10">
            <ClockIcon className="size-4 text-amber-300" />
            <span className="font-mono tabular-nums">{stage === 2 ? "done" : "29m 41s"}</span>
          </div>
        </div>

        {/* The lifecycle, as a tracker. */}
        <ol className="mt-6 grid grid-cols-3 gap-2" aria-label="Escrow stages">
          {STAGES.map((s, i) => (
            <li key={s.key} className="space-y-2">
              <div className="h-1.5 overflow-hidden rounded-full bg-white/10">
                <div
                  className={`h-full rounded-full bg-amber-400 transition-all duration-700 ${i <= stage ? "w-full" : "w-0"}`}
                />
              </div>
              <div className={`text-[11px] font-medium ${i <= stage ? "text-neutral-100" : "text-neutral-500"}`}>
                {s.badge}
              </div>
            </li>
          ))}
        </ol>

        <p className="mt-5 min-h-10 text-sm leading-relaxed text-neutral-300">{current.line}</p>

        <dl className="mt-4 grid grid-cols-2 gap-3 border-t border-white/10 pt-4 text-xs">
          <div>
            <dt className="text-neutral-500">Buyer</dt>
            <dd className="mt-0.5 font-mono text-neutral-200">u_bb76…aea5</dd>
          </div>
          <div>
            <dt className="text-neutral-500">Seller</dt>
            <dd className="mt-0.5 font-mono text-neutral-200">u_4b2c…abd</dd>
          </div>
        </dl>

        <ul className="mt-4 flex flex-wrap gap-2">
          {[
            { icon: LockIcon, text: "Non-custodial" },
            { icon: CheckIcon, text: "Confirmed by the node" },
          ].map(({ icon: I, text }) => (
            <li
              key={text}
              className="inline-flex items-center gap-1.5 rounded-full bg-white/5 px-2.5 py-1 text-[11px] font-medium text-neutral-300 ring-1 ring-white/10"
            >
              <I className="size-3.5 text-amber-300" />
              {text}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
