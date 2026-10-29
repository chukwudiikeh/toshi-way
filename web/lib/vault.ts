// The password vault: the account token, encrypted on this device under a password.
//
// This is how a wallet works. The token is the real credential (the server only knows its hash).
// A password the person can remember encrypts it here in the browser, and only the encrypted copy
// is kept on the device. The password never leaves the browser and the server never sees it.
// Moving to another device takes the token itself, which is also the way back in if the password is
// forgotten (there is no server-side reset, because there is nothing for the server to reset).
//
// Format: PBKDF2-SHA-256 (slow, salted) turns the password into an AES-256-GCM key; the session is
// encrypted with a random nonce. The iteration count is stored in the vault so it can be raised
// later without locking anyone out.

import type { Session } from "./session";

/** OWASP's current minimum for PBKDF2-HMAC-SHA-256. */
export const DEFAULT_ITERATIONS = 600_000;
const VERSION = 1;
const AAD = new TextEncoder().encode("toshi-way vault v1");

export type VaultFile = {
  v: 1;
  kdf: { name: "PBKDF2"; hash: "SHA-256"; iterations: number; salt: string };
  iv: string;
  /** The encrypted session (ciphertext and authentication tag). */
  ct: string;
  /** Not secret: shown on the lock screen so people know which account is on this device. */
  hint: { userId: string | null; kind: Session["kind"] };
};

export class VaultError extends Error {
  constructor(
    public code: "wrong_password" | "corrupt",
    message: string,
  ) {
    super(message);
  }
}

const b64 = {
  encode(bytes: Uint8Array): string {
    let s = "";
    for (const b of bytes) s += String.fromCharCode(b);
    return btoa(s);
  },
  decode(text: string): Uint8Array {
    return Uint8Array.from(atob(text), (c) => c.charCodeAt(0));
  },
};

/** A fresh `ArrayBuffer`, so WebCrypto accepts it whatever the view's backing buffer was. */
const buf = (bytes: Uint8Array): ArrayBuffer => bytes.slice().buffer;

async function deriveKey(password: string, salt: Uint8Array, iterations: number): Promise<CryptoKey> {
  const material = await crypto.subtle.importKey("raw", buf(new TextEncoder().encode(password.normalize("NFKC"))), "PBKDF2", false, [
    "deriveKey",
  ]);
  return crypto.subtle.deriveKey(
    { name: "PBKDF2", hash: "SHA-256", salt: buf(salt), iterations },
    material,
    { name: "AES-GCM", length: 256 },
    false,
    ["encrypt", "decrypt"],
  );
}

/** Encrypt `session` under `password`. `iterations` is only for tests; real use takes the default. */
export async function createVault(session: Session, password: string, iterations = DEFAULT_ITERATIONS): Promise<VaultFile> {
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const key = await deriveKey(password, salt, iterations);
  const plain = new TextEncoder().encode(JSON.stringify({ token: session.token, kind: session.kind, userId: session.userId }));
  const ct = new Uint8Array(await crypto.subtle.encrypt({ name: "AES-GCM", iv: buf(iv), additionalData: buf(AAD) }, key, buf(plain)));
  return {
    v: VERSION,
    kdf: { name: "PBKDF2", hash: "SHA-256", iterations, salt: b64.encode(salt) },
    iv: b64.encode(iv),
    ct: b64.encode(ct),
    hint: { userId: session.userId, kind: session.kind },
  };
}

/** Decrypt a vault. Throws `wrong_password` for a wrong password and `corrupt` for a damaged file. */
export async function openVault(file: VaultFile, password: string): Promise<Session> {
  let plain: ArrayBuffer;
  try {
    const key = await deriveKey(password, b64.decode(file.kdf.salt), file.kdf.iterations);
    plain = await crypto.subtle.decrypt(
      { name: "AES-GCM", iv: buf(b64.decode(file.iv)), additionalData: buf(AAD) },
      key,
      buf(b64.decode(file.ct)),
    );
  } catch {
    // AES-GCM cannot tell a wrong key from a tampered file: both fail authentication.
    throw new VaultError("wrong_password", "That password is not right.");
  }
  try {
    const v = JSON.parse(new TextDecoder().decode(plain)) as Partial<Session>;
    if (typeof v.token !== "string" || !v.token) throw new Error("no token");
    return {
      token: v.token,
      kind: v.kind === "operator" ? "operator" : "user",
      userId: typeof v.userId === "string" ? v.userId : null,
    };
  } catch {
    throw new VaultError("corrupt", "The saved account on this device is damaged. Import it again with your token.");
  }
}

/** Parse what is stored, accepting only a well-formed vault. */
export function parseVault(raw: string | null): VaultFile | null {
  if (!raw) return null;
  try {
    const v = JSON.parse(raw) as Partial<VaultFile>;
    const k = v.kdf;
    const ok =
      v.v === VERSION &&
      k?.name === "PBKDF2" &&
      k.hash === "SHA-256" &&
      Number.isInteger(k.iterations) &&
      k.iterations >= 1 &&
      k.iterations <= 10_000_000 &&
      typeof k.salt === "string" &&
      typeof v.iv === "string" &&
      typeof v.ct === "string" &&
      !!v.hint &&
      (v.hint.kind === "user" || v.hint.kind === "operator");
    return ok ? (v as VaultFile) : null;
  } catch {
    return null;
  }
}

// Passwords

export const MIN_PASSWORD_LENGTH = 10;

// A few of the passwords that are tried first. A real list is far longer; this only stops the
// most obvious choices, and the strength bar does the rest.
const COMMON = new Set([
  "password", "password1", "password123", "passw0rd", "1234567890", "12345678910", "qwertyuiop", "iloveyou123",
  "letmein123", "welcome123", "admin12345", "bitcoin123", "lightning1", "0123456789", "abcdefghij", "1q2w3e4r5t",
]);

export type PasswordCheck = {
  ok: boolean;
  /** What to tell the person when it is not acceptable. */
  message: string | null;
  /** 0 (unusable) to 4 (strong), for a strength bar. */
  strength: 0 | 1 | 2 | 3 | 4;
};

/** Whether a password is acceptable for protecting an account on this device. */
export function checkPassword(password: string): PasswordCheck {
  if (password.length < MIN_PASSWORD_LENGTH) {
    return { ok: false, message: `Use at least ${MIN_PASSWORD_LENGTH} characters.`, strength: password.length === 0 ? 0 : 1 };
  }
  if (new Set(password).size < 4) return { ok: false, message: "That repeats too few characters.", strength: 1 };
  if (COMMON.has(password.toLowerCase())) return { ok: false, message: "That is one of the first passwords an attacker tries.", strength: 1 };

  const classes = [/[a-z]/, /[A-Z]/, /[0-9]/, /[^A-Za-z0-9]/].filter((r) => r.test(password)).length;
  const strength = Math.min(4, (password.length >= 16 ? 2 : password.length >= 12 ? 1 : 0) + (classes >= 3 ? 2 : classes === 2 ? 1 : 0) + 1) as 1 | 2 | 3 | 4;
  return { ok: true, message: null, strength };
}

export const strengthLabels = ["", "Weak", "Fair", "Good", "Strong"] as const;
