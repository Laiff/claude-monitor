use crate::themes::Theme;
use ratatui::text::{Line, Span};

/// Configuration controlling visual appearance of a progress bar.
pub struct ProgressBarConfig {
    /// Total width in terminal columns of the bar portion (excluding label).
    pub width: u16,
    /// Character used to fill the completed portion of the bar.
    pub filled_char: char,
    /// Character used to fill the empty portion of the bar.
    pub empty_char: char,
    /// Whether to append a percentage figure after the bar.
    pub show_percentage: bool,
    /// Whether to append a textual label (e.g. counts) after the bar.
    pub show_label: bool,
}

impl Default for ProgressBarConfig {
    fn default() -> Self {
        Self {
            width: 50,
            filled_char: '\u{2588}', // █  FULL BLOCK
            empty_char: '\u{2591}',  // ░  LIGHT SHADE
            show_percentage: true,
            show_label: true,
        }
    }
}

// ── TokenProgressBar ─────────────────────────────────────────────────────────

/// Horizontal progress bar that shows token usage relative to a token limit.
///
/// Renders as a coloured fill + empty portion followed by a label that shows
/// the percentage and the `current / limit` counts formatted with thousands
/// separators.
pub struct TokenProgressBar<'a> {
    /// Percentage of the limit consumed, clamped to `[0.0, 100.0]`.
    pub percentage: f64,
    /// Tokens consumed so far.
    pub current: u64,
    /// Maximum token limit.
    pub limit: u64,
    /// Theme from which colour styles are taken.
    pub theme: &'a Theme,
    /// Visual configuration.
    pub config: ProgressBarConfig,
}

impl<'a> TokenProgressBar<'a> {
    /// Construct a new bar, computing the percentage automatically.
    pub fn new(current: u64, limit: u64, theme: &'a Theme) -> Self {
        let percentage = if limit > 0 {
            ((current as f64 / limit as f64) * 100.0).min(100.0)
        } else {
            0.0
        };
        Self {
            percentage,
            current,
            limit,
            theme,
            config: ProgressBarConfig::default(),
        }
    }

    /// Render the progress bar as a [`Line`] suitable for embedding in any
    /// ratatui widget that accepts `Line` values.
    pub fn to_line(&self) -> Line<'a> {
        let filled = ((self.percentage / 100.0) * self.config.width as f64) as u16;
        let empty = self.config.width.saturating_sub(filled);

        let bar_style = self.theme.progress_style(self.percentage);

        let filled_str: String =
            std::iter::repeat_n(self.config.filled_char, filled as usize).collect();
        let empty_str: String =
            std::iter::repeat_n(self.config.empty_char, empty as usize).collect();

        let label = format!(
            " {:.1}% ({}/{})",
            self.percentage,
            monitor_core::formatting::format_number(self.current as f64, 0),
            monitor_core::formatting::format_number(self.limit as f64, 0),
        );

        Line::from(vec![
            Span::styled(filled_str, bar_style),
            Span::styled(empty_str, self.theme.progress_empty),
            Span::styled(label, self.theme.progress_label),
        ])
    }
}

// ── TimeProgressBar ──────────────────────────────────────────────────────────

/// Horizontal progress bar that shows elapsed time within a session window.
///
/// The label shows how much time remains, formatted via
/// [`monitor_core::formatting::format_time`].
pub struct TimeProgressBar<'a> {
    /// Minutes that have elapsed in the current session.
    pub elapsed_minutes: f64,
    /// Total duration of the session window in minutes.
    pub total_minutes: f64,
    /// Theme from which colour styles are taken.
    pub theme: &'a Theme,
    /// Visual configuration.
    pub config: ProgressBarConfig,
}

impl<'a> TimeProgressBar<'a> {
    /// Construct a new time bar.
    pub fn new(elapsed: f64, total: f64, theme: &'a Theme) -> Self {
        Self {
            elapsed_minutes: elapsed,
            total_minutes: total,
            theme,
            config: ProgressBarConfig::default(),
        }
    }

    /// Render the progress bar as a [`Line`].
    pub fn to_line(&self) -> Line<'a> {
        let percentage = if self.total_minutes > 0.0 {
            (self.elapsed_minutes / self.total_minutes * 100.0).min(100.0)
        } else {
            0.0
        };

        let filled = ((percentage / 100.0) * self.config.width as f64) as u16;
        let empty = self.config.width.saturating_sub(filled);

        let bar_style = self.theme.progress_style(percentage);

        let filled_str: String =
            std::iter::repeat_n(self.config.filled_char, filled as usize).collect();
        let empty_str: String =
            std::iter::repeat_n(self.config.empty_char, empty as usize).collect();

        let remaining = (self.total_minutes - self.elapsed_minutes).max(0.0);
        let label = format!(
            " {} remaining",
            monitor_core::formatting::format_time(remaining)
        );

        Line::from(vec![
            Span::styled(filled_str, bar_style),
            Span::styled(empty_str, self.theme.progress_empty),
            Span::styled(label, self.theme.progress_label),
        ])
    }
}

// ── ModelUsageBar ────────────────────────────────────────────────────────────

/// A proportional multi-coloured bar that visualises per-model token usage.
///
/// Each model is rendered as a contiguous coloured segment whose width is
/// proportional to its share of total usage.  Short text labels follow the bar.
pub struct ModelUsageBar<'a> {
    /// Ordered list of `(model_name, percentage)` pairs.  Percentages should
    /// sum to ≤ 100.
    pub model_percentages: Vec<(String, f64)>,
    /// Theme from which model colour styles are taken.
    pub theme: &'a Theme,
    /// Total width of the bar in terminal columns.
    pub width: u16,
}

impl<'a> ModelUsageBar<'a> {
    /// Construct a new model usage bar.
    pub fn new(model_percentages: Vec<(String, f64)>, theme: &'a Theme) -> Self {
        Self {
            model_percentages,
            theme,
            width: 50,
        }
    }

    /// Render the bar as a [`Line`].
    pub fn to_line(&self) -> Line<'a> {
        let mut spans: Vec<Span<'a>> = Vec::new();

        // Coloured segments proportional to each model's share.
        for (model, pct) in &self.model_percentages {
            let chars = ((*pct / 100.0) * self.width as f64).round() as usize;
            if chars > 0 {
                let segment = "█".repeat(chars);
                spans.push(Span::styled(segment, self.theme.model_style(model)));
            }
        }

        // Space between bar and labels.
        spans.push(Span::raw(" "));

        // Textual labels after the bar.
        for (model, pct) in &self.model_percentages {
            if *pct > 0.0 {
                spans.push(Span::styled(
                    format!("{}: {:.0}% ", model, pct),
                    self.theme.model_style(model),
                ));
            }
        }

        Line::from(spans)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::Theme;

    // ── TokenProgressBar ─────────────────────────────────────────────────────

    #[test]
    fn test_token_progress_bar_to_line() {
        let theme = Theme::dark();
        let bar = TokenProgressBar::new(250, 1000, &theme);

        // 25 % usage: should yield exactly 3 spans
        let line = bar.to_line();
        assert_eq!(
            line.spans.len(),
            3,
            "expected 3 spans: filled, empty, label"
        );

        // Filled portion: 25 % of 50 columns = 12 chars of '█'
        let filled_span = &line.spans[0];
        assert_eq!(filled_span.content.chars().count(), 12);
        assert!(filled_span.content.chars().all(|c| c == '█'));

        // Empty portion: 50 − 12 = 38 chars of '░'
        let empty_span = &line.spans[1];
        assert_eq!(empty_span.content.chars().count(), 38);
        assert!(empty_span.content.chars().all(|c| c == '░'));

        // Label contains the percentage and counts.
        let label = &line.spans[2].content;
        assert!(label.contains("25.0%"), "label was: {label}");
        assert!(label.contains("250"), "label was: {label}");
        assert!(label.contains("1,000"), "label was: {label}");
    }

    #[test]
    fn test_token_progress_bar_zero() {
        let theme = Theme::dark();
        let bar = TokenProgressBar::new(0, 1000, &theme);
        let line = bar.to_line();

        // With 0 % usage the filled span should be empty.
        assert_eq!(line.spans[0].content.len(), 0);
        // Empty span should fill the full width.
        assert_eq!(line.spans[1].content.chars().count(), 50);
    }

    #[test]
    fn test_token_progress_bar_full() {
        let theme = Theme::dark();
        let bar = TokenProgressBar::new(1000, 1000, &theme);
        let line = bar.to_line();

        // 100 % usage: filled span must be exactly 50 chars wide.
        assert_eq!(line.spans[0].content.chars().count(), 50);
        // Empty span should be empty.
        assert_eq!(line.spans[1].content.len(), 0);

        let label = &line.spans[2].content;
        assert!(label.contains("100.0%"), "label was: {label}");
    }

    #[test]
    fn test_token_progress_bar_zero_limit() {
        // When limit == 0 the percentage must default to 0.0 (no divide-by-zero).
        let theme = Theme::dark();
        let bar = TokenProgressBar::new(500, 0, &theme);
        assert_eq!(bar.percentage, 0.0);
        let line = bar.to_line();
        // Should produce three spans without panicking.
        assert_eq!(line.spans.len(), 3);
    }

    // ── TimeProgressBar ──────────────────────────────────────────────────────

    #[test]
    fn test_time_progress_bar_remaining() {
        let theme = Theme::dark();
        // 150 out of 300 minutes elapsed → 50 % → yellow fill.
        let bar = TimeProgressBar::new(150.0, 300.0, &theme);
        let line = bar.to_line();

        assert_eq!(line.spans.len(), 3);

        let label = &line.spans[2].content;
        // 150 minutes remaining → "2h 30m remaining"
        assert!(
            label.contains("2h 30m remaining"),
            "unexpected label: {label}"
        );
    }

    #[test]
    fn test_time_progress_bar_zero_total() {
        let theme = Theme::dark();
        let bar = TimeProgressBar::new(0.0, 0.0, &theme);
        let line = bar.to_line();
        // Must not panic; filled span should be empty.
        assert_eq!(line.spans[0].content.len(), 0);
    }

    #[test]
    fn test_time_progress_bar_elapsed_exceeds_total() {
        let theme = Theme::dark();
        // Elapsed > total must not produce negative remaining time.
        let bar = TimeProgressBar::new(400.0, 300.0, &theme);
        let line = bar.to_line();
        let label = &line.spans[2].content;
        // Remaining clamped to 0 → "0m remaining"
        assert!(label.contains("0m remaining"), "unexpected label: {label}");
    }

    // ── ModelUsageBar ────────────────────────────────────────────────────────

    #[test]
    fn test_model_usage_bar_multiple_models() {
        let theme = Theme::dark();
        let models = vec![
            ("claude-3-opus".to_string(), 40.0_f64),
            ("claude-3-5-sonnet".to_string(), 35.0_f64),
            ("claude-3-haiku".to_string(), 25.0_f64),
        ];
        let bar = ModelUsageBar::new(models, &theme);
        let line = bar.to_line();

        // Must have at least the 3 coloured segments + 1 space + 3 labels = 7+.
        assert!(line.spans.len() >= 7, "got {} spans", line.spans.len());

        // Verify each model's label appears somewhere in the combined text.
        let full_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(full_text.contains("claude-3-opus"), "text: {full_text}");
        assert!(full_text.contains("claude-3-5-sonnet"), "text: {full_text}");
        assert!(full_text.contains("claude-3-haiku"), "text: {full_text}");
    }

    #[test]
    fn test_model_usage_bar_single_model() {
        let theme = Theme::dark();
        let models = vec![("claude-3-5-sonnet".to_string(), 100.0_f64)];
        let bar = ModelUsageBar::new(models, &theme);
        let line = bar.to_line();

        // The segment chars should total 50 (full width).
        let segment_chars: usize = line
            .spans
            .iter()
            .take(1) // first span is the segment
            .map(|s| s.content.chars().count())
            .sum();
        assert_eq!(segment_chars, 50);
    }

    #[test]
    fn test_model_usage_bar_zero_percentage_skipped() {
        let theme = Theme::dark();
        let models = vec![
            ("claude-3-opus".to_string(), 100.0_f64),
            ("claude-3-haiku".to_string(), 0.0_f64),
        ];
        let bar = ModelUsageBar::new(models, &theme);
        let line = bar.to_line();

        // haiku has 0 % – its label must not appear.
        let full_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            !full_text.contains("claude-3-haiku"),
            "zero-pct model should not appear: {full_text}"
        );
    }
}
