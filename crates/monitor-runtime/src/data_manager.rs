//! TTL-cached data manager for the monitoring runtime.
//!
//! Wraps [`analyze_usage`] with a configurable time-to-live cache and
//! transparent retry logic. Callers use [`DataManager::get_data`] to obtain
//! a fresh-or-cached [`AnalysisResult`]; the manager handles staleness checks,
//! up to three fetch attempts with exponential back-off, and graceful fallback
//! to the previous cache on transient failure.

use std::thread;
use std::time::{Duration, Instant};

use monitor_data::analysis::{analyze_usage, AnalysisResult};

// ── Defaults ──────────────────────────────────────────────────────────────────

/// Default cache TTL in seconds (mirrors Python's 30 s interval).
pub const DEFAULT_CACHE_TTL_SECS: u64 = 30;

/// Default look-back window in hours (8 days, matches Python's 192 h).
pub const DEFAULT_HOURS_BACK: u64 = 192;

/// Maximum number of fetch attempts before giving up and returning stale data.
const MAX_RETRY_ATTEMPTS: u32 = 3;

// ── DataManager ───────────────────────────────────────────────────────────────

/// TTL-cached wrapper around the full analysis pipeline.
///
/// # Example
/// ```no_run
/// use monitor_runtime::data_manager::DataManager;
///
/// let mut mgr = DataManager::new(30, 192, None);
/// if let Some(result) = mgr.get_data(false) {
///     println!("total tokens: {}", result.total_tokens);
/// }
/// ```
pub struct DataManager {
    /// Maximum age of cached data before it is considered stale.
    cache_ttl: Duration,
    /// Hours of history to analyse on each fresh fetch.
    hours_back: u64,
    /// Optional override for the JSONL data directory.
    data_path: Option<String>,
    /// Most recently fetched analysis result.
    cache: Option<AnalysisResult>,
    /// When the cache was last populated.
    cache_timestamp: Option<Instant>,
    /// Human-readable description of the last error encountered.
    last_error: Option<String>,
    /// When the last *successful* fetch completed.
    last_successful_fetch: Option<Instant>,
}

impl DataManager {
    /// Create a new manager.
    ///
    /// # Parameters
    /// - `cache_ttl_secs` – seconds before cached data is considered stale.
    /// - `hours_back`     – look-back window forwarded to `analyze_usage`.
    /// - `data_path`      – optional path override for JSONL discovery.
    pub fn new(cache_ttl_secs: u64, hours_back: u64, data_path: Option<String>) -> Self {
        Self {
            cache_ttl: Duration::from_secs(cache_ttl_secs),
            hours_back,
            data_path,
            cache: None,
            cache_timestamp: None,
            last_error: None,
            last_successful_fetch: None,
        }
    }

    // ── Public API ────────────────────────────────────────────────────────

    /// Return analysis data, using the cache when it is still valid.
    ///
    /// When `force_refresh` is `true` the cache is bypassed and a fresh fetch
    /// is always attempted. On fetch failure the previous cache (if any) is
    /// returned as a best-effort fallback.
    ///
    /// The fetch is retried up to [`MAX_RETRY_ATTEMPTS`] times with
    /// exponential back-off (0 ms → 100 ms → 200 ms).
    pub fn get_data(&mut self, force_refresh: bool) -> Option<&AnalysisResult> {
        if !force_refresh && self.is_cache_valid() {
            tracing::debug!("returning cached analysis result");
            return self.cache.as_ref();
        }

        match self.fetch_with_retry() {
            Ok(result) => {
                tracing::debug!(
                    entries = result.entries_count,
                    total_tokens = result.total_tokens,
                    "analysis cache updated"
                );
                self.cache = Some(result);
                self.cache_timestamp = Some(Instant::now());
                self.last_successful_fetch = Some(Instant::now());
                self.last_error = None;
                self.cache.as_ref()
            }
            Err(e) => {
                tracing::warn!(error = %e, "fetch failed; falling back to cached data");
                self.last_error = Some(e);
                // Return whatever we have, even if stale.
                self.cache.as_ref()
            }
        }
    }

    /// Discard the current cache, forcing the next [`get_data`] call to fetch.
    pub fn invalidate_cache(&mut self) {
        self.cache = None;
        self.cache_timestamp = None;
        tracing::debug!("cache invalidated");
    }

    /// Age of the current cache entry, or `None` if no data has been fetched.
    pub fn cache_age(&self) -> Option<Duration> {
        self.cache_timestamp.map(|ts| ts.elapsed())
    }

    /// Human-readable description of the last fetch error, or `None`.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    // ── Private helpers ───────────────────────────────────────────────────

    /// `true` when the cache holds data that is still within its TTL.
    fn is_cache_valid(&self) -> bool {
        match (self.cache.as_ref(), self.cache_timestamp) {
            (Some(_), Some(ts)) => ts.elapsed() < self.cache_ttl,
            _ => false,
        }
    }

    /// Attempt up to [`MAX_RETRY_ATTEMPTS`] fetches with exponential back-off.
    ///
    /// Back-off schedule: attempt 1 → 0 ms, attempt 2 → 100 ms, attempt 3 → 200 ms.
    fn fetch_with_retry(&mut self) -> Result<AnalysisResult, String> {
        let mut last_err = String::new();

        for attempt in 0..MAX_RETRY_ATTEMPTS {
            // Exponential back-off: 0, 100, 200 ms.
            if attempt > 0 {
                let sleep_ms = (attempt as u64) * 100;
                tracing::debug!(attempt, sleep_ms, "retrying fetch after back-off");
                thread::sleep(Duration::from_millis(sleep_ms));
            }

            match self.fetch_fresh() {
                Ok(result) => return Ok(result),
                Err(e) => {
                    tracing::warn!(attempt, error = %e, "fetch attempt failed");
                    last_err = e;
                }
            }
        }

        Err(last_err)
    }

    /// Call the analysis pipeline with this manager's configuration.
    fn fetch_fresh(&self) -> Result<AnalysisResult, String> {
        // analyze_usage is infallible by design; any I/O issues surface as
        // empty results rather than panics, so we wrap in a catch-unwind for
        // maximum robustness.
        let result = std::panic::catch_unwind(|| {
            analyze_usage(Some(self.hours_back), false, self.data_path.as_deref())
        })
        .map_err(|e| {
            format!(
                "analyze_usage panicked: {:?}",
                e.downcast_ref::<&str>().unwrap_or(&"unknown panic")
            )
        })?;

        Ok(result)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Helper: create a DataManager pointing at a non-existent path so that
    /// `analyze_usage` returns an empty result quickly (no I/O errors, just
    /// empty blocks). We use a temp dir that is immediately dropped so it is
    /// guaranteed empty.
    fn make_manager(ttl_secs: u64) -> DataManager {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().to_str().unwrap().to_string();
        // Keep `dir` alive until after the manager is constructed by passing
        // the path string; the directory will be dropped when `dir` goes out
        // of scope at the end of each test, but the string path is already
        // captured.
        let mgr = DataManager::new(ttl_secs, 24, Some(path));
        // Intentionally *not* holding `dir` here — the directory exists for
        // the duration of the test function's stack frame via the caller.
        // Actually we need the dir to stay alive, so return it too.
        // We work around this by using a path we control explicitly below.
        drop(dir);
        mgr
    }

    /// Returns a DataManager + TempDir.  The TempDir MUST be kept alive for
    /// the duration of the test (otherwise the directory is deleted before
    /// analyze_usage runs).
    fn make_manager_with_dir(ttl_secs: u64) -> (DataManager, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let path = dir.path().to_str().unwrap().to_string();
        let mgr = DataManager::new(ttl_secs, 24, Some(path));
        (mgr, dir)
    }

    // ── cache miss on first call ──────────────────────────────────────────

    #[test]
    fn test_cache_miss_on_first_call() {
        let (mut mgr, _dir) = make_manager_with_dir(30);

        // No cache yet.
        assert!(!mgr.is_cache_valid());
        assert!(mgr.cache_age().is_none());
        assert!(mgr.last_error().is_none());
    }

    // ── cache valid within TTL ────────────────────────────────────────────

    #[test]
    fn test_cache_valid_within_ttl() {
        let (mut mgr, _dir) = make_manager_with_dir(30);

        // First call: populates the cache.
        let first = mgr.get_data(false);
        assert!(first.is_some());

        // Snapshot the entry count from the first call.
        let first_entries = first.map(|r| r.entries_count);

        // Second call within TTL: should return the cached value (not a fresh fetch).
        let second = mgr.get_data(false);
        assert_eq!(second.map(|r| r.entries_count), first_entries);

        // Cache age should be very small (sub-second in a test).
        let age = mgr.cache_age().expect("cache age is Some after population");
        assert!(age < Duration::from_secs(5));
    }

    // ── cache expired after TTL ───────────────────────────────────────────

    #[test]
    fn test_cache_expired() {
        // TTL of 0 means the cache expires immediately.
        let (mut mgr, _dir) = make_manager_with_dir(0);

        // Populate cache.
        mgr.get_data(false);
        assert!(mgr.cache.is_some());

        // With TTL=0 the cache is always considered stale.
        assert!(!mgr.is_cache_valid());

        // Next call should trigger a fresh fetch.
        let result = mgr.get_data(false);
        assert!(result.is_some());
    }

    // ── manual cache invalidation ─────────────────────────────────────────

    #[test]
    fn test_invalidate_cache() {
        let (mut mgr, _dir) = make_manager_with_dir(30);

        mgr.get_data(false);
        assert!(mgr.cache.is_some());
        assert!(mgr.cache_timestamp.is_some());

        mgr.invalidate_cache();
        assert!(mgr.cache.is_none());
        assert!(mgr.cache_timestamp.is_none());
        assert!(mgr.cache_age().is_none());
    }

    // ── cache_age returns correct duration ────────────────────────────────

    #[test]
    fn test_cache_age() {
        let (mut mgr, _dir) = make_manager_with_dir(30);

        assert!(mgr.cache_age().is_none());

        mgr.get_data(false);

        let age = mgr.cache_age().expect("age is Some after first fetch");
        // Should be very small in a test environment.
        assert!(age < Duration::from_secs(5));
    }

    // ── force_refresh bypasses valid cache ────────────────────────────────

    #[test]
    fn test_force_refresh_bypasses_cache() {
        let (mut mgr, _dir) = make_manager_with_dir(60);

        mgr.get_data(false);
        let ts1 = mgr.cache_timestamp.unwrap();

        // Sleep briefly to ensure timestamps differ.
        thread::sleep(Duration::from_millis(10));

        mgr.get_data(true);
        let ts2 = mgr.cache_timestamp.unwrap();

        // Cache timestamp must have been updated.
        assert!(ts2 > ts1);
    }

    // ── last_error is None on success ─────────────────────────────────────

    #[test]
    fn test_no_error_on_success() {
        let (mut mgr, _dir) = make_manager_with_dir(30);
        mgr.get_data(false);
        assert!(mgr.last_error().is_none());
    }

    // ── make_manager (drop-dir variant) still constructs OK ───────────────

    #[test]
    fn test_make_manager_constructs() {
        let mgr = make_manager(30);
        assert!(mgr.cache.is_none());
        assert_eq!(mgr.hours_back, 24);
        assert_eq!(mgr.cache_ttl, Duration::from_secs(30));
    }
}
