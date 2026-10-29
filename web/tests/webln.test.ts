import { afterEach, describe, expect, it, vi } from "vitest";
import { getWebLN, payWithWebLN } from "@/lib/webln";

afterEach(() => vi.unstubAllGlobals());
const stubWindow = (webln?: unknown) => vi.stubGlobal("window", { webln });

describe("webln", () => {
  it("finds no wallet when none is injected, or when it is incomplete", () => {
    stubWindow();
    expect(getWebLN()).toBeNull();
    stubWindow({ enable: async () => {} });
    expect(getWebLN()).toBeNull();
  });

  it("hands the invoice to the wallet after enabling it", async () => {
    const calls: string[] = [];
    stubWindow({
      enable: async () => void calls.push("enable"),
      sendPayment: async (inv: string) => void calls.push(`pay:${inv}`),
    });
    await payWithWebLN("lnbcrt1abc");
    expect(calls).toEqual(["enable", "pay:lnbcrt1abc"]);
  });

  it("does not wait on a payment a hold invoice keeps pending", async () => {
    vi.useFakeTimers();
    stubWindow({ enable: async () => {}, sendPayment: () => new Promise(() => {}) });
    const done = payWithWebLN("lnbcrt1abc");
    await vi.advanceTimersByTimeAsync(3100);
    await expect(done).resolves.toBeUndefined();
    vi.useRealTimers();
  });

  it("reports a refused connection and a failed payment", async () => {
    stubWindow({ enable: async () => Promise.reject(new Error("no")), sendPayment: async () => {} });
    await expect(payWithWebLN("x")).rejects.toThrow(/did not allow/);
    stubWindow({ enable: async () => {}, sendPayment: async () => Promise.reject(new Error("insufficient balance")) });
    await expect(payWithWebLN("x")).rejects.toThrow(/insufficient balance/);
  });

  it("fails clearly without a wallet", async () => {
    stubWindow();
    await expect(payWithWebLN("x")).rejects.toThrow(/No WebLN wallet/);
  });
});
