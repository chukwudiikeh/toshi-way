// The release secret P: generated in the browser, kept in this browser, handed to the seller
// through a URL fragment. Only H = sha256(P) is sent to the server when the buyer accepts.

const HEX64 = /^[0-9a-f]{64}$/;
const FRAGMENT_KEY = "release";

export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

export function fromHex(hex: string): Uint8Array {
  if (hex.length % 2 !== 0 || !/^[0-9a-f]*$/.test(hex)) throw new Error("not hex");
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}

/** A fresh 32-byte secret as 64 lowercase hex characters. */
export function generateSecret(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return toHex(bytes);
}

/**
 * Normalise user input to a 64-hex secret, or null if it is anything else.
 * Short codes are never accepted: a 6-digit code could be brute-forced from the hash.
 */
export function parseSecret(input: string): string | null {
  const s = input.trim().toLowerCase();
  return HEX64.test(s) ? s : null;
}

/** sha256 of the secret's 32 bytes (not of its hex text), as hex. */
export async function hashSecret(secretHex: string): Promise<string> {
  const p = parseSecret(secretHex);
  if (!p) throw new Error("secret must be exactly 64 hex characters");
  const digest = await crypto.subtle.digest("SHA-256", fromHex(p) as BufferSource);
  return toHex(new Uint8Array(digest));
}

/** `<origin>/escrows/<id>#release=<P>`. The fragment never leaves the browser that opens it. */
export function buildReleaseLink(origin: string, escrowId: string, secretHex: string): string {
  const p = parseSecret(secretHex);
  if (!p) throw new Error("secret must be exactly 64 hex characters");
  return `${origin.replace(/\/+$/, "")}/escrows/${encodeURIComponent(escrowId)}#${FRAGMENT_KEY}=${p}`;
}

/** Read the secret from a URL fragment such as `#release=<P>`; null when absent or malformed. */
export function parseReleaseFragment(hash: string): string | null {
  const params = new URLSearchParams(hash.replace(/^#/, ""));
  const value = params.get(FRAGMENT_KEY);
  return value === null ? null : parseSecret(value);
}

// Storage on the buyer's device, one entry per escrow.

const storageKey = (escrowId: string) => `toshi.secret.${escrowId}`;

export function loadSecret(escrowId: string): string | null {
  try {
    const v = localStorage.getItem(storageKey(escrowId));
    return v === null ? null : parseSecret(v);
  } catch {
    return null;
  }
}

export function saveSecret(escrowId: string, secretHex: string): void {
  const p = parseSecret(secretHex);
  if (!p) throw new Error("secret must be exactly 64 hex characters");
  localStorage.setItem(storageKey(escrowId), p);
}
