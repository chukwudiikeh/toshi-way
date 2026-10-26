import { describe, expect, it } from "vitest";
import { checkPassword, createVault, MIN_PASSWORD_LENGTH, openVault, parseVault, VaultError } from "@/lib/vault";
import type { Session } from "@/lib/session";

// A low iteration count keeps the tests fast; the real default is far higher.
const FAST = 1000;
const session: Session = { token: "tw_" + "ab".repeat(32), kind: "user", userId: "u_4b2c5b5819089abd" };

describe("the vault", () => {
  it("round-trips a session under the right password", async () => {
    const vault = await createVault(session, "correct horse battery", FAST);
    expect(await openVault(vault, "correct horse battery")).toEqual(session);
  });

  it("never contains the token in the clear, but shows whose account it is", async () => {
    const vault = await createVault(session, "correct horse battery", FAST);
    const text = JSON.stringify(vault);
    expect(text).not.toContain(session.token);
    expect(text).not.toContain("ab".repeat(20));
    expect(vault.hint).toEqual({ userId: session.userId, kind: "user" });
  });

  it("refuses a wrong password, whatever it is close to", async () => {
    const vault = await createVault(session, "correct horse battery", FAST);
    for (const wrong of ["", "correct horse batterY", "correct horse battery ", "x"]) {
      await expect(openVault(vault, wrong)).rejects.toMatchObject({ code: "wrong_password" });
    }
  });

  it("uses a new salt and nonce every time", async () => {
    const [a, b] = [await createVault(session, "same password!!", FAST), await createVault(session, "same password!!", FAST)];
    expect(a.kdf.salt).not.toBe(b.kdf.salt);
    expect(a.iv).not.toBe(b.iv);
    expect(a.ct).not.toBe(b.ct);
  });

  it("detects tampering with any part", async () => {
    const vault = await createVault(session, "correct horse battery", FAST);
    const flip = (b64: string) => {
      const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
      bytes[0] ^= 1;
      return btoa(String.fromCharCode(...bytes));
    };
    for (const tampered of [{ ...vault, ct: flip(vault.ct) }, { ...vault, iv: flip(vault.iv) }, { ...vault, kdf: { ...vault.kdf, salt: flip(vault.kdf.salt) } }]) {
      await expect(openVault(tampered, "correct horse battery")).rejects.toBeInstanceOf(VaultError);
    }
  });

  it("treats the same password with different Unicode spellings as the same password", async () => {
    const vault = await createVault(session, "café au lait 2024", FAST); // é as one character
    expect(await openVault(vault, "café au lait 2024")).toEqual(session); // e + combining accent
  });

  it("keeps an operator's kind and a missing user id", async () => {
    const op: Session = { token: "operator-token-0123456789abcdef", kind: "operator", userId: null };
    expect(await openVault(await createVault(op, "operator password!", FAST), "operator password!")).toEqual(op);
  });

  it("rereads the iteration count it was made with", async () => {
    const vault = await createVault(session, "correct horse battery", 1500);
    expect(vault.kdf.iterations).toBe(1500);
    expect(await openVault(vault, "correct horse battery")).toEqual(session);
  });
});

describe("reading a stored vault", () => {
  it("accepts a well-formed vault and refuses anything else", async () => {
    const vault = await createVault(session, "correct horse battery", FAST);
    expect(parseVault(JSON.stringify(vault))).toEqual(vault);
    for (const bad of [
      null, "", "not json", "{}", "[]",
      JSON.stringify({ ...vault, v: 2 }),
      JSON.stringify({ ...vault, kdf: { ...vault.kdf, name: "scrypt" } }),
      JSON.stringify({ ...vault, kdf: { ...vault.kdf, iterations: 0 } }),
      JSON.stringify({ ...vault, kdf: { ...vault.kdf, iterations: 1e12 } }),
      JSON.stringify({ ...vault, ct: 5 }),
      JSON.stringify({ ...vault, hint: { userId: null, kind: "admin" } }),
    ]) {
      expect(parseVault(bad as string | null), String(bad).slice(0, 40)).toBeNull();
    }
  });
});

describe("password rules", () => {
  it("needs a minimum length", () => {
    const short = checkPassword("short1!");
    expect(short.ok).toBe(false);
    expect(short.message).toContain(String(MIN_PASSWORD_LENGTH));
    expect(checkPassword("").strength).toBe(0);
  });

  it("refuses repeated characters and the most common passwords", () => {
    expect(checkPassword("aaaaaaaaaaaa").ok).toBe(false);
    expect(checkPassword("abababababab").ok).toBe(false);
    expect(checkPassword("Password123").ok).toBe(false);
    expect(checkPassword("1234567890").ok).toBe(false);
  });

  it("accepts a reasonable password and scores longer, more varied ones higher", () => {
    expect(checkPassword("blue-kettle-moon").ok).toBe(true);
    const okay = checkPassword("tenletters1").strength;
    const better = checkPassword("a longer pass phrase 2024").strength;
    const best = checkPassword("Tr0ub4dor&3-correct-horse").strength;
    expect(better).toBeGreaterThanOrEqual(okay);
    expect(best).toBeGreaterThanOrEqual(better);
    expect(best).toBe(4);
  });
});
