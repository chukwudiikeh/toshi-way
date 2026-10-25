//! Where fiat exchange rates come from.
//!
//! The server fetches rates, never the browser, so the page makes no calls to third parties. A
//! rate is only ever used to work out a display-only value once, when an escrow is created.
//!
//! Providers are chosen by `FIAT_PROVIDER`: `off` (default), `coingecko`, `mempool`, or
//! `fixed:USD=65000,NGN=97500000` for demos and regtest, where there is no market. Fetched rates are
//! cached briefly, and a stale one is used for a few minutes if the feed is down, so a flaky feed
//! neither hammers the provider nor blocks creating escrows.

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use escrow_core::fiat::{self, CURRENCIES};
use tokio::sync::Mutex;

/// A reading of the price of one bitcoin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rate {
    /// Minor units (cents, kobo, ...) per whole bitcoin.
    pub rate_minor_per_btc: i64,
    /// When the rate was taken (unix seconds).
    pub as_of: i64,
    /// Human-readable origin, shown next to the value.
    pub source: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RateError {
    #[error("fiat values are not enabled on this server")]
    Disabled,
    #[error("'{0}' is not offered")]
    Unsupported(String),
    #[error("the price feed is unavailable")]
    Unavailable,
}

#[async_trait]
pub trait RateProvider: Send + Sync {
    /// Shown to users beside a value, for example "CoinGecko" or "demo rate".
    fn name(&self) -> String;
    /// Currencies this provider can price (upper-case codes).
    fn currencies(&self) -> Vec<String>;
    /// The price of one bitcoin in `currency` (an upper-case supported code).
    async fn rate(&self, currency: &str, now: i64) -> Result<Rate, RateError>;
}

/// Fiat values switched off.
pub struct Disabled;

#[async_trait]
impl RateProvider for Disabled {
    fn name(&self) -> String {
        "off".into()
    }
    fn currencies(&self) -> Vec<String> {
        Vec::new()
    }
    async fn rate(&self, _currency: &str, _now: i64) -> Result<Rate, RateError> {
        Err(RateError::Disabled)
    }
}

/// Fixed rates from configuration, for demos and tests. Labelled "demo rate" so nobody mistakes
/// them for a market price.
pub struct FixedRates {
    rates: HashMap<String, i64>,
}

impl FixedRates {
    /// Parse `USD=65000,NGN=97500000.5`: whole-currency prices of one bitcoin.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let mut rates = HashMap::new();
        for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (code, price) = part.split_once('=').ok_or_else(|| format!("'{part}' should look like USD=65000"))?;
            let code = fiat::normalize(code).ok_or_else(|| format!("'{code}' is not a supported currency"))?;
            let minor = major_to_minor(price.trim().parse::<f64>().map_err(|_| format!("'{price}' is not a number"))?, &code)
                .ok_or_else(|| format!("'{price}' is not a usable price"))?;
            rates.insert(code, minor);
        }
        if rates.is_empty() {
            return Err("no rates given".into());
        }
        Ok(Self { rates })
    }
}

#[async_trait]
impl RateProvider for FixedRates {
    fn name(&self) -> String {
        "demo rate".into()
    }
    fn currencies(&self) -> Vec<String> {
        let mut v: Vec<_> = self.rates.keys().cloned().collect();
        v.sort();
        v
    }
    async fn rate(&self, currency: &str, now: i64) -> Result<Rate, RateError> {
        let r = self.rates.get(currency).ok_or_else(|| RateError::Unsupported(currency.to_string()))?;
        Ok(Rate { rate_minor_per_btc: *r, as_of: now, source: self.name() })
    }
}

/// A price in whole currency units as a float, to minor units, or `None` if it is not a sane price.
pub fn major_to_minor(major: f64, currency: &str) -> Option<i64> {
    let exp = fiat::exponent(currency)?;
    if !major.is_finite() || major <= 0.0 {
        return None;
    }
    let minor = (major * 10f64.powi(exp as i32)).round();
    // Anything beyond what an i64 holds exactly in a float is a broken feed, not a price.
    (minor >= 1.0 && minor < 9.0e15).then_some(minor as i64)
}

/// Fetching a URL, so the HTTP client can be faked in tests.
#[async_trait]
pub trait Fetch: Send + Sync {
    async fn get(&self, url: &str) -> Result<String, String>;
}

pub struct ReqwestFetch(reqwest::Client);

impl ReqwestFetch {
    pub fn new() -> Self {
        Self(
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .user_agent("toshi-way")
                .build()
                .expect("an HTTP client"),
        )
    }
}

impl Default for ReqwestFetch {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Fetch for ReqwestFetch {
    async fn get(&self, url: &str) -> Result<String, String> {
        let res = self.0.get(url).send().await.map_err(|e| e.to_string())?;
        if !res.status().is_success() {
            return Err(format!("HTTP {}", res.status()));
        }
        res.text().await.map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feed {
    CoinGecko,
    Mempool,
}

impl Feed {
    fn name(self) -> &'static str {
        match self {
            Feed::CoinGecko => "CoinGecko",
            Feed::Mempool => "mempool.space",
        }
    }

    fn currencies(self) -> Vec<&'static str> {
        match self {
            Feed::CoinGecko => CURRENCIES.iter().map(|(c, _)| *c).collect(),
            // mempool.space prices only these.
            Feed::Mempool => vec!["USD", "EUR", "GBP", "CAD", "CHF", "AUD", "JPY"],
        }
    }

    fn url(self) -> String {
        match self {
            Feed::CoinGecko => {
                let list: Vec<String> = self.currencies().iter().map(|c| c.to_ascii_lowercase()).collect();
                format!("https://api.coingecko.com/api/v3/simple/price?ids=bitcoin&vs_currencies={}", list.join(","))
            }
            Feed::Mempool => "https://mempool.space/api/v1/prices".into(),
        }
    }

    /// Prices for every currency the feed gave us that we support, in minor units per bitcoin.
    pub fn parse(self, body: &str) -> Result<HashMap<String, i64>, String> {
        let json: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("not JSON: {e}"))?;
        let table = match self {
            Feed::CoinGecko => json.get("bitcoin").ok_or("no \"bitcoin\" entry")?,
            Feed::Mempool => &json,
        };
        let table = table.as_object().ok_or("unexpected shape")?;
        let mut out = HashMap::new();
        for code in self.currencies() {
            let key = match self {
                Feed::CoinGecko => code.to_ascii_lowercase(),
                Feed::Mempool => code.to_string(),
            };
            if let Some(minor) = table.get(&key).and_then(|v| v.as_f64()).and_then(|p| major_to_minor(p, code)) {
                out.insert(code.to_string(), minor);
            }
        }
        if out.is_empty() {
            return Err("no usable prices".into());
        }
        Ok(out)
    }
}

/// A live feed with a short cache and a limited tolerance for outages.
pub struct LiveRates {
    feed: Feed,
    fetch: Arc<dyn Fetch>,
    /// Fresh enough to use without asking again.
    ttl_secs: i64,
    /// How old a rate may be and still be used when the feed is down.
    max_stale_secs: i64,
    cache: Mutex<Option<(i64, HashMap<String, i64>)>>,
}

impl LiveRates {
    pub fn new(feed: Feed, fetch: Arc<dyn Fetch>) -> Self {
        Self { feed, fetch, ttl_secs: 60, max_stale_secs: 600, cache: Mutex::new(None) }
    }
}

#[async_trait]
impl RateProvider for LiveRates {
    fn name(&self) -> String {
        self.feed.name().into()
    }
    fn currencies(&self) -> Vec<String> {
        self.feed.currencies().iter().map(|c| c.to_string()).collect()
    }
    async fn rate(&self, currency: &str, now: i64) -> Result<Rate, RateError> {
        if !self.feed.currencies().contains(&currency) {
            return Err(RateError::Unsupported(currency.to_string()));
        }
        // Held across the fetch so a burst of requests causes one call, not many.
        let mut cache = self.cache.lock().await;
        let fresh = cache.as_ref().is_some_and(|(at, _)| now - at < self.ttl_secs);
        if !fresh {
            let fetched = match self.fetch.get(&self.feed.url()).await {
                Ok(body) => self.feed.parse(&body),
                Err(e) => Err(e),
            };
            match fetched {
                Ok(map) => *cache = Some((now, map)),
                Err(e) => {
                    tracing::warn!(feed = self.feed.name(), error = %e, "could not refresh the exchange rate");
                    // Keep serving an older rate for a while rather than failing outright.
                    if !cache.as_ref().is_some_and(|(at, _)| now - at < self.max_stale_secs) {
                        return Err(RateError::Unavailable);
                    }
                }
            }
        }
        let (at, map) = cache.as_ref().ok_or(RateError::Unavailable)?;
        let minor = map.get(currency).ok_or(RateError::Unavailable)?;
        Ok(Rate { rate_minor_per_btc: *minor, as_of: *at, source: self.name() })
    }
}

/// Build a provider from the `FIAT_PROVIDER` setting.
pub fn from_setting(setting: &str) -> Result<Arc<dyn RateProvider>, String> {
    let setting = setting.trim();
    match setting {
        "" | "off" => Ok(Arc::new(Disabled)),
        "coingecko" => Ok(Arc::new(LiveRates::new(Feed::CoinGecko, Arc::new(ReqwestFetch::new())))),
        "mempool" => Ok(Arc::new(LiveRates::new(Feed::Mempool, Arc::new(ReqwestFetch::new())))),
        other => match other.strip_prefix("fixed:") {
            Some(spec) => Ok(Arc::new(FixedRates::parse(spec)?)),
            None => Err(format!("unknown FIAT_PROVIDER '{other}' (use off, coingecko, mempool or fixed:USD=65000,...)")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const COINGECKO: &str = r#"{"bitcoin":{"usd":64231.5,"ngn":97500123,"jpy":9876543,"eur":59000,"zzz":1}}"#;
    const MEMPOOL: &str = r#"{"time":1700000000,"USD":64000,"EUR":59000,"GBP":50000,"CAD":88000,"CHF":57000,"AUD":97000,"JPY":9800000}"#;

    #[test]
    fn parses_what_coingecko_sends() {
        let m = Feed::CoinGecko.parse(COINGECKO).unwrap();
        assert_eq!(m["USD"], 6_423_150, "64,231.50 dollars is 6,423,150 cents");
        assert_eq!(m["NGN"], 9_750_012_300);
        assert_eq!(m["JPY"], 9_876_543, "yen have no minor unit");
        assert!(!m.contains_key("GBP"), "currencies the feed omitted are left out");
        assert!(!m.contains_key("ZZZ"));
    }

    #[test]
    fn parses_what_mempool_sends() {
        let m = Feed::Mempool.parse(MEMPOOL).unwrap();
        assert_eq!((m["USD"], m["EUR"], m["JPY"]), (6_400_000, 5_900_000, 9_800_000));
        assert!(!m.contains_key("NGN"), "mempool.space does not price naira");
    }

    #[test]
    fn refuses_garbage_and_broken_prices() {
        for (feed, body) in [
            (Feed::CoinGecko, "not json"),
            (Feed::CoinGecko, "{}"),
            (Feed::CoinGecko, r#"{"bitcoin":"x"}"#),
            (Feed::CoinGecko, r#"{"bitcoin":{"usd":0,"eur":-3,"gbp":"cheap","ngn":null}}"#),
            (Feed::Mempool, "[]"),
            (Feed::Mempool, r#"{"USD":1e300,"EUR":"NaN"}"#),
        ] {
            assert!(feed.parse(body).is_err(), "{body}");
        }
    }

    #[test]
    fn prices_are_converted_to_minor_units_safely() {
        assert_eq!(major_to_minor(65000.0, "USD"), Some(6_500_000));
        assert_eq!(major_to_minor(65000.456, "USD"), Some(6_500_046), "rounded to the nearest cent");
        assert_eq!(major_to_minor(1.0, "JPY"), Some(1));
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e300] {
            assert_eq!(major_to_minor(bad, "USD"), None, "{bad}");
        }
        assert_eq!(major_to_minor(5.0, "XYZ"), None);
    }

    #[tokio::test]
    async fn fixed_rates_parse_serve_and_say_what_they_are() {
        let f = FixedRates::parse("usd=65000, NGN=97500000").unwrap();
        assert_eq!(f.currencies(), ["NGN", "USD"]);
        let r = f.rate("USD", 123).await.unwrap();
        assert_eq!((r.rate_minor_per_btc, r.as_of, r.source.as_str()), (6_500_000, 123, "demo rate"));
        assert_eq!(f.rate("EUR", 1).await, Err(RateError::Unsupported("EUR".into())));
        for bad in ["", "USD", "USD=abc", "XYZ=5", "USD=0", "USD=-4"] {
            assert!(FixedRates::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[tokio::test]
    async fn disabled_serves_nothing() {
        assert_eq!(Disabled.rate("USD", 1).await, Err(RateError::Disabled));
        assert!(Disabled.currencies().is_empty());
    }

    struct Fake {
        calls: AtomicUsize,
        body: std::sync::Mutex<Result<String, String>>,
    }
    impl Fake {
        fn new(body: Result<&str, &str>) -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                body: std::sync::Mutex::new(body.map(str::to_string).map_err(str::to_string)),
            })
        }
        fn set(&self, body: Result<&str, &str>) {
            *self.body.lock().unwrap() = body.map(str::to_string).map_err(str::to_string);
        }
    }
    #[async_trait]
    impl Fetch for Fake {
        async fn get(&self, _url: &str) -> Result<String, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.body.lock().unwrap().clone()
        }
    }

    #[tokio::test]
    async fn caches_for_a_minute_so_bursts_make_one_call() {
        let fake = Fake::new(Ok(COINGECKO));
        let live = LiveRates::new(Feed::CoinGecko, fake.clone());
        for t in [1000, 1001, 1030, 1059] {
            assert_eq!(live.rate("USD", t).await.unwrap().rate_minor_per_btc, 6_423_150);
        }
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
        live.rate("USD", 1060).await.unwrap();
        assert_eq!(fake.calls.load(Ordering::SeqCst), 2, "refreshed once the minute is up");
        // The quote carries when the rate was actually taken, not when it was asked for.
        assert_eq!(live.rate("USD", 1100).await.unwrap().as_of, 1060);
    }

    #[tokio::test]
    async fn serves_a_stale_rate_for_ten_minutes_when_the_feed_is_down_then_gives_up() {
        let fake = Fake::new(Ok(COINGECKO));
        let live = LiveRates::new(Feed::CoinGecko, fake.clone());
        live.rate("USD", 1000).await.unwrap();
        fake.set(Err("connection refused"));
        assert_eq!(live.rate("USD", 1100).await.unwrap().as_of, 1000, "stale but recent enough");
        assert_eq!(live.rate("USD", 1599).await.unwrap().as_of, 1000);
        assert_eq!(live.rate("USD", 1600).await, Err(RateError::Unavailable), "too old to trust");
        fake.set(Ok(COINGECKO));
        assert_eq!(live.rate("USD", 1601).await.unwrap().as_of, 1601, "recovers when the feed does");
    }

    #[tokio::test]
    async fn a_feed_that_was_never_up_is_unavailable_and_junk_does_not_poison_the_cache() {
        let fake = Fake::new(Err("down"));
        let live = LiveRates::new(Feed::CoinGecko, fake.clone());
        assert_eq!(live.rate("USD", 1000).await, Err(RateError::Unavailable));
        fake.set(Ok("{\"bitcoin\":{\"usd\":0}}"));
        assert_eq!(live.rate("USD", 1001).await, Err(RateError::Unavailable), "unusable prices are not cached");
        fake.set(Ok(COINGECKO));
        assert!(live.rate("USD", 1002).await.is_ok());
    }

    #[tokio::test]
    async fn only_currencies_the_feed_prices_are_offered() {
        let live = LiveRates::new(Feed::Mempool, Fake::new(Ok(MEMPOOL)));
        assert_eq!(live.rate("NGN", 1).await, Err(RateError::Unsupported("NGN".into())));
        assert!(live.rate("USD", 1).await.is_ok());
        assert!(!live.currencies().contains(&"NGN".to_string()));
    }

    /// The real feeds, over the real network. Ignored by default (it needs internet and the feeds
    /// to be up): `cargo test -p escrow-api --lib -- --ignored live_feeds`.
    #[tokio::test]
    #[ignore = "needs internet access"]
    async fn live_feeds_answer_with_sane_prices() {
        for feed in [Feed::CoinGecko, Feed::Mempool] {
            let live = LiveRates::new(feed, Arc::new(ReqwestFetch::new()));
            for code in live.currencies() {
                let r = live.rate(&code, 1_000).await.unwrap_or_else(|e| panic!("{} {code}: {e}", feed.name()));
                // One bitcoin is worth far more than 1,000 and less than a trillion of any of these.
                let major = r.rate_minor_per_btc as f64 / 10f64.powi(fiat::exponent(&code).unwrap() as i32);
                assert!((1_000.0..1.0e12).contains(&major), "{} {code}: {major}", feed.name());
            }
            println!("{}: {} currencies ok", feed.name(), live.currencies().len());
        }
    }

    #[test]
    fn the_setting_picks_a_provider() {
        assert_eq!(from_setting("off").unwrap().name(), "off");
        assert_eq!(from_setting("").unwrap().name(), "off");
        assert_eq!(from_setting("coingecko").unwrap().name(), "CoinGecko");
        assert_eq!(from_setting("mempool").unwrap().name(), "mempool.space");
        assert_eq!(from_setting("fixed:USD=65000").unwrap().name(), "demo rate");
        for bad in ["bitcoinaverage", "fixed:", "fixed:USD", "fixed:XYZ=3"] {
            assert!(from_setting(bad).is_err(), "{bad}");
        }
    }
}
