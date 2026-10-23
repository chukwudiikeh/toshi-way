// The signed-in account, kept in localStorage. This is the only module that touches the token's
// storage; everything else reads it through getSession() or useSession().

import { useSyncExternalStore } from "react";

export type Session = {
  token: string;
  /** "user" for buyers and sellers, "operator" for the service operator. */
  kind: "user" | "operator";
  /** The user's id when known: shown so a seller can be told whom to invite. */
  userId: string | null;
};

const KEY = "toshi.session";
const listeners = new Set<() => void>();

let cachedRaw: string | null = null;
let cachedSession: Session | null = null;

function parse(raw: string | null): Session | null {
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

export function getSession(): Session | null {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(KEY);
  } catch {
    return null;
  }
  // Return the same object while the stored value is unchanged (useSyncExternalStore needs it).
  if (raw !== cachedRaw) {
    cachedRaw = raw;
    cachedSession = parse(raw);
  }
  return cachedSession;
}

function emit() {
  listeners.forEach((l) => l());
}

export function setSession(session: Session): void {
  localStorage.setItem(KEY, JSON.stringify(session));
  emit();
}

export function clearSession(): void {
  try {
    localStorage.removeItem(KEY);
  } finally {
    emit();
  }
}

/** Fill in the user's id once it becomes known (for example from an escrow's party ids). */
export function rememberUserId(userId: string): void {
  const s = getSession();
  if (s && s.kind === "user" && s.userId !== userId) setSession({ ...s, userId });
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  const onStorage = (e: StorageEvent) => {
    if (e.key === KEY || e.key === null) listener();
  };
  window.addEventListener("storage", onStorage);
  return () => {
    listeners.delete(listener);
    window.removeEventListener("storage", onStorage);
  };
}

/**
 * The current session. `undefined` while rendering on the server and during hydration, so
 * pages can show a neutral placeholder instead of flashing the signed-out view.
 */
export function useSession(): Session | null | undefined {
  return useSyncExternalStore<Session | null | undefined>(subscribe, getSession, () => undefined);
}
