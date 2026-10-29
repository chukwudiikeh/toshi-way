// Sealing the buyer's release secret to an arbiter, and opening it again.
//
// The scheme is ECIES built from WebCrypto primitives every browser has:
//   - the buyer makes a throwaway ECDH P-256 key pair and agrees a shared secret with the
//     arbiter's public key,
//   - HKDF-SHA-256 (salt = the throwaway public key, info below) turns it into an AES-256-GCM key,
//   - AES-GCM encrypts the 32-byte secret, with the arbiter's public key as additional data so the
//     ciphertext is only valid for that recipient.
// The blob is: throwaway public key (65 bytes) | nonce (12) | ciphertext+tag (48), as base64.
// The server stores the blob but cannot open it: the private key never leaves the arbiter's browser.

import { fromHex, toHex } from "./secret";

const INFO = new TextEncoder().encode("toshi-way arbiter v1");
const CURVE = { name: "ECDH", namedCurve: "P-256" } as const;

const b64 = {
  encode(bytes: Uint8Array): string {
    let s = "";
    for (const b of bytes) s += String.fromCharCode(b);
    return btoa(s);
  },
  decode(text: string): Uint8Array {
    const s = atob(text.trim());
    return Uint8Array.from(s, (c) => c.charCodeAt(0));
  },
};

/** Always a fresh `ArrayBuffer`, so WebCrypto accepts it whatever the view's backing buffer was. */
const buf = (bytes: Uint8Array): ArrayBuffer => bytes.slice().buffer;

export type ArbiterKeyPair = {
  /** Base64 SubjectPublicKeyInfo: what the server stores and buyers encrypt to. */
  publicKey: string;
  /** The private key as a JWK. Keep it on the arbiter's device only. */
  privateJwk: JsonWebKey;
};

export async function generateArbiterKeyPair(): Promise<ArbiterKeyPair> {
  const pair = await crypto.subtle.generateKey(CURVE, true, ["deriveBits"]);
  const spki = new Uint8Array(await crypto.subtle.exportKey("spki", pair.publicKey));
  return { publicKey: b64.encode(spki), privateJwk: await crypto.subtle.exportKey("jwk", pair.privateKey) };
}

/** The public half of a private key, so a restored backup can be matched to what is registered. */
export async function publicKeyOf(privateJwk: JsonWebKey): Promise<string> {
  const { d: _d, key_ops: _ops, ...pub } = privateJwk;
  void _d;
  void _ops;
  const key = await crypto.subtle.importKey("jwk", pub, CURVE, true, []);
  return b64.encode(new Uint8Array(await crypto.subtle.exportKey("spki", key)));
}

/** SHA-256 of the key bytes as hex in groups of four: the same form the server shows. */
export async function fingerprint(publicKey: string): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", buf(b64.decode(publicKey))));
  return toHex(digest).match(/.{4}/g)!.join(" ");
}

async function aesKey(shared: ArrayBuffer, salt: Uint8Array, usage: "encrypt" | "decrypt"): Promise<CryptoKey> {
  const base = await crypto.subtle.importKey("raw", shared, "HKDF", false, ["deriveKey"]);
  return crypto.subtle.deriveKey(
    { name: "HKDF", hash: "SHA-256", salt: buf(salt), info: buf(INFO) },
    base,
    { name: "AES-GCM", length: 256 },
    false,
    [usage],
  );
}

/** Encrypt a 64-hex release secret to the arbiter's public key. */
export async function sealSecret(secretHex: string, arbiterPublicKey: string): Promise<string> {
  const secret = fromHex(secretHex);
  if (secret.length !== 32) throw new Error("The release secret must be 32 bytes.");
  const spki = b64.decode(arbiterPublicKey);
  const recipient = await crypto.subtle.importKey("spki", buf(spki), CURVE, false, []);
  const ephemeral = await crypto.subtle.generateKey(CURVE, true, ["deriveBits"]);
  const ephPublic = new Uint8Array(await crypto.subtle.exportKey("raw", ephemeral.publicKey));
  const shared = await crypto.subtle.deriveBits({ name: "ECDH", public: recipient }, ephemeral.privateKey, 256);
  const key = await aesKey(shared, ephPublic, "encrypt");
  const nonce = crypto.getRandomValues(new Uint8Array(12));
  const ct = new Uint8Array(
    await crypto.subtle.encrypt({ name: "AES-GCM", iv: buf(nonce), additionalData: buf(spki) }, key, buf(secret)),
  );
  const out = new Uint8Array(ephPublic.length + nonce.length + ct.length);
  out.set(ephPublic, 0);
  out.set(nonce, ephPublic.length);
  out.set(ct, ephPublic.length + nonce.length);
  return b64.encode(out);
}

/**
 * Open a sealed secret with the arbiter's private key. Throws if the blob was not sealed to this
 * key, was altered, or is malformed. Returns the 64-hex secret.
 */
export async function openSealed(sealed: string, privateJwk: JsonWebKey): Promise<string> {
  const blob = b64.decode(sealed);
  if (blob.length !== 65 + 12 + 48) throw new Error("That sealed secret is the wrong size.");
  const ephPublic = blob.slice(0, 65);
  const nonce = blob.slice(65, 77);
  const ct = blob.slice(77);
  const spki = b64.decode(await publicKeyOf(privateJwk));
  const priv = await crypto.subtle.importKey("jwk", privateJwk, CURVE, false, ["deriveBits"]);
  const eph = await crypto.subtle.importKey("raw", buf(ephPublic), CURVE, false, []);
  const shared = await crypto.subtle.deriveBits({ name: "ECDH", public: eph }, priv, 256);
  const key = await aesKey(shared, ephPublic, "decrypt");
  let plain: ArrayBuffer;
  try {
    plain = await crypto.subtle.decrypt({ name: "AES-GCM", iv: buf(nonce), additionalData: buf(spki) }, key, buf(ct));
  } catch {
    throw new Error("This sealed secret was not made for your key, or it has been altered.");
  }
  return toHex(new Uint8Array(plain));
}

// The arbiter's private keys, on this device, one per public key fingerprint.

const keyName = (fp: string) => `toshi.arbiterKey.${fp.replace(/\s+/g, "")}`;

export function saveArbiterKey(fp: string, privateJwk: JsonWebKey): void {
  localStorage.setItem(keyName(fp), JSON.stringify(privateJwk));
}

export function loadArbiterKey(fp: string): JsonWebKey | null {
  try {
    const raw = localStorage.getItem(keyName(fp));
    return raw ? (JSON.parse(raw) as JsonWebKey) : null;
  } catch {
    return null;
  }
}
