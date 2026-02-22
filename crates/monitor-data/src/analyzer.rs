//! Session block analyser for Claude Monitor.
//!
//! Groups [`UsageEntry`] records into 5-hour session windows and detects
//! rate-limit notifications embedded in raw JSONL data.

use std::collections::HashMap;

use chrono::{DateTime, DurationRound, TimeDelta, Utc};
use monitor_core::data_processors::TimestampProcessor;
use monitor_core::models::{normalize_model_name, SessionBlock, TokenCounts, UsageEntry};
use regex::Regex;
use tracing::debug;

// ── LimitDetection ────────────────────────────────────────────────────────────

/// A rate- or token-limit notification found in the raw JSONL stream.
#[derive(Debug, Clone)]
pub struct LimitDetection {
    /// `"opus_limit"`, `"system_limit"`, or `"general_limit"`.
    pub limit_type: String,
    /// When the limit was encountered (UTC).
    pub timestamp: DateTime<Utc>,
    /// Human-readable content of the notification.
    pub content: String,
    /// When the limit will be lifted, if it could be determined.
    pub reset_time: Option<DateTime<Utc>>,
}

// ── SessionAnalyzer ───────────────────────────────────────────────────────────

/// Groups usage entries into fixed-size session windows and detects limits.
pub struct SessionAnalyzer {
    /// Width of each session window (default: 5 hours).
    session_duration_hours: u64,
}

impl SessionAnalyzer {
    /// Create a new analyser with the given session-window width.
    pub fn new(session_duration_hours: u64) -> Self {
        Self {
            session_duration_hours,
        }
    }

    /// The session duration as a [`TimeDelta`].
    fn session_delta(&self) -> TimeDelta {
        TimeDelta::hours(self.session_duration_hours as i64)
    }

    // ── Public methods ────────────────────────────────────────────────────────

    /// Transform a slice of sorted [`UsageEntry`] records into [`SessionBlock`]s.
    ///
    /// The algorithm:
    /// 1. Entries must be pre-sorted by timestamp (the reader guarantees this).
    /// 2. A new block is opened when the entry falls outside the current
    ///    block's 5-hour window **or** the gap since the last entry exceeds 5h.
    /// 3. Gap blocks (is_gap = true) are inserted between consecutive real
    ///    blocks when the inactivity period is >= 5h.
    /// 4. Active blocks (end_time > now) are marked `is_active = true`.
    pub fn transform_to_blocks(&self, entries: &[UsageEntry]) -> Vec<SessionBlock> {
        if entries.is_empty() {
            return Vec::new();
        }

        let mut blocks: Vec<SessionBlock> = Vec::new();
        let mut current_block: Option<SessionBlock> = None;

        for entry in entries {
            let need_new = match &current_block {
                None => true,
                Some(block) => self.should_create_new_block(block, entry),
            };

            if need_new {
                if let Some(mut block) = current_block.take() {
                    Self::finalize_block(&mut block);
                    // Insert a gap block if necessary.
                    if let Some(gap) = Self::check_for_gap(&block, entry, self.session_delta()) {
                        blocks.push(gap);
                    }
                    blocks.push(block);
                }
                current_block = Some(Self::create_new_block(entry, self.session_delta()));
            }

            if let Some(ref mut block) = current_block {
                Self::add_entry_to_block(block, entry);
            }
        }

        // Finalize the last block.
        if let Some(mut block) = current_block {
            Self::finalize_block(&mut block);
            blocks.push(block);
        }

        Self::mark_active_blocks(&mut blocks);

        debug!(
            "SessionAnalyzer: created {} blocks from {} entries",
            blocks.len(),
            entries.len()
        );
        blocks
    }

    /// Scan raw JSONL values and return all detected limit notifications.
    pub fn detect_limits(&self, raw_entries: &[serde_json::Value]) -> Vec<LimitDetection> {
        raw_entries
            .iter()
            .filter_map(|entry| self.detect_single_limit(entry))
            .collect()
    }

    // ── Block-building helpers ────────────────────────────────────────────────

    /// Round a UTC timestamp down to the start of its hour.
    fn round_to_hour(ts: DateTime<Utc>) -> DateTime<Utc> {
        ts.duration_trunc(TimeDelta::hours(1)).unwrap_or(ts)
    }

    /// Decide whether a new block must be opened for `entry`.
    fn should_create_new_block(&self, block: &SessionBlock, entry: &UsageEntry) -> bool {
        // Entry is past the block's nominal end time.
        if entry.timestamp >= block.end_time {
            return true;
        }
        // Gap since last entry is >= session duration.
        if let Some(last) = block.entries.last() {
            if (entry.timestamp - last.timestamp) >= self.session_delta() {
                return true;
            }
        }
        false
    }

    /// Open a new, empty [`SessionBlock`] anchored to the hour containing `entry`.
    fn create_new_block(entry: &UsageEntry, session_delta: TimeDelta) -> SessionBlock {
        let start_time = Self::round_to_hour(entry.timestamp);
        let end_time = start_time + session_delta;
        let id = start_time.format("%Y-%m-%dT%H:%M:%SZ").to_string();

        SessionBlock {
            id,
            start_time,
            end_time,
            entries: Vec::new(),
            token_counts: TokenCounts::default(),
            is_active: false,
            is_gap: false,
            burn_rate: None,
            actual_end_time: None,
            per_model_stats: HashMap::new(),
            models: Vec::new(),
            sent_messages_count: 0,
            cost_usd: 0.0,
            limit_messages: Vec::new(),
            projection_data: None,
            burn_rate_snapshot: None,
        }
    }

    /// Accumulate `entry`'s tokens and cost into `block`, updating per-model stats.
    fn add_entry_to_block(block: &mut SessionBlock, entry: &UsageEntry) {
        block.entries.push(entry.clone());

        let raw_model = if entry.model.is_empty() {
            "unknown"
        } else {
            &entry.model
        };
        let model = if raw_model == "unknown" {
            "unknown".to_string()
        } else {
            normalize_model_name(raw_model)
        };

        // Per-model stats.
        let stats = block.per_model_stats.entry(model.clone()).or_default();
        stats.input_tokens += entry.input_tokens;
        stats.output_tokens += entry.output_tokens;
        stats.cache_creation_tokens += entry.cache_creation_tokens;
        stats.cache_read_tokens += entry.cache_read_tokens;
        stats.cost_usd += entry.cost_usd;
        stats.entries_count += 1;

        // Block-level aggregation.
        block.token_counts.input_tokens += entry.input_tokens;
        block.token_counts.output_tokens += entry.output_tokens;
        block.token_counts.cache_creation_tokens += entry.cache_creation_tokens;
        block.token_counts.cache_read_tokens += entry.cache_read_tokens;
        block.cost_usd += entry.cost_usd;

        // Model list (no duplicates, preserve insertion order).
        if !block.models.contains(&model) {
            block.models.push(model);
        }

        block.sent_messages_count += 1;
    }

    /// Set `actual_end_time` to the timestamp of the last entry in `block`.
    fn finalize_block(block: &mut SessionBlock) {
        if let Some(last) = block.entries.last() {
            block.actual_end_time = Some(last.timestamp);
        }
        // Keep sent_messages_count consistent with actual entry count.
        block.sent_messages_count = block.entries.len() as u32;
    }

    /// Build a gap [`SessionBlock`] if the inactivity between `last_block` and
    /// `next_entry` is >= `session_delta`.
    fn check_for_gap(
        last_block: &SessionBlock,
        next_entry: &UsageEntry,
        session_delta: TimeDelta,
    ) -> Option<SessionBlock> {
        let actual_end = last_block.actual_end_time?;
        let gap = next_entry.timestamp - actual_end;
        if gap < session_delta {
            return None;
        }

        let gap_id = format!("gap-{}", actual_end.format("%Y-%m-%dT%H:%M:%SZ"));
        Some(SessionBlock {
            id: gap_id,
            start_time: actual_end,
            end_time: next_entry.timestamp,
            entries: Vec::new(),
            token_counts: TokenCounts::default(),
            is_active: false,
            is_gap: true,
            burn_rate: None,
            actual_end_time: None,
            per_model_stats: HashMap::new(),
            models: Vec::new(),
            sent_messages_count: 0,
            cost_usd: 0.0,
            limit_messages: Vec::new(),
            projection_data: None,
            burn_rate_snapshot: None,
        })
    }

    /// Mark each non-gap block whose `end_time` is in the future as active.
    fn mark_active_blocks(blocks: &mut [SessionBlock]) {
        let now = Utc::now();
        for block in blocks.iter_mut() {
            if !block.is_gap && block.end_time > now {
                block.is_active = true;
            }
        }
    }

    // ── Limit-detection helpers ───────────────────────────────────────────────

    fn detect_single_limit(&self, raw_data: &serde_json::Value) -> Option<LimitDetection> {
        let entry_type = raw_data.get("type").and_then(|v| v.as_str())?;
        match entry_type {
            "system" => self.process_system_message(raw_data),
            "user" => self.process_user_message(raw_data),
            _ => None,
        }
    }

    fn process_system_message(&self, raw_data: &serde_json::Value) -> Option<LimitDetection> {
        let content = raw_data.get("content").and_then(|v| v.as_str())?;
        let content_lower = content.to_lowercase();

        if !content_lower.contains("limit") && !content_lower.contains("rate") {
            return None;
        }

        let ts_value = raw_data.get("timestamp")?;
        let timestamp = TimestampProcessor::parse(ts_value)?;

        if is_opus_limit(&content_lower) {
            let (reset_time, _wait_minutes) = extract_wait_time(content, timestamp);
            Some(LimitDetection {
                limit_type: "opus_limit".to_string(),
                timestamp,
                content: content.to_string(),
                reset_time,
            })
        } else {
            Some(LimitDetection {
                limit_type: "system_limit".to_string(),
                timestamp,
                content: content.to_string(),
                reset_time: None,
            })
        }
    }

    fn process_user_message(&self, raw_data: &serde_json::Value) -> Option<LimitDetection> {
        let message = raw_data.get("message")?;
        let content_list = message.get("content")?.as_array()?;

        for item in content_list {
            if item.get("type").and_then(|v| v.as_str()) != Some("tool_result") {
                continue;
            }
            let tool_content = match item.get("content").and_then(|v| v.as_array()) {
                Some(arr) => arr,
                None => continue,
            };
            for tool_item in tool_content {
                let text = match tool_item.get("text").and_then(|v| v.as_str()) {
                    Some(t) => t,
                    None => continue,
                };
                if !text.to_lowercase().contains("limit reached") {
                    continue;
                }
                let ts_value = raw_data.get("timestamp")?;
                let timestamp = TimestampProcessor::parse(ts_value)?;
                let reset_time = parse_reset_timestamp(text);
                return Some(LimitDetection {
                    limit_type: "general_limit".to_string(),
                    timestamp,
                    content: text.to_string(),
                    reset_time,
                });
            }
        }
        None
    }
}

// ── Module-level limit helpers ────────────────────────────────────────────────

/// Return `true` when the lowercased content signals an Opus-specific limit.
fn is_opus_limit(content_lower: &str) -> bool {
    if !content_lower.contains("opus") {
        return false;
    }
    let phrases = [
        "rate limit",
        "limit exceeded",
        "limit reached",
        "limit hit",
        "limit",
    ];
    phrases.iter().any(|p| content_lower.contains(p))
}

/// Parse `"wait N minutes"` from `content` and compute the reset timestamp.
///
/// Returns `(reset_time, wait_minutes)`.
fn extract_wait_time(
    content: &str,
    timestamp: DateTime<Utc>,
) -> (Option<DateTime<Utc>>, Option<u64>) {
    let re = Regex::new(r"wait\s+(\d+)\s+minutes?").expect("regex is valid");
    if let Some(cap) = re.captures(&content.to_lowercase()) {
        if let Ok(minutes) = cap[1].parse::<u64>() {
            let reset = timestamp + TimeDelta::minutes(minutes as i64);
            return (Some(reset), Some(minutes));
        }
    }
    (None, None)
}

/// Extract a Unix-timestamp reset time from `"limit reached|<unix_ts>"`.
fn parse_reset_timestamp(text: &str) -> Option<DateTime<Utc>> {
    let re = Regex::new(r"limit reached\|(\d+)").expect("regex is valid");
    if let Some(cap) = re.captures(&text.to_lowercase()) {
        if let Ok(unix_secs) = cap[1].parse::<i64>() {
            return TimestampProcessor::parse(&serde_json::Value::Number(
                serde_json::Number::from(unix_secs),
            ));
        }
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_entry(ts_str: &str, input: u64, output: u64, model: &str) -> UsageEntry {
        let ts = DateTime::parse_from_rfc3339(ts_str)
            .unwrap()
            .with_timezone(&Utc);
        UsageEntry {
            timestamp: ts,
            input_tokens: input,
            output_tokens: output,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            cost_usd: 0.001,
            model: model.to_string(),
            message_id: format!("msg-{}", ts_str),
            request_id: format!("req-{}", ts_str),
        }
    }

    fn analyzer() -> SessionAnalyzer {
        SessionAnalyzer::new(5)
    }

    // ── transform_to_blocks ───────────────────────────────────────────────────

    #[test]
    fn test_empty_entries_returns_empty_blocks() {
        let blocks = analyzer().transform_to_blocks(&[]);
        assert!(blocks.is_empty());
    }

    #[test]
    fn test_single_entry_creates_one_block() {
        use chrono::Timelike;
        let entries = vec![make_entry(
            "2024-01-15T10:30:00Z",
            100,
            50,
            "claude-3-5-sonnet",
        )];
        let blocks = analyzer().transform_to_blocks(&entries);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].token_counts.input_tokens, 100);
        assert_eq!(blocks[0].token_counts.output_tokens, 50);
        // Block starts at rounded hour.
        assert_eq!(blocks[0].start_time.minute(), 0);
    }

    #[test]
    fn test_block_start_time_rounded_to_hour() {
        let entries = vec![make_entry(
            "2024-01-15T10:45:00Z",
            10,
            5,
            "claude-3-5-sonnet",
        )];
        let blocks = analyzer().transform_to_blocks(&entries);
        assert_eq!(
            blocks[0].start_time,
            Utc.with_ymd_and_hms(2024, 1, 15, 10, 0, 0).unwrap()
        );
        assert_eq!(
            blocks[0].end_time,
            Utc.with_ymd_and_hms(2024, 1, 15, 15, 0, 0).unwrap()
        );
    }

    #[test]
    fn test_entries_within_5h_window_go_into_same_block() {
        let entries = vec![
            make_entry("2024-01-15T10:00:00Z", 100, 50, "claude-3-5-sonnet"),
            make_entry("2024-01-15T12:00:00Z", 200, 100, "claude-3-5-sonnet"),
            make_entry("2024-01-15T14:30:00Z", 50, 25, "claude-3-5-sonnet"),
        ];
        let blocks = analyzer().transform_to_blocks(&entries);
        // All within 10:00-15:00 window → 1 block (no gap entries).
        let real_blocks: Vec<_> = blocks.iter().filter(|b| !b.is_gap).collect();
        assert_eq!(real_blocks.len(), 1);
        assert_eq!(real_blocks[0].entries.len(), 3);
    }

    #[test]
    fn test_entry_past_block_end_creates_new_block() {
        let entries = vec![
            make_entry("2024-01-15T10:00:00Z", 100, 50, "claude-3-5-sonnet"),
            make_entry("2024-01-15T16:00:00Z", 200, 100, "claude-3-5-sonnet"),
        ];
        let blocks = analyzer().transform_to_blocks(&entries);
        let real_blocks: Vec<_> = blocks.iter().filter(|b| !b.is_gap).collect();
        assert_eq!(real_blocks.len(), 2);
    }

    #[test]
    fn test_gap_block_inserted_between_sessions() {
        let entries = vec![
            make_entry("2024-01-15T10:00:00Z", 100, 50, "claude-3-5-sonnet"),
            // 10 hours later – gap >= 5h.
            make_entry("2024-01-15T20:00:00Z", 200, 100, "claude-3-5-sonnet"),
        ];
        let blocks = analyzer().transform_to_blocks(&entries);
        let gap_blocks: Vec<_> = blocks.iter().filter(|b| b.is_gap).collect();
        assert_eq!(gap_blocks.len(), 1);
        assert!(gap_blocks[0].id.starts_with("gap-"));
    }

    #[test]
    fn test_no_gap_block_when_sessions_are_close() {
        let entries = vec![
            make_entry("2024-01-15T10:00:00Z", 100, 50, "claude-3-5-sonnet"),
            // 3 hours later – within 5h window.
            make_entry("2024-01-15T13:00:00Z", 200, 100, "claude-3-5-sonnet"),
        ];
        let blocks = analyzer().transform_to_blocks(&entries);
        let gap_blocks: Vec<_> = blocks.iter().filter(|b| b.is_gap).collect();
        assert!(gap_blocks.is_empty());
    }

    #[test]
    fn test_active_block_marked_when_end_time_is_future() {
        // Use an entry from now – its block end_time will be in the future.
        let recent_ts = (Utc::now() - chrono::Duration::minutes(30))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let entries = vec![make_entry(&recent_ts, 10, 5, "claude-3-5-sonnet")];
        let blocks = analyzer().transform_to_blocks(&entries);

        let active: Vec<_> = blocks.iter().filter(|b| b.is_active).collect();
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn test_old_block_is_not_active() {
        let entries = vec![make_entry(
            "2024-01-15T10:00:00Z",
            100,
            50,
            "claude-3-5-sonnet",
        )];
        let blocks = analyzer().transform_to_blocks(&entries);
        assert!(!blocks[0].is_active);
    }

    #[test]
    fn test_per_model_stats_aggregation() {
        let entries = vec![
            make_entry(
                "2024-01-15T10:00:00Z",
                100,
                50,
                "claude-3-5-sonnet-20241022",
            ),
            make_entry(
                "2024-01-15T11:00:00Z",
                200,
                100,
                "claude-3-5-sonnet-20241022",
            ),
        ];
        let blocks = analyzer().transform_to_blocks(&entries);
        let stats = blocks[0].per_model_stats.get("claude-3-5-sonnet").unwrap();
        assert_eq!(stats.input_tokens, 300);
        assert_eq!(stats.output_tokens, 150);
        assert_eq!(stats.entries_count, 2);
    }

    #[test]
    fn test_per_model_stats_multiple_models() {
        let entries = vec![
            make_entry(
                "2024-01-15T10:00:00Z",
                100,
                50,
                "claude-3-5-sonnet-20241022",
            ),
            make_entry("2024-01-15T11:00:00Z", 200, 100, "claude-3-haiku-20240307"),
        ];
        let blocks = analyzer().transform_to_blocks(&entries);
        assert!(blocks[0].per_model_stats.contains_key("claude-3-5-sonnet"));
        assert!(blocks[0].per_model_stats.contains_key("claude-3-haiku"));
    }

    #[test]
    fn test_actual_end_time_is_last_entry_timestamp() {
        let entries = vec![
            make_entry("2024-01-15T10:00:00Z", 100, 50, "claude-3-5-sonnet"),
            make_entry("2024-01-15T12:00:00Z", 200, 100, "claude-3-5-sonnet"),
        ];
        let blocks = analyzer().transform_to_blocks(&entries);
        let expected = DateTime::parse_from_rfc3339("2024-01-15T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(blocks[0].actual_end_time.unwrap(), expected);
    }

    // ── detect_limits ─────────────────────────────────────────────────────────

    #[test]
    fn test_detect_limits_system_limit() {
        let raw = vec![serde_json::json!({
            "type": "system",
            "timestamp": "2024-01-15T10:00:00Z",
            "content": "You have hit a rate limit. Please wait.",
        })];
        let limits = analyzer().detect_limits(&raw);
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0].limit_type, "system_limit");
    }

    #[test]
    fn test_detect_limits_opus_limit_with_wait_time() {
        let raw = vec![serde_json::json!({
            "type": "system",
            "timestamp": "2024-01-15T10:00:00Z",
            "content": "Opus rate limit exceeded. Please wait 30 minutes.",
        })];
        let limits = analyzer().detect_limits(&raw);
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0].limit_type, "opus_limit");
        assert!(limits[0].reset_time.is_some());
    }

    #[test]
    fn test_detect_limits_general_limit_from_tool_result() {
        let raw = vec![serde_json::json!({
            "type": "user",
            "timestamp": "2024-01-15T10:00:00Z",
            "message": {
                "content": [{
                    "type": "tool_result",
                    "content": [{"text": "limit reached|1705312800"}]
                }]
            }
        })];
        let limits = analyzer().detect_limits(&raw);
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0].limit_type, "general_limit");
    }

    #[test]
    fn test_detect_limits_no_limit_content() {
        let raw = vec![serde_json::json!({
            "type": "system",
            "timestamp": "2024-01-15T10:00:00Z",
            "content": "Everything is fine.",
        })];
        let limits = analyzer().detect_limits(&raw);
        assert!(limits.is_empty());
    }

    #[test]
    fn test_detect_limits_non_system_non_user_ignored() {
        let raw = vec![serde_json::json!({
            "type": "assistant",
            "timestamp": "2024-01-15T10:00:00Z",
            "content": "You have hit a rate limit.",
        })];
        let limits = analyzer().detect_limits(&raw);
        assert!(limits.is_empty());
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    #[test]
    fn test_round_to_hour() {
        let ts = Utc.with_ymd_and_hms(2024, 1, 15, 10, 45, 30).unwrap();
        let rounded = SessionAnalyzer::round_to_hour(ts);
        assert_eq!(
            rounded,
            Utc.with_ymd_and_hms(2024, 1, 15, 10, 0, 0).unwrap()
        );
    }

    #[test]
    fn test_extract_wait_time() {
        let ts = Utc.with_ymd_and_hms(2024, 1, 15, 10, 0, 0).unwrap();
        let (reset, mins) = extract_wait_time("Please wait 30 minutes.", ts);
        assert_eq!(mins, Some(30));
        let expected_reset = Utc.with_ymd_and_hms(2024, 1, 15, 10, 30, 0).unwrap();
        assert_eq!(reset.unwrap(), expected_reset);
    }

    #[test]
    fn test_extract_wait_time_no_match() {
        let ts = Utc::now();
        let (reset, mins) = extract_wait_time("No wait time here.", ts);
        assert!(reset.is_none());
        assert!(mins.is_none());
    }

    #[test]
    fn test_gap_block_id_format() {
        let entries = vec![
            make_entry("2024-01-15T10:00:00Z", 100, 50, "claude-3-5-sonnet"),
            make_entry("2024-01-15T22:00:00Z", 200, 100, "claude-3-5-sonnet"),
        ];
        let blocks = analyzer().transform_to_blocks(&entries);
        let gap = blocks.iter().find(|b| b.is_gap).unwrap();
        assert!(gap.id.starts_with("gap-2024-01-15T10:00:00Z"));
    }
}
