//! Usage aggregation over daily and monthly time windows.
//!
//! Ports the Python `UsageAggregator` class from `data/aggregator.py`.

use std::collections::{BTreeMap, HashMap, HashSet};

use monitor_core::models::{normalize_model_name, SessionBlock, UsageEntry};

// ── AggregatedStats ───────────────────────────────────────────────────────────

/// Token and cost totals accumulated across multiple usage entries.
#[derive(Debug, Clone, Default)]
pub struct AggregatedStats {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost: f64,
    pub count: u32,
}

impl AggregatedStats {
    /// Add a single entry's counts to the running totals.
    pub fn add_entry(&mut self, entry: &UsageEntry) {
        self.input_tokens += entry.input_tokens;
        self.output_tokens += entry.output_tokens;
        self.cache_creation_tokens += entry.cache_creation_tokens;
        self.cache_read_tokens += entry.cache_read_tokens;
        self.cost += entry.cost_usd;
        self.count += 1;
    }

    /// Sum of all four token categories.
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

// ── AggregatedPeriod ──────────────────────────────────────────────────────────

/// All usage data within one period (one day or one month).
#[derive(Debug, Clone)]
pub struct AggregatedPeriod {
    /// The period key, e.g. `"2024-01-15"` (daily) or `"2024-01"` (monthly).
    pub period_key: String,
    /// Combined stats for the period.
    pub stats: AggregatedStats,
    /// Canonical model names seen in this period.
    pub models_used: HashSet<String>,
    /// Per-model breakdown.
    pub model_breakdowns: HashMap<String, AggregatedStats>,
}

impl AggregatedPeriod {
    fn new(period_key: impl Into<String>) -> Self {
        Self {
            period_key: period_key.into(),
            stats: AggregatedStats::default(),
            models_used: HashSet::new(),
            model_breakdowns: HashMap::new(),
        }
    }

    /// Accumulate `entry` into the period's aggregate.
    fn add_entry(&mut self, entry: &UsageEntry) {
        self.stats.add_entry(entry);

        let model = if entry.model.is_empty() {
            "unknown".to_string()
        } else {
            normalize_model_name(&entry.model)
        };

        self.models_used.insert(model.clone());
        self.model_breakdowns
            .entry(model)
            .or_default()
            .add_entry(entry);
    }
}

// ── UsageAggregator ───────────────────────────────────────────────────────────

/// Stateless helper that groups usage entries by time period.
pub struct UsageAggregator;

impl UsageAggregator {
    /// Aggregate `entries` by calendar day.  Key format: `"%Y-%m-%d"`.
    ///
    /// Returns periods sorted by key (ascending).
    pub fn aggregate_daily(entries: &[UsageEntry]) -> Vec<AggregatedPeriod> {
        Self::aggregate_by_period(entries, |ts| ts.format("%Y-%m-%d").to_string())
    }

    /// Aggregate `entries` by calendar month.  Key format: `"%Y-%m"`.
    ///
    /// Returns periods sorted by key (ascending).
    pub fn aggregate_monthly(entries: &[UsageEntry]) -> Vec<AggregatedPeriod> {
        Self::aggregate_by_period(entries, |ts| ts.format("%Y-%m").to_string())
    }

    /// Aggregate all entries from non-gap session blocks.
    ///
    /// `view_type` must be `"daily"` or `"monthly"`; anything else falls back
    /// to `"daily"`.
    pub fn aggregate_from_blocks(
        blocks: &[SessionBlock],
        view_type: &str,
    ) -> Vec<AggregatedPeriod> {
        let all_entries: Vec<&UsageEntry> = blocks
            .iter()
            .filter(|b| !b.is_gap)
            .flat_map(|b| b.entries.iter())
            .collect();

        // Build owned Vec<UsageEntry> to satisfy the slice signature.
        let owned: Vec<UsageEntry> = all_entries.into_iter().cloned().collect();

        if view_type == "monthly" {
            Self::aggregate_monthly(&owned)
        } else {
            Self::aggregate_daily(&owned)
        }
    }

    /// Sum up the stats from all periods into a single [`AggregatedStats`].
    pub fn calculate_totals(data: &[AggregatedPeriod]) -> AggregatedStats {
        let mut totals = AggregatedStats::default();
        for period in data {
            totals.input_tokens += period.stats.input_tokens;
            totals.output_tokens += period.stats.output_tokens;
            totals.cache_creation_tokens += period.stats.cache_creation_tokens;
            totals.cache_read_tokens += period.stats.cache_read_tokens;
            totals.cost += period.stats.cost;
            totals.count += period.stats.count;
        }
        totals
    }

    // ── Private ───────────────────────────────────────────────────────────────

    /// Generic aggregation driver.
    ///
    /// `key_fn` maps a UTC timestamp to the string period key.
    fn aggregate_by_period(
        entries: &[UsageEntry],
        key_fn: impl Fn(chrono::DateTime<chrono::Utc>) -> String,
    ) -> Vec<AggregatedPeriod> {
        // Use BTreeMap for automatically sorted keys.
        let mut map: BTreeMap<String, AggregatedPeriod> = BTreeMap::new();

        for entry in entries {
            let key = key_fn(entry.timestamp);
            map.entry(key.clone())
                .or_insert_with(|| AggregatedPeriod::new(key))
                .add_entry(entry);
        }

        map.into_values().collect()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_entry(ts_str: &str, input: u64, output: u64, cost: f64, model: &str) -> UsageEntry {
        UsageEntry {
            timestamp: DateTime::parse_from_rfc3339(ts_str)
                .unwrap()
                .with_timezone(&Utc),
            input_tokens: input,
            output_tokens: output,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            cost_usd: cost,
            model: model.to_string(),
            message_id: ts_str.to_string(),
            request_id: ts_str.to_string(),
        }
    }

    use chrono::DateTime;

    // ── aggregate_daily ───────────────────────────────────────────────────────

    #[test]
    fn test_daily_groups_by_date() {
        let entries = vec![
            make_entry("2024-01-15T08:00:00Z", 100, 50, 0.01, "claude-3-5-sonnet"),
            make_entry("2024-01-15T20:00:00Z", 200, 100, 0.02, "claude-3-5-sonnet"),
            make_entry("2024-01-16T10:00:00Z", 300, 150, 0.03, "claude-3-5-sonnet"),
        ];
        let periods = UsageAggregator::aggregate_daily(&entries);

        assert_eq!(periods.len(), 2);
        assert_eq!(periods[0].period_key, "2024-01-15");
        assert_eq!(periods[0].stats.count, 2);
        assert_eq!(periods[1].period_key, "2024-01-16");
        assert_eq!(periods[1].stats.count, 1);
    }

    #[test]
    fn test_daily_token_aggregation() {
        let entries = vec![
            make_entry("2024-01-15T08:00:00Z", 100, 50, 0.01, "claude-3-5-sonnet"),
            make_entry("2024-01-15T12:00:00Z", 200, 100, 0.02, "claude-3-5-sonnet"),
        ];
        let periods = UsageAggregator::aggregate_daily(&entries);

        assert_eq!(periods[0].stats.input_tokens, 300);
        assert_eq!(periods[0].stats.output_tokens, 150);
        assert!((periods[0].stats.cost - 0.03).abs() < 1e-9);
    }

    #[test]
    fn test_daily_empty_entries() {
        let periods = UsageAggregator::aggregate_daily(&[]);
        assert!(periods.is_empty());
    }

    #[test]
    fn test_daily_sorted_by_date() {
        let entries = vec![
            make_entry("2024-01-20T08:00:00Z", 10, 5, 0.01, "claude-3-5-sonnet"),
            make_entry("2024-01-10T08:00:00Z", 10, 5, 0.01, "claude-3-5-sonnet"),
            make_entry("2024-01-15T08:00:00Z", 10, 5, 0.01, "claude-3-5-sonnet"),
        ];
        let periods = UsageAggregator::aggregate_daily(&entries);

        let keys: Vec<&str> = periods.iter().map(|p| p.period_key.as_str()).collect();
        assert_eq!(keys, vec!["2024-01-10", "2024-01-15", "2024-01-20"]);
    }

    // ── aggregate_monthly ─────────────────────────────────────────────────────

    #[test]
    fn test_monthly_groups_by_month() {
        let entries = vec![
            make_entry("2024-01-05T08:00:00Z", 100, 50, 0.01, "claude-3-5-sonnet"),
            make_entry("2024-01-20T08:00:00Z", 200, 100, 0.02, "claude-3-5-sonnet"),
            make_entry("2024-02-01T08:00:00Z", 300, 150, 0.03, "claude-3-5-sonnet"),
        ];
        let periods = UsageAggregator::aggregate_monthly(&entries);

        assert_eq!(periods.len(), 2);
        assert_eq!(periods[0].period_key, "2024-01");
        assert_eq!(periods[0].stats.count, 2);
        assert_eq!(periods[1].period_key, "2024-02");
        assert_eq!(periods[1].stats.count, 1);
    }

    #[test]
    fn test_monthly_empty_entries() {
        let periods = UsageAggregator::aggregate_monthly(&[]);
        assert!(periods.is_empty());
    }

    // ── aggregate_from_blocks ─────────────────────────────────────────────────

    #[test]
    fn test_aggregate_from_blocks_daily() {
        use monitor_core::models::TokenCounts;
        use std::collections::HashMap;

        let entry1 = make_entry("2024-01-15T10:00:00Z", 100, 50, 0.01, "claude-3-5-sonnet");
        let entry2 = make_entry("2024-01-16T10:00:00Z", 200, 100, 0.02, "claude-3-5-sonnet");

        let block = SessionBlock {
            id: "block1".to_string(),
            start_time: DateTime::parse_from_rfc3339("2024-01-15T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            end_time: DateTime::parse_from_rfc3339("2024-01-15T15:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            entries: vec![entry1, entry2],
            token_counts: TokenCounts::default(),
            is_active: false,
            is_gap: false,
            burn_rate: None,
            actual_end_time: None,
            per_model_stats: HashMap::new(),
            models: vec![],
            sent_messages_count: 2,
            cost_usd: 0.03,
            limit_messages: vec![],
            projection_data: None,
            burn_rate_snapshot: None,
        };

        let periods = UsageAggregator::aggregate_from_blocks(&[block], "daily");
        assert_eq!(periods.len(), 2);
    }

    #[test]
    fn test_aggregate_from_blocks_skips_gap_blocks() {
        use monitor_core::models::TokenCounts;
        use std::collections::HashMap;

        let gap_block = SessionBlock {
            id: "gap-1".to_string(),
            start_time: DateTime::parse_from_rfc3339("2024-01-15T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            end_time: DateTime::parse_from_rfc3339("2024-01-15T20:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            entries: vec![],
            token_counts: TokenCounts::default(),
            is_active: false,
            is_gap: true,
            burn_rate: None,
            actual_end_time: None,
            per_model_stats: HashMap::new(),
            models: vec![],
            sent_messages_count: 0,
            cost_usd: 0.0,
            limit_messages: vec![],
            projection_data: None,
            burn_rate_snapshot: None,
        };

        let periods = UsageAggregator::aggregate_from_blocks(&[gap_block], "daily");
        assert!(periods.is_empty());
    }

    // ── calculate_totals ──────────────────────────────────────────────────────

    #[test]
    fn test_calculate_totals_sums_all_periods() {
        let entries = vec![
            make_entry("2024-01-15T08:00:00Z", 100, 50, 0.01, "claude-3-5-sonnet"),
            make_entry("2024-01-16T08:00:00Z", 200, 100, 0.02, "claude-3-5-sonnet"),
            make_entry("2024-01-17T08:00:00Z", 300, 150, 0.03, "claude-3-5-sonnet"),
        ];
        let periods = UsageAggregator::aggregate_daily(&entries);
        let totals = UsageAggregator::calculate_totals(&periods);

        assert_eq!(totals.input_tokens, 600);
        assert_eq!(totals.output_tokens, 300);
        assert_eq!(totals.count, 3);
        assert!((totals.cost - 0.06).abs() < 1e-9);
    }

    #[test]
    fn test_calculate_totals_empty() {
        let totals = UsageAggregator::calculate_totals(&[]);
        assert_eq!(totals.count, 0);
        assert_eq!(totals.total_tokens(), 0);
        assert_eq!(totals.cost, 0.0);
    }

    // ── models_used ───────────────────────────────────────────────────────────

    #[test]
    fn test_models_used_tracking() {
        let entries = vec![
            make_entry(
                "2024-01-15T08:00:00Z",
                100,
                50,
                0.01,
                "claude-3-5-sonnet-20241022",
            ),
            make_entry(
                "2024-01-15T09:00:00Z",
                100,
                50,
                0.01,
                "claude-3-haiku-20240307",
            ),
        ];
        let periods = UsageAggregator::aggregate_daily(&entries);
        assert_eq!(periods[0].models_used.len(), 2);
        assert!(periods[0].models_used.contains("claude-3-5-sonnet"));
        assert!(periods[0].models_used.contains("claude-3-haiku"));
    }

    // ── model_breakdowns ──────────────────────────────────────────────────────

    #[test]
    fn test_model_breakdowns_aggregation() {
        let entries = vec![
            make_entry(
                "2024-01-15T08:00:00Z",
                100,
                50,
                0.01,
                "claude-3-5-sonnet-20241022",
            ),
            make_entry(
                "2024-01-15T09:00:00Z",
                200,
                100,
                0.02,
                "claude-3-5-sonnet-20241022",
            ),
        ];
        let periods = UsageAggregator::aggregate_daily(&entries);
        let breakdown = periods[0]
            .model_breakdowns
            .get("claude-3-5-sonnet")
            .unwrap();
        assert_eq!(breakdown.input_tokens, 300);
        assert_eq!(breakdown.count, 2);
    }
}
