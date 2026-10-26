"use client";

import { useState, type FormEvent } from "react";
import { api, ApiError, describeError } from "@/lib/api";
import type { NewUser } from "@/lib/schemas";
import { startAccount, unlock, useVaultHint, useVaultState } from "@/lib/session";
import { VaultError } from "@/lib/vault";
import { KeyIcon } from "./landing/icons";
import { PageHeader } from "./PageParts";
import { NewPasswordFields, useNewPassword } from "./PasswordFields";
import { Button, Card, CopyField, Field, inputClass, Notice } from "./ui";

/**
 * Create an account. The person first chooses a password; it protects the account on this device, the
 * way a wallet's password does. The token the server hands back is then shown once, as the recovery key.
 */
export function CreateAccount({ onCreated }: { onCreated: (user: NewUser) => void }) {
  const pw = useNewPassword();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function create(e: FormEvent) {
    e.preventDefault();
    if (pw.problem) {
      setError(pw.problem);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const user = await api.register();
      // Encrypt and keep the token before anything else, so closing the tab cannot lose the account.
      await startAccount({ token: user.token, kind: "user", userId: user.id }, pw.password);
      pw.reset();
      onCreated(user);
    } catch (err) {
      setError(describeError(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card title="New here?">
      <form onSubmit={create} className="space-y-4" noValidate>
        <p className="text-sm text-neutral-600 dark:text-neutral-400">
          Choose a password to protect your account on this device. No email needed. Buyers and sellers both need
          an account.
        </p>
        <NewPasswordFields
          state={pw}
          names={{ password: "Create a password", confirm: "Confirm the password" }}
          label="Password"
        />
        <p className="text-xs text-neutral-500">
          This password stays on this device and can&apos;t be reset. If you forget it, you can sign in again with
          your token, which you&apos;ll see next.
        </p>
        {error && <Notice tone="error">{error}</Notice>}
        <Button type="submit" busy={busy}>
          Create account
        </Button>
      </form>
    </Card>
  );
}

/** Shown once, right after the account is created: the token is the recovery key. */
export function TokenReveal({ user, onDone }: { user: NewUser; onDone: () => void }) {
  const [saved, setSaved] = useState(false);
  return (
    <div className="space-y-6">
      <PageHeader icon={<KeyIcon className="size-5" />} title="Your account is ready">
        One last thing before you start: keep your recovery key safe.
      </PageHeader>
      <Card title="Save your token now" className="border-2 border-amber-400 dark:border-amber-600">
        <div className="space-y-5">
          <Notice tone="warn">
            This token is your <strong>recovery key</strong>. You sign in on this device with your password, but you
            need the token to sign in on another device, or if you forget your password.{" "}
            <strong>Keep it somewhere safe, like a password manager.</strong> Anyone who has it can act as you.
          </Notice>
          <div className="space-y-2">
            <div className="text-xs font-semibold uppercase tracking-[0.12em] text-neutral-500">Your token</div>
            <CopyField value={user.token} />
            <p className="text-xs text-neutral-500">You can see it again from the Account page, after entering your password.</p>
          </div>
          <div className="space-y-2">
            <div className="text-xs font-semibold uppercase tracking-[0.12em] text-neutral-500">Your user id</div>
            <CopyField value={user.id} />
            <p className="text-xs text-neutral-500">Not secret. Send it to a seller so they can make you an offer.</p>
          </div>
          <label className="flex items-center gap-3 rounded-lg border border-neutral-200 p-3 text-sm dark:border-neutral-800">
            <input type="checkbox" checked={saved} onChange={(e) => setSaved(e.target.checked)} className="size-4" />
            I have saved my token somewhere safe
          </label>
          <Button onClick={onDone} disabled={!saved} className="w-full sm:w-auto">
            Continue
          </Button>
        </div>
      </Card>
    </div>
  );
}

/**
 * Sign in. On a device that already has an account it asks for the password. Otherwise (a new device,
 * or after choosing to start over) it takes the token and a new password for this device.
 */
export function SignIn({ title, note }: { title?: string; note?: string }) {
  const state = useVaultState();
  const hint = useVaultHint();
  const [importing, setImporting] = useState(false);
  if (state === "locked" && hint && !importing) {
    return <UnlockForm hint={hint} note={note} onForgot={() => setImporting(true)} />;
  }
  return <ImportForm title={title} note={note} onBack={state === "locked" ? () => setImporting(false) : undefined} />;
}

function UnlockForm({
  hint,
  note,
  onForgot,
}: {
  hint: NonNullable<ReturnType<typeof useVaultHint>>;
  note?: string;
  onForgot: () => void;
}) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!password) {
      setError("Enter your password.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await unlock(password);
    } catch (err) {
      setError(err instanceof VaultError ? err.message : "Something went wrong. Please try again.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card title="Welcome back">
      <form onSubmit={submit} className="space-y-3">
        {note && <p className="text-sm text-neutral-600 dark:text-neutral-400">{note}</p>}
        <p className="text-sm text-neutral-600 dark:text-neutral-400">
          {hint.kind === "operator" ? "Operator account" : "Account"}
          {hint.userId && <span className="ml-1 font-mono text-xs">{hint.userId}</span>}
        </p>
        <Field label="Password">
          <input
            type="password"
            aria-label="Unlock password"
            autoComplete="current-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className={inputClass}
          />
        </Field>
        {error && <Notice tone="error">{error}</Notice>}
        <Button type="submit" busy={busy}>
          Unlock
        </Button>
        <button
          type="button"
          onClick={onForgot}
          className="block text-sm font-medium text-amber-700 hover:underline dark:text-amber-400"
        >
          Forgot your password, or using another account? Sign in with a token
        </button>
      </form>
    </Card>
  );
}

function ImportForm({ title = "Sign in with a token", note, onBack }: { title?: string; note?: string; onBack?: () => void }) {
  const [token, setToken] = useState("");
  const pw = useNewPassword();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: FormEvent) {
    e.preventDefault();
    const t = token.trim();
    if (!t) {
      setError("Paste your token.");
      return;
    }
    if (pw.problem) {
      setError(pw.problem);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      // `/api/v1/me` says whose token it is and what role it has, and proves the token works.
      const me = await api.me({ token: t, keepSessionOn401: true });
      const operator = me.role === "operator";
      await startAccount({ token: t, kind: operator ? "operator" : "user", userId: operator ? null : me.id }, pw.password);
      pw.reset();
    } catch (err) {
      setError(
        err instanceof ApiError && err.code === "unauthorized" ? "That token isn't recognised." : describeError(err),
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card title={title}>
      <form onSubmit={submit} className="space-y-4" noValidate>
        {note && <p className="text-sm text-neutral-600 dark:text-neutral-400">{note}</p>}
        <p className="text-sm text-neutral-600 dark:text-neutral-400">
          Paste your token to bring your account to this device, and choose a password to protect it here.
        </p>
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
        <NewPasswordFields
          state={pw}
          names={{ password: "New password for this device", confirm: "Confirm the new password" }}
          label="New password for this device"
        />
        {error && <Notice tone="error">{error}</Notice>}
        <Button type="submit" busy={busy}>
          Sign in
        </Button>
        {onBack && (
          <button type="button" onClick={onBack} className="block text-sm font-medium text-amber-700 hover:underline dark:text-amber-400">
            Back to unlocking with a password
          </button>
        )}
      </form>
    </Card>
  );
}
