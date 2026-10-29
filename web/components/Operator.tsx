"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import { api, ApiError, describeError } from "@/lib/api";
import { formatDuration, formatSats, useServerNow } from "@/lib/clock";
import type { Escrow } from "@/lib/schemas";
import { clearSession, useSession } from "@/lib/session";
import { SignIn } from "./Account";
import { panel } from "./dashboard/Widgets";
import { ShieldIcon } from "./landing/icons";
import { PageHeader } from "./PageParts";
import { Button, ConfirmButton, Notice, Spinner, StateBadge } from "./ui";

export function Operator() {
  const session = useSession();
  if (session === undefined) return <Spinner />;
  if (!session) return <SignIn title="Operator sign-in" note="Use the server's OPERATOR_TOKEN." />;
  if (session.kind !== "operator") {
    return (
      <div className="space-y-3">
        <Notice tone="info">You are signed in as a user. The operator queue needs the operator token.</Notice>
        <Button variant="secondary" onClick={clearSession}>
          Sign out
        </Button>
      </div>
    );
  }
  return <Queue />;
}

// Disputes first, then the nearest deadline.
const order = (a: Escrow, b: Escrow) =>
  Number(b.state === "DISPUTED") - Number(a.state === "DISPUTED") || a.deadline_at - b.deadline_at;

function Queue() {
  const [escrows, setEscrows] = useState<Escrow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const now = useServerNow();

  async function load() {
    try {
      const { escrows } = await api.operatorQueue();
      setEscrows([...escrows].sort(order));
      setError(null);
    } catch (err) {
      setError(describeError(err));
    }
  }

  useEffect(() => {
    let alive = true;
    const tick = () => {
      if (alive) load();
    };
    tick();
    const t = setInterval(tick, 5000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  async function cancel(id: string) {
    setBusy(id);
    setActionError(null);
    try {
      await api.cancel(id);
    } catch (err) {
      // invalid_state means it moved on by itself; the reload shows where it is now.
      if (!(err instanceof ApiError && err.code === "invalid_state")) setActionError(describeError(err));
    } finally {
      setBusy(null);
      load();
    }
  }

  const disputed = escrows?.filter((e) => e.state === "DISPUTED").length ?? 0;
  const locked = escrows?.filter((e) => e.state === "LOCKED" || e.state === "DISPUTED") ?? [];
  const lockedSats = locked.reduce((sum, e) => sum + e.amount_sats, 0);

  return (
    <div className="space-y-6">
      <PageHeader icon={<ShieldIcon className="size-5" />} title="Operator queue">
        All active escrows. Operators can cancel (refunding the buyer) but can never release funds to the seller.
      </PageHeader>
      {escrows && (
        <div className="grid grid-cols-3 gap-4">
          <Tile label="Active" value={String(escrows.length)} />
          <Tile label="Disputed" value={String(disputed)} attention={disputed > 0} />
          <Tile label="Locked" value={formatSats(lockedSats).replace(" sats", "")} sub="sats" />
        </div>
      )}
      {error && <Notice tone="error">{error}</Notice>}
      {actionError && <Notice tone="error">{actionError}</Notice>}
      {disputed > 0 && (
        <Notice tone="warn">
          {disputed} disputed escrow{disputed === 1 ? "" : "s"} need{disputed === 1 ? "s" : ""} review.
        </Notice>
      )}
      {escrows === null && !error && <Spinner />}
      {escrows?.length === 0 && <p className="text-sm text-neutral-500">No active escrows.</p>}
      <ul className="space-y-3">
        {escrows?.map((e) => (
          <li
            key={e.id}
            className={`${panel} border-l-4 p-4 sm:p-5 ${
              e.state === "DISPUTED" ? "border-l-orange-500" : "border-l-neutral-300 dark:border-l-neutral-700"
            }`}
          >
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0">
                <Link href={`/escrows/${e.id}`} className="font-semibold underline-offset-2 hover:underline">
                  {e.description || "(no description)"}
                </Link>
                <div className="mt-0.5 break-all font-mono text-xs text-neutral-500">{e.id}</div>
              </div>
              <StateBadge state={e.state} />
            </div>
            <div className="mt-3 flex flex-wrap items-center gap-x-6 gap-y-1 text-sm text-neutral-600 dark:text-neutral-400">
              <span className="font-mono text-base font-semibold tabular-nums text-neutral-900 dark:text-neutral-100">
                {formatSats(e.amount_sats)}
              </span>
              {(e.state === "LOCKED" || e.state === "DISPUTED") && (
                <span>Deadline in {now === null ? "…" : formatDuration(e.deadline_at - now)}</span>
              )}
              {e.state === "INVOICE_ISSUED" && e.invoice_expires_at && (
                <span>Invoice expires in {now === null ? "…" : formatDuration(e.invoice_expires_at - now)}</span>
              )}
            </div>
            {["INVOICE_ISSUED", "LOCKED", "DISPUTED"].includes(e.state) && (
              <div className="mt-4">
                <ConfirmButton
                  confirmText={`Cancel this escrow and refund ${formatSats(e.amount_sats)} to the buyer?`}
                  confirmLabel="Cancel and refund"
                  variant="danger"
                  busy={busy === e.id}
                  onConfirm={() => cancel(e.id)}
                >
                  Cancel
                </ConfirmButton>
              </div>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}

function Tile({ label, value, sub, attention }: { label: string; value: string; sub?: string; attention?: boolean }) {
  return (
    <div
      className={`${panel} p-4 ${attention ? "border-orange-400 bg-orange-50 ring-1 ring-orange-400/40 dark:border-orange-500/60 dark:bg-orange-500/10" : ""}`}
    >
      <div className="text-xs font-semibold uppercase tracking-[0.12em] text-neutral-500">{label}</div>
      <div className="mt-2 font-mono text-2xl font-semibold tabular-nums tracking-tight">{value}</div>
      {sub && <div className="text-xs text-neutral-500">{sub}</div>}
    </div>
  );
}
