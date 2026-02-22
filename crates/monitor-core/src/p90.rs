use crate::plans::{COMMON_TOKEN_LIMITS, DEFAULT_TOKEN_LIMIT, LIMIT_DETECTION_THRESHOLD};

// ── Percentile helper ─────────────────────────────────────────────────────────

/// Compute the `p`-th percentile of a **sorted** slice using standard linear
/// interpolation (the same algorithm used by NumPy's `percentile` function).
///
/// Returns `0.0` for an empty slice.
pub fn percentile(sorted_data: &[f64], p: f64) -> f64 {
    if sorted_data.is_empty() {
        return 0.0;
    }
    let len = sorted_data.len();
    if len == 1 {
        return sorted_data[0];
    }
    let rank = (p / 100.0) * (len as f64 - 1.0);
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        return sorted_data[lo];
    }
    let frac = rank - lo as f64;
    sorted_data[lo] + frac * (sorted_data[hi] - sorted_data[lo])
}

// ── P90Config ─────────────────────────────────────────────────────────────────

/// Configuration for the P90 token-limit estimator.
#[derive(Debug, Clone)]
pub struct P90Config {
    /// Well-known token-limit steps for detecting whether a session hit a cap.
    pub common_limits: Vec<u64>,
    /// Fraction of a limit at which a session is considered "at limit".
    pub limit_threshold: f64,
    /// Minimum value returned even when the P90 is lower.
    pub default_min_limit: u64,
    /// How many seconds a computed P90 result may be cached by external callers.
    pub cache_ttl_seconds: u64,
}

impl Default for P90Config {
    fn default() -> Self {
        Self {
            common_limits: COMMON_TOKEN_LIMITS.to_vec(),
            limit_threshold: LIMIT_DETECTION_THRESHOLD,
            default_min_limit: DEFAULT_TOKEN_LIMIT,
            cache_ttl_seconds: 300,
        }
    }
}

// ── P90Calculator ─────────────────────────────────────────────────────────────

/// Estimates the P90 token limit from a collection of historical session blocks.
pub struct P90Calculator {
    config: P90Config,
}

impl P90Calculator {
    /// Create a calculator with the supplied configuration.
    pub fn new(config: P90Config) -> Self {
        Self { config }
    }

    /// Create a calculator with the default (production) configuration.
    pub fn with_defaults() -> Self {
        Self::new(P90Config::default())
    }

    /// Estimate a P90 token limit from a slice of raw JSON block objects.
    ///
    /// Each block must expose the following fields:
    /// * `"isGap"`     — `bool`, whether the block is a gap (no activity).
    /// * `"isActive"`  — `bool`, whether the block is currently open.
    /// * `"totalTokens"` — `u64`, total tokens consumed in the block.
    ///
    /// Algorithm:
    /// 1. Filter out gap and active blocks.
    /// 2. Among the remainder, find blocks that hit a limit (tokens ≥ limit *
    ///    threshold for any well-known limit).
    /// 3. If no limit-hitting blocks exist, use *all* non-gap / non-active
    ///    blocks.
    /// 4. Compute the 90th percentile of the token counts from the chosen set.
    /// 5. Return `max(p90, default_min_limit)`.
    pub fn calculate_p90_limit(&self, blocks: &[serde_json::Value]) -> u64 {
        calculate_p90_from_blocks(blocks, &self.config)
    }
}

/// Standalone P90 calculation (same algorithm as [`P90Calculator::calculate_p90_limit`]).
pub fn calculate_p90_from_blocks(blocks: &[serde_json::Value], config: &P90Config) -> u64 {
    // Extract blocks that are neither gaps nor currently active.
    let completed: Vec<u64> = blocks
        .iter()
        .filter(|b| {
            let is_gap = b.get("isGap").and_then(|v| v.as_bool()).unwrap_or(false);
            let is_active = b.get("isActive").and_then(|v| v.as_bool()).unwrap_or(false);
            !is_gap && !is_active
        })
        .filter_map(|b| b.get("totalTokens").and_then(|v| v.as_u64()))
        .collect();

    if completed.is_empty() {
        return config.default_min_limit;
    }

    // 1. Try to use only sessions that hit a known token limit.
    let limit_hitting: Vec<f64> = completed
        .iter()
        .filter(|&&tokens| {
            config
                .common_limits
                .iter()
                .any(|&limit| tokens >= (limit as f64 * config.limit_threshold) as u64)
        })
        .map(|&t| t as f64)
        .collect();

    let sample: Vec<f64> = if !limit_hitting.is_empty() {
        let mut v = limit_hitting;
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v
    } else {
        // 2. Fall back to all completed sessions.
        let mut v: Vec<f64> = completed.iter().map(|&t| t as f64).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        v
    };

    let p90 = percentile(&sample, 90.0).round() as u64;
    p90.max(config.default_min_limit)
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_block(tokens: u64, is_gap: bool, is_active: bool) -> serde_json::Value {
        json!({
            "totalTokens": tokens,
            "isGap": is_gap,
            "isActive": is_active,
        })
    }

    fn default_config() -> P90Config {
        P90Config::default()
    }

    // ── percentile ───────────────────────────────────────────────────────────

    #[test]
    fn test_percentile_empty_returns_zero() {
        assert_eq!(percentile(&[], 90.0), 0.0);
    }

    #[test]
    fn test_percentile_single_element() {
        assert_eq!(percentile(&[42.0], 90.0), 42.0);
        assert_eq!(percentile(&[42.0], 0.0), 42.0);
        assert_eq!(percentile(&[42.0], 100.0), 42.0);
    }

    #[test]
    fn test_percentile_p50_even() {
        let data = vec![1.0, 2.0, 3.0, 4.0];
        // rank = 0.5 * 3 = 1.5 → interpolate between data[1]=2 and data[2]=3
        assert!((percentile(&data, 50.0) - 2.5).abs() < 1e-9);
    }

    #[test]
    fn test_percentile_p100() {
        let data = vec![10.0, 20.0, 30.0];
        assert!((percentile(&data, 100.0) - 30.0).abs() < 1e-9);
    }

    #[test]
    fn test_percentile_p0() {
        let data = vec![10.0, 20.0, 30.0];
        assert!((percentile(&data, 0.0) - 10.0).abs() < 1e-9);
    }

    #[test]
    fn test_percentile_p90_ten_elements() {
        // 1..=10 sorted: rank = 0.9 * 9 = 8.1 → 9 + 0.1*(10-9) = 9.1
        let data: Vec<f64> = (1..=10).map(|x| x as f64).collect();
        let p90 = percentile(&data, 90.0);
        assert!((p90 - 9.1).abs() < 1e-9, "p90 = {p90}");
    }

    // ── calculate_p90_from_blocks ─────────────────────────────────────────────

    #[test]
    fn test_p90_empty_blocks_returns_default() {
        let config = default_config();
        let result = calculate_p90_from_blocks(&[], &config);
        assert_eq!(result, DEFAULT_TOKEN_LIMIT);
    }

    #[test]
    fn test_p90_all_gaps_returns_default() {
        let config = default_config();
        let blocks: Vec<serde_json::Value> = (0..5)
            .map(|i| make_block(50_000 * (i + 1), true, false))
            .collect();
        let result = calculate_p90_from_blocks(&blocks, &config);
        assert_eq!(result, DEFAULT_TOKEN_LIMIT);
    }

    #[test]
    fn test_p90_all_active_returns_default() {
        let config = default_config();
        let blocks: Vec<serde_json::Value> = (0..5)
            .map(|i| make_block(50_000 * (i + 1), false, true))
            .collect();
        let result = calculate_p90_from_blocks(&blocks, &config);
        assert_eq!(result, DEFAULT_TOKEN_LIMIT);
    }

    #[test]
    fn test_p90_limit_hitting_sessions_preferred() {
        let config = default_config();
        // Two sessions at ~19k (Pro limit) and three tiny sessions.
        let mut blocks = vec![
            make_block(18_100, false, false), // hits 19k limit (>= 0.95 * 19000 = 18050)
            make_block(18_500, false, false), // hits 19k limit
            make_block(5_000, false, false),  // small
            make_block(3_000, false, false),  // small
            make_block(2_000, false, false),  // small
        ];
        // Add a gap block that should be ignored.
        blocks.push(make_block(200_000, true, false));

        let result = calculate_p90_from_blocks(&blocks, &config);
        // Only the two limit-hitting sessions are used: [18100, 18500].
        // p90 of [18100, 18500]: rank = 0.9 * 1 = 0.9 → 18100 + 0.9*400 = 18460
        // max(18460, 19000) = 19000
        assert_eq!(result, DEFAULT_TOKEN_LIMIT);
    }

    #[test]
    fn test_p90_no_limit_sessions_uses_all_completed() {
        let config = default_config();
        // All sessions well below any common limit.
        let blocks: Vec<serde_json::Value> = vec![
            make_block(1_000, false, false),
            make_block(2_000, false, false),
            make_block(3_000, false, false),
            make_block(4_000, false, false),
            make_block(5_000, false, false),
            make_block(6_000, false, false),
            make_block(7_000, false, false),
            make_block(8_000, false, false),
            make_block(9_000, false, false),
            make_block(10_000, false, false),
        ];
        let result = calculate_p90_from_blocks(&blocks, &config);
        // p90 of [1k..10k] in f64: 9.1k, but max(9100, 19000) = 19000
        assert_eq!(result, DEFAULT_TOKEN_LIMIT);
    }

    #[test]
    fn test_p90_result_is_at_least_default_min() {
        let config = default_config();
        let blocks = vec![make_block(100, false, false)];
        let result = calculate_p90_from_blocks(&blocks, &config);
        assert!(result >= DEFAULT_TOKEN_LIMIT);
    }

    #[test]
    fn test_p90_large_values_above_default() {
        let config = default_config();
        // Ten sessions all at ~88k (Max5 limit).
        let blocks: Vec<serde_json::Value> = (0..10)
            .map(|_| make_block(84_000, false, false)) // 84k >= 0.95*88k=83600
            .collect();
        let result = calculate_p90_from_blocks(&blocks, &config);
        // All sessions hit the 88k limit; p90 of ten identical values = 84000
        // max(84000, 19000) = 84000
        assert_eq!(result, 84_000);
    }

    // ── P90Calculator ────────────────────────────────────────────────────────

    #[test]
    fn test_p90_calculator_with_defaults() {
        let calc = P90Calculator::with_defaults();
        let result = calc.calculate_p90_limit(&[]);
        assert_eq!(result, DEFAULT_TOKEN_LIMIT);
    }

    #[test]
    fn test_p90_calculator_custom_config() {
        let config = P90Config {
            common_limits: vec![50_000],
            limit_threshold: 0.9,
            default_min_limit: 10_000,
            cache_ttl_seconds: 60,
        };
        let calc = P90Calculator::new(config);
        // One session at 46k (>= 0.9 * 50k = 45k).
        let blocks = vec![make_block(46_000, false, false)];
        let result = calc.calculate_p90_limit(&blocks);
        // p90 of [46000] = 46000; max(46000, 10000) = 46000
        assert_eq!(result, 46_000);
    }
}
