"use client";

import { useState, type FormEvent } from "react";
import { api, ApiError, describeError } from "@/lib/api";
import { setSession } from "@/lib/session";
import type { NewUser } from "@/lib/schemas";
import { Button, Card, CopyField, Field, inputClass, Notice } from "./ui";

/** Create an account. The token is handed back to the caller to show once. */
export function CreateAccount({ onCreated }: { onCreated: (user: NewUser) => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function create() {
    setBusy(true);
    setError(null);
    try {
      const user = await api.register();
      // Sign in straight away so closing the tab doesn't lose the account.
      setSession({ token: user.token, kind: "user", userId: user.id });
      onCreated(user);
    } catch (err) {
      setError(describeError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card title="New here?">
      <p className="mb-4 text-sm text-neutral-600 dark:text-neutral-400">
        An account is just a random token: no email, no password. Buyers and sellers both need one.
      </p>
      {error && (
        <div className="mb-3">
          <Notice tone="error">{error}</Notice>
        </div>
      )}
      <Button onClick={create} busy={busy}>
        Create account
      </Button>
    </Card>
  );
}

/** Shown once, right after the account is created. */
export function TokenReveal({ user, onDone }: { user: NewUser; onDone: () => void }) {
  const [saved, setSaved] = useState(false);
  return (
    <Card title="Save your token now" className="border-amber-400 dark:border-amber-700">
      <div className="space-y-4">
        <Notice tone="warn">
          This token is your login. <strong>It is shown only once and cannot be recovered.</strong> Store it in a
          password manager. Anyone who has it can act as you.
        </Notice>
        <div className="space-y-1">
          <div className="text-sm font-medium">Your token</div>
          <CopyField value={user.token} />
        </div>
        <div className="space-y-1">
          <div className="text-sm font-medium">Your user id</div>
          <CopyField value={user.id} />
          <p className="text-xs text-neutral-500">
            Not secret. Send it to a seller so they can make you an offer.
          </p>
        </div>
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={saved} onChange={(e) => setSaved(e.target.checked)} className="size-4" />
          I have saved my token somewhere safe
        </label>
        <Button onClick={onDone} disabled={!saved}>
          Continue
        </Button>
      </div>
    </Card>
  );
}

/**
 * Sign in with a token. The role is detected by asking the operator endpoint: operators get the
 * queue, users get 403.
 */
export function SignIn({ title = "Sign in with a token", note }: { title?: string; note?: string }) {
  const [token, setToken] = useState("");
  const [userId, setUserId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: FormEvent) {
    e.preventDefault();
    const t = token.trim();
    if (!t) {
      setError("Paste your token.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const opts = { token: t, keepSessionOn401: true };
      try {
        await api.operatorQueue(opts);
        setSession({ token: t, kind: "operator", userId: null });
        return;
      } catch (err) {
        if (!(err instanceof ApiError) || err.code !== "forbidden") throw err;
      }
      const { escrows } = await api.listMine(opts);
      // The API has no "who am I" call; an escrow we are party to tells us our id.
      const first = escrows.find((x) => x.your_role !== "operator");
      const known = first ? (first.your_role === "buyer" ? first.buyer_id : first.seller_id) : null;
      setSession({ token: t, kind: "user", userId: known ?? (userId.trim() || null) });
    } catch (err) {
      setError(
        err instanceof ApiError && err.code === "unauthorized"
          ? "That token isn't recognised."
          : describeError(err),
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card title={title}>
      <form onSubmit={submit} className="space-y-3">
        {note && <p className="text-sm text-neutral-600 dark:text-neutral-400">{note}</p>}
        <Field label="Token">
          <input
            type="password"
            autoComplete="off"
            spellCheck={false}
            value={token}
            onChange={(e) => setToken(e.target.value)}
            className={`${inputClass} font-mono`}
            placeholder="tw_…"
          />
        </Field>
        <Field label="Your user id (optional)" hint="Only used to show you your id; it is filled in automatically once you have an escrow.">
          <input
            autoComplete="off"
            spellCheck={false}
            value={userId}
            onChange={(e) => setUserId(e.target.value)}
            className={`${inputClass} font-mono`}
            placeholder="u_…"
          />
        </Field>
        {error && <Notice tone="error">{error}</Notice>}
        <Button type="submit" busy={busy}>
          Sign in
        </Button>
      </form>
    </Card>
  );
}
