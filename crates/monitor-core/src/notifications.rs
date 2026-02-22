//! Notification state management for Claude Monitor.
//!
//! Ports the Python `NotificationManager` class from
//! `src/claude_monitor/utils/notifications.py`.
//!
//! States are persisted to `~/.claude-monitor/notification_states.json` and
//! use a cooldown-based scheme so that notifications are not shown more often
//! than once per `cooldown_hours`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Notification keys ─────────────────────────────────────────────────────────

/// The three canonical notification keys recognised by the manager.
pub const KEY_SWITCH_TO_CUSTOM: &str = "switch_to_custom";
pub const KEY_EXCEED_MAX_LIMIT: &str = "exceed_max_limit";
pub const KEY_TOKENS_WILL_RUN_OUT: &str = "tokens_will_run_out";

// ── NotificationState ─────────────────────────────────────────────────────────

/// Persisted state for a single notification.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotificationState {
    /// Whether this notification has been triggered at least once.
    pub triggered: bool,
    /// UTC timestamp of the last trigger, or `None` if never triggered.
    pub timestamp: Option<DateTime<Utc>>,
}

impl NotificationState {
    /// Create a fresh, untriggered state.
    pub fn default_state() -> Self {
        Self {
            triggered: false,
            timestamp: None,
        }
    }
}

impl Default for NotificationState {
    fn default() -> Self {
        Self::default_state()
    }
}

// ── NotificationManager ───────────────────────────────────────────────────────

/// Manages notification states with cooldown-based suppression.
///
/// States are loaded from and persisted to a JSON file on disk so that
/// cooldown windows survive process restarts.
///
/// # Example
///
/// ```no_run
/// use monitor_core::notifications::NotificationManager;
/// use std::path::Path;
///
/// let mut mgr = NotificationManager::new(Path::new("/tmp/test-notifications"));
/// if mgr.should_notify("my_key", 24.0) {
///     mgr.mark_notified("my_key");
/// }
/// ```
pub struct NotificationManager {
    /// Path to the JSON file that stores notification states.
    notification_file: PathBuf,
    /// In-memory map of notification key → state.
    states: HashMap<String, NotificationState>,
}

impl NotificationManager {
    // ── Construction ──────────────────────────────────────────────────────────

    /// Create a `NotificationManager` that persists state to `config_dir`.
    ///
    /// If `config_dir` does not exist the manager still works in memory only;
    /// save errors are logged as warnings but never panic.
    pub fn new(config_dir: &Path) -> Self {
        let notification_file = config_dir.join("notification_states.json");
        let states = Self::load_states(&notification_file);
        Self {
            notification_file,
            states,
        }
    }

    /// Create a `NotificationManager` using the default `~/.claude-monitor/`
    /// config directory.
    ///
    /// Returns `None` when the home directory cannot be determined.
    pub fn with_default_path() -> Option<Self> {
        let config_dir = dirs::home_dir()?.join(".claude-monitor");
        Some(Self::new(&config_dir))
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Return `true` when the notification identified by `key` should fire.
    ///
    /// A notification should fire when:
    /// - `key` has not been seen before, **or**
    /// - it has never been triggered, **or**
    /// - its timestamp is absent, **or**
    /// - more than `cooldown_hours` have elapsed since the last trigger.
    pub fn should_notify(&mut self, key: &str, cooldown_hours: f64) -> bool {
        let state = self
            .states
            .entry(key.to_string())
            .or_insert_with(NotificationState::default_state);

        if !state.triggered {
            return true;
        }

        match state.timestamp {
            None => true,
            Some(ts) => {
                let elapsed_secs = (Utc::now() - ts).num_seconds() as f64;
                let cooldown_secs = cooldown_hours * 3600.0;
                elapsed_secs >= cooldown_secs
            }
        }
    }

    /// Mark the notification identified by `key` as triggered right now.
    ///
    /// Persists the updated states to disk.
    pub fn mark_notified(&mut self, key: &str) {
        self.states.insert(
            key.to_string(),
            NotificationState {
                triggered: true,
                timestamp: Some(Utc::now()),
            },
        );
        self.save_states();
    }

    /// Return `true` when `key` is in the triggered state **and** has a
    /// non-`None` timestamp (i.e., `mark_notified` was called at least once).
    pub fn is_notification_active(&self, key: &str) -> bool {
        match self.states.get(key) {
            None => false,
            Some(state) => state.triggered && state.timestamp.is_some(),
        }
    }

    /// Return a reference to the current state for `key`, or a default
    /// untriggered state when the key has not been seen.
    pub fn get_notification_state(&self, key: &str) -> NotificationState {
        self.states
            .get(key)
            .cloned()
            .unwrap_or_else(NotificationState::default_state)
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Load states from `path`, returning the three default keys on any error.
    fn load_states(path: &Path) -> HashMap<String, NotificationState> {
        if !path.exists() {
            return Self::default_states();
        }

        match std::fs::read_to_string(path) {
            Ok(content) => {
                match serde_json::from_str::<HashMap<String, NotificationState>>(&content) {
                    Ok(states) => states,
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            path = %path.display(),
                            "failed to deserialise notification states; using defaults"
                        );
                        Self::default_states()
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    path = %path.display(),
                    "failed to read notification states file; using defaults"
                );
                Self::default_states()
            }
        }
    }

    /// Persist the current in-memory states to disk.
    ///
    /// Errors are logged but never propagated to the caller.
    fn save_states(&self) {
        match serde_json::to_string_pretty(&self.states) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&self.notification_file, &json) {
                    tracing::warn!(
                        error = %e,
                        path = %self.notification_file.display(),
                        "failed to save notification states"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to serialise notification states");
            }
        }
    }

    /// Build the default three-key state map (all untriggered).
    fn default_states() -> HashMap<String, NotificationState> {
        let mut map = HashMap::new();
        map.insert(
            KEY_SWITCH_TO_CUSTOM.to_string(),
            NotificationState::default_state(),
        );
        map.insert(
            KEY_EXCEED_MAX_LIMIT.to_string(),
            NotificationState::default_state(),
        );
        map.insert(
            KEY_TOKENS_WILL_RUN_OUT.to_string(),
            NotificationState::default_state(),
        );
        map
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── helpers ───────────────────────────────────────────────────────────────

    fn make_mgr(dir: &TempDir) -> NotificationManager {
        NotificationManager::new(dir.path())
    }

    // ── NotificationState ──────────────────────────────────────────────────────

    #[test]
    fn test_notification_state_default_is_untriggered() {
        let state = NotificationState::default_state();
        assert!(!state.triggered);
        assert!(state.timestamp.is_none());
    }

    #[test]
    fn test_notification_state_default_trait() {
        let state = NotificationState::default();
        assert!(!state.triggered);
        assert!(state.timestamp.is_none());
    }

    #[test]
    fn test_notification_state_serialise_round_trip() {
        let state = NotificationState {
            triggered: true,
            timestamp: Some(Utc::now()),
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: NotificationState = serde_json::from_str(&json).unwrap();
        assert!(back.triggered);
        assert!(back.timestamp.is_some());
    }

    #[test]
    fn test_notification_state_round_trip_none_timestamp() {
        let state = NotificationState {
            triggered: false,
            timestamp: None,
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: NotificationState = serde_json::from_str(&json).unwrap();
        assert!(!back.triggered);
        assert!(back.timestamp.is_none());
    }

    // ── NotificationManager construction ─────────────────────────────────────

    #[test]
    fn test_manager_new_creates_default_keys() {
        let dir = TempDir::new().unwrap();
        let mgr = make_mgr(&dir);

        // All three canonical keys should exist after construction.
        assert!(!mgr.is_notification_active(KEY_SWITCH_TO_CUSTOM));
        assert!(!mgr.is_notification_active(KEY_EXCEED_MAX_LIMIT));
        assert!(!mgr.is_notification_active(KEY_TOKENS_WILL_RUN_OUT));
    }

    #[test]
    fn test_manager_new_no_state_file() {
        // Dir that definitely has no state file.
        let dir = TempDir::new().unwrap();
        let mgr = make_mgr(&dir);
        // should_notify returns true for an untriggered key
        let mut mgr = mgr;
        assert!(mgr.should_notify(KEY_SWITCH_TO_CUSTOM, 24.0));
    }

    // ── should_notify ─────────────────────────────────────────────────────────

    #[test]
    fn test_should_notify_unknown_key_returns_true() {
        let dir = TempDir::new().unwrap();
        let mut mgr = make_mgr(&dir);
        assert!(mgr.should_notify("completely_new_key", 24.0));
    }

    #[test]
    fn test_should_notify_untriggered_returns_true() {
        let dir = TempDir::new().unwrap();
        let mut mgr = make_mgr(&dir);
        // All canonical keys start untriggered.
        assert!(mgr.should_notify(KEY_EXCEED_MAX_LIMIT, 24.0));
    }

    #[test]
    fn test_should_notify_triggered_within_cooldown_returns_false() {
        let dir = TempDir::new().unwrap();
        let mut mgr = make_mgr(&dir);

        // Trigger the notification now.
        mgr.mark_notified(KEY_TOKENS_WILL_RUN_OUT);

        // With a 24-hour cooldown, it must not fire again immediately.
        assert!(!mgr.should_notify(KEY_TOKENS_WILL_RUN_OUT, 24.0));
    }

    #[test]
    fn test_should_notify_triggered_zero_cooldown_returns_true() {
        let dir = TempDir::new().unwrap();
        let mut mgr = make_mgr(&dir);

        mgr.mark_notified(KEY_SWITCH_TO_CUSTOM);

        // Zero cooldown means it always fires after the first trigger.
        assert!(mgr.should_notify(KEY_SWITCH_TO_CUSTOM, 0.0));
    }

    #[test]
    fn test_should_notify_triggered_with_old_timestamp_returns_true() {
        let dir = TempDir::new().unwrap();
        let mut mgr = make_mgr(&dir);

        // Manually insert a state with a very old timestamp (48 h ago).
        let old_ts = Utc::now() - chrono::Duration::hours(48);
        mgr.states.insert(
            KEY_EXCEED_MAX_LIMIT.to_string(),
            NotificationState {
                triggered: true,
                timestamp: Some(old_ts),
            },
        );

        // 24 h cooldown → old timestamp should allow another notification.
        assert!(mgr.should_notify(KEY_EXCEED_MAX_LIMIT, 24.0));
    }

    #[test]
    fn test_should_notify_triggered_recent_timestamp_returns_false() {
        let dir = TempDir::new().unwrap();
        let mut mgr = make_mgr(&dir);

        // 1 minute ago.
        let recent_ts = Utc::now() - chrono::Duration::minutes(1);
        mgr.states.insert(
            KEY_EXCEED_MAX_LIMIT.to_string(),
            NotificationState {
                triggered: true,
                timestamp: Some(recent_ts),
            },
        );

        assert!(!mgr.should_notify(KEY_EXCEED_MAX_LIMIT, 24.0));
    }

    // ── mark_notified ─────────────────────────────────────────────────────────

    #[test]
    fn test_mark_notified_sets_triggered_and_timestamp() {
        let dir = TempDir::new().unwrap();
        let mut mgr = make_mgr(&dir);

        mgr.mark_notified(KEY_SWITCH_TO_CUSTOM);

        let state = mgr.get_notification_state(KEY_SWITCH_TO_CUSTOM);
        assert!(state.triggered);
        assert!(state.timestamp.is_some());
    }

    #[test]
    fn test_mark_notified_persists_to_file() {
        let dir = TempDir::new().unwrap();
        {
            let mut mgr = make_mgr(&dir);
            mgr.mark_notified(KEY_SWITCH_TO_CUSTOM);
        } // mgr dropped here

        // Re-load from the same directory.
        let mgr2 = make_mgr(&dir);
        assert!(mgr2.is_notification_active(KEY_SWITCH_TO_CUSTOM));
    }

    #[test]
    fn test_mark_notified_arbitrary_key() {
        let dir = TempDir::new().unwrap();
        let mut mgr = make_mgr(&dir);

        mgr.mark_notified("custom_key");
        assert!(mgr.is_notification_active("custom_key"));
    }

    // ── is_notification_active ────────────────────────────────────────────────

    #[test]
    fn test_is_notification_active_false_when_not_triggered() {
        let dir = TempDir::new().unwrap();
        let mgr = make_mgr(&dir);
        assert!(!mgr.is_notification_active(KEY_SWITCH_TO_CUSTOM));
    }

    #[test]
    fn test_is_notification_active_false_for_unknown_key() {
        let dir = TempDir::new().unwrap();
        let mgr = make_mgr(&dir);
        assert!(!mgr.is_notification_active("nonexistent"));
    }

    #[test]
    fn test_is_notification_active_true_after_mark_notified() {
        let dir = TempDir::new().unwrap();
        let mut mgr = make_mgr(&dir);
        mgr.mark_notified(KEY_EXCEED_MAX_LIMIT);
        assert!(mgr.is_notification_active(KEY_EXCEED_MAX_LIMIT));
    }

    #[test]
    fn test_is_notification_active_false_when_triggered_but_no_timestamp() {
        let dir = TempDir::new().unwrap();
        let mut mgr = make_mgr(&dir);

        // Manually insert a triggered-but-no-timestamp state.
        mgr.states.insert(
            "edge_case".to_string(),
            NotificationState {
                triggered: true,
                timestamp: None,
            },
        );
        // is_notification_active requires both triggered=true AND timestamp!=None.
        assert!(!mgr.is_notification_active("edge_case"));
    }

    // ── get_notification_state ────────────────────────────────────────────────

    #[test]
    fn test_get_notification_state_returns_default_for_unknown_key() {
        let dir = TempDir::new().unwrap();
        let mgr = make_mgr(&dir);
        let state = mgr.get_notification_state("missing_key");
        assert!(!state.triggered);
        assert!(state.timestamp.is_none());
    }

    #[test]
    fn test_get_notification_state_returns_current_state() {
        let dir = TempDir::new().unwrap();
        let mut mgr = make_mgr(&dir);
        mgr.mark_notified(KEY_TOKENS_WILL_RUN_OUT);
        let state = mgr.get_notification_state(KEY_TOKENS_WILL_RUN_OUT);
        assert!(state.triggered);
        assert!(state.timestamp.is_some());
    }

    // ── persistence / round-trip ──────────────────────────────────────────────

    #[test]
    fn test_persistence_round_trip_all_keys() {
        let dir = TempDir::new().unwrap();

        {
            let mut mgr = make_mgr(&dir);
            mgr.mark_notified(KEY_SWITCH_TO_CUSTOM);
            mgr.mark_notified(KEY_EXCEED_MAX_LIMIT);
            // KEY_TOKENS_WILL_RUN_OUT intentionally left untriggered.
        }

        let mgr2 = make_mgr(&dir);
        assert!(mgr2.is_notification_active(KEY_SWITCH_TO_CUSTOM));
        assert!(mgr2.is_notification_active(KEY_EXCEED_MAX_LIMIT));
        assert!(!mgr2.is_notification_active(KEY_TOKENS_WILL_RUN_OUT));
    }

    #[test]
    fn test_persistence_invalid_json_falls_back_to_defaults() {
        let dir = TempDir::new().unwrap();
        let bad_path = dir.path().join("notification_states.json");
        std::fs::write(&bad_path, b"not valid json at all").unwrap();

        // Should not panic; should fall back to default states.
        let mgr = make_mgr(&dir);
        // All canonical keys untriggered.
        assert!(!mgr.is_notification_active(KEY_SWITCH_TO_CUSTOM));
        assert!(!mgr.is_notification_active(KEY_EXCEED_MAX_LIMIT));
        assert!(!mgr.is_notification_active(KEY_TOKENS_WILL_RUN_OUT));
    }

    #[test]
    fn test_persistence_file_contents_are_valid_json() {
        let dir = TempDir::new().unwrap();
        let mut mgr = make_mgr(&dir);
        mgr.mark_notified(KEY_SWITCH_TO_CUSTOM);

        let content = std::fs::read_to_string(dir.path().join("notification_states.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(parsed.is_object());
        assert!(parsed[KEY_SWITCH_TO_CUSTOM]["triggered"].as_bool().unwrap());
    }

    // ── default_states ────────────────────────────────────────────────────────

    #[test]
    fn test_default_states_contains_all_canonical_keys() {
        let defaults = NotificationManager::default_states();
        assert!(defaults.contains_key(KEY_SWITCH_TO_CUSTOM));
        assert!(defaults.contains_key(KEY_EXCEED_MAX_LIMIT));
        assert!(defaults.contains_key(KEY_TOKENS_WILL_RUN_OUT));
    }

    #[test]
    fn test_default_states_all_untriggered() {
        let defaults = NotificationManager::default_states();
        for state in defaults.values() {
            assert!(!state.triggered);
            assert!(state.timestamp.is_none());
        }
    }

    // ── cooldown edge cases ───────────────────────────────────────────────────

    #[test]
    fn test_should_notify_state_inserted_for_new_key() {
        let dir = TempDir::new().unwrap();
        let mut mgr = make_mgr(&dir);

        // First call with a new key inserts a default entry and returns true.
        assert!(mgr.should_notify("brand_new_key", 24.0));

        // State should now exist (untriggered).
        let state = mgr.get_notification_state("brand_new_key");
        assert!(!state.triggered);
    }

    #[test]
    fn test_should_notify_does_not_auto_persist() {
        // should_notify alone must not write the file (only mark_notified does).
        let dir = TempDir::new().unwrap();
        let mut mgr = make_mgr(&dir);
        let _ = mgr.should_notify("ephemeral", 24.0);

        let file_path = dir.path().join("notification_states.json");
        // File must not exist (was never saved via mark_notified).
        assert!(!file_path.exists());
    }
}
