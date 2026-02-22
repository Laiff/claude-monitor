use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Determines how usage cost is calculated for a session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CostMode {
    /// Automatically select the most appropriate mode.
    Auto,
    /// Use the cached cost value already stored in the data.
    Cached,
    /// Recalculate cost from token counts and pricing tables.
    #[serde(rename = "calculate")]
    Calculated,
}

/// A single API call record read from a JSONL usage file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEntry {
    /// UTC timestamp when the request was made.
    pub timestamp: DateTime<Utc>,
    /// Number of input (prompt) tokens consumed.
    pub input_tokens: u64,
    /// Number of output (completion) tokens generated.
    pub output_tokens: u64,
    /// Tokens written into the prompt cache.
    #[serde(default)]
    pub cache_creation_tokens: u64,
    /// Tokens read from the prompt cache.
    #[serde(default)]
    pub cache_read_tokens: u64,
    /// Monetary cost in US dollars for this entry.
    #[serde(default)]
    pub cost_usd: f64,
    /// Raw model identifier string from the API response.
    #[serde(default)]
    pub model: String,
    /// Unique message identifier.
    #[serde(default)]
    pub message_id: String,
    /// Unique request identifier.
    #[serde(default)]
    pub request_id: String,
}

/// Aggregated token counts across multiple usage entries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenCounts {
    /// Accumulated input (prompt) tokens.
    #[serde(default)]
    pub input_tokens: u64,
    /// Accumulated output (completion) tokens.
    #[serde(default)]
    pub output_tokens: u64,
    /// Accumulated cache-creation tokens.
    #[serde(default)]
    pub cache_creation_tokens: u64,
    /// Accumulated cache-read tokens.
    #[serde(default)]
    pub cache_read_tokens: u64,
}

impl TokenCounts {
    /// Sum of all four token categories.
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens + self.cache_creation_tokens + self.cache_read_tokens
    }
}

/// Instantaneous token consumption and cost burn rates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnRate {
    /// Tokens consumed per minute.
    pub tokens_per_minute: f64,
    /// US dollar cost per hour.
    pub cost_per_hour: f64,
}

/// Forward projection of how many tokens / dollars a session will consume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageProjection {
    /// Estimated total tokens by the end of the session.
    pub projected_total_tokens: u64,
    /// Estimated total cost (USD) by the end of the session.
    pub projected_total_cost: f64,
    /// Minutes remaining before the session window closes.
    pub remaining_minutes: f64,
}

/// Per-model breakdown of token usage within a session block.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelStats {
    /// Input tokens attributed to this model.
    pub input_tokens: u64,
    /// Output tokens attributed to this model.
    pub output_tokens: u64,
    /// Cache-creation tokens attributed to this model.
    pub cache_creation_tokens: u64,
    /// Cache-read tokens attributed to this model.
    pub cache_read_tokens: u64,
    /// Cost in USD attributed to this model.
    pub cost_usd: f64,
    /// Number of individual usage entries for this model.
    pub entries_count: u32,
}

/// A structured representation of a rate-limit notification embedded in the data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitMessage {
    /// Category of the limit that was hit (e.g. "token", "message").
    pub limit_type: String,
    /// ISO-8601 timestamp string when the limit was encountered.
    pub timestamp: String,
    /// Human-readable content of the limit notification.
    pub content: String,
    /// ISO-8601 timestamp string when the limit will be lifted, if known.
    pub reset_time: Option<String>,
}

/// A 5-hour usage window aggregating all API calls within that period.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionBlock {
    /// Unique identifier for this block.
    pub id: String,
    /// Inclusive start of the 5-hour window (UTC).
    pub start_time: DateTime<Utc>,
    /// Exclusive end of the 5-hour window (UTC).
    pub end_time: DateTime<Utc>,
    /// Individual usage records that fall within this window.
    #[serde(default)]
    pub entries: Vec<UsageEntry>,
    /// Aggregated token counts for the block.
    #[serde(default)]
    pub token_counts: TokenCounts,
    /// Whether this block is currently open / in-progress.
    #[serde(default)]
    pub is_active: bool,
    /// Whether this block represents a gap with no activity.
    #[serde(default)]
    pub is_gap: bool,
    /// Current burn rate, if the block is active.
    #[serde(default)]
    pub burn_rate: Option<BurnRate>,
    /// Timestamp of the last entry in the block (may differ from `end_time`).
    #[serde(default)]
    pub actual_end_time: Option<DateTime<Utc>>,
    /// Token and cost statistics broken down by model name.
    #[serde(default)]
    pub per_model_stats: HashMap<String, ModelStats>,
    /// Ordered list of model names seen in this block.
    #[serde(default)]
    pub models: Vec<String>,
    /// Number of user-initiated messages in this block.
    #[serde(default)]
    pub sent_messages_count: u32,
    /// Total monetary cost in USD for this block.
    #[serde(default)]
    pub cost_usd: f64,
    /// Structured limit-hit notifications embedded in the block.
    #[serde(default)]
    pub limit_messages: Vec<LimitMessage>,
    /// Opaque projection data, serialised for downstream consumers.
    #[serde(default)]
    pub projection_data: Option<serde_json::Value>,
    /// Snapshot of the burn rate captured at block close time.
    #[serde(default)]
    pub burn_rate_snapshot: Option<BurnRate>,
}

impl SessionBlock {
    /// Delegates to `token_counts.total_tokens()`.
    pub fn total_tokens(&self) -> u64 {
        self.token_counts.total_tokens()
    }

    /// Alias for `cost_usd`.
    pub fn total_cost(&self) -> f64 {
        self.cost_usd
    }

    /// Duration of the block in minutes, minimum 1.0.
    ///
    /// Uses `actual_end_time` when present (the timestamp of the last real
    /// entry), otherwise falls back to the nominal `end_time`.
    pub fn duration_minutes(&self) -> f64 {
        let end = self.actual_end_time.unwrap_or(self.end_time);
        let secs = (end - self.start_time).num_seconds() as f64;
        f64::max(secs / 60.0, 1.0)
    }
}

/// Normalise a raw model name string into a canonical key.
///
/// Replicates the Python `normalize_model_name()` function exactly:
///
/// * Claude 4 variants (any name containing `claude-*-4-` or `*-4-`) are
///   returned lowercased without further transformation.
/// * `opus` → `"claude-3-opus"` (unless a Claude 4 prefix is present).
/// * `sonnet` with `3.5`/`3-5` → `"claude-3-5-sonnet"`, otherwise
///   `"claude-3-sonnet"`.
/// * `haiku` with `3.5`/`3-5` → `"claude-3-5-haiku"`, otherwise
///   `"claude-3-haiku"`.
/// * Everything else is returned unchanged (original casing).
/// * Empty string → `""`.
///
/// # Examples
///
/// ```
/// use monitor_core::models::normalize_model_name;
///
/// assert_eq!(normalize_model_name("claude-3-opus-20240229"), "claude-3-opus");
/// assert_eq!(normalize_model_name("Claude 3.5 Sonnet"), "claude-3-5-sonnet");
/// assert_eq!(normalize_model_name("claude-sonnet-4-20250514"), "claude-sonnet-4-20250514");
/// ```
pub fn normalize_model_name(model: &str) -> String {
    if model.is_empty() {
        return String::new();
    }

    let lower = model.to_lowercase();

    // Claude 4 fast-path: any variant whose lowercased form already contains
    // one of the Claude-4 sub-strings is returned as-is (lowercased).
    if lower.contains("claude-opus-4-")
        || lower.contains("claude-sonnet-4-")
        || lower.contains("claude-haiku-4-")
        || lower.contains("sonnet-4-")
        || lower.contains("opus-4-")
        || lower.contains("haiku-4-")
    {
        return lower;
    }

    if lower.contains("opus") {
        // A second Claude-4 guard that mirrors the Python branch
        if lower.contains("4-") {
            return lower;
        }
        return "claude-3-opus".to_string();
    }

    if lower.contains("sonnet") {
        if lower.contains("4-") {
            return lower;
        }
        if lower.contains("3.5") || lower.contains("3-5") {
            return "claude-3-5-sonnet".to_string();
        }
        return "claude-3-sonnet".to_string();
    }

    if lower.contains("haiku") {
        if lower.contains("3.5") || lower.contains("3-5") {
            return "claude-3-5-haiku".to_string();
        }
        return "claude-3-haiku".to_string();
    }

    // Unknown model – return original string unchanged.
    model.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    // ── TokenCounts ────────────────────────────────────────────────────────

    #[test]
    fn test_token_counts_default() {
        let tc = TokenCounts::default();
        assert_eq!(tc.input_tokens, 0);
        assert_eq!(tc.output_tokens, 0);
        assert_eq!(tc.cache_creation_tokens, 0);
        assert_eq!(tc.cache_read_tokens, 0);
        assert_eq!(tc.total_tokens(), 0);
    }

    #[test]
    fn test_token_counts_total() {
        let tc = TokenCounts {
            input_tokens: 100,
            output_tokens: 200,
            cache_creation_tokens: 50,
            cache_read_tokens: 25,
        };
        assert_eq!(tc.total_tokens(), 375);
    }

    // ── SessionBlock ───────────────────────────────────────────────────────

    fn make_block(
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        actual_end: Option<DateTime<Utc>>,
    ) -> SessionBlock {
        SessionBlock {
            id: "test-block".to_string(),
            start_time: start,
            end_time: end,
            entries: vec![],
            token_counts: TokenCounts {
                input_tokens: 1_000,
                output_tokens: 500,
                cache_creation_tokens: 100,
                cache_read_tokens: 50,
            },
            is_active: false,
            is_gap: false,
            burn_rate: None,
            actual_end_time: actual_end,
            per_model_stats: HashMap::new(),
            models: vec![],
            sent_messages_count: 0,
            cost_usd: 3.14,
            limit_messages: vec![],
            projection_data: None,
            burn_rate_snapshot: None,
        }
    }

    #[test]
    fn test_session_block_duration_minutes_without_actual_end() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 1, 5, 0, 0).unwrap(); // 300 min
        let block = make_block(start, end, None);
        assert!((block.duration_minutes() - 300.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_session_block_duration_minutes_with_actual_end() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 1, 5, 0, 0).unwrap();
        let actual_end = Utc.with_ymd_and_hms(2024, 1, 1, 2, 30, 0).unwrap(); // 150 min
        let block = make_block(start, end, Some(actual_end));
        assert!((block.duration_minutes() - 150.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_session_block_duration_minutes_minimum_one() {
        // start == end → would be 0 minutes, should clamp to 1.
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let block = make_block(start, start, None);
        assert!((block.duration_minutes() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_session_block_total_tokens() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 1, 5, 0, 0).unwrap();
        let block = make_block(start, end, None);
        // 1000 + 500 + 100 + 50 = 1650
        assert_eq!(block.total_tokens(), 1_650);
    }

    #[test]
    fn test_session_block_total_cost() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 1, 5, 0, 0).unwrap();
        let block = make_block(start, end, None);
        assert!((block.total_cost() - 3.14).abs() < f64::EPSILON);
    }

    // ── normalize_model_name ───────────────────────────────────────────────

    #[test]
    fn test_normalize_model_name_empty() {
        assert_eq!(normalize_model_name(""), "");
    }

    #[test]
    fn test_normalize_model_name_opus() {
        assert_eq!(
            normalize_model_name("claude-3-opus-20240229"),
            "claude-3-opus"
        );
    }

    #[test]
    fn test_normalize_model_name_sonnet_35() {
        assert_eq!(
            normalize_model_name("Claude 3.5 Sonnet"),
            "claude-3-5-sonnet"
        );
    }

    #[test]
    fn test_normalize_model_name_sonnet_35_dash() {
        assert_eq!(
            normalize_model_name("claude-3-5-sonnet-20241022"),
            "claude-3-5-sonnet"
        );
    }

    #[test]
    fn test_normalize_model_name_sonnet_3() {
        assert_eq!(
            normalize_model_name("claude-3-sonnet-20240229"),
            "claude-3-sonnet"
        );
    }

    #[test]
    fn test_normalize_model_name_haiku() {
        assert_eq!(
            normalize_model_name("claude-3-haiku-20240307"),
            "claude-3-haiku"
        );
    }

    #[test]
    fn test_normalize_model_name_haiku_35() {
        assert_eq!(
            normalize_model_name("claude-3-5-haiku-20241022"),
            "claude-3-5-haiku"
        );
    }

    #[test]
    fn test_normalize_model_name_claude4_sonnet() {
        let name = "claude-sonnet-4-20250514";
        assert_eq!(normalize_model_name(name), name);
    }

    #[test]
    fn test_normalize_model_name_claude4_opus() {
        let name = "claude-opus-4-20250514";
        assert_eq!(normalize_model_name(name), name);
    }

    #[test]
    fn test_normalize_model_name_claude4_haiku() {
        let name = "claude-haiku-4-20250514";
        assert_eq!(normalize_model_name(name), name);
    }

    #[test]
    fn test_normalize_model_name_unknown() {
        // Unknown models are returned with their original casing.
        assert_eq!(normalize_model_name("gpt-4"), "gpt-4");
    }

    #[test]
    fn test_normalize_model_name_unknown_mixed_case() {
        assert_eq!(normalize_model_name("GPT-4-turbo"), "GPT-4-turbo");
    }

    // ── CostMode serde ────────────────────────────────────────────────────

    #[test]
    fn test_cost_mode_serde_auto() {
        let mode = CostMode::Auto;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, r#""auto""#);
        let back: CostMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, CostMode::Auto);
    }

    #[test]
    fn test_cost_mode_serde_cached() {
        let mode = CostMode::Cached;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, r#""cached""#);
        let back: CostMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, CostMode::Cached);
    }

    #[test]
    fn test_cost_mode_serde_calculated() {
        let mode = CostMode::Calculated;
        let json = serde_json::to_string(&mode).unwrap();
        // The Python value is "calculate" (no 'd')
        assert_eq!(json, r#""calculate""#);
        let back: CostMode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, CostMode::Calculated);
    }
}
