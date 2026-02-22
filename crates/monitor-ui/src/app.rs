//! Main application state and TUI event loop for Claude Monitor.
//!
//! [`App`] owns the theme, view mode, and the last received monitoring
//! snapshot.  It drives both the real-time and table view event loops.

use std::io;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Frame, Terminal};
use tokio::sync::mpsc;

use monitor_core::models::BurnRate;
use monitor_core::plans::Plans;

use crate::session_view::{self, SessionViewData};
use crate::table_view::{self, TableRowData, TableTotals};
use crate::themes::Theme;

// ── ViewMode ──────────────────────────────────────────────────────────────────

/// Which view the TUI is currently rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewMode {
    /// Live real-time session dashboard.
    Realtime,
    /// Daily aggregate usage table.
    Daily,
    /// Monthly aggregate usage table.
    Monthly,
}

// ── AppData / ActiveBlockData ─────────────────────────────────────────────────

/// Processed monitoring snapshot ready for the UI to consume.
#[derive(Debug, Clone)]
pub struct AppData {
    /// Total tokens across all session blocks.
    pub total_tokens: u64,
    /// Total cost (USD) across all session blocks.
    pub total_cost: f64,
    /// Token limit for the active plan.
    pub token_limit: u64,
    /// Active block data, or `None` when there is no ongoing session.
    pub active_block: Option<ActiveBlockData>,
}

/// Extracted display values for the currently active session block.
#[derive(Debug, Clone)]
pub struct ActiveBlockData {
    /// Tokens consumed in this session block.
    pub tokens_used: u64,
    /// Cost in USD accrued in this block.
    pub cost_usd: f64,
    /// Minutes elapsed since the block started.
    pub elapsed_minutes: f64,
    /// Total duration of the session window in minutes (5 hours = 300).
    pub total_minutes: f64,
    /// Tokens-per-minute burn rate, if calculable.
    pub burn_rate_tokens_per_min: Option<f64>,
    /// Cost-per-hour burn rate, if calculable.
    pub burn_rate_cost_per_hour: Option<f64>,
    /// Per-model token usage as `(model_name, percentage)` pairs.
    pub model_percentages: Vec<(String, f64)>,
    /// Number of user-sent messages in this block.
    pub sent_messages: u32,
    /// Formatted start time string.
    pub start_time: String,
    /// Formatted end (reset) time string (for display fallback).
    pub end_time: String,
    /// Raw UTC end time for timezone conversion.
    pub end_time_utc: chrono::DateTime<chrono::Utc>,
    /// Cache creation tokens for the block.
    pub cache_creation_tokens: u64,
    /// Cache read tokens for the block.
    pub cache_read_tokens: u64,
}

// ── App ───────────────────────────────────────────────────────────────────────

/// Root application state for the Claude Monitor TUI.
pub struct App {
    /// Active colour theme.
    pub theme: Theme,
    /// Current view mode.
    pub view_mode: ViewMode,
    /// Plan name string (e.g. `"pro"`).
    pub plan: String,
    /// Human-readable timezone string.
    pub timezone: String,
    /// Set to `true` to break out of the event loop on the next iteration.
    pub should_quit: bool,
    /// Most recent monitoring snapshot, `None` until the first data arrives.
    pub last_data: Option<AppData>,
}

impl App {
    /// Construct a new application with the given configuration.
    pub fn new(theme_name: &str, view_mode: ViewMode, plan: String, timezone: String) -> Self {
        Self {
            theme: Theme::from_name(theme_name),
            view_mode,
            plan,
            timezone,
            should_quit: false,
            last_data: None,
        }
    }

    // ── Public event loops ────────────────────────────────────────────────────

    /// Run the real-time monitoring TUI, receiving data from `rx`.
    ///
    /// Uses `crossterm::event::poll` (synchronous, with a 250 ms timeout) so
    /// that the terminal event loop stays on the current thread while data
    /// updates arrive on the async channel via `try_recv`.
    ///
    /// The loop exits on `q`, `Q`, or `Ctrl+C`.
    pub async fn run_realtime(
        mut self,
        mut rx: mpsc::Receiver<monitor_runtime::orchestrator::MonitoringData>,
    ) -> io::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let tick_rate = Duration::from_millis(250);

        let result = loop {
            terminal.draw(|frame| self.render(frame))?;

            // Handle keyboard events with a short timeout so we don't block.
            if event::poll(tick_rate)? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            break Ok(());
                        }
                        KeyCode::Char('q') | KeyCode::Char('Q') => break Ok(()),
                        _ => {}
                    }
                }
            }

            // Drain any pending data updates (non-blocking).
            loop {
                match rx.try_recv() {
                    Ok(data) => self.update_from_monitoring(data),
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        self.should_quit = true;
                        break;
                    }
                }
            }

            if self.should_quit {
                break Ok(());
            }
        };

        // Restore terminal state unconditionally.
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        result
    }

    /// Run a static table view (daily or monthly), then wait for `q` / `Ctrl+C`.
    pub async fn run_table(self, rows: Vec<TableRowData>, totals: TableTotals) -> io::Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let title = match self.view_mode {
            ViewMode::Daily => "Daily Usage",
            ViewMode::Monthly => "Monthly Usage",
            ViewMode::Realtime => "Usage",
        };

        let tick_rate = Duration::from_millis(250);

        loop {
            terminal.draw(|frame| {
                let area = frame.area();
                if rows.is_empty() {
                    table_view::render_no_data(frame, area, &self.theme);
                } else {
                    table_view::render_table_view(frame, area, title, &rows, &totals, &self.theme);
                }
            })?;

            if event::poll(tick_rate)? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            break;
                        }
                        KeyCode::Char('q') | KeyCode::Char('Q') => break,
                        _ => {}
                    }
                }
            }
        }

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;
        Ok(())
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Render the current application state into `frame`.
    fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        match self.view_mode {
            ViewMode::Realtime => {
                if let Some(ref app_data) = self.last_data {
                    if let Some(ref active) = app_data.active_block {
                        let plan_config = Plans::get_plan_by_name(&self.plan);
                        let cost_limit = plan_config
                            .as_ref()
                            .map(|p| p.cost_limit)
                            .unwrap_or(Plans::DEFAULT_COST_LIMIT);
                        let message_limit = plan_config
                            .as_ref()
                            .map(|p| p.message_limit)
                            .unwrap_or(Plans::DEFAULT_MESSAGE_LIMIT);

                        let burn_rate = active.burn_rate_tokens_per_min.map(|tpm| BurnRate {
                            tokens_per_minute: tpm,
                            cost_per_hour: active.burn_rate_cost_per_hour.unwrap_or(0.0),
                        });

                        // Resolve display timezone (fallback to UTC).
                        let now_utc = chrono::Utc::now();
                        let tz: chrono_tz::Tz = self.timezone.parse().unwrap_or(chrono_tz::Tz::UTC);
                        let now_local = now_utc.with_timezone(&tz);

                        // Format current time in user's timezone.
                        let current_time = now_local.format("%I:%M:%S %p").to_string();

                        // Format reset time in user's timezone.
                        let reset_dt = active.end_time_utc;
                        let reset_local = reset_dt.with_timezone(&tz);
                        let reset_time = reset_local.format("%I:%M %p").to_string();

                        // Compute predicted token exhaustion time.
                        let predicted_end = if let Some(ref br) = burn_rate {
                            if br.tokens_per_minute > 0.0
                                && app_data.token_limit > active.tokens_used
                            {
                                let remaining = app_data.token_limit - active.tokens_used;
                                let mins_left = remaining as f64 / br.tokens_per_minute;
                                let pred_utc =
                                    now_utc + chrono::Duration::seconds((mins_left * 60.0) as i64);
                                let pred_local = pred_utc.with_timezone(&tz);
                                Some(pred_local.format("%I:%M %p").to_string())
                            } else if active.tokens_used >= app_data.token_limit {
                                Some("Exceeded".to_string())
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        let view_data = SessionViewData {
                            plan: self.plan.clone(),
                            timezone: self.timezone.clone(),
                            tokens_used: active.tokens_used,
                            token_limit: app_data.token_limit,
                            cost_usd: active.cost_usd,
                            cost_limit,
                            elapsed_minutes: active.elapsed_minutes,
                            total_minutes: active.total_minutes,
                            burn_rate,
                            per_model_stats: active.model_percentages.clone(),
                            sent_messages: active.sent_messages,
                            message_limit,
                            current_time,
                            reset_time,
                            predicted_end,
                            is_active: true,
                            notifications: Vec::new(),
                            cache_creation_tokens: active.cache_creation_tokens,
                            cache_read_tokens: active.cache_read_tokens,
                        };
                        session_view::render_session_view(frame, area, &view_data, &self.theme);
                    } else {
                        session_view::render_no_session(frame, area, &self.theme);
                    }
                } else {
                    session_view::render_no_session(frame, area, &self.theme);
                }
            }
            // Table views are handled by `run_table`; render a blank frame
            // if this method is called unexpectedly in that mode.
            ViewMode::Daily | ViewMode::Monthly => {
                session_view::render_no_session(frame, area, &self.theme);
            }
        }
    }

    /// Convert incoming [`MonitoringData`] into [`AppData`] and store it.
    ///
    /// Extracts the active session block (if any), computes per-model
    /// percentages, elapsed time, and formats display strings.
    pub fn update_from_monitoring(&mut self, data: monitor_runtime::orchestrator::MonitoringData) {
        let analysis = &data.analysis;

        // Find the first active, non-gap block (most recent takes priority).
        let active_block_opt = analysis
            .blocks
            .iter()
            .rev()
            .find(|b| b.is_active && !b.is_gap);

        let active = active_block_opt.map(|block| {
            // Elapsed time: now - block.start_time, capped to window.
            let now = chrono::Utc::now();
            let elapsed_secs = (now - block.start_time).num_seconds().max(0) as f64;
            let elapsed_minutes = elapsed_secs / 60.0;

            // Total window duration in minutes (5 hours default).
            let window_secs = (block.end_time - block.start_time).num_seconds() as f64;
            let total_minutes = (window_secs / 60.0).max(1.0);

            // For the token progress bar, only count input + output tokens.
            // Cache tokens are displayed separately in the "Cache Tokens" row.
            let display_tokens = block.token_counts.input_tokens + block.token_counts.output_tokens;

            // Compute burn rate from the active session's actual tokens and
            // elapsed time (require at least 30s to avoid division spikes).
            let burn_rate_tokens_per_min = if elapsed_minutes > 0.5 {
                Some(display_tokens as f64 / elapsed_minutes)
            } else {
                None
            };
            let burn_rate_cost_per_hour = if elapsed_minutes > 0.5 {
                Some((block.cost_usd / elapsed_minutes) * 60.0)
            } else {
                None
            };

            // Per-model percentages: compute relative to input+output tokens
            // only (cache tokens are shown separately).
            let io_total: u64 = block
                .per_model_stats
                .values()
                .map(|s| s.input_tokens + s.output_tokens)
                .sum();
            let model_percentages: Vec<(String, f64)> = if io_total > 0 {
                block
                    .per_model_stats
                    .iter()
                    .map(|(model, stats)| {
                        let model_io = stats.input_tokens + stats.output_tokens;
                        let pct = (model_io as f64 / io_total as f64) * 100.0;
                        (model.clone(), pct)
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };

            // Sort descending so the bar renders the largest segment first.
            let mut model_percentages = model_percentages;
            model_percentages
                .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            ActiveBlockData {
                tokens_used: display_tokens,
                cost_usd: block.cost_usd,
                elapsed_minutes,
                total_minutes,
                burn_rate_tokens_per_min,
                burn_rate_cost_per_hour,
                model_percentages,
                sent_messages: block.sent_messages_count,
                start_time: block.start_time.format("%H:%M:%S").to_string(),
                end_time: block.end_time.format("%H:%M:%S").to_string(),
                end_time_utc: block.end_time,
                cache_creation_tokens: block.token_counts.cache_creation_tokens,
                cache_read_tokens: block.token_counts.cache_read_tokens,
            }
        });

        self.last_data = Some(AppData {
            total_tokens: analysis.total_tokens,
            total_cost: analysis.total_cost,
            token_limit: data.token_limit,
            active_block: active,
        });
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_runtime::data::analysis::{AnalysisMetadata, AnalysisResult};

    // ── ViewMode ──────────────────────────────────────────────────────────────

    #[test]
    fn test_view_mode_enum_equality() {
        assert_eq!(ViewMode::Realtime, ViewMode::Realtime);
        assert_eq!(ViewMode::Daily, ViewMode::Daily);
        assert_eq!(ViewMode::Monthly, ViewMode::Monthly);
        assert_ne!(ViewMode::Realtime, ViewMode::Daily);
        assert_ne!(ViewMode::Daily, ViewMode::Monthly);
    }

    #[test]
    fn test_view_mode_clone() {
        let mode = ViewMode::Monthly;
        let clone = mode.clone();
        assert_eq!(mode, clone);
    }

    // ── App::new ──────────────────────────────────────────────────────────────

    #[test]
    fn test_app_creation_defaults() {
        let app = App::new(
            "dark",
            ViewMode::Realtime,
            "pro".to_string(),
            "UTC".to_string(),
        );
        assert_eq!(app.plan, "pro");
        assert_eq!(app.timezone, "UTC");
        assert_eq!(app.view_mode, ViewMode::Realtime);
        assert!(!app.should_quit);
        assert!(app.last_data.is_none());
    }

    #[test]
    fn test_app_creation_light_theme() {
        let app = App::new(
            "light",
            ViewMode::Daily,
            "max5".to_string(),
            "UTC".to_string(),
        );
        assert_eq!(app.view_mode, ViewMode::Daily);
        assert_eq!(app.plan, "max5");
    }

    #[test]
    fn test_app_creation_unknown_theme_falls_back() {
        // Should not panic for unknown theme names.
        let app = App::new(
            "neon",
            ViewMode::Monthly,
            "custom".to_string(),
            "UTC".to_string(),
        );
        assert_eq!(app.view_mode, ViewMode::Monthly);
    }

    // ── update_from_monitoring ────────────────────────────────────────────────

    fn make_empty_analysis() -> AnalysisResult {
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

    fn make_monitoring_data_no_active() -> monitor_runtime::orchestrator::MonitoringData {
        monitor_runtime::orchestrator::MonitoringData {
            analysis: make_empty_analysis(),
            token_limit: 19_000,
            plan: "pro".to_string(),
            session_id: None,
            session_count: 0,
        }
    }

    fn make_monitoring_data_with_active() -> monitor_runtime::orchestrator::MonitoringData {
        use chrono::Utc;
        use monitor_core::models::{BurnRate, SessionBlock, TokenCounts};
        use std::collections::HashMap;

        let now = Utc::now();
        let start = now - chrono::Duration::minutes(90);
        let end = start + chrono::Duration::hours(5);

        let mut per_model_stats = HashMap::new();
        per_model_stats.insert(
            "claude-3-5-sonnet".to_string(),
            monitor_core::models::ModelStats {
                input_tokens: 800,
                output_tokens: 200,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
                cost_usd: 0.05,
                entries_count: 3,
            },
        );

        let block = SessionBlock {
            id: "active-1".to_string(),
            start_time: start,
            end_time: end,
            entries: vec![],
            token_counts: TokenCounts {
                input_tokens: 800,
                output_tokens: 200,
                cache_creation_tokens: 0,
                cache_read_tokens: 0,
            },
            is_active: true,
            is_gap: false,
            burn_rate: None,
            actual_end_time: None,
            per_model_stats,
            models: vec!["claude-3-5-sonnet".to_string()],
            sent_messages_count: 15,
            cost_usd: 0.05,
            limit_messages: vec![],
            projection_data: None,
            burn_rate_snapshot: Some(BurnRate {
                tokens_per_minute: 11.1,
                cost_per_hour: 0.033,
            }),
        };

        let mut analysis = make_empty_analysis();
        analysis.blocks = vec![block];
        analysis.total_tokens = 1_000;
        analysis.total_cost = 0.05;

        monitor_runtime::orchestrator::MonitoringData {
            analysis,
            token_limit: 19_000,
            plan: "pro".to_string(),
            session_id: Some("active-1".to_string()),
            session_count: 1,
        }
    }

    #[test]
    fn test_update_from_monitoring_no_active_block() {
        let mut app = App::new(
            "dark",
            ViewMode::Realtime,
            "pro".to_string(),
            "UTC".to_string(),
        );
        app.update_from_monitoring(make_monitoring_data_no_active());

        let data = app.last_data.as_ref().unwrap();
        assert!(data.active_block.is_none());
        assert_eq!(data.total_tokens, 0);
        assert_eq!(data.token_limit, 19_000);
    }

    #[test]
    fn test_update_from_monitoring_with_active_block() {
        let mut app = App::new(
            "dark",
            ViewMode::Realtime,
            "pro".to_string(),
            "UTC".to_string(),
        );
        app.update_from_monitoring(make_monitoring_data_with_active());

        let data = app.last_data.as_ref().unwrap();
        assert!(data.active_block.is_some());
        let active = data.active_block.as_ref().unwrap();
        assert_eq!(active.tokens_used, 1_000);
        assert_eq!(active.sent_messages, 15);
        assert!((active.cost_usd - 0.05).abs() < 1e-9);
    }

    #[test]
    fn test_update_from_monitoring_model_percentages_computed() {
        let mut app = App::new(
            "dark",
            ViewMode::Realtime,
            "pro".to_string(),
            "UTC".to_string(),
        );
        app.update_from_monitoring(make_monitoring_data_with_active());

        let active = app
            .last_data
            .as_ref()
            .unwrap()
            .active_block
            .as_ref()
            .unwrap();
        // Only sonnet in this test block.
        assert!(!active.model_percentages.is_empty());
        let (model, pct) = &active.model_percentages[0];
        assert!(model.contains("sonnet"));
        // 1000 tokens total, sonnet has 1000 → 100 %
        assert!((pct - 100.0).abs() < 1e-6);
    }

    #[test]
    fn test_update_from_monitoring_burn_rate_extracted() {
        let mut app = App::new(
            "dark",
            ViewMode::Realtime,
            "pro".to_string(),
            "UTC".to_string(),
        );
        app.update_from_monitoring(make_monitoring_data_with_active());

        let active = app
            .last_data
            .as_ref()
            .unwrap()
            .active_block
            .as_ref()
            .unwrap();
        assert!(active.burn_rate_tokens_per_min.is_some());
        let tpm = active.burn_rate_tokens_per_min.unwrap();
        // 1000 tokens over ~90 min ≈ 11.1 tokens/min (computed, not snapshot).
        assert!(
            tpm > 10.0 && tpm < 13.0,
            "burn rate should be ~11, got {tpm}"
        );
    }

    #[test]
    fn test_update_from_monitoring_elapsed_minutes_positive() {
        let mut app = App::new(
            "dark",
            ViewMode::Realtime,
            "pro".to_string(),
            "UTC".to_string(),
        );
        app.update_from_monitoring(make_monitoring_data_with_active());

        let active = app
            .last_data
            .as_ref()
            .unwrap()
            .active_block
            .as_ref()
            .unwrap();
        // Block started 90 min ago, so elapsed_minutes should be close to 90.
        assert!(
            active.elapsed_minutes > 80.0,
            "elapsed = {}",
            active.elapsed_minutes
        );
        assert!(
            active.elapsed_minutes < 120.0,
            "elapsed = {}",
            active.elapsed_minutes
        );
    }

    #[test]
    fn test_update_from_monitoring_total_minutes_is_window_duration() {
        let mut app = App::new(
            "dark",
            ViewMode::Realtime,
            "pro".to_string(),
            "UTC".to_string(),
        );
        app.update_from_monitoring(make_monitoring_data_with_active());

        let active = app
            .last_data
            .as_ref()
            .unwrap()
            .active_block
            .as_ref()
            .unwrap();
        // 5-hour window = 300 minutes.
        assert!(
            (active.total_minutes - 300.0).abs() < 1.0,
            "total = {}",
            active.total_minutes
        );
    }

    #[test]
    fn test_update_from_monitoring_overwrites_previous_data() {
        let mut app = App::new(
            "dark",
            ViewMode::Realtime,
            "pro".to_string(),
            "UTC".to_string(),
        );
        app.update_from_monitoring(make_monitoring_data_no_active());
        assert!(app.last_data.as_ref().unwrap().active_block.is_none());

        app.update_from_monitoring(make_monitoring_data_with_active());
        assert!(app.last_data.as_ref().unwrap().active_block.is_some());
    }

    #[test]
    fn test_update_from_monitoring_gap_block_not_active() {
        use monitor_core::models::{SessionBlock, TokenCounts};
        use std::collections::HashMap;

        let now = chrono::Utc::now();
        let start = now - chrono::Duration::hours(1);
        let end = start + chrono::Duration::hours(5);

        let gap_block = SessionBlock {
            id: "gap-1".to_string(),
            start_time: start,
            end_time: end,
            entries: vec![],
            token_counts: TokenCounts::default(),
            is_active: true, // is_active=true but is_gap=true → should be excluded
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

        let mut analysis = make_empty_analysis();
        analysis.blocks = vec![gap_block];

        let monitoring_data = monitor_runtime::orchestrator::MonitoringData {
            analysis,
            token_limit: 19_000,
            plan: "pro".to_string(),
            session_id: None,
            session_count: 0,
        };

        let mut app = App::new(
            "dark",
            ViewMode::Realtime,
            "pro".to_string(),
            "UTC".to_string(),
        );
        app.update_from_monitoring(monitoring_data);

        // Gap blocks must not be treated as active sessions.
        assert!(app.last_data.as_ref().unwrap().active_block.is_none());
    }
}
