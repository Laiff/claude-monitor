//! Main analysis pipeline for Claude Monitor.
//!
//! Orchestrates loading, block-creation, burn-rate computation and limit
//! detection, returning an [`AnalysisResult`] ready for the UI layer.

use chrono::Utc;
use monitor_core::calculations::BurnRateCalculator;
use monitor_core::models::{CostMode, LimitMessage, SessionBlock};

use crate::analyzer::{LimitDetection, SessionAnalyzer};
use crate::reader::load_usage_entries;

// ── Public types ──────────────────────────────────────────────────────────────

/// Metadata produced alongside the analysis result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnalysisMetadata {
    /// ISO-8601 timestamp when this result was generated.
    pub generated_at: String,
    /// Number of hours analysed, or `None` if all history was loaded.
    pub hours_analyzed: Option<u64>,
    /// Total number of [`UsageEntry`] records processed.
    pub entries_processed: usize,
    /// Number of [`SessionBlock`]s created.
    pub blocks_created: usize,
    /// Number of rate-limit notifications detected.
    pub limits_detected: usize,
    /// Wall-clock seconds spent loading the JSONL files.
    pub load_time_seconds: f64,
    /// Wall-clock seconds spent building session blocks.
    pub transform_time_seconds: f64,
}

/// The complete output of [`analyze_usage`].
#[derive(Debug, Clone)]
pub struct AnalysisResult {
    /// Session blocks (may include gap blocks).
    pub blocks: Vec<SessionBlock>,
    /// Metadata about this analysis run.
    pub metadata: AnalysisMetadata,
    /// Total number of usage entries loaded.
    pub entries_count: usize,
    /// Sum of all token counts across all blocks.
    pub total_tokens: u64,
    /// Sum of all costs (USD) across all blocks.
    pub total_cost: f64,
}

// ── Public function ───────────────────────────────────────────────────────────

/// Run the full analysis pipeline.
///
/// 1. Load usage entries (and raw JSONL) from `data_path`.
/// 2. Build 5-hour session blocks via [`SessionAnalyzer`].
/// 3. Compute burn rates for active blocks.
/// 4. Detect limits and attach them to the matching blocks.
/// 5. Return an [`AnalysisResult`].
///
/// When `quick_start` is `true` and `hours_back` is `None`, the analysis is
/// limited to the last 24 hours for a faster startup experience.
pub fn analyze_usage(
    hours_back: Option<u64>,
    quick_start: bool,
    data_path: Option<&str>,
) -> AnalysisResult {
    // Apply quick-start override.
    let effective_hours = if quick_start && hours_back.is_none() {
        Some(24)
    } else {
        hours_back
    };

    // ── Step 1: Load entries ──────────────────────────────────────────────────
    let load_start = std::time::Instant::now();
    let (entries, raw_entries) = load_usage_entries(
        data_path,
        effective_hours,
        CostMode::Auto,
        true, // always include raw for limit detection
    );
    let load_time = load_start.elapsed().as_secs_f64();

    // ── Step 2: Build blocks ──────────────────────────────────────────────────
    let transform_start = std::time::Instant::now();
    let analyzer = SessionAnalyzer::new(5);
    let mut blocks = analyzer.transform_to_blocks(&entries);
    let transform_time = transform_start.elapsed().as_secs_f64();

    // ── Step 3: Burn rates ────────────────────────────────────────────────────
    process_burn_rates(&mut blocks);

    // ── Step 4: Limits ────────────────────────────────────────────────────────
    let mut limits_detected = 0usize;
    if let Some(raw) = &raw_entries {
        let detections = analyzer.detect_limits(raw);
        limits_detected = detections.len();
        assign_limits_to_blocks(&mut blocks, &detections);
    }

    // ── Step 5: Build result ──────────────────────────────────────────────────
    let total_tokens: u64 = blocks.iter().map(|b| b.total_tokens()).sum();
    let total_cost: f64 = blocks.iter().map(|b| b.cost_usd).sum();

    let metadata = AnalysisMetadata {
        generated_at: Utc::now().to_rfc3339(),
        hours_analyzed: effective_hours,
        entries_processed: entries.len(),
        blocks_created: blocks.len(),
        limits_detected,
        load_time_seconds: load_time,
        transform_time_seconds: transform_time,
    };

    AnalysisResult {
        blocks,
        metadata,
        entries_count: entries.len(),
        total_tokens,
        total_cost,
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Compute and attach burn rates (and projections) to every active block.
fn process_burn_rates(blocks: &mut [SessionBlock]) {
    for block in blocks.iter_mut() {
        if !block.is_active {
            continue;
        }
        if let Some(burn_rate) = BurnRateCalculator::calculate_burn_rate(block) {
            let projection = BurnRateCalculator::project_block_usage(
                &burn_rate,
                block.end_time,
                block.total_tokens(),
                block.cost_usd,
            );
            block.burn_rate_snapshot = Some(burn_rate);
            if let Some(proj) = projection {
                block.projection_data = Some(serde_json::json!({
                    "totalTokens": proj.projected_total_tokens,
                    "totalCost": proj.projected_total_cost,
                    "remainingMinutes": proj.remaining_minutes,
                }));
            }
        }
    }
}

/// Attach each [`LimitDetection`] to the [`SessionBlock`] whose time window
/// contains the limit's timestamp.
fn assign_limits_to_blocks(blocks: &mut [SessionBlock], detections: &[LimitDetection]) {
    for detection in detections {
        for block in blocks.iter_mut() {
            if block.is_gap {
                continue;
            }
            if block.start_time <= detection.timestamp && detection.timestamp <= block.end_time {
                block.limit_messages.push(LimitMessage {
                    limit_type: detection.limit_type.clone(),
                    timestamp: detection.timestamp.to_rfc3339(),
                    content: detection.content.clone(),
                    reset_time: detection.reset_time.map(|t| t.to_rfc3339()),
                });
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_jsonl(dir: &std::path::Path, name: &str, lines: &[&str]) {
        let path = dir.join(name);
        let mut file = std::fs::File::create(&path).unwrap();
        for line in lines {
            writeln!(file, "{}", line).unwrap();
        }
    }

    fn sample_entry(ts: &str, input: u64, output: u64, msg_id: &str, req_id: &str) -> String {
        serde_json::json!({
            "timestamp": ts,
            "input_tokens": input,
            "output_tokens": output,
            "model": "claude-3-5-sonnet-20241022",
            "message_id": msg_id,
            "requestId": req_id,
        })
        .to_string()
    }

    // ── analyze_usage ─────────────────────────────────────────────────────────

    #[test]
    fn test_analyze_usage_empty_directory() {
        let dir = TempDir::new().unwrap();
        let result = analyze_usage(None, false, Some(dir.path().to_str().unwrap()));

        assert!(result.blocks.is_empty());
        assert_eq!(result.entries_count, 0);
        assert_eq!(result.total_tokens, 0);
    }

    #[test]
    fn test_analyze_usage_basic_pipeline() {
        let dir = TempDir::new().unwrap();
        let line1 = sample_entry("2024-01-15T10:00:00Z", 100, 50, "msg1", "req1");
        let line2 = sample_entry("2024-01-15T11:00:00Z", 200, 100, "msg2", "req2");
        write_jsonl(dir.path(), "usage.jsonl", &[&line1, &line2]);

        let result = analyze_usage(None, false, Some(dir.path().to_str().unwrap()));

        assert_eq!(result.entries_count, 2);
        assert!(!result.blocks.is_empty());
        assert_eq!(result.total_tokens, 450); // 100+50+200+100
    }

    #[test]
    fn test_analyze_usage_quick_start_sets_24h() {
        let dir = TempDir::new().unwrap();
        // Write an old entry that should be filtered out with 24h cutoff.
        let old = sample_entry("2024-01-01T00:00:00Z", 100, 50, "msg-old", "req-old");
        // Write a recent entry (within 24 hours).
        let recent_ts = (Utc::now() - chrono::Duration::minutes(30))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let recent = sample_entry(&recent_ts, 200, 100, "msg-new", "req-new");
        write_jsonl(dir.path(), "usage.jsonl", &[&old, &recent]);

        let result = analyze_usage(None, true, Some(dir.path().to_str().unwrap()));

        // Only recent entry should be present due to 24h quick_start filter.
        assert_eq!(result.entries_count, 1);
        assert_eq!(result.metadata.hours_analyzed, Some(24));
    }

    #[test]
    fn test_analyze_usage_explicit_hours_back_not_overridden_by_quick_start() {
        let dir = TempDir::new().unwrap();
        let result = analyze_usage(Some(48), true, Some(dir.path().to_str().unwrap()));
        // hours_back = 48 should be preserved even with quick_start.
        assert_eq!(result.metadata.hours_analyzed, Some(48));
    }

    #[test]
    fn test_analyze_usage_metadata_fields_populated() {
        let dir = TempDir::new().unwrap();
        let line = sample_entry("2024-01-15T10:00:00Z", 100, 50, "msg1", "req1");
        write_jsonl(dir.path(), "usage.jsonl", &[&line]);

        let result = analyze_usage(None, false, Some(dir.path().to_str().unwrap()));

        assert!(!result.metadata.generated_at.is_empty());
        assert!(result.metadata.load_time_seconds >= 0.0);
        assert!(result.metadata.transform_time_seconds >= 0.0);
        assert_eq!(result.metadata.entries_processed, 1);
        assert_eq!(result.metadata.blocks_created, result.blocks.len());
    }

    #[test]
    fn test_analyze_usage_total_cost_sums_blocks() {
        let dir = TempDir::new().unwrap();
        let line1 = sample_entry("2024-01-15T10:00:00Z", 100, 50, "msg1", "req1");
        let line2 = sample_entry("2024-01-15T11:00:00Z", 200, 100, "msg2", "req2");
        write_jsonl(dir.path(), "usage.jsonl", &[&line1, &line2]);

        let result = analyze_usage(None, false, Some(dir.path().to_str().unwrap()));

        let expected: f64 = result.blocks.iter().map(|b| b.cost_usd).sum();
        assert!((result.total_cost - expected).abs() < 1e-9);
    }

    #[test]
    fn test_analyze_usage_limit_detection() {
        let dir = TempDir::new().unwrap();
        let usage = sample_entry("2024-01-15T10:00:00Z", 100, 50, "msg1", "req1");
        let limit = serde_json::json!({
            "type": "system",
            "timestamp": "2024-01-15T10:30:00Z",
            "content": "You have hit a rate limit. Please wait.",
        })
        .to_string();
        write_jsonl(dir.path(), "usage.jsonl", &[&usage, &limit]);

        let result = analyze_usage(None, false, Some(dir.path().to_str().unwrap()));
        assert_eq!(result.metadata.limits_detected, 1);
    }

    #[test]
    fn test_assign_limits_to_blocks_correct_block() {
        use chrono::TimeZone;
        use monitor_core::models::TokenCounts;
        use std::collections::HashMap;

        let block_start = Utc.with_ymd_and_hms(2024, 1, 15, 10, 0, 0).unwrap();
        let block_end = Utc.with_ymd_and_hms(2024, 1, 15, 15, 0, 0).unwrap();

        let block = SessionBlock {
            id: "test".to_string(),
            start_time: block_start,
            end_time: block_end,
            entries: vec![],
            token_counts: TokenCounts::default(),
            is_active: false,
            is_gap: false,
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

        let mut blocks = vec![block];
        assign_limits_to_blocks(
            &mut blocks,
            &[LimitDetection {
                limit_type: "system_limit".to_string(),
                timestamp: Utc.with_ymd_and_hms(2024, 1, 15, 10, 30, 0).unwrap(),
                content: "rate limit".to_string(),
                reset_time: None,
            }],
        );
        assert_eq!(blocks[0].limit_messages.len(), 1);
        assert_eq!(blocks[0].limit_messages[0].limit_type, "system_limit");
    }
}
