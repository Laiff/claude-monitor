//! JSONL file discovery and loading for Claude Monitor.
//!
//! Reads usage records produced by the Claude CLI from `~/.claude/projects/`
//! and converts them into [`UsageEntry`] structs for downstream processing.

use std::collections::HashSet;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use monitor_core::data_processors::{DataConverter, TimestampProcessor, TokenExtractor};
use monitor_core::models::{CostMode, UsageEntry};
use monitor_core::pricing::PricingCalculator;
use tracing::{debug, warn};

// ── Public API ────────────────────────────────────────────────────────────────

/// Find all `.jsonl` files recursively under `data_path`, sorted by path.
pub fn find_jsonl_files(data_path: &Path) -> Vec<PathBuf> {
    if !data_path.exists() {
        warn!("Data path does not exist: {}", data_path.display());
        return Vec::new();
    }

    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(data_path)
        .follow_links(true)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.file_type().is_file()
                && entry
                    .path()
                    .extension()
                    .map(|ext| ext == "jsonl")
                    .unwrap_or(false)
        })
        .map(|entry| entry.into_path())
        .collect();

    files.sort();
    files
}

/// Load and parse JSONL files into [`UsageEntry`] objects.
///
/// * `data_path` – directory to scan (defaults to `~/.claude/projects`).
/// * `hours_back` – when set, only entries within the last N hours are kept.
/// * `mode` – how to compute the USD cost for each entry.
/// * `include_raw` – when `true`, the raw [`serde_json::Value`] for every
///   processed line is returned alongside the typed entries.
///
/// Returns `(entries, raw_entries)`.  `raw_entries` is `None` when
/// `include_raw` is `false`.
pub fn load_usage_entries(
    data_path: Option<&str>,
    hours_back: Option<u64>,
    mode: CostMode,
    include_raw: bool,
) -> (Vec<UsageEntry>, Option<Vec<serde_json::Value>>) {
    let path = resolve_data_path(data_path);
    let mut pricing = PricingCalculator::new(None);

    let cutoff_time: Option<DateTime<Utc>> =
        hours_back.map(|h| Utc::now() - chrono::Duration::hours(h as i64));

    let jsonl_files = find_jsonl_files(&path);
    if jsonl_files.is_empty() {
        warn!("No JSONL files found in {}", path.display());
        return (Vec::new(), None);
    }

    let mut all_entries: Vec<UsageEntry> = Vec::new();
    let mut raw_entries: Option<Vec<serde_json::Value>> =
        if include_raw { Some(Vec::new()) } else { None };
    let mut processed_hashes: HashSet<String> = HashSet::new();

    for file_path in &jsonl_files {
        let (entries, raw_data) = process_single_file(
            file_path,
            mode.clone(),
            cutoff_time,
            &mut processed_hashes,
            include_raw,
            &mut pricing,
        );
        all_entries.extend(entries);
        if include_raw {
            if let (Some(dest), Some(src)) = (raw_entries.as_mut(), raw_data) {
                dest.extend(src);
            }
        }
    }

    all_entries.sort_by_key(|e| e.timestamp);

    debug!(
        "Processed {} entries from {} files",
        all_entries.len(),
        jsonl_files.len()
    );

    (all_entries, raw_entries)
}

/// Load all raw JSONL entries without any filtering or type mapping.
///
/// Useful for limit-detection downstream which needs the full raw data.
pub fn load_all_raw_entries(data_path: Option<&str>) -> Vec<serde_json::Value> {
    let path = resolve_data_path(data_path);
    let jsonl_files = find_jsonl_files(&path);

    let mut all_raw: Vec<serde_json::Value> = Vec::new();

    for file_path in &jsonl_files {
        match std::fs::File::open(file_path) {
            Ok(file) => {
                let reader = std::io::BufReader::new(file);
                for line in reader.lines() {
                    let line = match line {
                        Ok(l) => l,
                        Err(_) => continue,
                    };
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    match serde_json::from_str(trimmed) {
                        Ok(value) => all_raw.push(value),
                        Err(_) => continue,
                    }
                }
            }
            Err(e) => {
                warn!(
                    "Error loading raw entries from {}: {}",
                    file_path.display(),
                    e
                );
            }
        }
    }

    all_raw
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Resolve the data path: use `data_path` when given, otherwise fall back
/// to `~/.claude/projects` via the `HOME` environment variable or the
/// platform home dir.
fn resolve_data_path(data_path: Option<&str>) -> PathBuf {
    if let Some(p) = data_path {
        return PathBuf::from(p);
    }

    // Use dirs::home_dir() for cross-platform home detection.
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".claude").join("projects")
}

/// Process a single JSONL file and return parsed entries plus optional raw
/// JSON values.
fn process_single_file(
    file_path: &Path,
    mode: CostMode,
    cutoff: Option<DateTime<Utc>>,
    hashes: &mut HashSet<String>,
    include_raw: bool,
    pricing: &mut PricingCalculator,
) -> (Vec<UsageEntry>, Option<Vec<serde_json::Value>>) {
    let mut entries: Vec<UsageEntry> = Vec::new();
    let mut raw_data: Option<Vec<serde_json::Value>> =
        if include_raw { Some(Vec::new()) } else { None };

    let file = match std::fs::File::open(file_path) {
        Ok(f) => f,
        Err(e) => {
            warn!("Failed to read file {}: {}", file_path.display(), e);
            return (Vec::new(), None);
        }
    };

    let reader = std::io::BufReader::new(file);
    let mut entries_read = 0u64;
    let mut entries_filtered = 0u64;
    let mut entries_mapped = 0u64;

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let data: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                debug!(
                    "Failed to parse JSON line in {}: {}",
                    file_path.display(),
                    e
                );
                continue;
            }
        };

        entries_read += 1;

        if !should_process_entry(&data, cutoff, hashes) {
            entries_filtered += 1;
            continue;
        }

        if let Some(entry) = map_to_usage_entry(&data, mode.clone(), pricing) {
            entries_mapped += 1;
            entries.push(entry);
            // Register hash so duplicate lines are skipped.
            if let Some(h) = create_unique_hash(&data) {
                hashes.insert(h);
            }
        }

        if include_raw {
            if let Some(dest) = raw_data.as_mut() {
                dest.push(data);
            }
        }
    }

    debug!(
        "File {}: {} read, {} filtered, {} mapped",
        file_path.display(),
        entries_read,
        entries_filtered,
        entries_mapped,
    );

    (entries, raw_data)
}

/// Returns `true` when the entry should be processed.
///
/// An entry is skipped when:
/// * It has a timestamp older than the cutoff.
/// * Its unique hash (message_id:request_id) was already seen.
fn should_process_entry(
    data: &serde_json::Value,
    cutoff: Option<DateTime<Utc>>,
    hashes: &HashSet<String>,
) -> bool {
    // Time filter.
    if let Some(cutoff_ts) = cutoff {
        if let Some(ts_value) = data.get("timestamp") {
            if let Some(ts) = TimestampProcessor::parse(ts_value) {
                if ts < cutoff_ts {
                    return false;
                }
            }
        }
    }

    // Deduplication filter.
    if let Some(h) = create_unique_hash(data) {
        if hashes.contains(&h) {
            return false;
        }
    }

    true
}

/// Build the deduplication hash `"{message_id}:{request_id}"`.
///
/// Returns `None` when either component is absent.
fn create_unique_hash(data: &serde_json::Value) -> Option<String> {
    // message_id: try "message_id", then "message.id"
    let message_id = data
        .get("message_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            data.get("message")
                .and_then(|m| m.get("id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });

    // request_id: try "requestId", then "request_id"
    let request_id = data
        .get("requestId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            data.get("request_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        });

    match (message_id, request_id) {
        (Some(mid), Some(rid)) => Some(format!("{}:{}", mid, rid)),
        _ => None,
    }
}

/// Map a raw JSON value to a [`UsageEntry`], returning `None` on failure.
fn map_to_usage_entry(
    data: &serde_json::Value,
    mode: CostMode,
    pricing: &mut PricingCalculator,
) -> Option<UsageEntry> {
    // Require a valid timestamp.
    let ts_value = data.get("timestamp")?;
    let timestamp = TimestampProcessor::parse(ts_value)?;

    // Require at least some token counts.
    let tokens = TokenExtractor::extract(data);
    if tokens.input_tokens == 0 && tokens.output_tokens == 0 {
        return None;
    }

    let model = DataConverter::extract_model_name(data);

    // Build a normalised entry map for the pricing calculator.
    let entry_for_pricing = serde_json::json!({
        "model": model,
        "input_tokens": tokens.input_tokens,
        "output_tokens": tokens.output_tokens,
        "cache_creation_input_tokens": tokens.cache_creation_input_tokens,
        "cache_read_input_tokens": tokens.cache_read_input_tokens,
        "costUSD": data.get("costUSD").cloned().unwrap_or(serde_json::Value::Null),
        "cost_usd": data.get("cost_usd").cloned().unwrap_or(serde_json::Value::Null),
    });
    let cost_usd = pricing.calculate_cost_for_entry(&entry_for_pricing, mode);

    // Extract IDs.
    let message_id = data
        .get("message_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            data.get("message")
                .and_then(|m| m.get("id"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_default();

    let request_id = data
        .get("requestId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            data.get("request_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());

    Some(UsageEntry {
        timestamp,
        input_tokens: tokens.input_tokens,
        output_tokens: tokens.output_tokens,
        cache_creation_tokens: tokens.cache_creation_input_tokens,
        cache_read_tokens: tokens.cache_read_input_tokens,
        cost_usd,
        model,
        message_id,
        request_id,
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn write_jsonl(dir: &Path, name: &str, lines: &[&str]) -> PathBuf {
        let path = dir.join(name);
        let mut file = std::fs::File::create(&path).unwrap();
        for line in lines {
            writeln!(file, "{}", line).unwrap();
        }
        path
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

    // ── find_jsonl_files ──────────────────────────────────────────────────────

    #[test]
    fn test_find_jsonl_files_in_flat_dir() {
        let dir = TempDir::new().unwrap();
        write_jsonl(dir.path(), "a.jsonl", &["line"]);
        write_jsonl(dir.path(), "b.jsonl", &["line"]);

        let files = find_jsonl_files(dir.path());
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|p| p.extension().unwrap() == "jsonl"));
    }

    #[test]
    fn test_find_jsonl_files_recursive() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("project-abc");
        std::fs::create_dir_all(&sub).unwrap();
        write_jsonl(dir.path(), "root.jsonl", &["line"]);
        write_jsonl(&sub, "nested.jsonl", &["line"]);

        let files = find_jsonl_files(dir.path());
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_find_jsonl_files_nonexistent_path() {
        let files = find_jsonl_files(Path::new("/tmp/does-not-exist-monitor-test-xyz"));
        assert!(files.is_empty());
    }

    #[test]
    fn test_find_jsonl_files_sorted() {
        let dir = TempDir::new().unwrap();
        write_jsonl(dir.path(), "c.jsonl", &["x"]);
        write_jsonl(dir.path(), "a.jsonl", &["x"]);
        write_jsonl(dir.path(), "b.jsonl", &["x"]);

        let files = find_jsonl_files(dir.path());
        let names: Vec<&str> = files
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, vec!["a.jsonl", "b.jsonl", "c.jsonl"]);
    }

    // ── load_usage_entries ────────────────────────────────────────────────────

    #[test]
    fn test_load_usage_entries_basic() {
        let dir = TempDir::new().unwrap();
        let line = sample_entry("2024-01-15T10:00:00Z", 100, 50, "msg1", "req1");
        write_jsonl(dir.path(), "usage.jsonl", &[&line]);

        let (entries, raw) = load_usage_entries(
            Some(dir.path().to_str().unwrap()),
            None,
            CostMode::Auto,
            false,
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].input_tokens, 100);
        assert_eq!(entries[0].output_tokens, 50);
        assert!(raw.is_none());
    }

    #[test]
    fn test_load_usage_entries_include_raw() {
        let dir = TempDir::new().unwrap();
        let line = sample_entry("2024-01-15T10:00:00Z", 100, 50, "msg1", "req1");
        write_jsonl(dir.path(), "usage.jsonl", &[&line]);

        let (entries, raw) = load_usage_entries(
            Some(dir.path().to_str().unwrap()),
            None,
            CostMode::Auto,
            true,
        );

        assert_eq!(entries.len(), 1);
        assert!(raw.is_some());
        assert_eq!(raw.unwrap().len(), 1);
    }

    #[test]
    fn test_load_usage_entries_deduplication() {
        let dir = TempDir::new().unwrap();
        // Same message_id:request_id pair written twice.
        let line = sample_entry("2024-01-15T10:00:00Z", 100, 50, "msg1", "req1");
        write_jsonl(dir.path(), "usage.jsonl", &[&line, &line]);

        let (entries, _) = load_usage_entries(
            Some(dir.path().to_str().unwrap()),
            None,
            CostMode::Auto,
            false,
        );

        // Second duplicate must be dropped.
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_load_usage_entries_hours_back_filter() {
        let dir = TempDir::new().unwrap();

        // Old entry (2024 – definitely beyond any hours_back window).
        let old = sample_entry("2024-01-01T00:00:00Z", 10, 5, "msg-old", "req-old");
        // Recent entry: 1 minute ago.
        let recent_ts = (Utc::now() - chrono::Duration::minutes(1))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();
        let recent = sample_entry(&recent_ts, 200, 100, "msg-new", "req-new");
        write_jsonl(dir.path(), "usage.jsonl", &[&old, &recent]);

        let (entries, _) = load_usage_entries(
            Some(dir.path().to_str().unwrap()),
            Some(24), // last 24 hours
            CostMode::Auto,
            false,
        );

        // Only the recent entry should pass the filter.
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].input_tokens, 200);
    }

    #[test]
    fn test_load_usage_entries_malformed_lines_skipped() {
        let dir = TempDir::new().unwrap();
        let good = sample_entry("2024-01-15T10:00:00Z", 100, 50, "msg1", "req1");
        write_jsonl(dir.path(), "usage.jsonl", &["{not valid json{{", &good, ""]);

        let (entries, _) = load_usage_entries(
            Some(dir.path().to_str().unwrap()),
            None,
            CostMode::Auto,
            false,
        );

        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_load_usage_entries_sorted_by_timestamp() {
        let dir = TempDir::new().unwrap();
        let later = sample_entry("2024-01-15T12:00:00Z", 200, 100, "msg2", "req2");
        let earlier = sample_entry("2024-01-15T08:00:00Z", 100, 50, "msg1", "req1");
        // Write later first, earlier second.
        write_jsonl(dir.path(), "usage.jsonl", &[&later, &earlier]);

        let (entries, _) = load_usage_entries(
            Some(dir.path().to_str().unwrap()),
            None,
            CostMode::Auto,
            false,
        );

        assert_eq!(entries.len(), 2);
        assert!(entries[0].timestamp < entries[1].timestamp);
    }

    #[test]
    fn test_load_usage_entries_empty_directory() {
        let dir = TempDir::new().unwrap();
        let (entries, raw) = load_usage_entries(
            Some(dir.path().to_str().unwrap()),
            None,
            CostMode::Auto,
            false,
        );
        assert!(entries.is_empty());
        assert!(raw.is_none());
    }

    // ── load_all_raw_entries ──────────────────────────────────────────────────

    #[test]
    fn test_load_all_raw_entries() {
        let dir = TempDir::new().unwrap();
        let line1 = sample_entry("2024-01-15T10:00:00Z", 100, 50, "msg1", "req1");
        let line2 = sample_entry("2024-01-15T11:00:00Z", 200, 100, "msg2", "req2");
        write_jsonl(dir.path(), "usage.jsonl", &[&line1, &line2]);

        let raw = load_all_raw_entries(Some(dir.path().to_str().unwrap()));
        assert_eq!(raw.len(), 2);
    }

    #[test]
    fn test_load_all_raw_entries_skips_malformed() {
        let dir = TempDir::new().unwrap();
        let good = sample_entry("2024-01-15T10:00:00Z", 100, 50, "msg1", "req1");
        write_jsonl(dir.path(), "usage.jsonl", &["{bad", &good]);

        let raw = load_all_raw_entries(Some(dir.path().to_str().unwrap()));
        assert_eq!(raw.len(), 1);
    }

    // ── create_unique_hash ────────────────────────────────────────────────────

    #[test]
    fn test_create_unique_hash_present() {
        let data = serde_json::json!({
            "message_id": "abc",
            "requestId": "xyz",
        });
        let hash = create_unique_hash(&data).unwrap();
        assert_eq!(hash, "abc:xyz");
    }

    #[test]
    fn test_create_unique_hash_nested_message_id() {
        let data = serde_json::json!({
            "message": {"id": "nested-id"},
            "requestId": "req-xyz",
        });
        let hash = create_unique_hash(&data).unwrap();
        assert_eq!(hash, "nested-id:req-xyz");
    }

    #[test]
    fn test_create_unique_hash_missing_returns_none() {
        let data = serde_json::json!({"other": "field"});
        assert!(create_unique_hash(&data).is_none());
    }

    #[test]
    fn test_create_unique_hash_request_id_snake_case() {
        let data = serde_json::json!({
            "message_id": "mid",
            "request_id": "rid",
        });
        let hash = create_unique_hash(&data).unwrap();
        assert_eq!(hash, "mid:rid");
    }
}
