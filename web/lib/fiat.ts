// Display-only fiat values. The escrow is always in sats; this only helps people see roughly what
// a sum is worth. Money is integers throughout, mirroring the server: a rate is "minor units per
// bitcoin" (cents, kobo, ...) and amounts are in the currency's minor unit.

const SATS_PER_BTC = BigInt(100_000_000);
const TWO = BigInt(2);

/** What `sats` is worth at `rateMinorPerBtc`, in minor units, rounded half up (as the server does). */
export function valueOfSats(sats: number, rateMinorPerBtc: number): number {
  const scaled = BigInt(sats) * BigInt(rateMinorPerBtc);
  return Number((scaled + SATS_PER_BTC / TWO) / SATS_PER_BTC);
}

/** How many sats are worth `amountMinor` at `rateMinorPerBtc`, rounded half up: the inverse of `valueOfSats`. */
export function satsForValue(amountMinor: number, rateMinorPerBtc: number): number {
  const rate = BigInt(rateMinorPerBtc);
  return Number((BigInt(amountMinor) * SATS_PER_BTC + rate / TWO) / rate);
}

/** Format an amount in a currency's minor unit, for example (325, "USD", 2) -> "$3.25". */
export function formatFiat(amountMinor: number, currency: string, exponent: number): string {
  const major = amountMinor / 10 ** exponent;
  try {
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency,
      currencyDisplay: "narrowSymbol",
      minimumFractionDigits: exponent,
      maximumFractionDigits: exponent,
    }).format(major);
  } catch {
    // An unknown currency code: fall back to the plain number and the code.
    return `${major.toFixed(exponent)} ${currency}`;
  }
}

/** The price of one bitcoin, for example (6_500_000, "USD", 2) -> "$65,000.00". */
export function formatRate(rateMinorPerBtc: number, currency: string, exponent: number): string {
  return formatFiat(rateMinorPerBtc, currency, exponent);
}
