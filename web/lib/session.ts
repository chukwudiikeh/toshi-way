// The signed-in account on this device. This is the only module that touches where it is kept;
// everything else reads it through getSession() or useSession().
//
// An account is a token. It is kept in one of three ways:
//   - in a password vault (lib/vault.ts): the token encrypted under a password, in localStorage.
//     Opening the app asks for the password, and the decrypted session then lives in memory (and in
//     this tab's sessionStorage, so a refresh does not ask again) until the tab closes or the app sits
//     idle for a while.
//   - unprotected: the plain token in localStorage, as every account used to be. These keep working
//     and are nudged to add a password.
//   - not at all: nobody has signed in on this device.

import { useSyncExternalStore } from "react";
import { createVault, DEFAULT_ITERATIONS, openVault, parseVault, type VaultFile } from "./vault";

export type Session = {
  token: string;
  /** "user" for buyers and sellers, "operator" for the service operator. */
  kind: "user" | "operator";
  /** The user's id when known: shown so a seller can be told whom to invite. */
  userId: string | null;
};

/** `none`: no account here. `locked`: a vault is here, waiting for its password. `unlocked`: open. `unprotected`: a plain token, no password. */
export type VaultState = "none" | "locked" | "unlocked" | "unprotected";

const LEGACY_KEY = "toshi.session";
const VAULT_KEY = "toshi.vault";
const TAB_KEY = "toshi.unlocked";
/** How long the app may sit untouched before an unlocked account locks again. */
export const IDLE_LOCK_MS = 30 * 60 * 1000;

const listeners = new Set<() => void>();

// The key-derivation cost for new vaults. Only tests lower it, to keep them quick.
let kdfIterations = DEFAULT_ITERATIONS;
export function setKdfIterationsForTests(n: number): void {
  kdfIterations = n;
}

let loaded = false;
let active: Session | null = null;
let vault: VaultFile | null = null;
let lastActive = 0;

function read(store: "local" | "session", key: string): string | null {
  try {
    return (store === "local" ? localStorage : sessionStorage).getItem(key);
  } catch {
    return null;
  }
}

function write(store: "local" | "session", key: string, value: string | null): void {
  try {
    const s = store === "local" ? localStorage : sessionStorage;
    if (value === null) s.removeItem(key);
    else s.setItem(key, value);
  } catch {
    // Storage can be unavailable (a private window, blocked site data); the session still works in memory.
  }
}

function parseSession(raw: string | null): Session | null {
  if (!raw) return null;
  try {
    const v = JSON.parse(raw) as Partial<Session>;
    if (typeof v.token !== "string" || !v.token) return null;
    return {
      token: v.token,
      kind: v.kind === "operator" ? "operator" : "user",
      userId: typeof v.userId === "string" ? v.userId : null,
    };
  } catch {
    return null;
  }
}

/** Work out the current state from what is stored. Runs once, then the module keeps it current. */
function load(): void {
  if (loaded) return;
  loaded = true;
  vault = parseVault(read("local", VAULT_KEY));
  if (vault) {
    // A tab that was unlocked a moment ago (a refresh) stays unlocked, unless it has been idle too long.
    try {
      const tab = JSON.parse(read("session", TAB_KEY) ?? "null") as { session?: unknown; at?: unknown } | null;
      const s = parseSession(JSON.stringify(tab?.session ?? null));
      if (s && typeof tab?.at === "number" && Date.now() - tab.at < IDLE_LOCK_MS) {
        active = s;
        lastActive = tab.at;
      } else {
        write("session", TAB_KEY, null);
      }
    } catch {
      write("session", TAB_KEY, null);
    }
  } else {
    active = parseSession(read("local", LEGACY_KEY));
  }
}

function emit(): void {
  listeners.forEach((l) => l());
}

function persistTab(): void {
  if (vault && active) write("session", TAB_KEY, JSON.stringify({ session: active, at: lastActive }));
  else write("session", TAB_KEY, null);
}

export function getVaultState(): VaultState {
  load();
  if (vault) return active ? "unlocked" : "locked";
  return active ? "unprotected" : "none";
}

/** The current session, or null when there is none or the account is locked. */
export function getSession(): Session | null {
  load();
  return active;
}

/** Whose account is on this device, for the lock screen. Not secret. */
export function getVaultHint(): VaultFile["hint"] | null {
  load();
  return vault?.hint ?? null;
}

/** Use `session` for now, without saving it anywhere (the caller decides how it is kept). */
export function setSession(session: Session): void {
  load();
  active = session;
  lastActive = Date.now();
  persistTab();
  emit();
}

/** Start a new account on this device, protected by `password`. Replaces any account already here. */
export async function startAccount(session: Session, password: string): Promise<void> {
  load();
  const file = await createVault(session, password, kdfIterations);
  vault = file;
  write("local", VAULT_KEY, JSON.stringify(file));
  write("local", LEGACY_KEY, null); // the plain token must not stay behind
  active = session;
  lastActive = Date.now();
  persistTab();
  emit();
}

/** Add a password to the unprotected account that is signed in now. */
export async function protectAccount(password: string): Promise<void> {
  load();
  if (!active) throw new Error("Nobody is signed in.");
  await startAccount(active, password);
}

/** Open the vault on this device. Throws a VaultError for a wrong password. */
export async function unlock(password: string): Promise<void> {
  load();
  if (!vault) throw new Error("There is no saved account on this device.");
  const session = await openVault(vault, password);
  active = session;
  lastActive = Date.now();
  persistTab();
  emit();
}

/** Check a password against the vault without changing anything. Returns the session it protects. */
export async function verifyPassword(password: string): Promise<Session> {
  load();
  if (!vault) throw new Error("There is no saved account on this device.");
  return openVault(vault, password);
}

/** Change the password: prove the old one, then encrypt the same session under the new one. */
export async function changePassword(current: string, next: string): Promise<void> {
  const session = await verifyPassword(current);
  await startAccount(session, next);
}

/** Lock the account: the encrypted copy stays, the open one is forgotten. An unprotected account is simply signed out. */
export function lock(): void {
  load();
  active = null;
  write("session", TAB_KEY, null);
  if (!vault) write("local", LEGACY_KEY, null);
  emit();
}

/** Same as locking: what "sign out" means for the app. */
export function clearSession(): void {
  lock();
}

/** Forget the account on this device completely, including the encrypted copy. The token is the only way back. */
export function removeVault(): void {
  load();
  active = null;
  vault = null;
  write("local", VAULT_KEY, null);
  write("local", LEGACY_KEY, null);
  write("session", TAB_KEY, null);
  emit();
}

/** Fill in the user's id once it becomes known (for example from an escrow's party ids). */
export function rememberUserId(userId: string): void {
  const s = getSession();
  if (s && s.kind === "user" && s.userId !== userId) setSession({ ...s, userId });
}

/** Note that someone is using the app, which postpones the idle lock. */
export function touch(): void {
  if (!active || !vault) return;
  lastActive = Date.now();
  // Writing on every event would be wasteful; the stored time only needs to be roughly right.
  persistTab();
}

/** Lock if an unlocked account has sat idle for too long. Returns whether it locked. */
export function lockIfIdle(now = Date.now()): boolean {
  load();
  if (vault && active && now - lastActive >= IDLE_LOCK_MS) {
    lock();
    return true;
  }
  return false;
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  const onStorage = (e: StorageEvent) => {
    if (e.key === VAULT_KEY || e.key === LEGACY_KEY || e.key === null) {
      // Another tab changed what is saved on this device: read it again.
      loaded = false;
      active = null;
      vault = null;
      listener();
    }
  };
  window.addEventListener("storage", onStorage);
  return () => {
    listeners.delete(listener);
    window.removeEventListener("storage", onStorage);
  };
}

/**
 * The current session. `undefined` while rendering on the server and during hydration, so pages can
 * show a neutral placeholder instead of flashing the signed-out view. `null` when nobody is signed in
 * or the account is locked.
 */
export function useSession(): Session | null | undefined {
  return useSyncExternalStore<Session | null | undefined>(subscribe, getSession, () => undefined);
}

/** Whether this device has an account, and whether it is open. `undefined` until the browser has answered. */
export function useVaultState(): VaultState | undefined {
  return useSyncExternalStore<VaultState | undefined>(subscribe, getVaultState, () => undefined);
}

/** The account saved on this device, if any, for showing whose it is. */
export function useVaultHint(): VaultFile["hint"] | null | undefined {
  // The hint object is stable until the vault changes, so it is a valid snapshot.
  return useSyncExternalStore<VaultFile["hint"] | null | undefined>(subscribe, getVaultHint, () => undefined);
}
