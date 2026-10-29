import { describe, expect, it } from "vitest";
import {
  buildReleaseLink,
  fromHex,
  generateSecret,
  hashSecret,
  parseReleaseFragment,
  parseSecret,
} from "@/lib/secret";

const ZEROS = "00".repeat(32);

describe("generateSecret", () => {
  it("is 32 random bytes as 64 lowercase hex characters", () => {
    const p = generateSecret();
    expect(p).toMatch(/^[0-9a-f]{64}$/);
    expect(fromHex(p)).toHaveLength(32);
  });

  it("does not repeat", () => {
    const seen = new Set(Array.from({ length: 100 }, generateSecret));
    expect(seen.size).toBe(100);
  });
});

describe("hashSecret", () => {
  it("hashes the 32 bytes, not the hex text (known vector)", async () => {
    expect(await hashSecret(ZEROS)).toBe("66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925");
  });

  it("refuses anything that isn't a 64-hex secret", async () => {
    await expect(hashSecret("123456")).rejects.toThrow();
  });
});

describe("parseSecret", () => {
  it("accepts exactly 64 hex characters, normalising case and whitespace", () => {
    expect(parseSecret(ZEROS)).toBe(ZEROS);
    expect(parseSecret(`  ${"AB".repeat(32)}\n`)).toBe("ab".repeat(32));
  });

  it.each([
    ["empty", ""],
    ["a 6-digit code", "123456"],
    ["63 characters", "a".repeat(63)],
    ["65 characters", "a".repeat(65)],
    ["non-hex", "g".repeat(64)],
    ["0x-prefixed", `0x${"a".repeat(62)}`],
    ["inner space", `${"a".repeat(32)} ${"a".repeat(31)}`],
  ])("rejects %s", (_, input) => {
    expect(parseSecret(input)).toBeNull();
  });
});

describe("release links", () => {
  const p = "0123456789abcdef".repeat(4);

  it("puts the secret in the fragment, never the path or query", () => {
    const link = buildReleaseLink("https://app.example/", "f7e255b8-1", p);
    expect(link).toBe(`https://app.example/escrows/f7e255b8-1#release=${p}`);
    const url = new URL(link);
    expect(url.pathname + url.search).not.toContain(p);
  });

  it("round-trips through the fragment parser", () => {
    const url = new URL(buildReleaseLink("http://localhost:3001", "abc", p));
    expect(parseReleaseFragment(url.hash)).toBe(p);
  });

  it("rejects malformed or missing fragments", () => {
    expect(parseReleaseFragment("")).toBeNull();
    expect(parseReleaseFragment("#release=123456")).toBeNull();
    expect(parseReleaseFragment(`#release=${p}0`)).toBeNull();
    expect(parseReleaseFragment(`#other=${p}`)).toBeNull();
  });

  it("refuses to build a link around a bad secret", () => {
    expect(() => buildReleaseLink("http://x", "id", "123456")).toThrow();
  });
});
