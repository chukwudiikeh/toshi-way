//! Hold-window arithmetic, based on what was measured on regtest (docs/flow.md, section 7):
//! LND cancels a held HTLC `HOLD_EXPIRY_DELTA` blocks before it expires, and the payer adds
//! `PAYER_CLTV_BUFFER` blocks on top of the invoice's final CLTV delta.

use thiserror::Error;

/// LND's `invoices.holdexpirydelta` default: held HTLCs are cancelled this many blocks early.
pub const HOLD_EXPIRY_DELTA: u32 = 12;
/// Extra blocks the payer adds to the invoice's final CLTV delta (observed: 24 -> 27).
pub const PAYER_CLTV_BUFFER: u32 = 3;
/// Lowest final CLTV delta we will ask for.
pub const MIN_FINAL_CLTV_DELTA: u32 = 18;

#[derive(Debug, Clone)]
pub struct TimingConfig {
    /// Assumed seconds per block when converting a wall-clock deadline to blocks.
    pub block_secs: i64,
    /// Extra blocks kept between our deadline and LND's own auto-cancel, to absorb block-time variance.
    pub safety_blocks: u32,
    /// Longest hold we accept. Must fit inside what the network can support.
    pub max_hold_secs: i64,
    /// Shortest hold we accept.
    pub min_hold_secs: i64,
    /// How long an unpaid invoice stays payable.
    pub invoice_expiry_secs: i64,
}

impl Default for TimingConfig {
    fn default() -> Self {
        Self {
            block_secs: 600,
            safety_blocks: 6,
            max_hold_secs: 6 * 3600,
            min_hold_secs: 60,
            invoice_expiry_secs: 600,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TimingError {
    #[error("deadline must be at least {min} seconds from now")]
    TooSoon { min: i64 },
    #[error("deadline exceeds the maximum hold of {max} seconds")]
    TooLong { max: i64 },
}

impl TimingConfig {
    pub fn validate_deadline(&self, now: i64, deadline_at: i64) -> Result<(), TimingError> {
        let hold = deadline_at - now;
        if hold < self.min_hold_secs {
            return Err(TimingError::TooSoon { min: self.min_hold_secs });
        }
        if hold > self.max_hold_secs {
            return Err(TimingError::TooLong { max: self.max_hold_secs });
        }
        Ok(())
    }

    /// Final CLTV delta for an invoice that must stay held until `deadline_at`, with our
    /// deadline landing `safety_blocks` before LND's auto-cancel.
    pub fn final_cltv_delta(&self, now: i64, deadline_at: i64) -> u32 {
        let hold_blocks = ((deadline_at - now).max(0) + self.block_secs - 1) / self.block_secs;
        let delta = hold_blocks as u32 + HOLD_EXPIRY_DELTA + self.safety_blocks;
        delta.saturating_sub(PAYER_CLTV_BUFFER).max(MIN_FINAL_CLTV_DELTA)
    }

    /// Height at which our own deadline must fire for an HTLC expiring at `expiry_height`.
    pub fn settle_by_height(&self, expiry_height: u32) -> u32 {
        expiry_height.saturating_sub(HOLD_EXPIRY_DELTA + self.safety_blocks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_deadlines_outside_the_window() {
        let cfg = TimingConfig::default();
        assert_eq!(cfg.validate_deadline(1000, 1010), Err(TimingError::TooSoon { min: 60 }));
        assert_eq!(cfg.validate_deadline(1000, 1000), Err(TimingError::TooSoon { min: 60 }));
        assert_eq!(cfg.validate_deadline(1000, 500), Err(TimingError::TooSoon { min: 60 }));
        assert_eq!(
            cfg.validate_deadline(1000, 1000 + 6 * 3600 + 1),
            Err(TimingError::TooLong { max: 6 * 3600 })
        );
        assert!(cfg.validate_deadline(1000, 1000 + 3600).is_ok());
        assert!(cfg.validate_deadline(1000, 1000 + 6 * 3600).is_ok());
    }

    #[test]
    fn cltv_delta_covers_the_hold_plus_lnd_margin() {
        let cfg = TimingConfig::default();
        // 1 hour = 6 blocks; 6 + 12 + 6 - 3 = 21.
        assert_eq!(cfg.final_cltv_delta(0, 3600), 21);
        // 6 hours = 36 blocks; 36 + 12 + 6 - 3 = 51.
        assert_eq!(cfg.final_cltv_delta(0, 6 * 3600), 51);
        // Partial blocks round up: 61 seconds is still 1 block, floored at the minimum.
        assert_eq!(cfg.final_cltv_delta(0, 61), MIN_FINAL_CLTV_DELTA);
    }

    #[test]
    fn our_deadline_lands_before_lnd_auto_cancel() {
        let cfg = TimingConfig::default();
        let start_height = 1000u32;
        let hold_secs = 3 * 3600;
        let delta = cfg.final_cltv_delta(0, hold_secs);
        let expiry_height = start_height + delta + PAYER_CLTV_BUFFER;
        let lnd_cancels_at = expiry_height - HOLD_EXPIRY_DELTA;
        let deadline_height = start_height + (hold_secs / cfg.block_secs) as u32;
        assert!(deadline_height + cfg.safety_blocks <= lnd_cancels_at);
        assert_eq!(cfg.settle_by_height(expiry_height), deadline_height);
    }
}
