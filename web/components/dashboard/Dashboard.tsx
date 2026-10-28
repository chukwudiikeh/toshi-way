"use client";

import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import { api, describeError } from "@/lib/api";
import { serverNow, useServerNow } from "@/lib/clock";
import { greeting, summarize, todos } from "@/lib/dashboard";
import type { ArbiterKey, Balance, Escrow, NodeInfo, Totals } from "@/lib/schemas";
import { rememberUserId, type Session } from "@/lib/session";
import { Notice, Spinner } from "../ui";
import { EscrowTable } from "./EscrowTable";
import { AccountCard, Activity, ArbiterCard, Attention, BalanceCard, NodeCard, StatCards } from "./Widgets";

/** What a signed-in buyer, seller or arbiter sees first: where things stand, and what needs them. */
export function Dashboard({ session }: { session: Session }) {
  const [escrows, setEscrows] = useState<Escrow[] | null>(null);
  const [totals, setTotals] = useState<Totals | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const [node, setNode] = useState<NodeInfo | null>(null);
  const [arbiterKey, setArbiterKey] = useState<ArbiterKey | null>(null);
  const [balance, setBalance] = useState<Balance | null>(null);
  const tick = useServerNow();
  const now = Math.floor(tick ?? serverNow());

  useEffect(() => {
    let alive = true;
    const load = async () => {
      try {
        const { escrows, totals } = await api.listMine();
        if (!alive) return;
        setEscrows(escrows);
        setTotals(totals);
        setError(null);
        // An escrow tells us who we are, so a returning visitor's id is always shown.
        const party = escrows.find((e) => e.your_role === "buyer" || e.your_role === "seller");
        const myId = party && (party.your_role === "buyer" ? party.buyer_id : party.seller_id);
        if (myId) rememberUserId(myId);
        api.balance().then((b) => alive && setBalance(b)).catch(() => {});
      } catch (err) {
        if (alive) setError(describeError(err));
      }
    };
    load();
    const t = setInterval(load, 5000);
    // The node and arbiter panels are context, not essential: if they fail the page just omits them.
    api.node().then((n) => alive && setNode(n)).catch(() => {});
    api.arbiterKey().then((k) => alive && setArbiterKey(k)).catch(() => {});
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  const summary = useMemo(() => summarize(escrows ?? [], now, totals), [escrows, now, totals]);
  const attention = useMemo(() => todos(escrows ?? [], now), [escrows, now]);
  const asArbiter = (escrows ?? []).filter((e) => e.your_role === "arbiter").length;

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <h1 className="text-3xl font-bold tracking-tight sm:text-4xl">{greeting(new Date().getHours())}</h1>
          <p className="mt-1 text-sm text-neutral-500">Where your escrows stand, and what needs you.</p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Link
            href="/node"
            className="inline-flex min-h-10 items-center rounded-lg px-4 py-2 text-sm font-semibold ring-1 ring-neutral-300 hover:bg-neutral-100 dark:ring-neutral-700 dark:hover:bg-neutral-800"
          >
            My node
          </Link>
          <Link
            href="/escrows/new"
            className="inline-flex min-h-10 items-center rounded-lg bg-amber-500 px-4 py-2 text-sm font-semibold text-black shadow-sm hover:bg-amber-400"
          >
            New escrow
          </Link>
        </div>
      </div>

      {error && <Notice tone="error">{error}</Notice>}
      {escrows === null && !error && <Spinner />}

      {escrows && (
        <>
          <StatCards summary={summary} />
          <div className="grid grid-cols-1 items-start gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]">
            <div className="min-w-0 space-y-6">
              <Attention items={attention} now={now} />
              {totals && totals.escrows > escrows.length && (
                <p className="text-xs text-neutral-500">
                  Showing {escrows.length} of your {totals.escrows} escrows: everything still in play and your newest
                  finished ones. The totals above count all of them.
                </p>
              )}
              <EscrowTable escrows={escrows} now={now} />
            </div>
            <aside className="min-w-0 space-y-4">
              <AccountCard userId={session.userId} />
              <BalanceCard balance={balance} />
              <NodeCard node={node} />
              <ArbiterCard key_={arbiterKey} asArbiter={asArbiter} />
              <Activity escrows={escrows} now={now} />
            </aside>
          </div>
        </>
      )}
    </div>
  );
}
