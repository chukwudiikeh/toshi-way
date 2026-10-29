// The backup file: the things that live only in this browser, encrypted under a password.
//
// Signing in on a new device needs only the token. But a buyer's release secrets
// (`toshi.secret.<escrowId>`) and an arbiter's private keys (`toshi.arbiterKey.<fingerprint>`) are
// made in the browser and never reach the server, so they are lost with the browser's data. This
// file carries them to another device. It never contains the token.
//
// Same scheme as the vault: PBKDF2-SHA-256 into AES-256-GCM, with the iteration count stored in the file.

import { useSyncExternalStore } from "react";
import { b64, buf, DEFAULT_ITERATIONS, deriveKey } from "./vault";

const AAD = new TextEncoder().encode("toshi-way backup v1");
const SECRET_PREFIX = "toshi.secret.";
const KEY_PREFIX = "toshi.arbiterKey.";
const COVERED = "toshi.backup.covered";
const CHANGED = "toshi:backup-changed";

export type BackupContents = {
  secrets: Record<string, string>;
  arbiterKeys: Record<string, JsonWebKey>;
};

export type BackupFile = {
  v: 1;
  kind: "toshi-way-backup";
  kdf: { name: "PBKDF2"; hash: "SHA-256"; iterations: number; salt: string };
  iv: string;
  ct: string;
};

export class BackupError extends Error {
  constructor(
    public code: "wrong_password" | "not_a_backup",
    message: string,
  ) {
    super(message);
  }
}

// What this browser holds

function storageKeys(): string[] {
  try {
    return Array.from({ length: localStorage.length }, (_, i) => localStorage.key(i)).filter((k): k is string => k !== null);
  } catch {
    return [];
  }
}

/** Everything in this browser that a backup would carry. */
export function collectContents(): BackupContents {
  const out: BackupContents = { secrets: {}, arbiterKeys: {} };
  for (const k of storageKeys()) {
    const raw = localStorage.getItem(k);
    if (raw === null) continue;
    if (k.startsWith(SECRET_PREFIX) && /^[0-9a-f]{64}$/.test(raw)) out.secrets[k.slice(SECRET_PREFIX.length)] = raw;
    else if (k.startsWith(KEY_PREFIX)) {
      try {
        out.arbiterKeys[k.slice(KEY_PREFIX.length)] = JSON.parse(raw) as JsonWebKey;
      } catch {
        // A damaged entry is skipped rather than failing the whole backup.
      }
    }
  }
  return out;
}

export const itemCount = (c: BackupContents) => Object.keys(c.secrets).length + Object.keys(c.arbiterKeys).length;

const itemIds = (c: BackupContents) => [
  ...Object.keys(c.secrets).map((k) => SECRET_PREFIX + k),
  ...Object.keys(c.arbiterKeys).map((k) => KEY_PREFIX + k),
];

// Encrypting

export async function createBackup(
  contents: BackupContents,
  password: string,
  iterations = DEFAULT_ITERATIONS,
): Promise<BackupFile> {
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const key = await deriveKey(password, salt, iterations);
  const plain = new TextEncoder().encode(JSON.stringify(contents));
  const ct = new Uint8Array(await crypto.subtle.encrypt({ name: "AES-GCM", iv: buf(iv), additionalData: buf(AAD) }, key, buf(plain)));
  return {
    v: 1,
    kind: "toshi-way-backup",
    kdf: { name: "PBKDF2", hash: "SHA-256", iterations, salt: b64.encode(salt) },
    iv: b64.encode(iv),
    ct: b64.encode(ct),
  };
}

function parseBackup(text: string): BackupFile {
  try {
    const v = JSON.parse(text) as Partial<BackupFile>;
    const k = v.kdf;
    if (
      v.v === 1 &&
      v.kind === "toshi-way-backup" &&
      k?.name === "PBKDF2" &&
      k.hash === "SHA-256" &&
      Number.isInteger(k.iterations) &&
      k.iterations >= 1 &&
      k.iterations <= 10_000_000 &&
      typeof k.salt === "string" &&
      typeof v.iv === "string" &&
      typeof v.ct === "string"
    ) {
      return v as BackupFile;
    }
  } catch {
    // fall through
  }
  throw new BackupError("not_a_backup", "That file is not a toshi-way backup.");
}

export async function openBackup(text: string, password: string): Promise<BackupContents> {
  const file = parseBackup(text);
  let plain: ArrayBuffer;
  try {
    const key = await deriveKey(password, b64.decode(file.kdf.salt), file.kdf.iterations);
    plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: buf(b64.decode(file.iv)), additionalData: buf(AAD) }, key, buf(b64.decode(file.ct)));
  } catch {
    throw new BackupError("wrong_password", "That password is not right for this backup.");
  }
  try {
    const v = JSON.parse(new TextDecoder().decode(plain)) as Partial<BackupContents>;
    const out: BackupContents = { secrets: {}, arbiterKeys: {} };
    for (const [id, p] of Object.entries(v.secrets ?? {})) if (typeof p === "string" && /^[0-9a-f]{64}$/.test(p)) out.secrets[id] = p;
    for (const [fp, jwk] of Object.entries(v.arbiterKeys ?? {})) if (jwk && typeof jwk === "object") out.arbiterKeys[fp] = jwk;
    return out;
  } catch {
    throw new BackupError("not_a_backup", "That file is not a toshi-way backup.");
  }
}

/** Put a backup's items into this browser. Nothing already here is overwritten. Returns how many were added. */
export function restoreContents(c: BackupContents): number {
  let added = 0;
  for (const [id, p] of Object.entries(c.secrets)) {
    const k = SECRET_PREFIX + id;
    if (localStorage.getItem(k) !== null) continue;
    localStorage.setItem(k, p);
    added++;
  }
  for (const [fp, jwk] of Object.entries(c.arbiterKeys)) {
    const k = KEY_PREFIX + fp;
    if (localStorage.getItem(k) !== null) continue;
    localStorage.setItem(k, JSON.stringify(jwk));
    added++;
  }
  markCovered(c);
  return added;
}

// Knowing what is not yet backed up

function covered(): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem(COVERED) ?? "[]") as string[]);
  } catch {
    return new Set();
  }
}

/** Remember that these items are in a backup the person has. */
export function markCovered(c: BackupContents): void {
  try {
    localStorage.setItem(COVERED, JSON.stringify([...covered(), ...itemIds(c)]));
  } catch {
    // Nothing to do: the reminder will simply stay.
  }
  notifyChanged();
}

/** How many secrets and keys here are not in any backup yet. */
export function pendingBackupCount(): number {
  const done = covered();
  return itemIds(collectContents()).filter((id) => !done.has(id)).length;
}

/** Call after saving a secret or key so reminders update straight away. */
export function notifyChanged(): void {
  if (typeof window !== "undefined") window.dispatchEvent(new Event(CHANGED));
}

function subscribe(cb: () => void): () => void {
  window.addEventListener(CHANGED, cb);
  window.addEventListener("storage", cb);
  return () => {
    window.removeEventListener(CHANGED, cb);
    window.removeEventListener("storage", cb);
  };
}

/** The number of items not yet backed up; `undefined` before the browser can say. */
export function usePendingBackup(): number | undefined {
  return useSyncExternalStore<number | undefined>(subscribe, pendingBackupCount, () => undefined);
}

/** A file name such as `toshi-way-backup-2026-10-26.json`. */
export const backupFileName = (d = new Date()) => `toshi-way-backup-${d.toISOString().slice(0, 10)}.json`;
