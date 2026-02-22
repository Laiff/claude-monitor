use chrono::{DateTime, Utc};

use crate::models::{BurnRate, SessionBlock, UsageProjection};

/// Interface for any time-windowed usage block from which burn rate and
/// projection figures can be derived.
pub trait BlockLike {
    /// Whether the block is currently open / in-progress.
    fn is_active(&self) -> bool;
    /// Duration of the block in minutes (minimum 1.0).
    fn duration_minutes(&self) -> f64;
    /// Total token count across all categories.
    fn total_tokens(&self) -> u64;
    /// Total monetary cost (USD) for this block.
    fn cost_usd(&self) -> f64;
    /// Nominal or actual end time of the block.
    fn end_time(&self) -> DateTime<Utc>;
}

impl BlockLike for SessionBlock {
    fn is_active(&self) -> bool {
        self.is_active
    }

    fn duration_minutes(&self) -> f64 {
        self.duration_minutes()
    }

    fn total_tokens(&self) -> u64 {
        self.total_tokens()
    }

    fn cost_usd(&self) -> f64 {
        self.cost_usd
    }

    fn end_time(&self) -> DateTime<Utc> {
        self.actual_end_time.unwrap_or(self.end_time)
    }
}

// ── BurnRateCalculator ────────────────────────────────────────────────────────

/// Stateless collection of burn-rate and projection calculations.
pub struct BurnRateCalculator;

impl BurnRateCalculator {
    /// Compute the instantaneous burn rate for a block.
    ///
    /// Returns `None` when:
    /// * The block is not active.
    /// * Duration is less than 1.0 minute.
    /// * Total token count is 0.
    pub fn calculate_burn_rate<B: BlockLike>(block: &B) -> Option<BurnRate> {
        if !block.is_active() {
            return None;
        }
        let duration_minutes = block.duration_minutes();
        if duration_minutes < 1.0 {
            return None;
        }
        let total_tokens = block.total_tokens();
        if total_tokens == 0 {
            return None;
        }
        let tokens_per_minute = total_tokens as f64 / duration_minutes;
        let cost_per_hour = (block.cost_usd() / duration_minutes) * 60.0;
        Some(BurnRate {
            tokens_per_minute,
            cost_per_hour,
        })
    }

    /// Project how far a session will go given the current burn rate.
    ///
    /// Returns `None` when the block's end time has already passed.
    pub fn project_block_usage(
        burn_rate: &BurnRate,
        end_time: DateTime<Utc>,
        current_tokens: u64,
        current_cost: f64,
    ) -> Option<UsageProjection> {
        let now = Utc::now();
        let remaining_secs = (end_time - now).num_seconds();
        if remaining_secs <= 0 {
            return None;
        }
        let remaining_minutes = remaining_secs as f64 / 60.0;
        let remaining_hours = remaining_minutes / 60.0;

        let projected_total_tokens =
            current_tokens + (burn_rate.tokens_per_minute * remaining_minutes).round() as u64;
        let projected_total_cost = current_cost + burn_rate.cost_per_hour * remaining_hours;

        Some(UsageProjection {
            projected_total_tokens,
            projected_total_cost,
            remaining_minutes,
        })
    }

    /// Compute the rolling hourly burn rate (tokens / minute) by summing tokens
    /// from all blocks that overlap the last 60 minutes.
    ///
    /// Blocks that partially overlap the hour window contribute a proportional
    /// fraction of their tokens.
    pub fn calculate_hourly_burn_rate<B: BlockLike>(
        blocks: &[B],
        current_time: DateTime<Utc>,
    ) -> f64 {
        let window_start = current_time - chrono::Duration::hours(1);
        let mut total_tokens: f64 = 0.0;

        for block in blocks {
            let block_end = block.end_time();
            // Approximate block start from end time and duration.
            let block_start =
                block_end - chrono::Duration::seconds((block.duration_minutes() * 60.0) as i64);

            // Skip blocks entirely outside the window.
            if block_end <= window_start || block_start >= current_time {
                continue;
            }

            // Clamp overlap to [window_start, current_time].
            let overlap_start = block_start.max(window_start);
            let overlap_end = block_end.min(current_time);
            let overlap_secs = (overlap_end - overlap_start).num_seconds();
            let block_secs = (block_end - block_start).num_seconds();

            if block_secs <= 0 {
                continue;
            }

            let proportion = overlap_secs as f64 / block_secs as f64;
            total_tokens += block.total_tokens() as f64 * proportion;
        }

        // Normalise to tokens per minute over a 60-minute window.
        total_tokens / 60.0
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{SessionBlock, TokenCounts};
    use chrono::TimeZone;
    use std::collections::HashMap;

    fn make_block(
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        tokens: u64,
        cost: f64,
        is_active: bool,
    ) -> SessionBlock {
        SessionBlock {
            id: "test".to_string(),
            start_time: start,
            end_time: end,
            entries: vec![],
            token_counts: TokenCounts {
                input_tokens: tokens,
                output_tokens: 0,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            },
            is_active,
            is_gap: false,
            burn_rate: None,
            actual_end_time: None,
            per_model_stats: HashMap::new(),
            models: vec![],
            sent_messages_count: 0,
            cost_usd: cost,
            limit_messages: vec![],
            projection_data: None,
            burn_rate_snapshot: None,
        }
    }

    // ── calculate_burn_rate ──────────────────────────────────────────────────

    #[test]
    fn test_burn_rate_active_block() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 1, 1, 0, 0).unwrap(); // 60 min
        let block = make_block(start, end, 6_000, 6.0, true);

        let rate = BurnRateCalculator::calculate_burn_rate(&block).unwrap();
        // 6000 tokens / 60 min = 100 tokens/min
        assert!((rate.tokens_per_minute - 100.0).abs() < 1e-6);
        // (6.0 / 60) * 60 = 6.0 $/hr
        assert!((rate.cost_per_hour - 6.0).abs() < 1e-6);
    }

    #[test]
    fn test_burn_rate_inactive_block_returns_none() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 1, 1, 0, 0).unwrap();
        let block = make_block(start, end, 6_000, 6.0, false);

        assert!(BurnRateCalculator::calculate_burn_rate(&block).is_none());
    }

    #[test]
    fn test_burn_rate_zero_tokens_returns_none() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 1, 1, 0, 0).unwrap();
        let block = make_block(start, end, 0, 0.0, true);

        assert!(BurnRateCalculator::calculate_burn_rate(&block).is_none());
    }

    // ── project_block_usage ──────────────────────────────────────────────────

    #[test]
    fn test_projection_with_future_end_time() {
        let burn_rate = BurnRate {
            tokens_per_minute: 100.0,
            cost_per_hour: 6.0,
        };
        // End time 60 min from now.
        let end_time = Utc::now() + chrono::Duration::minutes(60);
        let proj = BurnRateCalculator::project_block_usage(&burn_rate, end_time, 1_000, 1.0);

        let p = proj.unwrap();
        // Should add roughly 6000 tokens (100/min * 60 min) to the 1000 current.
        assert!(p.projected_total_tokens >= 6_000 + 900); // allow for test timing
        assert!((p.remaining_minutes - 60.0).abs() < 5.0); // within 5 min tolerance
    }

    #[test]
    fn test_projection_with_past_end_time_returns_none() {
        let burn_rate = BurnRate {
            tokens_per_minute: 100.0,
            cost_per_hour: 6.0,
        };
        let end_time = Utc::now() - chrono::Duration::minutes(10);
        let proj = BurnRateCalculator::project_block_usage(&burn_rate, end_time, 1_000, 1.0);
        assert!(proj.is_none());
    }

    // ── calculate_hourly_burn_rate ───────────────────────────────────────────

    #[test]
    fn test_hourly_burn_rate_empty_blocks() {
        let now = Utc::now();
        let rate = BurnRateCalculator::calculate_hourly_burn_rate::<SessionBlock>(&[], now);
        assert_eq!(rate, 0.0);
    }

    #[test]
    fn test_hourly_burn_rate_full_overlap() {
        let now = Utc::now();
        // Block fully inside the last hour: 30 min duration with 3000 tokens.
        let end = now - chrono::Duration::minutes(10);
        let start = end - chrono::Duration::minutes(30);
        let block = make_block(start, end, 3_000, 3.0, false);

        let rate = BurnRateCalculator::calculate_hourly_burn_rate(&[block], now);
        // Full 3000 tokens over 60-min window = 50 tokens/min.
        assert!((rate - 50.0).abs() < 1e-3, "rate = {rate}");
    }

    #[test]
    fn test_hourly_burn_rate_partial_overlap() {
        let now = Utc::now();
        // Block starts 90 min ago and ends 30 min ago: 60 min duration.
        // Only the last 30 min of the block fall in the window.
        let end = now - chrono::Duration::minutes(30);
        let start = end - chrono::Duration::minutes(60);
        let block = make_block(start, end, 6_000, 6.0, false);

        let rate = BurnRateCalculator::calculate_hourly_burn_rate(&[block], now);
        // 3000 tokens contributed (50% of block within the window) / 60 = 50 tokens/min.
        assert!((rate - 50.0).abs() < 1e-2, "partial overlap rate = {rate}");
    }

    #[test]
    fn test_hourly_burn_rate_block_outside_window_is_ignored() {
        let now = Utc::now();
        // Block ended 2 hours ago – outside the 1-hour window.
        let end = now - chrono::Duration::hours(2);
        let start = end - chrono::Duration::minutes(30);
        let block = make_block(start, end, 9_000, 9.0, false);

        let rate = BurnRateCalculator::calculate_hourly_burn_rate(&[block], now);
        assert_eq!(rate, 0.0);
    }

    // ── BlockLike for SessionBlock ───────────────────────────────────────────

    #[test]
    fn test_block_like_impl_on_session_block() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 1, 2, 0, 0).unwrap(); // 120 min
        let block = make_block(start, end, 1_200, 2.4, true);

        assert!(block.is_active());
        assert!((block.duration_minutes() - 120.0).abs() < 1e-6);
        assert_eq!(block.total_tokens(), 1_200);
        assert!((block.cost_usd() - 2.4).abs() < 1e-9);
        assert_eq!(BlockLike::end_time(&block), end);
    }
}
