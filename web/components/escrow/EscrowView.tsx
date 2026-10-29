"use client";

import Link from "next/link";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";
import { describeError } from "@/lib/api";
import { formatLocalTime, formatSats } from "@/lib/clock";
import { parseReleaseFragment } from "@/lib/secret";
import { useSession } from "@/lib/session";
import { SignIn } from "../Account";
import { Card, CopyField, Notice, Spinner, StateBadge } from "../ui";
import { useEscrow, useEvents } from "./hooks";
import { EscrowPanel } from "./panels";
import { Timeline } from "./Timeline";

type FromLink = { secret: string | null };

/**
 * Take a release secret out of the URL fragment (`#release=<64 hex>`) and remove it from the
 * address bar straight away so it doesn't linger in history or get copied along with the URL.
 * Runs on load and on `hashchange` (a link opened in a tab already showing this escrow only
 * changes the fragment). The secret is kept only in memory; nothing is released until someone
 * presses the button.
 */
function useReleaseFromLink(): FromLink | null {
  const [fromLink, setFromLink] = useState<FromLink | null>(null);
  useEffect(() => {
    const take = () => {
      const hash = window.location.hash;
      if (!new URLSearchParams(hash.replace(/^#/, "")).has("release")) return;
      window.history.replaceState(window.history.state, "", window.location.pathname + window.location.search);
      setFromLink({ secret: parseReleaseFragment(hash) });
    };
    take();
    window.addEventListener("hashchange", take);
    return () => window.removeEventListener("hashchange", take);
  }, []);
  return fromLink;
}

export function EscrowView() {
  const { id } = useParams<{ id: string }>();
  const session = useSession();
  const fromLink = useReleaseFromLink();

  if (session === undefined) return <Spinner />;

  return (
    <div className="space-y-6">
      {fromLink && !fromLink.secret && (
        <Notice tone="error">
          This release link is damaged: the secret must be exactly 64 hex characters. Ask the buyer to show the QR
          code again.
        </Notice>
      )}
      {fromLink?.secret && !session && (
        <Notice tone="info">
          You opened a release link. The secret is held in this page only, so sign in here without reloading,
          then press &ldquo;Release payment&rdquo;.
        </Notice>
      )}
      {!session ? (
        <SignIn title="Sign in to see this escrow" />
      ) : (
        <Loaded id={id} linkSecret={fromLink?.secret ?? null} />
      )}
    </div>
  );
}

function Loaded({ id, linkSecret }: { id: string; linkSecret: string | null }) {
  const { escrow, setEscrow, error, refresh } = useEscrow(id);
  const events = useEvents(id, escrow ? `${escrow.state}|${escrow.updated_at}` : null);

  if (!escrow) {
    if (error?.code === "not_found") {
      return (
        <Card title="Escrow not found">
          <p className="text-sm text-neutral-600 dark:text-neutral-400">
            This escrow doesn&apos;t exist, or you aren&apos;t a party to it. Check that you&apos;re signed in with
            the right account.
          </p>
          <Link href="/" className="mt-3 inline-block text-sm font-semibold text-amber-600 underline">
            Back to my escrows
          </Link>
        </Card>
      );
    }
    return error ? <Notice tone="error">{describeError(error)} Retrying…</Notice> : <Spinner />;
  }

  return (
    <>
      {error && error.code !== "not_found" && (
        <Notice tone="warn">
          {describeError(error)} Showing the last known state; this page keeps retrying.
        </Notice>
      )}
      <Summary escrow={escrow} />
      <EscrowPanel escrow={escrow} setEscrow={setEscrow} refresh={refresh} linkSecret={linkSecret} />
      <Timeline events={events} escrow={escrow} />
    </>
  );
}

function Summary({ escrow }: { escrow: NonNullable<ReturnType<typeof useEscrow>["escrow"]> }) {
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="text-xs uppercase tracking-wide text-neutral-500">
            Escrow · you are the {escrow.your_role}
          </div>
          <h1 className="break-words text-xl font-bold sm:text-2xl">{escrow.description || "(no description)"}</h1>
        </div>
        <StateBadge state={escrow.state} />
      </div>
      <dl className="grid grid-cols-1 gap-x-6 gap-y-2 text-sm sm:grid-cols-2">
        <div>
          <dt className="text-neutral-500">Amount</dt>
          <dd className="font-mono text-lg font-semibold tabular-nums">{formatSats(escrow.amount_sats)}</dd>
        </div>
        <div>
          <dt className="text-neutral-500">Created</dt>
          <dd>{formatLocalTime(escrow.created_at)}</dd>
        </div>
      </dl>
      <details className="text-xs text-neutral-500">
        <summary className="cursor-pointer">Ids</summary>
        <div className="mt-2 space-y-2">
          <div>
            Escrow <CopyField value={escrow.id} />
          </div>
          <div>
            Buyer <CopyField value={escrow.buyer_id} />
          </div>
          <div>
            Seller <CopyField value={escrow.seller_id} />
          </div>
          {escrow.payment_hash && (
            <div>
              Payment hash <CopyField value={escrow.payment_hash} />
            </div>
          )}
        </div>
      </details>
    </div>
  );
}
