//! Async monitoring orchestrator.
//!
//! Coordinates [`DataManager`] and [`SessionMonitor`] in a tokio task, sending
//! periodic [`MonitoringData`] snapshots through an `mpsc` channel so the TUI
//! event loop can consume them without any shared mutable state.

use std::time::Duration;

use monitor_core::plans::Plans;
use monitor_data::analysis::AnalysisResult;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time;

use crate::data_manager::DataManager;
use crate::session_monitor::SessionMonitor;

// ── Public types ──────────────────────────────────────────────────────────────

/// A single monitoring snapshot forwarded to the TUI layer.
///
/// This is the primary data contract between the background runtime and the
/// presentation layer.
#[derive(Debug, Clone)]
pub struct MonitoringData {
    /// Full analysis result from the data pipeline.
    pub analysis: AnalysisResult,
    /// Token limit for the configured plan (may differ from `analysis` totals).
    pub token_limit: u64,
    /// Canonical plan name (e.g. `"pro"`, `"max5"`).
    pub plan: String,
    /// Active session ID, if any.
    pub session_id: Option<String>,
    /// Total number of sessions observed since startup.
    pub session_count: usize,
}

// ── MonitoringOrchestrator ────────────────────────────────────────────────────

/// Background monitoring coordinator.
///
/// Call [`MonitoringOrchestrator::start`] to spin up the monitoring loop in a
/// dedicated tokio task and receive a channel endpoint for [`MonitoringData`]
/// updates.
pub struct MonitoringOrchestrator {
    /// How often to refresh the analysis.
    update_interval: Duration,
    /// Optional override for the JSONL data directory.
    data_path: Option<String>,
    /// Canonical plan name used for limit look-ups.
    plan: String,
}

impl MonitoringOrchestrator {
    /// Create a new orchestrator.
    ///
    /// # Parameters
    /// - `update_interval_secs` – seconds between monitoring refreshes.
    /// - `data_path`            – optional JSONL directory override.
    /// - `plan`                 – canonical plan name (e.g. `"pro"`).
    pub fn new(update_interval_secs: u64, data_path: Option<String>, plan: String) -> Self {
        Self {
            update_interval: Duration::from_secs(update_interval_secs),
            data_path,
            plan,
        }
    }

    /// Start the monitoring loop.
    ///
    /// Spawns a tokio task that runs the monitoring loop. Returns:
    /// - An `mpsc::Receiver<MonitoringData>` for the caller to poll.
    /// - A [`MonitoringHandle`] that can be used to abort the loop.
    pub fn start(self) -> (mpsc::Receiver<MonitoringData>, MonitoringHandle) {
        // Buffer a modest number of snapshots so slow consumers don't stall the loop.
        let (tx, rx) = mpsc::channel(16);

        let handle = tokio::spawn(async move {
            self.monitoring_loop(tx).await;
        });

        (rx, MonitoringHandle { handle })
    }

    // ── Private implementation ────────────────────────────────────────────

    /// The main monitoring loop.
    ///
    /// Performs an immediate fetch on startup, then repeats on `update_interval`.
    /// The loop exits when the receiver side of the channel is closed.
    async fn monitoring_loop(self, tx: mpsc::Sender<MonitoringData>) {
        let mut data_manager = DataManager::new(30, 192, self.data_path.clone());
        let mut session_monitor = SessionMonitor::new();

        // Initial fetch (force refresh to populate immediately).
        self.fetch_and_send(&mut data_manager, &mut session_monitor, &tx, true)
            .await;

        let mut interval = time::interval(self.update_interval);
        // Consume the first tick which fires immediately; we already fetched above.
        interval.tick().await;

        loop {
            interval.tick().await;

            if tx.is_closed() {
                tracing::debug!("monitoring channel closed; exiting loop");
                break;
            }

            self.fetch_and_send(&mut data_manager, &mut session_monitor, &tx, false)
                .await;
        }
    }

    /// Fetch fresh data and send a [`MonitoringData`] snapshot to the channel.
    async fn fetch_and_send(
        &self,
        data_manager: &mut DataManager,
        session_monitor: &mut SessionMonitor,
        tx: &mpsc::Sender<MonitoringData>,
        force: bool,
    ) {
        // Obtain analysis result (clone so we can own it for the snapshot).
        let analysis = match data_manager.get_data(force) {
            Some(r) => r.clone(),
            None => {
                tracing::warn!("no analysis data available; skipping send");
                return;
            }
        };

        // Convert to Value so SessionMonitor can validate and track sessions.
        let as_value = analysis_to_value(&analysis);
        let (_, errors) = session_monitor.update(&as_value);
        if !errors.is_empty() {
            tracing::debug!(?errors, "session monitor validation errors");
        }

        let token_limit = Plans::get_token_limit(&self.plan);
        let session_id = session_monitor.current_session_id().map(|s| s.to_string());
        let session_count = session_monitor.session_count();

        let snapshot = MonitoringData {
            analysis,
            token_limit,
            plan: self.plan.clone(),
            session_id,
            session_count,
        };

        if let Err(e) = tx.send(snapshot).await {
            tracing::warn!(error = %e, "failed to send monitoring snapshot; receiver dropped");
        }
    }
}

// ── MonitoringHandle ──────────────────────────────────────────────────────────

/// A handle to the background monitoring task.
///
/// Drop or call [`MonitoringHandle::abort`] to stop the loop.
pub struct MonitoringHandle {
    handle: tokio::task::JoinHandle<()>,
}

impl MonitoringHandle {
    /// Immediately abort the monitoring loop.
    pub fn abort(&self) {
        self.handle.abort();
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Convert an [`AnalysisResult`] to the `serde_json::Value` shape that
/// [`SessionMonitor::validate_data`] expects.
///
/// Shape: `{ "blocks": [ { id, isActive, totalTokens, costUSD, startTime? } ] }`.
fn analysis_to_value(result: &AnalysisResult) -> Value {
    let blocks: Vec<Value> = result
        .blocks
        .iter()
        .map(|b| {
            serde_json::json!({
                "id": b.id,
                "isActive": b.is_active,
                "totalTokens": b.total_tokens(),
                "costUSD": b.cost_usd,
                "startTime": b.start_time.to_rfc3339(),
            })
        })
        .collect();

    serde_json::json!({ "blocks": blocks })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_data::analysis::{AnalysisMetadata, AnalysisResult};

    // ── helpers ───────────────────────────────────────────────────────────

    fn empty_result() -> AnalysisResult {
        AnalysisResult {
            blocks: vec![],
            metadata: AnalysisMetadata {
                generated_at: "2024-01-01T00:00:00Z".to_string(),
                hours_analyzed: None,
                entries_processed: 0,
                blocks_created: 0,
                limits_detected: 0,
                load_time_seconds: 0.0,
                transform_time_seconds: 0.0,
            },
            entries_count: 0,
            total_tokens: 0,
            total_cost: 0.0,
        }
    }

    // ── orchestrator creation ─────────────────────────────────────────────

    #[test]
    fn test_orchestrator_creation() {
        let orch =
            MonitoringOrchestrator::new(5, Some("/tmp/test-data".to_string()), "pro".to_string());
        assert_eq!(orch.update_interval, Duration::from_secs(5));
        assert_eq!(orch.data_path.as_deref(), Some("/tmp/test-data"));
        assert_eq!(orch.plan, "pro");
    }

    // ── MonitoringData structure ──────────────────────────────────────────

    #[test]
    fn test_monitoring_data_structure() {
        let data = MonitoringData {
            analysis: empty_result(),
            token_limit: 19_000,
            plan: "pro".to_string(),
            session_id: Some("test-session".to_string()),
            session_count: 1,
        };

        assert_eq!(data.token_limit, 19_000);
        assert_eq!(data.plan, "pro");
        assert_eq!(data.session_id.as_deref(), Some("test-session"));
        assert_eq!(data.session_count, 1);
        assert_eq!(data.analysis.total_tokens, 0);
        assert!(data.analysis.blocks.is_empty());
    }

    #[test]
    fn test_monitoring_data_clone() {
        let data = MonitoringData {
            analysis: empty_result(),
            token_limit: 88_000,
            plan: "max5".to_string(),
            session_id: None,
            session_count: 0,
        };
        let cloned = data.clone();
        assert_eq!(cloned.token_limit, 88_000);
        assert_eq!(cloned.plan, "max5");
        assert!(cloned.session_id.is_none());
    }

    // ── analysis_to_value ─────────────────────────────────────────────────

    #[test]
    fn test_analysis_to_value_empty_blocks() {
        let value = analysis_to_value(&empty_result());
        assert!(value.get("blocks").is_some());
        assert!(value["blocks"].as_array().unwrap().is_empty());
    }

    // ── existing test compatibility ───────────────────────────────────────

    #[test]
    fn test_monitoring_data_construction() {
        let data = MonitoringData {
            analysis: empty_result(),
            token_limit: 19_000,
            plan: "pro".to_string(),
            session_id: None,
            session_count: 0,
        };
        assert_eq!(data.token_limit, 19_000);
        assert_eq!(data.plan, "pro");
        assert!(data.analysis.blocks.is_empty());
    }

    #[test]
    fn test_monitoring_data_plan_stored() {
        let data = MonitoringData {
            analysis: empty_result(),
            token_limit: 88_000,
            plan: "max5".to_string(),
            session_id: None,
            session_count: 0,
        };
        assert_eq!(data.plan, "max5");
        assert_eq!(data.token_limit, 88_000);
    }

    // ── async: start / abort ──────────────────────────────────────────────

    #[tokio::test]
    async fn test_orchestrator_start_and_abort() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().to_str().unwrap().to_string();

        let orch = MonitoringOrchestrator::new(60, Some(path), "pro".to_string());
        let (_rx, handle) = orch.start();

        // Give the task a moment to start, then abort it.
        tokio::time::sleep(Duration::from_millis(50)).await;
        handle.abort();
    }

    // ── async: receives initial snapshot ─────────────────────────────────

    #[tokio::test]
    async fn test_orchestrator_sends_initial_snapshot() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().to_str().unwrap().to_string();

        let orch = MonitoringOrchestrator::new(60, Some(path), "pro".to_string());
        let (mut rx, handle) = orch.start();

        // The first snapshot should arrive quickly (empty data dir → empty result).
        let snapshot = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("timed out waiting for snapshot")
            .expect("channel closed before receiving snapshot");

        assert_eq!(snapshot.plan, "pro");
        assert_eq!(snapshot.token_limit, 19_000);

        handle.abort();
    }
}
