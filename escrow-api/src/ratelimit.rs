//! Fixed-window rate limiter keyed by caller.

use std::{collections::HashMap, sync::Mutex};

#[derive(Default)]
pub struct RateLimiter {
    windows: Mutex<HashMap<String, (i64, u32)>>,
}

impl RateLimiter {
    /// Count a request for `key` at time `now` (unix seconds). `Err(secs)` = over the limit,
    /// retry after that many seconds.
    pub fn check(&self, key: &str, limit: u32, now: i64) -> Result<(), u64> {
        let window = now.div_euclid(60);
        let mut g = self.windows.lock().unwrap();
        if g.len() > 50_000 {
            g.retain(|_, (w, _)| *w == window);
        }
        let entry = g.entry(key.to_string()).or_insert((window, 0));
        if entry.0 != window {
            *entry = (window, 0);
        }
        if entry.1 >= limit {
            return Err((60 - now.rem_euclid(60)) as u64);
        }
        entry.1 += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_over_the_limit_and_resets_next_window() {
        let rl = RateLimiter::default();
        for _ in 0..3 {
            assert!(rl.check("a", 3, 120).is_ok());
        }
        assert_eq!(rl.check("a", 3, 150), Err(30));
        assert!(rl.check("b", 3, 150).is_ok(), "other callers are unaffected");
        assert!(rl.check("a", 3, 180).is_ok(), "a new window starts fresh");
    }
}
