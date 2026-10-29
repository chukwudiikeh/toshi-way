"use client";

import Link from "next/link";
import { useEffect, useState } from "react";
import { api, describeError } from "@/lib/api";
import { formatLocalTime, formatSats } from "@/lib/clock";
import type { Escrow, NewUser } from "@/lib/schemas";
import { rememberUserId, useSession, type Session } from "@/lib/session";
import { CreateAccount, SignIn, TokenReveal } from "./Account";
import { Card, CopyField, Notice, Spinner, StateBadge } from "./ui";

export function Home() {
  const session = useSession();
  const [created, setCreated] = useState<NewUser | null>(null);

  if (session === undefined) return <Spinner />;

  if (created) return <TokenReveal user={created} onDone={() => setCreated(null)} />;

  if (!session) {
    return (
      <div className="space-y-6">
        <Intro />
        <div className="grid gap-6 md:grid-cols-2">
          <CreateAccount onCreated={setCreated} />
          <SignIn />
        </div>
      </div>
    );
  }

  if (session.kind === "operator") {
    return (
      <Card title="Signed in as operator">
        <p className="text-sm">
          Operators review the queue and can cancel escrows.{" "}
          <Link href="/operator" className="font-semibold text-amber-600 underline">
            Open the operator queue
          </Link>
          .
        </p>
      </Card>
    );
  }

  return <MyEscrows session={session} />;
}

function Intro() {
  return (
    <div className="space-y-2">
      <h1 className="text-2xl font-bold tracking-tight sm:text-3xl">Escrow on Lightning, without a middleman</h1>
      <p className="text-neutral-600 dark:text-neutral-400">
        The buyer&apos;s payment is locked by the Lightning network, not held by us. It goes to the seller only
        when the buyer hands over a secret; if that doesn&apos;t happen before the deadline, the buyer is refunded
        automatically.
      </p>
    </div>
  );
}

function MyEscrows({ session }: { session: Session }) {
  const [escrows, setEscrows] = useState<Escrow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const load = async () => {
      try {
        const { escrows } = await api.listMine();
        if (!alive) return;
        setEscrows(escrows);
        setError(null);
        const party = escrows.find((e) => e.your_role === "buyer" || e.your_role === "seller");
        const myId = party && (party.your_role === "buyer" ? party.buyer_id : party.seller_id);
        if (myId) rememberUserId(myId);
      } catch (err) {
        if (alive) setError(describeError(err));
      }
    };
    load();
    const t = setInterval(load, 5000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  return (
    <div className="space-y-6">
      <Card title="Your account">
        {session.userId ? (
          <div className="space-y-1">
            <div className="text-sm font-medium">Your user id</div>
            <CopyField value={session.userId} />
            <p className="text-xs text-neutral-500">Send this to a seller so they can make you an offer.</p>
          </div>
        ) : (
          <p className="text-sm text-neutral-600 dark:text-neutral-400">
            Your user id isn&apos;t stored on this device. It will appear once you have an escrow, or sign out and
            sign in again with your id.
          </p>
        )}
      </Card>

      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-xl font-bold">My escrows</h1>
        <Link
          href="/escrows/new"
          className="inline-flex min-h-10 items-center rounded-lg bg-amber-500 px-4 py-2 text-sm font-semibold text-black hover:bg-amber-400"
        >
          New escrow (as seller)
        </Link>
      </div>

      {error && <Notice tone="error">{error}</Notice>}
      {escrows === null && !error && <Spinner />}
      {escrows?.length === 0 && (
        <p className="text-sm text-neutral-500">
          No escrows yet. Create one as a seller, or give your user id to a seller.
        </p>
      )}
      {escrows && escrows.length > 0 && (
        <ul className="divide-y divide-neutral-200 overflow-hidden rounded-xl border border-neutral-200 bg-white dark:divide-neutral-800 dark:border-neutral-800 dark:bg-neutral-900">
          {escrows.map((e) => (
            <li key={e.id}>
              <Link
                href={`/escrows/${e.id}`}
                className="flex flex-wrap items-center gap-x-4 gap-y-1 px-4 py-3 hover:bg-neutral-50 dark:hover:bg-neutral-800/60"
              >
                <div className="min-w-0 flex-1">
                  <div className="truncate font-medium">{e.description || "(no description)"}</div>
                  <div className="text-xs text-neutral-500">
                    You are the {e.your_role} · created {formatLocalTime(e.created_at)}
                  </div>
                </div>
                <div className="font-mono text-sm tabular-nums">{formatSats(e.amount_sats)}</div>
                <StateBadge state={e.state} />
              </Link>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
