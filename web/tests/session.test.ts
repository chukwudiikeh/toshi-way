import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Session } from "@/lib/session";

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

const alice: Session = { token: "tw_" + "ab".repeat(32), kind: "user", userId: "u_alice" };
let local: Storage;
let tab: Storage;

/** A fresh copy of the module, as after a page load, over the same storage. */
async function load() {
  vi.resetModules();
  const m = await import("@/lib/session");
  m.setKdfIterationsForTests(1000);
  return m;
}

beforeEach(() => {
  local = memoryStorage();
  tab = memoryStorage();
  vi.stubGlobal("localStorage", local);
  vi.stubGlobal("sessionStorage", tab);
  vi.stubGlobal("window", { addEventListener() {}, removeEventListener() {} });
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("a new account", () => {
  it("is saved only in encrypted form and is open straight away", async () => {
    const s = await load();
    expect(s.getVaultState()).toBe("none");
    await s.startAccount(alice, "a long enough password");
    expect(s.getVaultState()).toBe("unlocked");
    expect(s.getSession()).toEqual(alice);
    // The token is nowhere on the device in the clear.
    expect(local.getItem("toshi.vault")).not.toContain(alice.token);
    expect(local.getItem("toshi.session")).toBeNull();
  });

  it("replaces a plain token that was already there", async () => {
    local.setItem("toshi.session", JSON.stringify(alice));
    const s = await load();
    expect(s.getVaultState()).toBe("unprotected");
    await s.protectAccount("a long enough password");
    expect(s.getVaultState()).toBe("unlocked");
    expect(local.getItem("toshi.session")).toBeNull();
    expect(local.getItem("toshi.vault")).not.toContain(alice.token);
  });
});

describe("locking and unlocking", () => {
  it("locks, keeps the encrypted copy, and opens again with the password", async () => {
    const s = await load();
    await s.startAccount(alice, "a long enough password");
    s.lock();
    expect(s.getVaultState()).toBe("locked");
    expect(s.getSession()).toBeNull();
    expect(s.getVaultHint()).toEqual({ userId: "u_alice", kind: "user" });

    await s.unlock("a long enough password");
    expect(s.getVaultState()).toBe("unlocked");
    expect(s.getSession()).toEqual(alice);
  });

  it("stays locked after a wrong password", async () => {
    const s = await load();
    await s.startAccount(alice, "a long enough password");
    s.lock();
    await expect(s.unlock("not the password")).rejects.toMatchObject({ code: "wrong_password" });
    expect(s.getVaultState()).toBe("locked");
    expect(s.getSession()).toBeNull();
  });

  it("remembers the account across a page load as locked", async () => {
    let s = await load();
    await s.startAccount(alice, "a long enough password");
    s.lock();
    s = await load(); // a new page load, same device
    expect(s.getVaultState()).toBe("locked");
    await s.unlock("a long enough password");
    expect(s.getSession()).toEqual(alice);
  });

  it("an unprotected account is simply signed out when locked", async () => {
    local.setItem("toshi.session", JSON.stringify(alice));
    const s = await load();
    s.lock();
    expect(s.getVaultState()).toBe("none");
    expect(local.getItem("toshi.session")).toBeNull();
  });
});

describe("refreshing the page", () => {
  it("stays open in the same tab without asking again", async () => {
    let s = await load();
    await s.startAccount(alice, "a long enough password");
    s = await load(); // refresh: memory is gone, this tab's storage is not
    expect(s.getVaultState()).toBe("unlocked");
    expect(s.getSession()).toEqual(alice);
  });

  it("asks for the password in a new tab", async () => {
    const s1 = await load();
    await s1.startAccount(alice, "a long enough password");
    tab = memoryStorage(); // a different tab has its own session storage
    vi.stubGlobal("sessionStorage", tab);
    const s2 = await load();
    expect(s2.getVaultState()).toBe("locked");
  });

  it("asks again once the tab has been idle too long", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000_000);
    let s = await load();
    await s.startAccount(alice, "a long enough password");
    vi.setSystemTime(1_000_000 + s.IDLE_LOCK_MS + 1000);
    s = await load();
    expect(s.getVaultState()).toBe("locked");
  });
});

describe("the idle lock", () => {
  it("locks an open account that has been left alone, and not one in use", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(5_000_000);
    const s = await load();
    await s.startAccount(alice, "a long enough password");

    vi.setSystemTime(5_000_000 + s.IDLE_LOCK_MS - 1000);
    expect(s.lockIfIdle()).toBe(false);
    s.touch(); // someone clicked something
    vi.setSystemTime(5_000_000 + s.IDLE_LOCK_MS + 5000);
    expect(s.lockIfIdle()).toBe(false);

    vi.setSystemTime(5_000_000 + s.IDLE_LOCK_MS - 1000 + s.IDLE_LOCK_MS + 6000);
    expect(s.lockIfIdle()).toBe(true);
    expect(s.getVaultState()).toBe("locked");
  });

  it("never locks an account with no password, which has nothing to unlock it with", async () => {
    local.setItem("toshi.session", JSON.stringify(alice));
    const s = await load();
    expect(s.lockIfIdle(Date.now() + 10 * s.IDLE_LOCK_MS)).toBe(false);
    expect(s.getVaultState()).toBe("unprotected");
  });
});

describe("removing the account from this device", () => {
  it("wipes the encrypted copy as well as the open session", async () => {
    const s = await load();
    await s.startAccount(alice, "a long enough password");
    s.removeVault();
    expect(s.getVaultState()).toBe("none");
    expect(local.getItem("toshi.vault")).toBeNull();
    expect(tab.getItem("toshi.unlocked")).toBeNull();
    const again = await load();
    expect(again.getVaultState()).toBe("none");
  });
});

describe("a damaged vault", () => {
  it("is ignored rather than crashing", async () => {
    local.setItem("toshi.vault", "{not json");
    const s = await load();
    expect(s.getVaultState()).toBe("none");
  });
});

describe("a second account", () => {
  it("replaces the first on this device, including its encrypted copy", async () => {
    const s = await load();
    await s.startAccount(alice, "a long enough password");
    const bob: Session = { token: "tw_" + "cd".repeat(32), kind: "user", userId: "u_bob" };
    await s.startAccount(bob, "another good password");
    s.lock();
    await expect(s.unlock("a long enough password")).rejects.toBeDefined();
    await s.unlock("another good password");
    expect(s.getSession()).toEqual(bob);
  });
});

describe("remembering a user id", () => {
  it("fills in a missing id for the open session", async () => {
    const s = await load();
    s.setSession({ token: "tw_x", kind: "user", userId: null });
    s.rememberUserId("u_found");
    expect(s.getSession()?.userId).toBe("u_found");
  });
});
