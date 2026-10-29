import { describe, expect, it } from "vitest";
import { formatFiat, formatRate, valueOfSats } from "@/lib/fiat";
import { Escrow, NewEscrowForm, RateQuote, RatesInfo } from "@/lib/schemas";

describe("valueOfSats (mirrors the server's integer maths)", () => {
  it("values a sum in minor units", () => {
    expect(valueOfSats(5_000, 6_500_000)).toBe(325); // 5,000 sats at 65,000.00 USD per bitcoin
    expect(valueOfSats(100_000_000, 6_500_000)).toBe(6_500_000);
    expect(valueOfSats(1, 6_500_000)).toBe(0);
  });

  it("rounds half up, like the server", () => {
    expect(valueOfSats(50, 1_000_000)).toBe(1);
    expect(valueOfSats(49, 1_000_000)).toBe(0);
    expect(valueOfSats(150, 1_000_000)).toBe(2);
  });

  it("matches the server for naira and yen", () => {
    expect(valueOfSats(1_000, 9_750_000_000)).toBe(97_500); // 975.00 NGN
    expect(valueOfSats(1_000, 9_800_000)).toBe(98); // 98 JPY
  });

  it("does not lose precision on big rates", () => {
    expect(valueOfSats(1_000_000, 9_750_000_000)).toBe(97_500_000);
  });
});

describe("formatting", () => {
  it("formats amounts with the currency's own decimals", () => {
    expect(formatFiat(325, "USD", 2)).toBe("$3.25");
    expect(formatFiat(97_500, "NGN", 2)).toBe("₦975.00");
    expect(formatFiat(98, "JPY", 0)).toBe("¥98");
    expect(formatFiat(0, "USD", 2)).toBe("$0.00");
  });

  it("formats the price of a bitcoin", () => {
    expect(formatRate(6_500_000, "USD", 2)).toBe("$65,000.00");
  });

  it("does not throw on a currency it does not know", () => {
    expect(formatFiat(250, "XXZ", 2)).toMatch(/2\.50/);
  });
});

describe("fiat in the schemas", () => {
  const sample = {
    id: "e1", buyer_id: "u_b", seller_id: "u_s", amount_sats: 5000, description: "d", payment_hash: null,
    state: "CREATED", invoice_expires_at: null, deadline_at: 2000, created_at: 1000, updated_at: 1000, your_role: "buyer",
  };

  it("defaults to no fiat value", () => {
    expect(Escrow.parse(sample).fiat).toBeNull();
  });

  it("reads a locked quote", () => {
    const fiat = { currency: "USD", exponent: 2, amount_minor: 325, rate_minor_per_btc: 6_500_000, quoted_at: 1000, source: "demo rate" };
    expect(Escrow.parse({ ...sample, fiat }).fiat).toEqual(fiat);
  });

  it("refuses a nonsense quote", () => {
    for (const bad of [
      { currency: "US", exponent: 2, amount_minor: 1, rate_minor_per_btc: 1, quoted_at: 1, source: "x" },
      { currency: "USD", exponent: 9, amount_minor: 1, rate_minor_per_btc: 1, quoted_at: 1, source: "x" },
      { currency: "USD", exponent: 2, amount_minor: -1, rate_minor_per_btc: 1, quoted_at: 1, source: "x" },
      { currency: "USD", exponent: 2, amount_minor: 1, rate_minor_per_btc: 0, quoted_at: 1, source: "x" },
    ]) {
      expect(Escrow.safeParse({ ...sample, fiat: bad }).success).toBe(false);
    }
  });

  it("reads the rates the API sends", () => {
    expect(RatesInfo.parse({ enabled: true, source: "demo rate", currencies: [{ code: "USD", exponent: 2 }] }).enabled).toBe(true);
    expect(RateQuote.parse({ currency: "USD", exponent: 2, rate_minor_per_btc: 6_500_000, quoted_at: 5, source: "demo rate" }).currency).toBe("USD");
  });

  it("sends the currency from the form, and leaves it out when blank", () => {
    const form = { buyer_id: "", amount_sats: "5000", description: "parcel", hold_amount: "2", hold_unit: "hours" };
    expect(NewEscrowForm.parse({ ...form, fiat_currency: " usd " })).toMatchObject({ fiat_currency: "usd" });
    expect("fiat_currency" in NewEscrowForm.parse({ ...form, fiat_currency: "" })).toBe(false);
    expect("fiat_currency" in NewEscrowForm.parse(form)).toBe(false);
  });
});
