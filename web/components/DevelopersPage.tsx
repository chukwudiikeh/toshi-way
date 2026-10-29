"use client";

import { useEffect, useState, useSyncExternalStore, type ReactNode } from "react";
import { api, API_URL } from "@/lib/api";
import { useSession } from "@/lib/session";
import { SignIn } from "./Account";
import { ApiKeysCard } from "./ApiKeysCard";
import { CheckIcon, CodeIcon } from "./landing/icons";
import { PageHeader } from "./PageParts";
import { Card, CopyField, Notice } from "./ui";
import { WebhooksCard } from "./WebhooksCard";

const tabs = [
  { id: "start", label: "Get started" },
  { id: "keys", label: "API keys" },
  { id: "webhooks", label: "Webhooks" },
] as const;
type Tab = (typeof tabs)[number]["id"];

const subscribeHash = (cb: () => void) => {
  window.addEventListener("hashchange", cb);
  return () => window.removeEventListener("hashchange", cb);
};
const noop = () => () => {};

const goTo = (t: Tab) => {
  window.location.hash = t;
};

/** Where a business connects its own software: a checklist to follow, then keys and webhooks. */
export function DevelopersPage() {
  const session = useSession();
  const hash = useSyncExternalStore(subscribeHash, () => window.location.hash.slice(1), () => "");
  const tab: Tab = tabs.find((t) => t.id === hash)?.id ?? "start";
  // The address the business's server should call: the API's own, or this site when it proxies it.
  const origin = useSyncExternalStore(noop, () => window.location.origin, () => "");
  const base = API_URL || origin;
  const [haveKey, setHaveKey] = useState<boolean | null>(null);
  const [haveHook, setHaveHook] = useState<boolean | null>(null);

  const signedIn = session?.kind === "user";
  useEffect(() => {
    if (!signedIn) return;
    let alive = true;
    api.apiKeys().then((k) => alive && setHaveKey(k.api_keys.length > 0)).catch(() => {});
    api.webhooks().then((w) => alive && setHaveHook(w.webhooks.length > 0)).catch(() => {});
    return () => {
      alive = false;
    };
  }, [signedIn, tab]);

  if (session === undefined) return null;
  if (!session) return <SignIn note="Sign in to set up the API." />;
  if (session.kind === "operator") return <Notice tone="info">Operators don&apos;t have API keys.</Notice>;

  return (
    <div className="mx-auto w-full max-w-2xl space-y-6">
      <PageHeader icon={<CodeIcon className="size-5" />} title="API">
        Create escrows and follow them from your own website or system.
      </PageHeader>
      <div role="tablist" aria-label="API sections" className="grid grid-cols-3 gap-1 rounded-xl bg-neutral-100 p-1 dark:bg-neutral-800">
        {tabs.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            aria-selected={tab === t.id}
            onClick={() => goTo(t.id)}
            className={`rounded-lg px-3 py-2 text-sm font-semibold transition-colors ${
              tab === t.id ? "bg-white shadow-sm dark:bg-neutral-950" : "text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200"
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      {tab === "start" && (
        <>
          <Card title="Set up in three steps">
            <ul className="divide-y divide-neutral-200 dark:divide-neutral-800">
              <Task done={haveKey} title="Make an API key" body="Your software uses it to create and read escrows." action="Make a key" onGo={() => goTo("keys")} />
              <Task done={haveHook} title="Add a webhook" body="Hear the moment an escrow is accepted, locked or settled. Optional, but better than checking again and again." action="Add a webhook" onGo={() => goTo("webhooks")} />
              <Task done={null} title="Make your first call" body="Copy the example below, with your key in place of $TOSHI_KEY." />
            </ul>
          </Card>
          <Card title="Your first call">
            <div className="space-y-4 text-sm">
              <div className="space-y-1.5">
                <div className="font-medium">Address</div>
                <CopyField value={`${base}/api/v1`} />
              </div>
              <div className="space-y-1.5">
                <div className="font-medium">Check your key works</div>
                <CopyField value={`curl ${base}/api/v1/me -H "Authorization: Bearer $TOSHI_KEY"`} />
              </div>
              <div className="space-y-1.5">
                <div className="font-medium">Make an escrow offer</div>
                <CopyField
                  value={`curl -X POST ${base}/api/v1/escrows -H "Authorization: Bearer $TOSHI_KEY" -H "Content-Type: application/json" -H "Idempotency-Key: order-1001" -d '{"amount_sats":25000,"description":"Order 1001","hold_secs":3600}'`}
                />
                <p className="text-xs text-neutral-500">
                  The reply has a <code>checkout_url</code>: send your customer there to accept and pay.
                </p>
              </div>
              <p className="text-xs text-neutral-500">
                Everything else is in <code>docs/api.md</code>, and there is a JavaScript client in <code>sdk/js</code>.
              </p>
            </div>
          </Card>
        </>
      )}
      {tab === "keys" && <ApiKeysCard />}
      {tab === "webhooks" && <WebhooksCard />}
    </div>
  );
}

function Task({
  done,
  title,
  body,
  action,
  onGo,
}: {
  done: boolean | null;
  title: string;
  body: ReactNode;
  action?: string;
  onGo?: () => void;
}) {
  return (
    <li className="flex items-start gap-3 py-4 first:pt-0 last:pb-0">
      <span
        className={`mt-0.5 grid size-6 shrink-0 place-items-center rounded-full ${
          done ? "bg-emerald-500 text-white" : "border border-neutral-300 text-transparent dark:border-neutral-700"
        }`}
        aria-label={done ? "Done" : undefined}
      >
        <CheckIcon className="size-4" />
      </span>
      <div className="min-w-0 flex-1">
        <div className="text-sm font-semibold">{title}</div>
        <p className="mt-0.5 text-sm text-neutral-600 dark:text-neutral-400">{body}</p>
      </div>
      {action && onGo && !done && (
        <button type="button" onClick={onGo} className="shrink-0 text-sm font-semibold text-amber-700 hover:underline dark:text-amber-400">
          {action}
        </button>
      )}
    </li>
  );
}
