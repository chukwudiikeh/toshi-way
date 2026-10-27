"use client";

import { useState, type FormEvent } from "react";
import {
  changePassword,
  lock,
  protectAccount,
  removeVault,
  useSession,
  useVaultState,
  verifyPassword,
} from "@/lib/session";
import { VaultError } from "@/lib/vault";
import { SignIn } from "./Account";
import { ApiKeysCard } from "./ApiKeysCard";
import { BackupCard } from "./BackupCard";
import { WebhooksCard } from "./WebhooksCard";
import { NewPasswordFields, useNewPassword } from "./PasswordFields";
import { Button, Card, ConfirmButton, CopyField, Field, inputClass, Notice } from "./ui";

/** Where a person manages how their account is kept on this device. */
export function AccountPage() {
  const session = useSession();
  const state = useVaultState();
  if (session === undefined) return null;
  if (!session) return <SignIn />;

  return (
    <div className="space-y-6">
      <h1 className="text-2xl font-bold tracking-tight">Account</h1>
      <Card title="Your account">
        <div className="space-y-3">
          {session.userId && (
            <div className="space-y-1">
              <div className="text-sm font-medium">User id</div>
              <CopyField value={session.userId} />
            </div>
          )}
          <p className="text-sm text-neutral-600 dark:text-neutral-400">
            {state === "unprotected"
              ? "This account is saved on this device without a password. Add one below."
              : "Your account is protected by a password on this device."}
          </p>
        </div>
      </Card>
      {state === "unprotected" ? <AddPassword /> : <ProtectedTools />}
      {session.kind === "user" && <ApiKeysCard />}
      {session.kind === "user" && <WebhooksCard />}
      <BackupCard />
      <Card title="This device">
        <div className="flex flex-wrap items-start gap-3">
          {state !== "unprotected" && <Button variant="secondary" onClick={lock}>Lock now</Button>}
          <ConfirmButton
            variant="danger"
            confirmLabel="Remove from this device"
            confirmText="This removes your account from this device. You can only get back in with your token, so make sure you have it saved."
            onConfirm={removeVault}
          >
            Remove from this device
          </ConfirmButton>
        </div>
      </Card>
    </div>
  );
}

function AddPassword() {
  const pw = useNewPassword();
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (pw.problem) return setError(pw.problem);
    setBusy(true);
    setError(null);
    try {
      await protectAccount(pw.password);
      pw.reset();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Something went wrong.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card title="Protect with a password">
      <form onSubmit={submit} className="space-y-4" noValidate>
        <NewPasswordFields state={pw} names={{ password: "Create a password", confirm: "Confirm the password" }} />
        {error && <Notice tone="error">{error}</Notice>}
        <Button type="submit" busy={busy}>Protect account</Button>
      </form>
    </Card>
  );
}

function ProtectedTools() {
  return (
    <>
      <ShowToken />
      <ChangePassword />
    </>
  );
}

function ShowToken() {
  const [password, setPassword] = useState("");
  const [token, setToken] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      setToken((await verifyPassword(password)).token);
      setPassword("");
    } catch (err) {
      setError(err instanceof VaultError ? err.message : "Something went wrong.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card title="Your token">
      {token ? (
        <div className="space-y-3">
          <CopyField value={token} />
          <p className="text-xs text-neutral-500">Anyone with this token can act as you. Keep it somewhere safe.</p>
          <Button variant="secondary" onClick={() => setToken(null)}>Hide</Button>
        </div>
      ) : (
        <form onSubmit={submit} className="space-y-3">
          <p className="text-sm text-neutral-600 dark:text-neutral-400">
            You need your token to sign in on another device or if you forget your password.
          </p>
          <Field label="Password">
            <input
              type="password"
              aria-label="Password to show token"
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              className={inputClass}
            />
          </Field>
          {error && <Notice tone="error">{error}</Notice>}
          <Button type="submit" busy={busy}>Show my token</Button>
        </form>
      )}
    </Card>
  );
}

function ChangePassword() {
  const [current, setCurrent] = useState("");
  const pw = useNewPassword();
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setDone(false);
    if (pw.problem) return setError(pw.problem);
    setBusy(true);
    setError(null);
    try {
      await changePassword(current, pw.password);
      setCurrent("");
      pw.reset();
      setDone(true);
    } catch (err) {
      setError(err instanceof VaultError ? err.message : "Something went wrong.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Card title="Change password">
      <form onSubmit={submit} className="space-y-4" noValidate>
        <Field label="Current password">
          <input
            type="password"
            aria-label="Current password"
            autoComplete="current-password"
            value={current}
            onChange={(e) => setCurrent(e.target.value)}
            className={inputClass}
          />
        </Field>
        <NewPasswordFields state={pw} names={{ password: "New password", confirm: "Confirm new password" }} label="New password" />
        {error && <Notice tone="error">{error}</Notice>}
        {done && <Notice tone="success">Password changed.</Notice>}
        <Button type="submit" busy={busy}>Change password</Button>
      </form>
    </Card>
  );
}
