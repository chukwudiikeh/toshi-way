import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api, ApiError, describeError } from "@/lib/api";
import { getSession, setSession } from "@/lib/session";

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

const json = (status: number, body: unknown, headers: Record<string, string> = {}) =>
  new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json", ...headers } });

beforeEach(() => {
  vi.stubGlobal("localStorage", memoryStorage());
  vi.stubGlobal("window", { addEventListener() {}, removeEventListener() {} });
});
afterEach(() => vi.unstubAllGlobals());

describe("api client", () => {
  it("sends the bearer token and parses the error format", async () => {
    setSession({ token: "tw_abc", kind: "user", userId: null });
    const fetchMock = vi.fn().mockResolvedValue(json(422, { error: { code: "wrong_preimage", message: "nope" } }));
    vi.stubGlobal("fetch", fetchMock);

    const err = await api.release("e1", "a".repeat(64)).catch((e) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err).toMatchObject({ code: "wrong_preimage", status: 422 });
    expect(describeError(err)).toMatch(/doesn't match/);

    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toMatch(/\/api\/v1\/escrows\/e1\/release$/);
    expect(init.headers.Authorization).toBe("Bearer tw_abc");
    expect(JSON.parse(init.body)).toEqual({ preimage: "a".repeat(64) });
  });

  it("signs out on 401", async () => {
    setSession({ token: "tw_old", kind: "user", userId: null });
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(json(401, { error: { code: "unauthorized", message: "x" } })));
    await expect(api.listMine()).rejects.toMatchObject({ code: "unauthorized" });
    expect(getSession()).toBeNull();
  });

  it("keeps the session when only checking a typed token", async () => {
    setSession({ token: "tw_mine", kind: "user", userId: null });
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(json(401, { error: { code: "unauthorized", message: "x" } })));
    await expect(api.listMine({ token: "tw_typo", keepSessionOn401: true })).rejects.toBeInstanceOf(ApiError);
    expect(getSession()?.token).toBe("tw_mine");
  });

  it("reports Retry-After on 429", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(json(429, { error: { code: "rate_limited", message: "slow" } }, { "Retry-After": "7" })),
    );
    await expect(api.get("e1")).rejects.toMatchObject({ code: "rate_limited", retryAfter: 7 });
  });

  it("turns a network failure into a friendly error", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("Failed to fetch")));
    await expect(api.get("e1")).rejects.toMatchObject({ code: "network", status: 0 });
  });

  it("rejects responses that don't match the schema", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(json(200, { id: "e1", state: "WHATEVER" })));
    await expect(api.get("e1")).rejects.toMatchObject({ code: "bad_response" });
  });

  it("sends the Idempotency-Key on create", async () => {
    const fetchMock = vi.fn().mockResolvedValue(json(500, {}));
    vi.stubGlobal("fetch", fetchMock);
    await api.create({ buyer_id: "u_1", amount_sats: 1, description: "d", hold_secs: 60 }, "key-1").catch(() => {});
    expect(fetchMock.mock.calls[0][1].headers["Idempotency-Key"]).toBe("key-1");
  });
});
