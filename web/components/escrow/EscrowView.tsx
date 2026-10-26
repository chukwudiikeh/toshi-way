"use client";

import Link from "next/link";
import { useParams } from "next/navigation";
import { useEffect, useState } from "react";
import { describeError } from "@/lib/api";
import { parseReleaseFragment } from "@/lib/secret";
import { useSession } from "@/lib/session";
import type { NewUser } from "@/lib/schemas";
import { CreateAccount, SignIn, TokenReveal } from "../Account";
import { Card, Notice, Spinner } from "../ui";
import { DetailsCard, EscrowHeader, PeopleCard, Progress, TechnicalCard, Terms, YourMove } from "./EscrowSummary";
import { useEscrow, useEvents } from "./hooks";
import { BackupReminder } from "../BackupCard";
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
  const [created, setCreated] = useState<NewUser | null>(null);

  if (session === undefined) return <Spinner />;
  // Someone who followed an offer link has no account yet: let them make one right here.
  if (created) return <TokenReveal user={created} onDone={() => setCreated(null)} />;

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
        <div className="grid gap-6 md:grid-cols-2">
          <CreateAccount onCreated={setCreated} />
          <SignIn title="Already have an account?" note="Sign in to see this escrow." />
        </div>
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
      <EscrowHeader escrow={escrow} />
      <Progress escrow={escrow} />
      <div className="grid grid-cols-1 items-start gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]">
        <div className="min-w-0 space-y-6">
          <YourMove escrow={escrow} />
          <BackupReminder />
          <EscrowPanel escrow={escrow} setEscrow={setEscrow} refresh={refresh} linkSecret={linkSecret} events={events} />
          <Terms escrow={escrow} />
          <Timeline events={events} escrow={escrow} />
        </div>
        <aside className="min-w-0 space-y-4">
          <DetailsCard escrow={escrow} />
          <PeopleCard escrow={escrow} />
          <TechnicalCard escrow={escrow} />
        </aside>
      </div>
    </>
  );
}
