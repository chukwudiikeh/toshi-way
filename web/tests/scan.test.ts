import { describe, expect, it } from "vitest";
import { secretFromScan } from "@/lib/scan";
import { buildReleaseLink } from "@/lib/secret";

const secret = "ab".repeat(32);
const id = "f7e255b8-1111-2222-3333-444455556666";

describe("secretFromScan", () => {
  it("reads the buyer's release link", () => {
    expect(secretFromScan(buildReleaseLink("https://app.example", id, secret), id)).toEqual({ secret });
  });

  it("accepts a bare 64-hex secret, in any case and with whitespace", () => {
    expect(secretFromScan(`  ${secret.toUpperCase()}\n`, id)).toEqual({ secret });
  });

  it("refuses a link for a different escrow", () => {
    const other = buildReleaseLink("https://app.example", "someone-elses-escrow", secret);
    expect(secretFromScan(other, id)).toEqual({ error: "That release code is for a different escrow." });
  });

  it.each([
    ["a short code", "123456"],
    ["a 63-character secret", "a".repeat(63)],
    ["a 65-character secret", "a".repeat(65)],
    ["non-hex text", "z".repeat(64)],
    ["a link without a secret", `https://app.example/escrows/${id}`],
    ["a link with a short secret", `https://app.example/escrows/${id}#release=1234`],
    ["a Lightning invoice", "lightning:LNBCRT10N1EXAMPLE"],
    ["empty text", ""],
  ])("refuses %s", (_, text) => {
    expect(secretFromScan(text, id)).toEqual({ error: "That QR code is not a release code." });
  });
});
