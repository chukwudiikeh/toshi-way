"use client";

import { useCallback, useEffect, useState, type FormEvent } from "react";
import { api, describeError } from "@/lib/api";
import { formatLocalTime } from "@/lib/clock";
import type { ApiKeyInfo, NewApiKey } from "@/lib/schemas";
import { Button, Card, ConfirmButton, CopyField, Field, inputClass, Notice, Spinner } from "./ui";

const scopeChoices = [
  { id: "escrows:read", label: "Read escrows", note: "List escrows and see their status and history." },
  { id: "escrows:write", label: "Create and cancel escrows", note: "Make offers as the seller, and cancel ones nobody has accepted." },
];

const scopeLabel = (id: string) => scopeChoices.find((s) => s.id === id)?.label ?? id;

/**
 * Keys for a business's own software. A key can do only what its scopes allow, never touches the
 * node or the account, and can be revoked. The secret is shown once, when the key is made.
 */
export function ApiKeysCard() {
  const [keys, setKeys] = useState<ApiKeyInfo[] | null>(null);
  const [fresh, setFresh] = useState<NewApiKey | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setKeys((await api.apiKeys()).api_keys);
    } catch (err) {
      setError(describeError(err));
    }
  }, []);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- loads from the API once on mount
    void load();
  }, [load]);

  async function revoke(id: string) {
    setError(null);
    try {
      await api.revokeApiKey(id);
      if (fresh?.id === id) setFresh(null);
      await load();
    } catch (err) {
      setError(describeError(err));
    }
  }

  return (
    <Card title="API keys" id="api-keys">
      <div className="space-y-6">
        <p className="text-sm text-neutral-600 dark:text-neutral-400">
          For a business that wants its own software to create escrows and check on them. A key does only what you
          allow, can&apos;t change your node or your account, and you can revoke it at any time.
        </p>
        {error && <Notice tone="error">{error}</Notice>}
        {fresh && (
          <Notice tone="warn">
            <div className="space-y-2">
              <p>
                <strong>Copy this key now.</strong> It is shown once and can&apos;t be shown again. Keep it in your
                server&apos;s settings, never in a web page or an app you give to customers.
              </p>
              <CopyField value={fresh.key} label="Copy key" />
              <Button variant="secondary" onClick={() => setFresh(null)}>
                I have copied it
              </Button>
            </div>
          </Notice>
        )}
        {keys === null && !error ? <Spinner /> : <KeyList keys={keys ?? []} onRevoke={revoke} />}
        <NewKey
          onCreated={async (k) => {
            setFresh(k);
            await load();
          }}
        />
      </div>
    </Card>
  );
}

function KeyList({ keys, onRevoke }: { keys: ApiKeyInfo[]; onRevoke: (id: string) => void }) {
  if (keys.length === 0) return <p className="text-sm text-neutral-500">No keys yet.</p>;
  return (
    <ul className="divide-y divide-neutral-200 rounded-lg border border-neutral-200 dark:divide-neutral-800 dark:border-neutral-800">
      {keys.map((k) => (
        <li key={k.id} className="flex flex-wrap items-start justify-between gap-3 p-3">
          <div className="min-w-0 space-y-0.5 text-sm">
            <div className={`font-semibold ${k.revoked_at ? "text-neutral-500 line-through" : ""}`}>{k.name}</div>
            <div className="font-mono text-xs text-neutral-500">tk_…{k.last_chars}</div>
            <div className="text-xs text-neutral-500">{k.scopes.map(scopeLabel).join(" · ")}</div>
            <div className="text-xs text-neutral-500">
              Made {formatLocalTime(k.created_at)}
              {k.last_used_at ? ` · last used ${formatLocalTime(k.last_used_at)}` : " · never used"}
              {k.revoked_at ? ` · revoked ${formatLocalTime(k.revoked_at)}` : ""}
            </div>
          </div>
          {!k.revoked_at && (
            <ConfirmButton
              variant="danger"
              confirmLabel="Revoke key"
              confirmText={`Revoke "${k.name}"? Software using it stops working straight away.`}
              onConfirm={() => onRevoke(k.id)}
            >
              Revoke
            </ConfirmButton>
          )}
        </li>
      ))}
    </ul>
  );
}

function NewKey({ onCreated }: { onCreated: (k: NewApiKey) => void | Promise<void> }) {
  const [name, setName] = useState("");
  const [scopes, setScopes] = useState<string[]>(["escrows:read", "escrows:write"]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!name.trim()) return setError("Give the key a name, such as the site or system that will use it.");
    if (scopes.length === 0) return setError("Choose at least one thing the key may do.");
    setBusy(true);
    setError(null);
    try {
      const made = await api.createApiKey({ name: name.trim(), scopes });
      setName("");
      await onCreated(made);
    } catch (err) {
      setError(describeError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} className="space-y-3 border-t border-neutral-200 pt-5 dark:border-neutral-800" noValidate>
      <div className="text-sm font-medium">Make a new key</div>
      <Field label="Name">
        <input
          aria-label="Key name"
          value={name}
          onChange={(e) => setName(e.target.value)}
          maxLength={60}
          placeholder="Online shop"
          className={inputClass}
        />
      </Field>
      <fieldset className="space-y-2">
        <legend className="text-sm font-medium">What it may do</legend>
        {scopeChoices.map((s) => (
          <label key={s.id} className="flex items-start gap-3 text-sm">
            <input
              type="checkbox"
              className="mt-0.5 size-4"
              checked={scopes.includes(s.id)}
              onChange={(e) => setScopes(e.target.checked ? [...scopes, s.id] : scopes.filter((x) => x !== s.id))}
            />
            <span>
              {s.label}
              <span className="block text-xs text-neutral-500">{s.note}</span>
            </span>
          </label>
        ))}
      </fieldset>
      {error && <Notice tone="error">{error}</Notice>}
      <Button type="submit" busy={busy}>
        Make key
      </Button>
    </form>
  );
}
