"use client";

import { useCallback, useEffect, useState, type FormEvent } from "react";
import { api, ApiError, describeError } from "@/lib/api";
import { formatLocalTime } from "@/lib/clock";
import type { NewWebhook, WebhookInfo } from "@/lib/schemas";
import { Button, Card, ConfirmButton, CopyField, Field, inputClass, Notice, Spinner } from "./ui";

const statusStyle: Record<WebhookInfo["status"], { label: string; chip: string }> = {
  ok: { label: "Working", chip: "bg-emerald-100 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300" },
  failing: { label: "Failing", chip: "bg-amber-100 text-amber-800 dark:bg-amber-950 dark:text-amber-300" },
  disabled: { label: "Switched off", chip: "bg-red-100 text-red-800 dark:bg-red-950 dark:text-red-300" },
};

/**
 * Where a business's software is told about changes to its escrows. Each message is signed with a
 * secret shown once, so the receiver can tell it came from here.
 */
export function WebhooksCard() {
  const [hooks, setHooks] = useState<WebhookInfo[] | null>(null);
  const [fresh, setFresh] = useState<NewWebhook | null>(null);
  const [unavailable, setUnavailable] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setHooks((await api.webhooks()).webhooks);
    } catch (err) {
      setError(describeError(err));
    }
  }, []);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- loads from the API once on mount
    void load();
  }, [load]);

  async function remove(id: string) {
    setError(null);
    try {
      await api.deleteWebhook(id);
      if (fresh?.id === id) setFresh(null);
      await load();
    } catch (err) {
      setError(describeError(err));
    }
  }

  return (
    <Card title="Webhooks" id="webhooks">
      <div className="space-y-6">
        <p className="text-sm text-neutral-600 dark:text-neutral-400">
          Tell your own software the moment an escrow changes: accepted, paid and locked, settled, refunded, disputed.
          Only changes from the time you add an address are sent. Each message is signed, so your server can check it
          came from here.
        </p>
        {error && <Notice tone="error">{error}</Notice>}
        {unavailable && <Notice tone="warn">This server isn&apos;t set up for webhooks yet. Ask whoever runs it.</Notice>}
        {fresh && (
          <Notice tone="warn">
            <div className="space-y-2">
              <p>
                <strong>Copy the signing secret now.</strong> It is shown once. Your server uses it to check each message
                is really from us.
              </p>
              <CopyField value={fresh.secret} label="Copy secret" />
              <Button variant="secondary" onClick={() => setFresh(null)}>
                I have copied it
              </Button>
            </div>
          </Notice>
        )}
        {hooks === null && !error ? <Spinner /> : <HookList hooks={hooks ?? []} onDelete={remove} />}
        <NewHook
          onCreated={async (h) => {
            setFresh(h);
            await load();
          }}
          onUnavailable={() => setUnavailable(true)}
        />
      </div>
    </Card>
  );
}

function HookList({ hooks, onDelete }: { hooks: WebhookInfo[]; onDelete: (id: string) => void }) {
  if (hooks.length === 0) return <p className="text-sm text-neutral-500">No addresses yet.</p>;
  return (
    <ul className="divide-y divide-neutral-200 rounded-lg border border-neutral-200 dark:divide-neutral-800 dark:border-neutral-800">
      {hooks.map((h) => (
        <HookRow key={h.id} hook={h} onDelete={onDelete} />
      ))}
    </ul>
  );
}

function HookRow({ hook, onDelete }: { hook: WebhookInfo; onDelete: (id: string) => void }) {
  const [result, setResult] = useState<string | null>(null);
  const [ok, setOk] = useState(true);
  const [busy, setBusy] = useState(false);
  const s = statusStyle[hook.status];

  async function test() {
    setBusy(true);
    setResult(null);
    try {
      const r = await api.testWebhook(hook.id);
      setOk(r.delivered);
      setResult(r.delivered ? "Delivered: your server answered " + r.status + "." : "Not delivered: " + r.status + ".");
    } catch (err) {
      setOk(false);
      setResult(describeError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <li className="space-y-2 p-3">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 space-y-1 text-sm">
          <div className="break-all font-mono text-xs">{hook.url}</div>
          <div className="flex flex-wrap items-center gap-2 text-xs text-neutral-500">
            <span className={`rounded-full px-2 py-0.5 font-semibold ${s.chip}`}>{s.label}</span>
            <span>Added {formatLocalTime(hook.created_at)}</span>
            {hook.last_attempt_at && (
              <span>
                · last try {formatLocalTime(hook.last_attempt_at)} ({hook.last_status})
              </span>
            )}
          </div>
          {hook.status === "failing" && hook.failing_since && (
            <div className="text-xs text-amber-700 dark:text-amber-400">
              Failing since {formatLocalTime(hook.failing_since)}. We keep trying, and nothing is skipped.
            </div>
          )}
          {hook.status === "disabled" && (
            <div className="text-xs text-red-700 dark:text-red-400">
              Switched off after days of failures. Delete it and add the address again once your server is back.
            </div>
          )}
        </div>
        <div className="flex flex-wrap gap-2">
          <Button variant="secondary" onClick={test} busy={busy}>
            Send test
          </Button>
          <ConfirmButton
            variant="danger"
            confirmLabel="Delete address"
            confirmText="Delete this address? Nothing more will be sent to it."
            onConfirm={() => onDelete(hook.id)}
          >
            Delete
          </ConfirmButton>
        </div>
      </div>
      {result && <Notice tone={ok ? "success" : "error"}>{result}</Notice>}
    </li>
  );
}

function NewHook({ onCreated, onUnavailable }: { onCreated: (h: NewWebhook) => void | Promise<void>; onUnavailable: () => void }) {
  const [url, setUrl] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!url.trim()) return setError("Enter the address of your server, starting with https://");
    setBusy(true);
    setError(null);
    try {
      const made = await api.createWebhook(url.trim());
      setUrl("");
      await onCreated(made);
    } catch (err) {
      if (err instanceof ApiError && err.code === "webhooks_unavailable") onUnavailable();
      else setError(describeError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} className="space-y-3 border-t border-neutral-200 pt-5 dark:border-neutral-800" noValidate>
      <div className="text-sm font-medium">Add an address</div>
      <Field label="Address" hint="Where we send the messages. It must be https:// and reachable from the internet.">
        <input
          aria-label="Webhook address"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="https://shop.example.com/toshi-webhook"
          autoComplete="off"
          spellCheck={false}
          className={`${inputClass} font-mono`}
        />
      </Field>
      {error && <Notice tone="error">{error}</Notice>}
      <Button type="submit" busy={busy}>
        Add address
      </Button>
    </form>
  );
}
