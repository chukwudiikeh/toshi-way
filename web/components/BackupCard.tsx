"use client";

import Link from "next/link";
import { useRef, useState, type FormEvent } from "react";
import {
  BackupError,
  backupFileName,
  collectContents,
  createBackup,
  itemCount,
  markCovered,
  openBackup,
  restoreContents,
  usePendingBackup,
} from "@/lib/backup";
import { checkPassword } from "@/lib/vault";
import { Button, Card, Field, inputClass, Notice } from "./ui";

/** A reminder when this browser holds secrets or keys that no backup has. Shown where they are made. */
export function BackupReminder() {
  const pending = usePendingBackup();
  if (!pending) return null;
  return (
    <Notice tone="warn">
      {pending === 1 ? "A secret in this browser isn't" : `${pending} secrets and keys in this browser aren't`} backed up
      yet. If this browser&apos;s data is lost, they are gone.{" "}
      <Link href="/account#backup" className="font-semibold underline">
        Make a backup
      </Link>
      .
    </Notice>
  );
}

/** Download an encrypted backup of the release secrets and arbiter keys, or load one on a new device. */
export function BackupCard() {
  const pending = usePendingBackup();
  // `pending` is undefined until the browser can be read, which keeps the server and first render the same.
  const total = pending === undefined ? 0 : itemCount(collectContents());
  return (
    <Card title="Backup" id="backup">
      <div className="space-y-6">
        <p className="text-sm text-neutral-600 dark:text-neutral-400">
          Your token gets you into your account on any device. But a buyer&apos;s release secrets and an arbiter&apos;s
          private keys are made in this browser and kept only here. A backup file carries them to another device. It
          is encrypted with a password and does not contain your token.
        </p>
        <p className="text-sm font-medium" aria-live="polite">
          {total === 0
            ? "Nothing to back up yet."
            : `${total} item${total === 1 ? "" : "s"} in this browser${pending ? `, ${pending} not backed up yet` : ", all backed up"}.`}
        </p>
        <ExportBackup disabled={total === 0} />
        <ImportBackup />
      </div>
    </Card>
  );
}

function ExportBackup({ disabled }: { disabled: boolean }) {
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setDone(false);
    const check = checkPassword(password);
    if (!check.ok) return setError(check.message);
    setBusy(true);
    setError(null);
    try {
      const contents = collectContents();
      const file = await createBackup(contents, password);
      const url = URL.createObjectURL(new Blob([JSON.stringify(file)], { type: "application/json" }));
      const a = document.createElement("a");
      a.href = url;
      a.download = backupFileName();
      a.click();
      URL.revokeObjectURL(url);
      markCovered(contents);
      setPassword("");
      setDone(true);
    } catch {
      setError("Could not make the backup. Please try again.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} className="space-y-3" noValidate>
      <Field label="Password for the backup file" hint="Use your account password so there is only one to remember. You will need it to open the file.">
        <input
          type="password"
          aria-label="Backup password"
          autoComplete="new-password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className={inputClass}
        />
      </Field>
      {error && <Notice tone="error">{error}</Notice>}
      {done && <Notice tone="success">Backup downloaded. Keep the file somewhere safe, away from this device.</Notice>}
      <Button type="submit" busy={busy} disabled={disabled}>
        Download backup
      </Button>
    </form>
  );
}

function ImportBackup() {
  const fileRef = useRef<HTMLInputElement>(null);
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setMessage(null);
    const file = fileRef.current?.files?.[0];
    if (!file) return setError("Choose a backup file.");
    if (!password) return setError("Enter the backup's password.");
    setBusy(true);
    setError(null);
    try {
      const contents = await openBackup(await file.text(), password);
      const added = restoreContents(contents);
      const skipped = itemCount(contents) - added;
      setMessage(
        `Restored ${added} item${added === 1 ? "" : "s"}.` + (skipped ? ` ${skipped} already here, left as they were.` : ""),
      );
      setPassword("");
      if (fileRef.current) fileRef.current.value = "";
    } catch (err) {
      setError(err instanceof BackupError ? err.message : "Could not read that file.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} className="space-y-3 border-t border-neutral-200 pt-5 dark:border-neutral-800">
      <div className="text-sm font-medium">Restore from a backup</div>
      <Field label="Backup file">
        <input ref={fileRef} type="file" accept="application/json,.json" aria-label="Backup file" className="block w-full text-sm" />
      </Field>
      <Field label="Backup password">
        <input
          type="password"
          aria-label="Password of the backup file"
          autoComplete="off"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          className={inputClass}
        />
      </Field>
      {error && <Notice tone="error">{error}</Notice>}
      {message && <Notice tone="success">{message}</Notice>}
      <Button type="submit" variant="secondary" busy={busy}>
        Restore
      </Button>
    </form>
  );
}
