//! A fiat value shown next to an escrow's amount, for display only.
//!
//! The amount of an escrow is always in sats and nothing in the escrow logic ever reads this: it
//! exists so people can see roughly what a sum is worth in their own currency. The value is taken
//! once, when the escrow is created, and kept, so it never changes afterwards (sats move in price
//! over a hold; the quote says what the sum was worth at the moment the offer was made).
//!
//! Money is integer arithmetic throughout. A rate is held as *minor units per bitcoin* (cents,
//! kobo, ...), so no floating point enters a stored amount.

use thiserror::Error;

/// Supported currencies and how many decimal places their minor unit has.
pub const CURRENCIES: &[(&str, u32)] = &[
    ("USD", 2),
    ("EUR", 2),
    ("GBP", 2),
    ("CAD", 2),
    ("AUD", 2),
    ("CHF", 2),
    ("JPY", 0),
    ("NGN", 2),
    ("KES", 2),
    ("GHS", 2),
    ("ZAR", 2),
    ("INR", 2),
    ("BRL", 2),
];

const SATS_PER_BTC: i128 = 100_000_000;
/// A rate outside this range (in minor units per BTC) is a broken feed, not a price.
const MIN_RATE: i64 = 1;
const MAX_RATE: i64 = 1_000_000_000_000_000;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FiatError {
    #[error("unsupported currency '{0}'")]
    UnsupportedCurrency(String),
    #[error("the exchange rate is not usable")]
    BadRate,
}

/// Decimal places of a supported currency's minor unit.
pub fn exponent(currency: &str) -> Option<u32> {
    CURRENCIES.iter().find(|(c, _)| *c == currency).map(|(_, e)| *e)
}

/// `Some(upper-cased code)` if it is a supported currency, whatever the input's case.
pub fn normalize(currency: &str) -> Option<String> {
    let upper = currency.trim().to_ascii_uppercase();
    exponent(&upper).map(|_| upper)
}

/// What an escrow's sum was worth when the offer was made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiatQuote {
    /// ISO 4217 code, upper case.
    pub currency: String,
    /// The sum in the currency's minor unit (cents, kobo, ...), rounded half up.
    pub amount_minor: i64,
    /// The rate used: minor units per whole bitcoin.
    pub rate_minor_per_btc: i64,
    /// When the rate was taken (unix seconds).
    pub quoted_at: i64,
    /// Where the rate came from, for example "CoinGecko" or "demo rate".
    pub source: String,
}

impl FiatQuote {
    /// Value `amount_sats` at `rate_minor_per_btc` and lock the result.
    pub fn new(
        amount_sats: u64,
        currency: &str,
        rate_minor_per_btc: i64,
        quoted_at: i64,
        source: &str,
    ) -> Result<Self, FiatError> {
        let currency = normalize(currency).ok_or_else(|| FiatError::UnsupportedCurrency(currency.to_string()))?;
        if !(MIN_RATE..=MAX_RATE).contains(&rate_minor_per_btc) {
            return Err(FiatError::BadRate);
        }
        // Round half up in 128-bit integers: sats * rate / 100,000,000.
        let scaled = amount_sats as i128 * rate_minor_per_btc as i128;
        let amount = (scaled + SATS_PER_BTC / 2) / SATS_PER_BTC;
        let amount_minor = i64::try_from(amount).map_err(|_| FiatError::BadRate)?;
        Ok(Self { currency, amount_minor, rate_minor_per_btc, quoted_at, source: source.to_string() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(sats: u64, cur: &str, rate: i64) -> Result<FiatQuote, FiatError> {
        FiatQuote::new(sats, cur, rate, 1_000, "test")
    }

    #[test]
    fn values_a_sum_in_minor_units() {
        // 65,000.00 USD per BTC: one BTC is 6,500,000 cents; 5,000 sats is 3.25 USD.
        assert_eq!(q(5_000, "USD", 6_500_000).unwrap().amount_minor, 325);
        assert_eq!(q(100_000_000, "USD", 6_500_000).unwrap().amount_minor, 6_500_000);
        assert_eq!(q(1, "USD", 6_500_000).unwrap().amount_minor, 0, "a sat is worth less than a cent");
    }

    #[test]
    fn rounds_half_up() {
        // 1,000,000 cents per BTC: 50 sats is exactly 0.5 cent, which rounds up to 1.
        assert_eq!(q(50, "USD", 1_000_000).unwrap().amount_minor, 1);
        assert_eq!(q(49, "USD", 1_000_000).unwrap().amount_minor, 0);
        assert_eq!(q(150, "USD", 1_000_000).unwrap().amount_minor, 2);
    }

    #[test]
    fn currencies_without_decimals_work() {
        assert_eq!(exponent("JPY"), Some(0));
        // 10,000,000 JPY per BTC: 5,000 sats is 500 yen.
        assert_eq!(q(5_000, "JPY", 10_000_000).unwrap().amount_minor, 500);
    }

    #[test]
    fn handles_the_largest_amounts_without_overflow() {
        // Escrow maximum is 1,000,000 sats; even absurd inputs stay exact in 128-bit.
        assert!(q(u64::MAX, "USD", MAX_RATE).is_err(), "too big for i64 is refused, not wrapped");
        assert_eq!(q(1_000_000, "NGN", 9_750_000_000).unwrap().amount_minor, 97_500_000);
    }

    #[test]
    fn refuses_unsupported_currencies_and_broken_rates() {
        assert_eq!(q(1, "XYZ", 100), Err(FiatError::UnsupportedCurrency("XYZ".into())));
        assert_eq!(q(1, "", 100), Err(FiatError::UnsupportedCurrency("".into())));
        for bad in [0, -5, MAX_RATE + 1, i64::MIN, i64::MAX] {
            assert_eq!(q(1, "USD", bad), Err(FiatError::BadRate), "{bad}");
        }
    }

    #[test]
    fn accepts_the_currency_in_any_case() {
        assert_eq!(normalize(" usd "), Some("USD".into()));
        assert_eq!(normalize("ngn"), Some("NGN".into()));
        assert_eq!(normalize("US"), None);
        assert_eq!(q(5_000, "usd", 6_500_000).unwrap().currency, "USD");
    }

    #[test]
    fn every_listed_currency_has_a_sensible_exponent() {
        for (code, exp) in CURRENCIES {
            assert_eq!(code.len(), 3);
            assert!(*exp <= 2, "{code}");
        }
    }
}
