//! Session-level tracking and validation for monitoring data.
//!
//! [`SessionMonitor`] ingests the raw `serde_json::Value` produced by the
//! analysis pipeline, validates its structure, detects session boundaries, and
//! maintains a history of observed sessions.

use serde_json::Value;

// ── Public types ──────────────────────────────────────────────────────────────

/// Summary information about a single Claude session.
#[derive(Debug, Clone)]
pub struct SessionInfo {
    /// Unique session / block identifier.
    pub id: String,
    /// ISO-8601 string of when the session started, if present.
    pub started_at: Option<String>,
    /// Total tokens consumed in this session.
    pub tokens: u64,
    /// Total cost (USD) for this session.
    pub cost: f64,
}

// ── SessionMonitor ────────────────────────────────────────────────────────────

/// Tracks active sessions and maintains a history of completed sessions.
///
/// Call [`SessionMonitor::update`] on every monitoring cycle, passing the
/// `serde_json::Value` representation of an [`AnalysisResult`]. The monitor
/// emits validation errors for malformed data and records session transitions.
pub struct SessionMonitor {
    /// ID of the currently active session block, if any.
    current_session_id: Option<String>,
    /// Ordered log of all sessions that have been observed.
    session_history: Vec<SessionInfo>,
}

impl SessionMonitor {
    /// Create a new, empty monitor.
    pub fn new() -> Self {
        Self {
            current_session_id: None,
            session_history: Vec::new(),
        }
    }

    // ── Public API ────────────────────────────────────────────────────────

    /// Update session tracking with fresh monitoring data.
    ///
    /// Steps:
    /// 1. Validate the data structure.
    /// 2. Locate the active block in `data["blocks"]`.
    /// 3. If an active block is found and its ID differs from the current
    ///    session, trigger a session-change transition.
    /// 4. If no active block is found but a session is currently tracked,
    ///    trigger a session-end transition.
    ///
    /// Returns `(is_valid, errors)` where `is_valid` is `true` when
    /// validation passed and `errors` lists any structural problems.
    pub fn update(&mut self, data: &Value) -> (bool, Vec<String>) {
        let (is_valid, errors) = self.validate_data(data);
        if !is_valid {
            return (false, errors);
        }

        // Find the active block (if any).
        let active_block = data["blocks"].as_array().and_then(|blocks| {
            blocks
                .iter()
                .find(|b| b["isActive"].as_bool() == Some(true))
        });

        match active_block {
            Some(block) => {
                let session_id = block["id"].as_str().unwrap_or("").to_string();

                if self.current_session_id.as_deref() != Some(session_id.as_str()) {
                    // Session changed (or first session encountered).
                    self.on_session_change(&session_id, block);
                }

                self.current_session_id = Some(session_id);
            }
            None => {
                if self.current_session_id.is_some() {
                    self.on_session_end();
                    self.current_session_id = None;
                }
            }
        }

        (true, errors)
    }

    /// Validate the structure of monitoring data.
    ///
    /// Checks:
    /// - `data` is a JSON object.
    /// - `data["blocks"]` exists and is an array.
    /// - Each block has the required fields (`id`, `isActive`, `totalTokens`,
    ///   `costUSD`) with correct types.
    ///
    /// Returns `(is_valid, errors)`.
    pub fn validate_data(&self, data: &Value) -> (bool, Vec<String>) {
        let mut errors = Vec::new();

        if !data.is_object() {
            errors.push("data must be a JSON object".to_string());
            return (false, errors);
        }

        let blocks = match data.get("blocks") {
            Some(v) => v,
            None => {
                errors.push("missing required field: blocks".to_string());
                return (false, errors);
            }
        };

        if !blocks.is_array() {
            errors.push("field 'blocks' must be an array".to_string());
            return (false, errors);
        }

        for (idx, block) in blocks.as_array().unwrap().iter().enumerate() {
            // id
            if block.get("id").is_none() {
                errors.push(format!("block[{idx}]: missing required field 'id'"));
            }
            // isActive
            match block.get("isActive") {
                None => errors.push(format!("block[{idx}]: missing required field 'isActive'")),
                Some(v) if !v.is_boolean() => {
                    errors.push(format!("block[{idx}]: 'isActive' must be a boolean"))
                }
                _ => {}
            }
            // totalTokens
            match block.get("totalTokens") {
                None => errors.push(format!(
                    "block[{idx}]: missing required field 'totalTokens'"
                )),
                Some(v) if !v.is_number() => {
                    errors.push(format!("block[{idx}]: 'totalTokens' must be a number"))
                }
                _ => {}
            }
            // costUSD
            match block.get("costUSD") {
                None => errors.push(format!("block[{idx}]: missing required field 'costUSD'")),
                Some(v) if !v.is_number() => {
                    errors.push(format!("block[{idx}]: 'costUSD' must be a number"))
                }
                _ => {}
            }
        }

        let is_valid = errors.is_empty();
        (is_valid, errors)
    }

    /// The ID of the currently active session, or `None`.
    pub fn current_session_id(&self) -> Option<&str> {
        self.current_session_id.as_deref()
    }

    /// Total number of session transitions recorded (including the current
    /// session if active).
    pub fn session_count(&self) -> usize {
        self.session_history.len()
    }

    /// Ordered history of all observed sessions.
    pub fn session_history(&self) -> &[SessionInfo] {
        &self.session_history
    }

    // ── Private helpers ───────────────────────────────────────────────────

    /// Called when a new active session is detected.
    fn on_session_change(&mut self, session_id: &str, block: &Value) {
        tracing::info!(session_id, "session started / changed");

        let tokens = block["totalTokens"].as_u64().unwrap_or(0);
        let cost = block["costUSD"].as_f64().unwrap_or(0.0);
        let started_at = block
            .get("startTime")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        self.session_history.push(SessionInfo {
            id: session_id.to_string(),
            started_at,
            tokens,
            cost,
        });
    }

    /// Called when the active session ends (no active block found).
    fn on_session_end(&self) {
        if let Some(id) = &self.current_session_id {
            tracing::info!(session_id = %id, "session ended");
        }
    }
}

impl Default for SessionMonitor {
    fn default() -> Self {
        Self::new()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── helpers ───────────────────────────────────────────────────────────

    /// Build a minimal valid block value.
    fn valid_block(id: &str, is_active: bool, tokens: u64, cost: f64) -> Value {
        json!({
            "id": id,
            "isActive": is_active,
            "totalTokens": tokens,
            "costUSD": cost,
        })
    }

    /// Build a minimal valid data payload containing the given blocks.
    fn valid_data(blocks: Vec<Value>) -> Value {
        json!({ "blocks": blocks })
    }

    // ── validate_data ─────────────────────────────────────────────────────

    #[test]
    fn test_validate_valid_data() {
        let monitor = SessionMonitor::new();
        let data = valid_data(vec![
            valid_block("block-1", false, 1000, 0.5),
            valid_block("block-2", true, 2000, 1.0),
        ]);
        let (is_valid, errors) = monitor.validate_data(&data);
        assert!(is_valid, "expected valid, errors: {errors:?}");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_validate_missing_blocks() {
        let monitor = SessionMonitor::new();
        let data = json!({ "other_field": 42 });
        let (is_valid, errors) = monitor.validate_data(&data);
        assert!(!is_valid);
        assert!(
            errors.iter().any(|e| e.contains("blocks")),
            "error should mention 'blocks'"
        );
    }

    #[test]
    fn test_validate_blocks_not_array() {
        let monitor = SessionMonitor::new();
        let data = json!({ "blocks": "not-an-array" });
        let (is_valid, errors) = monitor.validate_data(&data);
        assert!(!is_valid);
        assert!(errors.iter().any(|e| e.contains("array")));
    }

    #[test]
    fn test_validate_non_object_data() {
        let monitor = SessionMonitor::new();
        let (is_valid, errors) = monitor.validate_data(&json!([1, 2, 3]));
        assert!(!is_valid);
        assert!(errors.iter().any(|e| e.contains("object")));
    }

    #[test]
    fn test_validate_invalid_block_missing_fields() {
        let monitor = SessionMonitor::new();
        // Block completely missing all required fields.
        let data = json!({ "blocks": [{}] });
        let (is_valid, errors) = monitor.validate_data(&data);
        assert!(!is_valid);
        // Should have errors for id, isActive, totalTokens, costUSD.
        assert!(errors.iter().any(|e| e.contains("id")));
        assert!(errors.iter().any(|e| e.contains("isActive")));
        assert!(errors.iter().any(|e| e.contains("totalTokens")));
        assert!(errors.iter().any(|e| e.contains("costUSD")));
    }

    #[test]
    fn test_validate_invalid_block_wrong_types() {
        let monitor = SessionMonitor::new();
        let data = json!({
            "blocks": [{
                "id": "x",
                "isActive": "yes",       // should be bool
                "totalTokens": "lots",   // should be number
                "costUSD": "free",       // should be number
            }]
        });
        let (is_valid, errors) = monitor.validate_data(&data);
        assert!(!is_valid);
        assert!(errors.iter().any(|e| e.contains("isActive")));
        assert!(errors.iter().any(|e| e.contains("totalTokens")));
        assert!(errors.iter().any(|e| e.contains("costUSD")));
    }

    #[test]
    fn test_validate_empty_blocks_array() {
        let monitor = SessionMonitor::new();
        let data = valid_data(vec![]);
        let (is_valid, errors) = monitor.validate_data(&data);
        // Empty blocks array is structurally valid.
        assert!(is_valid, "errors: {errors:?}");
    }

    // ── session_start ─────────────────────────────────────────────────────

    #[test]
    fn test_session_start() {
        let mut monitor = SessionMonitor::new();
        assert!(monitor.current_session_id().is_none());

        let data = valid_data(vec![valid_block("sess-1", true, 500, 0.25)]);
        let (is_valid, _) = monitor.update(&data);

        assert!(is_valid);
        assert_eq!(monitor.current_session_id(), Some("sess-1"));
        assert_eq!(monitor.session_count(), 1);
    }

    // ── session_change ────────────────────────────────────────────────────

    #[test]
    fn test_session_change() {
        let mut monitor = SessionMonitor::new();

        // First session.
        let data1 = valid_data(vec![valid_block("sess-1", true, 100, 0.1)]);
        monitor.update(&data1);
        assert_eq!(monitor.current_session_id(), Some("sess-1"));

        // Second session with a different ID.
        let data2 = valid_data(vec![valid_block("sess-2", true, 200, 0.2)]);
        monitor.update(&data2);
        assert_eq!(monitor.current_session_id(), Some("sess-2"));

        // Both sessions recorded in history.
        assert_eq!(monitor.session_count(), 2);
        assert_eq!(monitor.session_history()[0].id, "sess-1");
        assert_eq!(monitor.session_history()[1].id, "sess-2");
    }

    // ── session_end ───────────────────────────────────────────────────────

    #[test]
    fn test_session_end() {
        let mut monitor = SessionMonitor::new();

        // Start a session.
        let data1 = valid_data(vec![valid_block("sess-1", true, 100, 0.1)]);
        monitor.update(&data1);
        assert_eq!(monitor.current_session_id(), Some("sess-1"));

        // No active block → session ends.
        let data2 = valid_data(vec![valid_block("sess-1", false, 100, 0.1)]);
        monitor.update(&data2);
        assert!(monitor.current_session_id().is_none());

        // History still contains the session.
        assert_eq!(monitor.session_count(), 1);
    }

    // ── session_history ───────────────────────────────────────────────────

    #[test]
    fn test_session_history() {
        let mut monitor = SessionMonitor::new();

        for (id, tokens, cost) in [("a", 100u64, 0.1f64), ("b", 200, 0.2), ("c", 300, 0.3)] {
            let data = valid_data(vec![valid_block(id, true, tokens, cost)]);
            monitor.update(&data);
        }

        let history = monitor.session_history();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].id, "a");
        assert_eq!(history[1].id, "b");
        assert_eq!(history[2].id, "c");
        assert!((history[2].cost - 0.3).abs() < 1e-9);
        assert_eq!(history[2].tokens, 300);
    }

    // ── no duplicate history entry for unchanged active session ───────────

    #[test]
    fn test_no_duplicate_on_same_session() {
        let mut monitor = SessionMonitor::new();

        let data = valid_data(vec![valid_block("sess-stable", true, 1000, 0.5)]);
        monitor.update(&data);
        monitor.update(&data);
        monitor.update(&data);

        // Only one entry despite three updates with the same session ID.
        assert_eq!(monitor.session_count(), 1);
    }

    // ── invalid data returns validation errors ────────────────────────────

    #[test]
    fn test_update_with_invalid_data_returns_errors() {
        let mut monitor = SessionMonitor::new();
        let bad_data = json!({ "no_blocks": true });
        let (is_valid, errors) = monitor.update(&bad_data);
        assert!(!is_valid);
        assert!(!errors.is_empty());
        // Session state unchanged.
        assert!(monitor.current_session_id().is_none());
    }
}
