"use client";

import { useState, type ComponentProps, type ReactNode } from "react";
import type { EscrowState } from "@/lib/schemas";
import { formatDuration, formatLocalTime, useServerNow } from "@/lib/clock";

type Variant = "primary" | "secondary" | "danger" | "ghost";

const variants: Record<Variant, string> = {
  primary: "bg-amber-500 text-black hover:bg-amber-400 disabled:bg-amber-500/50",
  secondary:
    "border border-neutral-300 bg-white text-neutral-900 hover:bg-neutral-100 dark:border-neutral-700 dark:bg-neutral-900 dark:text-neutral-100 dark:hover:bg-neutral-800",
  danger: "bg-red-600 text-white hover:bg-red-500 disabled:bg-red-600/50",
  ghost: "text-neutral-700 hover:bg-neutral-100 dark:text-neutral-300 dark:hover:bg-neutral-800",
};

export function Button({
  variant = "primary",
  busy = false,
  className = "",
  children,
  disabled,
  ...props
}: ComponentProps<"button"> & { variant?: Variant; busy?: boolean }) {
  return (
    <button
      type="button"
      {...props}
      disabled={disabled || busy}
      aria-busy={busy || undefined}
      className={`inline-flex min-h-10 items-center justify-center gap-2 rounded-lg px-4 py-2 text-sm font-semibold transition-colors disabled:cursor-not-allowed disabled:opacity-60 ${variants[variant]} ${className}`}
    >
      {busy && <span className="size-3 animate-spin rounded-full border-2 border-current border-t-transparent" />}
      {children}
    </button>
  );
}

export function Card({ title, children, className = "" }: { title?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section
      className={`rounded-xl border border-neutral-200 bg-white p-5 shadow-sm dark:border-neutral-800 dark:bg-neutral-900 ${className}`}
    >
      {title && <h2 className="mb-3 text-base font-semibold">{title}</h2>}
      {children}
    </section>
  );
}

export function Field({
  label,
  hint,
  error,
  children,
}: {
  label: string;
  hint?: ReactNode;
  error?: string;
  children: ReactNode;
}) {
  return (
    <label className="block space-y-1">
      <span className="text-sm font-medium">{label}</span>
      {children}
      {error ? (
        <span className="block text-sm text-red-600 dark:text-red-400">{error}</span>
      ) : (
        hint && <span className="block text-xs text-neutral-500">{hint}</span>
      )}
    </label>
  );
}

export const inputClass =
  "w-full rounded-lg border border-neutral-300 bg-white px-3 py-2 text-sm outline-none focus:border-amber-500 focus:ring-2 focus:ring-amber-500/30 dark:border-neutral-700 dark:bg-neutral-950";

export function Notice({
  tone = "info",
  children,
}: {
  tone?: "info" | "warn" | "error" | "success";
  children: ReactNode;
}) {
  const tones = {
    info: "border-sky-200 bg-sky-50 text-sky-900 dark:border-sky-900 dark:bg-sky-950/40 dark:text-sky-200",
    warn: "border-amber-300 bg-amber-50 text-amber-950 dark:border-amber-800 dark:bg-amber-950/40 dark:text-amber-100",
    error: "border-red-300 bg-red-50 text-red-900 dark:border-red-900 dark:bg-red-950/40 dark:text-red-200",
    success:
      "border-emerald-300 bg-emerald-50 text-emerald-900 dark:border-emerald-900 dark:bg-emerald-950/40 dark:text-emerald-200",
  };
  return (
    <div role={tone === "error" ? "alert" : "status"} className={`rounded-lg border px-4 py-3 text-sm ${tones[tone]}`}>
      {children}
    </div>
  );
}

export function CopyButton({ value, label = "Copy" }: { value: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <Button
      variant="secondary"
      className="shrink-0"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          setCopied(true);
          setTimeout(() => setCopied(false), 1500);
        } catch {
          // Clipboard blocked (e.g. plain http on a LAN address): the text stays selectable.
        }
      }}
    >
      {copied ? "Copied" : label}
    </Button>
  );
}

/** A value shown in monospace with a copy button; long values wrap rather than overflow. */
export function CopyField({ value, label }: { value: string; label?: string }) {
  return (
    <div className="flex items-start gap-2">
      <code className="min-w-0 flex-1 break-all rounded-lg bg-neutral-100 px-3 py-2 font-mono text-xs dark:bg-neutral-800">
        {value}
      </code>
      <CopyButton value={value} label={label} />
    </div>
  );
}

const stateStyles: Record<EscrowState, string> = {
  CREATED: "bg-neutral-200 text-neutral-800 dark:bg-neutral-800 dark:text-neutral-200",
  INVOICE_ISSUED: "bg-sky-100 text-sky-900 dark:bg-sky-950 dark:text-sky-200",
  LOCKED: "bg-amber-100 text-amber-900 dark:bg-amber-950 dark:text-amber-200",
  DISPUTED: "bg-orange-200 text-orange-950 dark:bg-orange-950 dark:text-orange-200",
  SETTLED: "bg-emerald-100 text-emerald-900 dark:bg-emerald-950 dark:text-emerald-200",
  REFUNDED: "bg-violet-100 text-violet-900 dark:bg-violet-950 dark:text-violet-200",
  EXPIRED_UNPAID: "bg-neutral-200 text-neutral-700 dark:bg-neutral-800 dark:text-neutral-300",
  FAILED: "bg-red-100 text-red-900 dark:bg-red-950 dark:text-red-200",
};

export const stateLabels: Record<EscrowState, string> = {
  CREATED: "Offer made",
  INVOICE_ISSUED: "Awaiting payment",
  LOCKED: "Funds locked",
  DISPUTED: "Disputed",
  SETTLED: "Settled",
  REFUNDED: "Refunded",
  EXPIRED_UNPAID: "Ended unpaid",
  FAILED: "Needs attention",
};

export function StateBadge({ state }: { state: EscrowState }) {
  return (
    <span
      title={state}
      className={`inline-flex items-center rounded-full px-2.5 py-0.5 text-xs font-semibold whitespace-nowrap ${stateStyles[state]}`}
    >
      {stateLabels[state]}
    </span>
  );
}

/** A countdown against the server-corrected clock, with the local time it ends. */
export function Countdown({ until, label, endedText }: { until: number; label: string; endedText: string }) {
  const now = useServerNow();
  const left = now === null ? null : until - now;
  return (
    <div className="rounded-lg bg-neutral-100 px-4 py-3 dark:bg-neutral-800/60">
      <div className="text-xs uppercase tracking-wide text-neutral-500">{label}</div>
      <div className="font-mono text-2xl font-semibold tabular-nums" aria-live="off">
        {left === null ? "…" : left > 0 ? formatDuration(left) : endedText}
      </div>
      <div className="text-xs text-neutral-500">
        {formatLocalTime(until)} (your local time; the server&apos;s clock decides)
      </div>
    </div>
  );
}

/** A button that asks for confirmation inline before running a consequential action. */
export function ConfirmButton({
  children,
  confirmText,
  confirmLabel,
  onConfirm,
  variant = "secondary",
  busy,
  disabled,
}: {
  children: ReactNode;
  confirmText: ReactNode;
  confirmLabel: string;
  onConfirm: () => void;
  variant?: Variant;
  busy?: boolean;
  disabled?: boolean;
}) {
  const [asking, setAsking] = useState(false);
  if (!asking) {
    return (
      <Button variant={variant} onClick={() => setAsking(true)} busy={busy} disabled={disabled}>
        {children}
      </Button>
    );
  }
  return (
    <div className="w-full space-y-2 rounded-lg border border-neutral-300 p-3 dark:border-neutral-700">
      <p className="text-sm">{confirmText}</p>
      <div className="flex flex-wrap gap-2">
        <Button
          variant={variant === "secondary" ? "primary" : variant}
          busy={busy}
          onClick={() => {
            setAsking(false);
            onConfirm();
          }}
        >
          {confirmLabel}
        </Button>
        <Button variant="ghost" onClick={() => setAsking(false)}>
          Keep it
        </Button>
      </div>
    </div>
  );
}

export function Spinner({ label = "Loading…" }: { label?: string }) {
  return (
    <div className="flex items-center gap-2 py-6 text-sm text-neutral-500" role="status">
      <span className="size-4 animate-spin rounded-full border-2 border-current border-t-transparent" />
      {label}
    </div>
  );
}
