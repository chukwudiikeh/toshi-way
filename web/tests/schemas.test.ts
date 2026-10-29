import { describe, expect, it } from "vitest";
import { Escrow, EscrowEvents, NewEscrowForm, SecretInput } from "@/lib/schemas";

// The sample from docs/brief.md.
const sample = {
  id: "f7e255b8-0000-4000-8000-000000000000",
  buyer_id: "u_5f26",
  seller_id: "u_cf4e",
  amount_sats: 1000,
  description: "demo parcel",
  payment_hash: "68af07" + "0".repeat(58),
  state: "LOCKED",
  invoice_expires_at: 1792666545,
  deadline_at: 1792667399,
  created_at: 1792665599,
  updated_at: 1792665600,
  your_role: "buyer",
};

describe("Escrow", () => {
  it("accepts the documented sample", () => {
    expect(Escrow.parse(sample)).toMatchObject({ state: "LOCKED", your_role: "buyer" });
  });

  it("accepts a fresh offer with nulls before acceptance", () => {
    expect(Escrow.safeParse({ ...sample, state: "CREATED", payment_hash: null, invoice_expires_at: null }).success).toBe(
      true,
    );
  });

  it.each([
    ["an unknown state", { state: "PAID" }],
    ["an unknown role", { your_role: "admin" }],
    ["a short payment hash", { payment_hash: "68af07" }],
    ["a string amount", { amount_sats: "1000" }],
    ["a missing deadline", { deadline_at: undefined }],
    ["fractional timestamps", { created_at: 1.5 }],
  ])("rejects %s", (_, patch) => {
    expect(Escrow.safeParse({ ...sample, ...patch }).success).toBe(false);
  });
});

describe("EscrowEvents", () => {
  it("accepts the events payload", () => {
    const v = {
      audit: [{ actor: "node", action: "node_state", from_state: "INVOICE_ISSUED", to_state: "LOCKED", reason: "x", at: 1 }],
      node_events: [{ node_invoice_state: "ACCEPTED", at: 1 }],
    };
    expect(EscrowEvents.safeParse(v).success).toBe(true);
    expect(EscrowEvents.safeParse({ audit: [] }).success).toBe(false);
  });
});

describe("NewEscrowForm", () => {
  const form = { buyer_id: " u_1 ", amount_sats: "1000", description: "parcel", hold_amount: "2", hold_unit: "hours" };

  it("converts the hold time to seconds and trims fields", () => {
    expect(NewEscrowForm.parse(form)).toEqual({
      buyer_id: "u_1",
      amount_sats: 1000,
      description: "parcel",
      hold_secs: 7200,
    });
  });

  it.each([
    ["a zero amount", { amount_sats: "0" }],
    ["too many sats", { amount_sats: "1000001" }],
    ["fractional sats", { amount_sats: "1.5" }],
    ["no buyer", { buyer_id: "  " }],
    ["a hold under a minute", { hold_amount: "0.5", hold_unit: "minutes" }],
    ["a hold over six hours", { hold_amount: "7", hold_unit: "hours" }],
    ["a long description", { description: "x".repeat(501) }],
  ])("rejects %s", (_, patch) => {
    expect(NewEscrowForm.safeParse({ ...form, ...patch }).success).toBe(false);
  });

  it("accepts the bounds", () => {
    expect(NewEscrowForm.parse({ ...form, hold_amount: "1", hold_unit: "minutes" }).hold_secs).toBe(60);
    expect(NewEscrowForm.parse({ ...form, hold_amount: "6", hold_unit: "hours" }).hold_secs).toBe(21600);
  });
});

describe("SecretInput", () => {
  it("only accepts 64 hex characters", () => {
    expect(SecretInput.parse(` ${"A".repeat(64)} `)).toBe("a".repeat(64));
    expect(SecretInput.safeParse("123456").success).toBe(false);
    expect(SecretInput.safeParse("a".repeat(63)).success).toBe(false);
  });
});
