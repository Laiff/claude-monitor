//! Real-time session view for the Claude Monitor TUI.
//!
//! Renders the live session screen showing token usage, cost, burn rates,
//! model distribution, and time information.  The layout exactly matches the
//! Python reference output.

use ratatui::{
    layout::Rect,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use monitor_core::models::BurnRate;

use crate::themes::Theme;

/// All data required to render the session view.
pub struct SessionViewData {
    /// Plan name (e.g. `"pro"`, `"max5"`).
    pub plan: String,
    /// Human-readable timezone string.
    pub timezone: String,
    /// Tokens consumed in the current session.
    pub tokens_used: u64,
    /// Token limit for the current plan.
    pub token_limit: u64,
    /// Cost accrued in USD for the current session.
    pub cost_usd: f64,
    /// Configured cost limit in USD.
    pub cost_limit: f64,
    /// Minutes elapsed in the current 5-hour session window.
    pub elapsed_minutes: f64,
    /// Total session window duration in minutes (e.g. 300 for 5 hours).
    pub total_minutes: f64,
    /// Current token and cost burn rates, if calculable.
    pub burn_rate: Option<BurnRate>,
    /// Per-model token usage as `(model_name, percentage)` pairs.
    pub per_model_stats: Vec<(String, f64)>,
    /// Number of user-sent messages in this session.
    pub sent_messages: u32,
    /// Message limit for the current plan.
    pub message_limit: u32,
    /// Formatted current wall-clock time string.
    pub current_time: String,
    /// Formatted session reset time string.
    pub reset_time: String,
    /// Optional predicted token exhaustion time string.
    pub predicted_end: Option<String>,
    /// Whether the session is currently active.
    pub is_active: bool,
    /// Notification strings to display at the bottom of the view.
    pub notifications: Vec<String>,
    /// Cache creation tokens for the current session block.
    pub cache_creation_tokens: u64,
    /// Cache read tokens for the current session block.
    pub cache_read_tokens: u64,
}

// ── Formatting helpers ────────────────────────────────────────────────────────

/// Format a number with thousands separators (e.g. 1234567 → "1,234,567").
fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(ch);
    }
    result.chars().rev().collect()
}

/// Return the colour-indicator emoji for a given percentage.
///
/// * `< 50 %`  → 🟢
/// * `50–80 %` → 🟡
/// * `≥ 80 %`  → 🔴
fn pct_indicator(pct: f64) -> &'static str {
    if pct >= 80.0 {
        "🔴"
    } else if pct >= 50.0 {
        "🟡"
    } else {
        "🟢"
    }
}

/// Build a 50-character bar string, capping fill at 100 %.
///
/// Returns a tuple `(filled_str, empty_str)` each ready for display.
fn build_bar(pct: f64, width: usize) -> (String, String) {
    let capped = pct.clamp(0.0, 100.0);
    let filled = ((capped / 100.0) * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    ("█".repeat(filled), "░".repeat(empty))
}

/// Return the short display name for a model.
fn short_model_name(model: &str) -> &'static str {
    let lower = model.to_lowercase();
    if lower.contains("opus") {
        "Opus"
    } else if lower.contains("sonnet") {
        "Sonnet"
    } else if lower.contains("haiku") {
        "Haiku"
    } else {
        "Other"
    }
}

/// Return the burn-rate tier emoji for a given tokens/min rate.
fn burn_emoji(tokens_per_minute: f64) -> &'static str {
    if tokens_per_minute >= 1000.0 {
        "⚡"
    } else if tokens_per_minute >= 500.0 {
        "🚀"
    } else if tokens_per_minute >= 100.0 {
        "➡️"
    } else {
        "🐌"
    }
}

/// Return the theme style for a model's bar segment.
fn model_bar_style(model: &str, theme: &Theme) -> ratatui::style::Style {
    let lower = model.to_lowercase();
    if lower.contains("opus") {
        theme.model_opus
    } else if lower.contains("sonnet") {
        theme.model_sonnet
    } else if lower.contains("haiku") {
        theme.model_haiku
    } else {
        theme.model_unknown
    }
}

// ── Row builders ──────────────────────────────────────────────────────────────

/// Pad an emoji + label to 25 display columns.
///
/// Most emoji occupy 2 display columns; the space after them is 1 column; the
/// label text is purely ASCII-width.  The function appends trailing spaces so
/// the total reaches 25 columns.
fn pad_label(emoji: &str, label: &str) -> String {
    // Emoji takes ~2 display columns, space = 1, then label text.
    let emoji_width: usize = 2;
    let content_width = emoji_width + 1 + label.len();
    let padding = if content_width < 25 {
        25 - content_width
    } else {
        1
    };
    format!("{} {}{}", emoji, label, " ".repeat(padding))
}

/// Build a progress row with styled components matching the Python output:
///
/// ```text
/// <label><indicator> [<bar>] <pct>%    <current> / <limit>
/// ```
fn progress_row<'a>(
    emoji: &str,
    label: &str,
    percentage: f64,
    current_str: String,
    limit_str: String,
    theme: &'a Theme,
) -> Line<'a> {
    let padded = pad_label(emoji, label);
    let indicator = pct_indicator(percentage);
    let (filled, empty) = build_bar(percentage, 50);
    let bar_style = theme.progress_style(percentage.min(100.0));
    let pct_style = theme.cost_style(percentage);

    Line::from(vec![
        Span::styled(padded, theme.label),
        Span::raw(indicator),
        Span::styled(" [", theme.dim),
        Span::styled(filled, bar_style),
        Span::styled(empty, theme.progress_empty),
        Span::styled("] ", theme.dim),
        Span::styled(format!("{:>5.1}%", percentage), pct_style),
        Span::raw("    "),
        Span::styled(current_str, theme.value),
        Span::styled(" / ", theme.dim),
        Span::styled(limit_str, theme.dim),
    ])
}

// ── Main render ───────────────────────────────────────────────────────────────

/// Render the real-time session view into `area`.
///
/// Everything is drawn as a single [`Paragraph`] whose lines are built to
/// exactly match the Python reference output.
pub fn render_session_view(frame: &mut Frame, area: Rect, data: &SessionViewData, theme: &Theme) {
    let lines = build_session_lines(data, theme);
    let paragraph = Paragraph::new(Text::from(lines));
    frame.render_widget(paragraph, area);
}

/// Build the full `Vec<Line>` for the session view (extracted for testability).
pub fn build_session_lines<'a>(data: &SessionViewData, theme: &'a Theme) -> Vec<Line<'a>> {
    // Pre-allocate with enough capacity for all rows.
    let mut lines: Vec<Line<'a>> = Vec::with_capacity(32);

    // ── Header ────────────────────────────────────────────────────────────────
    // Line 1: title
    lines.push(Line::from(vec![
        Span::styled("✦ ✧ ✦ ✧", theme.header_sparkle),
        Span::styled(" CLAUDE CODE USAGE MONITOR ", theme.header),
        Span::styled("✦ ✧ ✦ ✧", theme.header_sparkle),
    ]));
    // Line 2: separator
    lines.push(Line::from(Span::styled("=".repeat(78), theme.separator)));
    // Line 3: plan | timezone
    lines.push(Line::from(vec![
        Span::styled("[ ", theme.label),
        Span::styled(data.plan.to_lowercase(), theme.value),
        Span::styled(" | ", theme.label),
        Span::styled(data.timezone.to_lowercase(), theme.value),
        Span::styled(" ]", theme.label),
    ]));
    // Lines 4-6: three empty lines (Python output has blank lines here)
    lines.push(Line::from(""));
    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // ── Cost Usage ────────────────────────────────────────────────────────────
    let cost_pct = if data.cost_limit > 0.0 {
        (data.cost_usd / data.cost_limit) * 100.0
    } else {
        0.0
    };
    lines.push(progress_row(
        "💰",
        "Cost Usage:",
        cost_pct,
        format!("${:.2}", data.cost_usd),
        format!("${:.2}", data.cost_limit),
        theme,
    ));
    lines.push(Line::from(""));

    // ── Messages Usage ────────────────────────────────────────────────────────
    let msg_pct = if data.message_limit > 0 {
        (data.sent_messages as f64 / data.message_limit as f64) * 100.0
    } else {
        0.0
    };
    lines.push(progress_row(
        "📨",
        "Messages Usage:",
        msg_pct,
        format_with_commas(data.sent_messages as u64),
        format_with_commas(data.message_limit as u64),
        theme,
    ));
    lines.push(Line::from(""));

    // ── Token Usage ───────────────────────────────────────────────────────────
    // Percentage can exceed 100 % for display purposes; bar is capped at 100 %.
    let token_pct = if data.token_limit > 0 {
        (data.tokens_used as f64 / data.token_limit as f64) * 100.0
    } else {
        0.0
    };
    let padded_token = pad_label("📊", "Token Usage:");
    let token_indicator = pct_indicator(token_pct);
    let (filled_tok, empty_tok) = build_bar(token_pct, 50);
    let bar_style_tok = theme.progress_style(token_pct.min(100.0));
    let token_pct_style = theme.cost_style(token_pct);
    lines.push(Line::from(vec![
        Span::styled(padded_token, theme.label),
        Span::raw(token_indicator),
        Span::styled(" [", theme.dim),
        Span::styled(filled_tok, bar_style_tok),
        Span::styled(empty_tok, theme.progress_empty),
        Span::styled("] ", theme.dim),
        Span::styled(format!("{:>5.1}%", token_pct), token_pct_style),
        Span::raw("    "),
        Span::styled(format_with_commas(data.tokens_used), theme.value),
        Span::styled(" / ", theme.dim),
        Span::styled(format_with_commas(data.token_limit), theme.dim),
    ]));
    lines.push(Line::from(""));

    // ── Cache Tokens ──────────────────────────────────────────────────────────
    lines.push(Line::from(vec![
        Span::styled(pad_label("💾", "Cache Tokens:"), theme.label),
        Span::styled("Creation: ", theme.dim),
        Span::styled(format_with_commas(data.cache_creation_tokens), theme.value),
        Span::styled("  Read: ", theme.dim),
        Span::styled(format_with_commas(data.cache_read_tokens), theme.value),
    ]));
    lines.push(Line::from(""));

    // ── Thin separator ────────────────────────────────────────────────────────
    lines.push(Line::from(Span::styled("─".repeat(78), theme.separator)));

    // ── Time to Reset ─────────────────────────────────────────────────────────
    let time_pct = if data.total_minutes > 0.0 {
        (data.elapsed_minutes / data.total_minutes * 100.0).min(100.0)
    } else {
        0.0
    };
    let remaining_mins = (data.total_minutes - data.elapsed_minutes).max(0.0);
    let hours = (remaining_mins / 60.0) as u64;
    let mins = (remaining_mins % 60.0) as u64;
    let time_suffix = format!("{}h {}m", hours, mins);

    let padded_time = pad_label("⏱️", "Time to Reset:");
    let time_indicator = pct_indicator(time_pct);
    let (filled_time, empty_time) = build_bar(time_pct, 50);
    let bar_style_time = theme.progress_style(time_pct);
    lines.push(Line::from(vec![
        Span::styled(padded_time, theme.label),
        Span::raw(time_indicator),
        Span::styled(" [", theme.dim),
        Span::styled(filled_time, bar_style_time),
        Span::styled(empty_time, theme.progress_empty),
        Span::styled("] ", theme.dim),
        Span::styled(time_suffix, theme.value),
    ]));
    lines.push(Line::from(""));

    // ── Model Distribution ────────────────────────────────────────────────────
    let padded_model = pad_label("🤖", "Model Distribution:");

    // Build proportionally coloured bar segments per model.
    let bar_width: usize = 50;
    let mut model_spans: Vec<Span<'a>> = Vec::new();
    let mut total_filled: usize = 0;
    let active_models: Vec<&(String, f64)> = data
        .per_model_stats
        .iter()
        .filter(|(_, pct)| *pct > 0.0)
        .collect();

    for (i, (model, pct)) in active_models.iter().enumerate() {
        let chars = ((*pct / 100.0) * bar_width as f64).floor() as usize;
        // Last model gets the remaining chars to fill exactly bar_width.
        let chars = if i == active_models.len() - 1 {
            bar_width.saturating_sub(total_filled)
        } else {
            chars.min(bar_width.saturating_sub(total_filled))
        };
        if chars > 0 {
            let segment = "█".repeat(chars);
            let style = model_bar_style(model, theme);
            model_spans.push(Span::styled(segment, style));
            total_filled += chars;
        }
    }
    // If no models, fill with empty.
    if total_filled < bar_width {
        model_spans.push(Span::styled(
            "░".repeat(bar_width - total_filled),
            theme.progress_empty,
        ));
    }

    let mut row_spans: Vec<Span<'a>> = Vec::with_capacity(6 + model_spans.len());
    row_spans.push(Span::styled(padded_model, theme.label));
    row_spans.push(Span::raw("🤖"));
    row_spans.push(Span::styled(" [", theme.dim));
    row_spans.extend(model_spans);
    row_spans.push(Span::styled("] ", theme.dim));

    // Build model summary with per-model colors and dimmed separators.
    let visible_models: Vec<&(String, f64)> = data
        .per_model_stats
        .iter()
        .filter(|(_, pct)| *pct > 0.0)
        .collect();
    if visible_models.is_empty() {
        row_spans.push(Span::styled("No data", theme.dim));
    } else {
        for (i, (model, pct)) in visible_models.iter().enumerate() {
            if i > 0 {
                row_spans.push(Span::styled(" | ", theme.dim));
            }
            let style = model_bar_style(model, theme);
            row_spans.push(Span::styled(
                format!("{} {:.1}%", short_model_name(model), pct),
                style,
            ));
        }
    }
    lines.push(Line::from(row_spans));

    // ── Second thin separator ─────────────────────────────────────────────────
    lines.push(Line::from(Span::styled("─".repeat(78), theme.separator)));

    // ── Burn Rate ─────────────────────────────────────────────────────────────
    if let Some(ref br) = data.burn_rate {
        let emoji = burn_emoji(br.tokens_per_minute);
        let velocity_style = theme.velocity_style(br.tokens_per_minute);
        lines.push(Line::from(vec![
            Span::styled(pad_label("🔥", "Burn Rate:"), theme.label),
            Span::styled(
                format!("{:.1} tokens/min", br.tokens_per_minute),
                velocity_style,
            ),
            Span::raw(" "),
            Span::raw(emoji),
        ]));

        // ── Cost Rate ─────────────────────────────────────────────────────────
        let cost_per_min = if data.elapsed_minutes > 0.0 {
            data.cost_usd / data.elapsed_minutes
        } else {
            0.0
        };
        lines.push(Line::from(vec![
            Span::styled(pad_label("💲", "Cost Rate:"), theme.label),
            Span::styled(format!("${:.4} $/min", cost_per_min), theme.value),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled(pad_label("🔥", "Burn Rate:"), theme.label),
            Span::styled("--", theme.dim),
        ]));
        lines.push(Line::from(vec![
            Span::styled(pad_label("💲", "Cost Rate:"), theme.label),
            Span::styled("--", theme.dim),
        ]));
    }
    lines.push(Line::from(""));

    // ── Predictions ───────────────────────────────────────────────────────────
    lines.push(Line::from(Span::styled("🔮 Predictions:", theme.info)));
    let predicted_end_str = data.predicted_end.as_deref().unwrap_or("N/A").to_string();
    lines.push(Line::from(vec![
        Span::styled("  Tokens will run out:  ", theme.dim),
        Span::styled(predicted_end_str, theme.warning),
    ]));
    lines.push(Line::from(vec![
        Span::styled("  Limit resets at:      ", theme.dim),
        Span::styled(data.reset_time.clone(), theme.value),
    ]));
    lines.push(Line::from(""));

    // ── Status bar ────────────────────────────────────────────────────────────
    let (status_text, status_style) = if data.is_active {
        ("Active session", theme.success)
    } else {
        ("Inactive", theme.dim)
    };
    lines.push(Line::from(vec![
        Span::styled("⏰ ", theme.info),
        Span::styled(data.current_time.clone(), theme.info),
        Span::raw("          "),
        Span::styled("📝 ", theme.dim),
        Span::styled(status_text, status_style),
        Span::styled(" | Ctrl+C to exit ", theme.dim),
        Span::styled("🟢", theme.success),
    ]));

    lines
}

/// Render the "no active session" waiting screen.
///
/// Used when there is no [`SessionViewData`] available yet (first startup or
/// between sessions).
pub fn render_no_session(frame: &mut Frame, area: Rect, theme: &Theme) {
    let text = vec![
        Line::from(""),
        Line::from(Span::styled("No active session detected", theme.dim)),
        Line::from(""),
        Line::from(Span::styled("Waiting for Claude activity...", theme.info)),
        Line::from(Span::styled("Press 'q' or Ctrl+C to exit", theme.dim)),
    ];
    let paragraph = Paragraph::new(Text::from(text)).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Claude Monitor "),
    );
    frame.render_widget(paragraph, area);
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::Theme;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn make_session_data() -> SessionViewData {
        SessionViewData {
            plan: "pro".to_string(),
            timezone: "UTC".to_string(),
            tokens_used: 5_000,
            token_limit: 19_000,
            cost_usd: 2.50,
            cost_limit: 18.0,
            elapsed_minutes: 90.0,
            total_minutes: 300.0,
            burn_rate: Some(BurnRate {
                tokens_per_minute: 55.5,
                cost_per_hour: 1.67,
            }),
            per_model_stats: vec![
                ("claude-3-5-sonnet".to_string(), 75.0),
                ("claude-3-haiku".to_string(), 25.0),
            ],
            sent_messages: 42,
            message_limit: 250,
            current_time: "12:00:00".to_string(),
            reset_time: "17:00:00".to_string(),
            predicted_end: Some("14:30:00".to_string()),
            is_active: true,
            notifications: vec!["80% token limit reached".to_string()],
            cache_creation_tokens: 1_000,
            cache_read_tokens: 5_000,
        }
    }

    // ── Data construction ─────────────────────────────────────────────────────

    #[test]
    fn test_session_view_data_creation() {
        let data = make_session_data();
        assert_eq!(data.plan, "pro");
        assert_eq!(data.tokens_used, 5_000);
        assert_eq!(data.token_limit, 19_000);
        assert_eq!(data.sent_messages, 42);
        assert!(data.burn_rate.is_some());
        assert_eq!(data.notifications.len(), 1);
        assert_eq!(data.cache_creation_tokens, 1_000);
        assert_eq!(data.cache_read_tokens, 5_000);
    }

    #[test]
    fn test_session_view_data_no_burn_rate() {
        let mut data = make_session_data();
        data.burn_rate = None;
        assert!(data.burn_rate.is_none());
    }

    #[test]
    fn test_session_view_data_no_predicted_end() {
        let mut data = make_session_data();
        data.predicted_end = None;
        assert!(data.predicted_end.is_none());
    }

    #[test]
    fn test_session_view_data_empty_notifications() {
        let mut data = make_session_data();
        data.notifications.clear();
        assert!(data.notifications.is_empty());
    }

    // ── build_session_lines content checks ───────────────────────────────────

    #[test]
    fn test_lines_contain_title() {
        let theme = Theme::dark();
        let data = make_session_data();
        let lines = build_session_lines(&data, &theme);
        let title: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            title.contains("CLAUDE CODE USAGE MONITOR"),
            "title missing: {title}"
        );
    }

    #[test]
    fn test_lines_contain_separator_equals() {
        let theme = Theme::dark();
        let data = make_session_data();
        let lines = build_session_lines(&data, &theme);
        let sep: String = lines[1].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(sep.chars().count(), 78, "separator width");
        assert!(sep.chars().all(|c| c == '='), "separator chars: {sep}");
    }

    #[test]
    fn test_lines_contain_plan_timezone() {
        let theme = Theme::dark();
        let data = make_session_data();
        let lines = build_session_lines(&data, &theme);
        let info: String = lines[2].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(info.contains("pro"), "plan: {info}");
        assert!(info.contains("utc"), "timezone: {info}");
        assert!(info.contains("[ ") && info.contains(" | ") && info.contains(" ]"));
    }

    #[test]
    fn test_lines_contain_cache_tokens() {
        let theme = Theme::dark();
        let data = make_session_data();
        let lines = build_session_lines(&data, &theme);
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref().to_string()))
            .collect::<Vec<_>>()
            .join("");
        assert!(
            all_text.contains("Cache Tokens"),
            "no cache row: {all_text}"
        );
        assert!(all_text.contains("1,000"), "cache creation: {all_text}");
        assert!(all_text.contains("5,000"), "cache read: {all_text}");
    }

    #[test]
    fn test_lines_contain_burn_rate() {
        let theme = Theme::dark();
        let data = make_session_data();
        let lines = build_session_lines(&data, &theme);
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref().to_string()))
            .collect::<Vec<_>>()
            .join("");
        assert!(all_text.contains("Burn Rate"), "no burn rate: {all_text}");
        assert!(all_text.contains("tokens/min"), "no tokens/min: {all_text}");
    }

    #[test]
    fn test_lines_contain_cost_rate() {
        let theme = Theme::dark();
        let data = make_session_data();
        let lines = build_session_lines(&data, &theme);
        let all_text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref().to_string()))
            .collect::<Vec<_>>()
            .join("");
        assert!(all_text.contains("Cost Rate"), "no cost rate: {all_text}");
        assert!(all_text.contains("$/min"), "no $/min: {all_text}");
    }

    #[test]
    fn test_format_with_commas() {
        assert_eq!(super::format_with_commas(0), "0");
        assert_eq!(super::format_with_commas(999), "999");
        assert_eq!(super::format_with_commas(1_000), "1,000");
        assert_eq!(super::format_with_commas(1_234_567), "1,234,567");
    }

    #[test]
    fn test_pct_indicator() {
        assert_eq!(super::pct_indicator(0.0), "🟢");
        assert_eq!(super::pct_indicator(49.9), "🟢");
        assert_eq!(super::pct_indicator(50.0), "🟡");
        assert_eq!(super::pct_indicator(79.9), "🟡");
        assert_eq!(super::pct_indicator(80.0), "🔴");
        assert_eq!(super::pct_indicator(143.3), "🔴");
    }

    #[test]
    fn test_build_bar_full_when_over_100() {
        let (filled, empty) = super::build_bar(143.3, 50);
        assert_eq!(filled.chars().count(), 50, "bar should be full at 143%");
        assert!(empty.is_empty(), "empty portion should be empty at 143%");
    }

    #[test]
    fn test_build_bar_partial() {
        let (filled, empty) = super::build_bar(50.0, 50);
        assert_eq!(filled.chars().count(), 25);
        assert_eq!(empty.chars().count(), 25);
    }

    #[test]
    fn test_short_model_name() {
        assert_eq!(super::short_model_name("claude-3-5-sonnet"), "Sonnet");
        assert_eq!(super::short_model_name("claude-opus-4"), "Opus");
        assert_eq!(super::short_model_name("claude-3-haiku"), "Haiku");
        assert_eq!(super::short_model_name("gpt-4"), "Other");
    }

    // ── Model Distribution styled spans ─────────────────────────────────────

    #[test]
    fn test_model_distribution_has_colored_labels_and_dimmed_separators() {
        use ratatui::style::Color;

        let theme = Theme::dark();
        let data = make_session_data(); // sonnet 75%, haiku 25%
        let lines = build_session_lines(&data, &theme);

        // Find the model distribution line (contains "Model Distribution").
        let model_line = lines
            .iter()
            .find(|l| {
                l.spans
                    .iter()
                    .any(|s| s.content.contains("Model Distribution"))
            })
            .expect("model distribution line must exist");

        // Check that "Sonnet 75.0%" span has model_sonnet color (Cyan in dark).
        let sonnet_span = model_line
            .spans
            .iter()
            .find(|s| s.content.contains("Sonnet"))
            .expect("Sonnet label span must exist");
        assert_eq!(
            sonnet_span.style.fg,
            Some(Color::Cyan),
            "Sonnet label must be Cyan (model_sonnet), got: {:?}",
            sonnet_span.style.fg
        );

        // Check that "Haiku 25.0%" span has model_haiku color (Green in dark).
        let haiku_span = model_line
            .spans
            .iter()
            .find(|s| s.content.contains("Haiku"))
            .expect("Haiku label span must exist");
        assert_eq!(
            haiku_span.style.fg,
            Some(Color::Green),
            "Haiku label must be Green (model_haiku), got: {:?}",
            haiku_span.style.fg
        );

        // Check that separator " | " span uses dim style.
        let sep_span = model_line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == " | ");
        assert!(sep_span.is_some(), "dimmed ' | ' separator span must exist");
        let sep = sep_span.unwrap();
        assert_eq!(
            sep.style, theme.dim,
            "separator must use dim style, got: {:?}",
            sep.style
        );
    }

    // ── Render (does not panic) ───────────────────────────────────────────────

    #[test]
    fn test_render_session_view_does_not_panic() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        let data = make_session_data();

        terminal
            .draw(|frame| {
                let area = frame.area();
                render_session_view(frame, area, &data, &theme);
            })
            .unwrap();
    }

    #[test]
    fn test_render_session_view_no_models_does_not_panic() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        let mut data = make_session_data();
        data.per_model_stats.clear();

        terminal
            .draw(|frame| {
                let area = frame.area();
                render_session_view(frame, area, &data, &theme);
            })
            .unwrap();
    }

    #[test]
    fn test_render_session_view_no_burn_rate_does_not_panic() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        let mut data = make_session_data();
        data.burn_rate = None;

        terminal
            .draw(|frame| {
                let area = frame.area();
                render_session_view(frame, area, &data, &theme);
            })
            .unwrap();
    }

    #[test]
    fn test_render_no_session_does_not_panic() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                render_no_session(frame, area, &theme);
            })
            .unwrap();
    }

    #[test]
    fn test_render_session_view_with_light_theme_does_not_panic() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::light();
        let data = make_session_data();

        terminal
            .draw(|frame| {
                let area = frame.area();
                render_session_view(frame, area, &data, &theme);
            })
            .unwrap();
    }

    #[test]
    fn test_render_session_view_zero_limits_does_not_panic() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        let mut data = make_session_data();
        data.token_limit = 0;
        data.cost_limit = 0.0;
        data.message_limit = 0;
        data.total_minutes = 0.0;

        terminal
            .draw(|frame| {
                let area = frame.area();
                render_session_view(frame, area, &data, &theme);
            })
            .unwrap();
    }

    #[test]
    fn test_render_session_view_over_limit_tokens_does_not_panic() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        let mut data = make_session_data();
        // Simulate > 100 % token usage
        data.tokens_used = 315_310;
        data.token_limit = 220_000;

        terminal
            .draw(|frame| {
                let area = frame.area();
                render_session_view(frame, area, &data, &theme);
            })
            .unwrap();
    }
}
