"use client";

import { useCallback, useEffect, useState } from "react";
import { api, describeError } from "@/lib/api";
import { fingerprint, generateArbiterKeyPair, loadArbiterKey, publicKeyOf, saveArbiterKey } from "@/lib/arbiter";
import type { ArbiterKey } from "@/lib/schemas";
import { useSession } from "@/lib/session";
import { SignIn } from "./Account";
import { Button, Card, CopyField, Field, inputClass, Notice, Spinner } from "./ui";

/**
 * Where someone sets up to act as an arbiter. They make a key pair in this browser; the public half
 * is registered so buyers can encrypt their release secret to it, and the private half never leaves
 * this device. Losing it means no longer being able to decide the escrows that were sealed to it.
 */
export function ArbiterSetup() {
  const session = useSession();
  const [registered, setRegistered] = useState<ArbiterKey | null>(null);
  const [haveLocal, setHaveLocal] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [backup, setBackup] = useState("");
  const [restoreText, setRestoreText] = useState("");
  const [message, setMessage] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const key = await api.arbiterKey();
      setRegistered(key);
      setHaveLocal(key.fingerprint ? loadArbiterKey(key.fingerprint) !== null : null);
    } catch (err) {
      setError(describeError(err));
    }
  }, []);

  useEffect(() => {
    if (!session || session.kind !== "user") return;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- loads from the API once on mount
    void load();
  }, [session, load]);

  if (session === undefined) return <Spinner />;
  if (!session) return <SignIn note="Sign in to set up as an arbiter." />;
  if (session.kind === "operator") return <Notice tone="info">Operators can&apos;t act as arbiters.</Notice>;
  if (error) return <Notice tone="error">{error}</Notice>;
  if (!registered) return <Spinner />;

  async function create() {
    setBusy(true);
    setMessage(null);
    try {
      const pair = await generateArbiterKeyPair();
      const fp = await fingerprint(pair.publicKey);
      // Save the private key first: if registering fails the key is still here, and if the page is
      // closed after registering there is no window where the public key exists without it.
      saveArbiterKey(fp, pair.privateJwk);
      await api.setArbiterKey(pair.publicKey);
      setMessage("Your arbiter key is ready. Back it up below before you rely on it.");
      await load();
    } catch (err) {
      setError(describeError(err));
    } finally {
      setBusy(false);
    }
  }

  async function showBackup() {
    const fp = registered?.fingerprint;
    const jwk = fp ? loadArbiterKey(fp) : null;
    setBackup(jwk ? JSON.stringify(jwk) : "");
  }

  async function restore() {
    setMessage(null);
    try {
      const jwk = JSON.parse(restoreText) as JsonWebKey;
      if (!jwk.d) throw new Error("That is not a private key backup.");
      const fp = await fingerprint(await publicKeyOf(jwk));
      saveArbiterKey(fp, jwk);
      setRestoreText("");
      setMessage(`Restored the key with fingerprint ${fp}.`);
      await load();
    } catch {
      setMessage("That backup could not be read. Paste the whole text you copied.");
    }
  }

  const fp = registered.fingerprint;
  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-xl font-bold">Act as an arbiter</h1>
        <p className="text-sm text-neutral-600 dark:text-neutral-400">
          An arbiter is someone a seller and buyer both trust to decide a dispute. When a buyer starts an escrow that
          names you, their release secret is encrypted so that only you can open it. If there is a dispute you
          decide: pay the seller, or refund the buyer. Nobody else, including this service, can open it.
        </p>
      </div>

      {message && <Notice tone="info">{message}</Notice>}

      {fp ? (
        <Card title="Your arbiter key">
          <div className="space-y-4">
            <div>
              <div className="text-sm font-medium">Key fingerprint</div>
              <div className="break-words font-mono text-sm">{fp}</div>
              <p className="mt-1 text-xs text-neutral-500">
                Tell sellers and buyers this out of band. Before they trust an escrow, they compare it with the one
                they see on the escrow page, which makes sure nobody swapped your key.
              </p>
            </div>
            <div>
              <div className="text-sm font-medium">Your user id (give this to sellers)</div>
              <CopyField value={session.userId ?? ""} />
            </div>
            {haveLocal === false && (
              <Notice tone="error">
                This browser does not have the private key for this fingerprint, so you can&apos;t decide escrows
                from here. Restore your backup below.
              </Notice>
            )}
            {haveLocal && (
              <Notice tone="success">The private key is on this device. Back it up so a cleared browser doesn&apos;t lock you out.</Notice>
            )}
          </div>
        </Card>
      ) : (
        <Notice tone="info">You haven&apos;t set up a key yet, so nobody can name you as their arbiter.</Notice>
      )}

      <Card title={fp ? "Make a new key" : "Make your key"}>
        <div className="space-y-3">
          <p className="text-sm text-neutral-600 dark:text-neutral-400">
            {fp
              ? "New escrows will use the new key. Escrows already sealed to your old key keep it, so keep its backup."
              : "This creates a key pair in this browser and registers the public half."}
          </p>
          <Button onClick={create} busy={busy}>
            Create key
          </Button>
        </div>
      </Card>

      <Card title="Back up and restore">
        <div className="space-y-4">
          <Notice tone="warn">
            The backup is your private key. Anyone who has it can open the secrets sealed to you. Store it in a
            password manager, and never paste it into anything you don&apos;t trust.
          </Notice>
          <div className="space-y-2">
            <Button variant="secondary" onClick={showBackup} disabled={!haveLocal}>
              Show backup of my current key
            </Button>
            {backup && <CopyField value={backup} label="Copy backup" />}
          </div>
          <Field label="Restore from a backup" hint="Paste the text you saved. It is stored on this device only.">
            <textarea
              value={restoreText}
              onChange={(e) => setRestoreText(e.target.value)}
              className={`${inputClass} font-mono text-xs`}
              rows={3}
              spellCheck={false}
              autoComplete="off"
            />
          </Field>
          <Button variant="secondary" onClick={restore} disabled={!restoreText.trim()}>
            Restore
          </Button>
        </div>
      </Card>
    </div>
  );
}
