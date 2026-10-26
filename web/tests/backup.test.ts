import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  BackupError,
  collectContents,
  createBackup,
  itemCount,
  markCovered,
  openBackup,
  pendingBackupCount,
  restoreContents,
} from "@/lib/backup";

function memoryStorage(): Storage {
  const m = new Map<string, string>();
  return {
    get length() {
      return m.size;
    },
    clear: () => m.clear(),
    getItem: (k) => m.get(k) ?? null,
    key: (i) => [...m.keys()][i] ?? null,
    removeItem: (k) => void m.delete(k),
    setItem: (k, v) => void m.set(k, String(v)),
  };
}

const P1 = "a1".repeat(32);
const P2 = "b2".repeat(32);
const jwk = { kty: "EC", crv: "P-256", d: "secret", x: "x", y: "y" } as JsonWebKey;
const FAST = 1000;

beforeEach(() => {
  vi.stubGlobal("localStorage", memoryStorage());
  vi.stubGlobal("window", { dispatchEvent: () => true, addEventListener: () => {}, removeEventListener: () => {} });
});

describe("backup", () => {
  it("collects secrets and arbiter keys, and nothing else", () => {
    localStorage.setItem(`toshi.secret.e1`, P1);
    localStorage.setItem(`toshi.arbiterKey.AB12`, JSON.stringify(jwk));
    localStorage.setItem("toshi.vault", "{}");
    localStorage.setItem("toshi.secret.bad", "not hex");
    const c = collectContents();
    expect(c.secrets).toEqual({ e1: P1 });
    expect(c.arbiterKeys).toEqual({ AB12: jwk });
    expect(itemCount(c)).toBe(2);
  });

  it("round-trips under the right password and rejects the wrong one", async () => {
    const file = await createBackup({ secrets: { e1: P1 }, arbiterKeys: { AB12: jwk } }, "right password", FAST);
    const text = JSON.stringify(file);
    expect(text).not.toContain(P1);
    expect(await openBackup(text, "right password")).toEqual({ secrets: { e1: P1 }, arbiterKeys: { AB12: jwk } });
    await expect(openBackup(text, "wrong password")).rejects.toMatchObject({ code: "wrong_password" });
  });

  it("refuses a file that is not a backup", async () => {
    await expect(openBackup("hello", "x")).rejects.toBeInstanceOf(BackupError);
    await expect(openBackup('{"v":1}', "x")).rejects.toMatchObject({ code: "not_a_backup" });
  });

  it("restores without overwriting what is already here", () => {
    localStorage.setItem("toshi.secret.e1", P2);
    const added = restoreContents({ secrets: { e1: P1, e2: P1 }, arbiterKeys: { AB12: jwk } });
    expect(added).toBe(2);
    expect(localStorage.getItem("toshi.secret.e1")).toBe(P2);
    expect(localStorage.getItem("toshi.secret.e2")).toBe(P1);
  });

  it("counts what is not yet backed up", () => {
    localStorage.setItem("toshi.secret.e1", P1);
    expect(pendingBackupCount()).toBe(1);
    markCovered(collectContents());
    expect(pendingBackupCount()).toBe(0);
    localStorage.setItem("toshi.secret.e2", P2);
    expect(pendingBackupCount()).toBe(1);
  });
});
