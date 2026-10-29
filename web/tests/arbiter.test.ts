import { describe, expect, it } from "vitest";
import { fingerprint, generateArbiterKeyPair, openSealed, publicKeyOf, sealSecret } from "@/lib/arbiter";
import { generateSecret } from "@/lib/secret";

describe("sealing a release secret to an arbiter", () => {
  it("round-trips: only the arbiter\'s private key opens it", async () => {
    const arbiter = await generateArbiterKeyPair();
    const secret = generateSecret();
    const sealed = await sealSecret(secret, arbiter.publicKey);
    expect(await openSealed(sealed, arbiter.privateJwk)).toBe(secret);
  });

  it("is 125 bytes, and never contains the secret", async () => {
    const arbiter = await generateArbiterKeyPair();
    const secret = generateSecret();
    const sealed = await sealSecret(secret, arbiter.publicKey);
    expect(atob(sealed).length).toBe(65 + 12 + 48);
    expect(sealed).not.toContain(secret);
    // 168 base64 characters: within what the server accepts for a sealed secret.
    expect(sealed.length).toBeGreaterThanOrEqual(100);
    expect(sealed.length).toBeLessThanOrEqual(1024);
  });

  it("makes a different blob each time for the same secret", async () => {
    const arbiter = await generateArbiterKeyPair();
    const secret = generateSecret();
    expect(await sealSecret(secret, arbiter.publicKey)).not.toBe(await sealSecret(secret, arbiter.publicKey));
  });

  it("is refused by anyone else\'s key", async () => {
    const [arbiter, other] = [await generateArbiterKeyPair(), await generateArbiterKeyPair()];
    const sealed = await sealSecret(generateSecret(), arbiter.publicKey);
    await expect(openSealed(sealed, other.privateJwk)).rejects.toThrow(/not made for your key/);
  });

  it("detects tampering with any part of the blob", async () => {
    const arbiter = await generateArbiterKeyPair();
    const sealed = await sealSecret(generateSecret(), arbiter.publicKey);
    const bytes = Uint8Array.from(atob(sealed), (c) => c.charCodeAt(0));
    for (const index of [10, 70, 90, bytes.length - 1]) {
      const copy = bytes.slice();
      copy[index] ^= 1;
      const tampered = btoa(String.fromCharCode(...copy));
      await expect(openSealed(tampered, arbiter.privateJwk), `byte ${index}`).rejects.toThrow();
    }
  });

  it("rejects blobs of the wrong size and secrets of the wrong length", async () => {
    const arbiter = await generateArbiterKeyPair();
    await expect(openSealed(btoa("short"), arbiter.privateJwk)).rejects.toThrow(/wrong size/);
    await expect(sealSecret("ab".repeat(31), arbiter.publicKey)).rejects.toThrow(/32 bytes/);
    await expect(sealSecret("zz".repeat(32), arbiter.publicKey)).rejects.toThrow();
  });

  it("derives the public key from a private backup, and fingerprints it in the server\'s format", async () => {
    const arbiter = await generateArbiterKeyPair();
    expect(await publicKeyOf(arbiter.privateJwk)).toBe(arbiter.publicKey);
    const fp = await fingerprint(arbiter.publicKey);
    expect(fp).toMatch(/^([0-9a-f]{4} ){15}[0-9a-f]{4}$/);
    expect(fp).not.toBe(await fingerprint((await generateArbiterKeyPair()).publicKey));
  });

  it("has a public key in the shape the server accepts (91-byte P-256 SPKI)", async () => {
    const arbiter = await generateArbiterKeyPair();
    const der = Uint8Array.from(atob(arbiter.publicKey), (c) => c.charCodeAt(0));
    expect(der.length).toBe(91);
    expect(Array.from(der.slice(0, 4))).toEqual([0x30, 0x59, 0x30, 0x13]);
    expect(der[26]).toBe(0x04);
  });
});
