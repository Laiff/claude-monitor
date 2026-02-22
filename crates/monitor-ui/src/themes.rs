use ratatui::style::{Color, Modifier, Style};

/// Terminal background type detection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BackgroundType {
    Dark,
    Light,
    Unknown,
}

/// Detect terminal background type from the `COLORFGBG` environment variable.
///
/// The variable has the format `"foreground;background"`.  Background values
/// 0–6 are considered dark; 7–15 are considered light.  If the variable is
/// absent or unparseable, `BackgroundType::Dark` is returned as the safe
/// default.
pub fn detect_background() -> BackgroundType {
    if let Ok(val) = std::env::var("COLORFGBG") {
        if let Some(bg) = val.split(';').next_back() {
            if let Ok(bg_num) = bg.parse::<u8>() {
                return if bg_num <= 6 {
                    BackgroundType::Dark
                } else {
                    BackgroundType::Light
                };
            }
        }
    }
    BackgroundType::Dark
}

/// Complete theme definition carrying all UI styles used by monitor-ui
/// components.
#[derive(Debug, Clone)]
pub struct Theme {
    // ── Header ───────────────────────────────────────────────────────────────
    pub header: Style,
    pub header_sparkle: Style,
    pub separator: Style,

    // ── Text ─────────────────────────────────────────────────────────────────
    pub text: Style,
    pub dim: Style,
    pub bold: Style,
    pub label: Style,
    pub value: Style,

    // ── Status ───────────────────────────────────────────────────────────────
    pub info: Style,
    pub success: Style,
    pub warning: Style,
    pub error: Style,

    // ── Progress bars ────────────────────────────────────────────────────────
    /// Filled portion when usage is below 50 %.
    pub progress_low: Style,
    /// Filled portion when usage is between 50 % and 80 %.
    pub progress_medium: Style,
    /// Filled portion when usage is at or above 80 %.
    pub progress_high: Style,
    /// Unfilled (empty) portion of a progress bar.
    pub progress_empty: Style,
    pub progress_label: Style,

    // ── Cost ─────────────────────────────────────────────────────────────────
    pub cost_low: Style,
    pub cost_medium: Style,
    pub cost_high: Style,

    // ── Models ───────────────────────────────────────────────────────────────
    pub model_opus: Style,
    pub model_sonnet: Style,
    pub model_haiku: Style,
    pub model_unknown: Style,

    // ── Table ────────────────────────────────────────────────────────────────
    pub table_header: Style,
    pub table_border: Style,
    pub table_row: Style,
    pub table_row_alt: Style,
    pub table_total: Style,

    // ── Notifications ────────────────────────────────────────────────────────
    pub notification_info: Style,
    pub notification_warning: Style,
    pub notification_error: Style,

    // ── Burn rate / velocity ─────────────────────────────────────────────────
    /// Snail – very low burn rate.
    pub velocity_slow: Style,
    /// Arrow – moderate burn rate.
    pub velocity_normal: Style,
    /// Rocket – high burn rate.
    pub velocity_fast: Style,
    /// Lightning – extreme burn rate.
    pub velocity_extreme: Style,
}

impl Theme {
    // ── Constructors ─────────────────────────────────────────────────────────

    /// Dark-background terminal theme (default).
    pub fn dark() -> Self {
        Self {
            header: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            header_sparkle: Style::default().fg(Color::Yellow),
            separator: Style::default().fg(Color::DarkGray),

            text: Style::default().fg(Color::White),
            dim: Style::default().fg(Color::DarkGray),
            bold: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            label: Style::default().fg(Color::Gray),
            value: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),

            info: Style::default().fg(Color::Cyan),
            success: Style::default().fg(Color::Green),
            warning: Style::default().fg(Color::Yellow),
            error: Style::default().fg(Color::Red),

            progress_low: Style::default().fg(Color::Green),
            progress_medium: Style::default().fg(Color::Yellow),
            progress_high: Style::default().fg(Color::Red),
            progress_empty: Style::default().fg(Color::DarkGray),
            progress_label: Style::default().fg(Color::Gray),

            cost_low: Style::default().fg(Color::Green),
            cost_medium: Style::default().fg(Color::Yellow),
            cost_high: Style::default().fg(Color::Red),

            model_opus: Style::default().fg(Color::Magenta),
            model_sonnet: Style::default().fg(Color::Cyan),
            model_haiku: Style::default().fg(Color::Green),
            model_unknown: Style::default().fg(Color::Gray),

            table_header: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            table_border: Style::default().fg(Color::DarkGray),
            table_row: Style::default().fg(Color::White),
            table_row_alt: Style::default().fg(Color::Gray),
            table_total: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),

            notification_info: Style::default().fg(Color::Cyan),
            notification_warning: Style::default().fg(Color::Yellow),
            notification_error: Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),

            velocity_slow: Style::default().fg(Color::Green),
            velocity_normal: Style::default().fg(Color::Cyan),
            velocity_fast: Style::default().fg(Color::Yellow),
            velocity_extreme: Style::default().fg(Color::Red),
        }
    }

    /// Light-background terminal theme.
    ///
    /// Uses dark colours for text and bright accent colours so that content
    /// remains legible against a white/light-grey terminal canvas.
    pub fn light() -> Self {
        Self {
            header: Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
            header_sparkle: Style::default().fg(Color::Magenta),
            separator: Style::default().fg(Color::Gray),

            text: Style::default().fg(Color::Black),
            dim: Style::default().fg(Color::Gray),
            bold: Style::default()
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
            label: Style::default().fg(Color::DarkGray),
            value: Style::default()
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),

            info: Style::default().fg(Color::Blue),
            success: Style::default().fg(Color::Green),
            warning: Style::default().fg(Color::Yellow),
            error: Style::default().fg(Color::Red),

            progress_low: Style::default().fg(Color::Green),
            progress_medium: Style::default().fg(Color::Yellow),
            progress_high: Style::default().fg(Color::Red),
            progress_empty: Style::default().fg(Color::Gray),
            progress_label: Style::default().fg(Color::DarkGray),

            cost_low: Style::default().fg(Color::Green),
            cost_medium: Style::default().fg(Color::Yellow),
            cost_high: Style::default().fg(Color::Red),

            model_opus: Style::default().fg(Color::Magenta),
            model_sonnet: Style::default().fg(Color::Blue),
            model_haiku: Style::default().fg(Color::Green),
            model_unknown: Style::default().fg(Color::DarkGray),

            table_header: Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
            table_border: Style::default().fg(Color::Gray),
            table_row: Style::default().fg(Color::Black),
            table_row_alt: Style::default().fg(Color::DarkGray),
            table_total: Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),

            notification_info: Style::default().fg(Color::Blue),
            notification_warning: Style::default().fg(Color::Yellow),
            notification_error: Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),

            velocity_slow: Style::default().fg(Color::Green),
            velocity_normal: Style::default().fg(Color::Blue),
            velocity_fast: Style::default().fg(Color::Yellow),
            velocity_extreme: Style::default().fg(Color::Red),
        }
    }

    /// Classic terminal theme using only the basic 8-colour ANSI palette.
    ///
    /// Avoids bold modifiers to maintain a retro aesthetic and maximise
    /// compatibility with minimal terminal emulators.
    pub fn classic() -> Self {
        Self {
            header: Style::default().fg(Color::Cyan),
            header_sparkle: Style::default().fg(Color::White),
            separator: Style::default().fg(Color::DarkGray),

            text: Style::default().fg(Color::White),
            dim: Style::default().fg(Color::DarkGray),
            bold: Style::default().fg(Color::White),
            label: Style::default().fg(Color::Gray),
            value: Style::default().fg(Color::White),

            info: Style::default().fg(Color::Cyan),
            success: Style::default().fg(Color::Green),
            warning: Style::default().fg(Color::Yellow),
            error: Style::default().fg(Color::Red),

            progress_low: Style::default().fg(Color::Green),
            progress_medium: Style::default().fg(Color::Yellow),
            progress_high: Style::default().fg(Color::Red),
            progress_empty: Style::default().fg(Color::DarkGray),
            progress_label: Style::default().fg(Color::White),

            cost_low: Style::default().fg(Color::Green),
            cost_medium: Style::default().fg(Color::Yellow),
            cost_high: Style::default().fg(Color::Red),

            model_opus: Style::default().fg(Color::Magenta),
            model_sonnet: Style::default().fg(Color::Cyan),
            model_haiku: Style::default().fg(Color::Green),
            model_unknown: Style::default().fg(Color::White),

            table_header: Style::default().fg(Color::Cyan),
            table_border: Style::default().fg(Color::DarkGray),
            table_row: Style::default().fg(Color::White),
            table_row_alt: Style::default().fg(Color::Gray),
            table_total: Style::default().fg(Color::Yellow),

            notification_info: Style::default().fg(Color::Cyan),
            notification_warning: Style::default().fg(Color::Yellow),
            notification_error: Style::default().fg(Color::Red),

            velocity_slow: Style::default().fg(Color::Green),
            velocity_normal: Style::default().fg(Color::Cyan),
            velocity_fast: Style::default().fg(Color::Yellow),
            velocity_extreme: Style::default().fg(Color::Red),
        }
    }

    /// Choose a theme automatically based on the detected terminal background.
    pub fn auto_detect() -> Self {
        match detect_background() {
            BackgroundType::Light => Self::light(),
            _ => Self::dark(),
        }
    }

    /// Construct a theme by name.  Falls back to `auto_detect` for unknown
    /// names.
    pub fn from_name(name: &str) -> Self {
        match name {
            "light" => Self::light(),
            "dark" => Self::dark(),
            "classic" => Self::classic(),
            _ => Self::auto_detect(),
        }
    }

    // ── Style helpers ────────────────────────────────────────────────────────

    /// Return the appropriate progress-bar fill style for a given percentage.
    ///
    /// * `< 50 %`  → `progress_low`
    /// * `50–80 %` → `progress_medium`
    /// * `≥ 80 %`  → `progress_high`
    pub fn progress_style(&self, percentage: f64) -> Style {
        if percentage >= 80.0 {
            self.progress_high
        } else if percentage >= 50.0 {
            self.progress_medium
        } else {
            self.progress_low
        }
    }

    /// Return the appropriate cost style for a given percentage of the limit.
    ///
    /// Uses the same thresholds as [`Self::progress_style`].
    pub fn cost_style(&self, percentage: f64) -> Style {
        if percentage >= 80.0 {
            self.cost_high
        } else if percentage >= 50.0 {
            self.cost_medium
        } else {
            self.cost_low
        }
    }

    /// Return the model-colour style that best matches a raw model name string.
    pub fn model_style(&self, model: &str) -> Style {
        let lower = model.to_lowercase();
        if lower.contains("opus") {
            self.model_opus
        } else if lower.contains("sonnet") {
            self.model_sonnet
        } else if lower.contains("haiku") {
            self.model_haiku
        } else {
            self.model_unknown
        }
    }

    /// Return the velocity style for a given tokens-per-minute burn rate.
    ///
    /// | Tokens / min | Tier     |
    /// |-------------|----------|
    /// | ≥ 1 000      | extreme  |
    /// | ≥ 500        | fast     |
    /// | ≥ 100        | normal   |
    /// | < 100        | slow     |
    pub fn velocity_style(&self, tokens_per_min: f64) -> Style {
        if tokens_per_min >= 1000.0 {
            self.velocity_extreme
        } else if tokens_per_min >= 500.0 {
            self.velocity_fast
        } else if tokens_per_min >= 100.0 {
            self.velocity_normal
        } else {
            self.velocity_slow
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    // ── Theme construction ───────────────────────────────────────────────────

    #[test]
    fn test_dark_theme_creation() {
        let t = Theme::dark();
        // Verify key fields are meaningfully set (not the default unstyled value
        // for all of them).
        assert_eq!(t.header.fg, Some(Color::Cyan));
        assert_eq!(t.success.fg, Some(Color::Green));
        assert_eq!(t.warning.fg, Some(Color::Yellow));
        assert_eq!(t.error.fg, Some(Color::Red));
        assert_eq!(t.model_opus.fg, Some(Color::Magenta));
        assert_eq!(t.model_sonnet.fg, Some(Color::Cyan));
        assert_eq!(t.model_haiku.fg, Some(Color::Green));
        assert_eq!(t.velocity_extreme.fg, Some(Color::Red));
    }

    #[test]
    fn test_light_theme_creation() {
        let t = Theme::light();
        assert_eq!(t.header.fg, Some(Color::Blue));
        assert_eq!(t.text.fg, Some(Color::Black));
        assert_eq!(t.model_sonnet.fg, Some(Color::Blue));
        assert_eq!(t.table_row.fg, Some(Color::Black));
        assert_eq!(t.velocity_normal.fg, Some(Color::Blue));
    }

    #[test]
    fn test_classic_theme_creation() {
        let t = Theme::classic();
        // Classic has no bold modifiers on primary text fields.
        assert!(!t.bold.add_modifier.contains(Modifier::BOLD));
        assert_eq!(t.header.fg, Some(Color::Cyan));
        assert_eq!(t.table_total.fg, Some(Color::Yellow));
        // Classic notification_error must NOT have BOLD (unlike dark/light).
        assert!(!t.notification_error.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_from_name_dark() {
        let t = Theme::from_name("dark");
        assert_eq!(t.header.fg, Some(Color::Cyan));
    }

    #[test]
    fn test_from_name_light() {
        let t = Theme::from_name("light");
        assert_eq!(t.header.fg, Some(Color::Blue));
    }

    #[test]
    fn test_from_name_classic() {
        let t = Theme::from_name("classic");
        // Classic header is Cyan without BOLD.
        assert_eq!(t.header.fg, Some(Color::Cyan));
        assert!(!t.header.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_from_name_unknown_falls_back() {
        // Unknown names must not panic and must return a valid theme.
        let t = Theme::from_name("does-not-exist");
        // Must have at least one meaningful style set.
        assert!(t.header.fg.is_some());
    }

    // ── progress_style thresholds ────────────────────────────────────────────

    #[test]
    fn test_progress_style_below_50() {
        let t = Theme::dark();
        assert_eq!(t.progress_style(0.0).fg, Some(Color::Green));
        assert_eq!(t.progress_style(49.9).fg, Some(Color::Green));
    }

    #[test]
    fn test_progress_style_50_to_80() {
        let t = Theme::dark();
        assert_eq!(t.progress_style(50.0).fg, Some(Color::Yellow));
        assert_eq!(t.progress_style(79.9).fg, Some(Color::Yellow));
    }

    #[test]
    fn test_progress_style_at_80_and_above() {
        let t = Theme::dark();
        assert_eq!(t.progress_style(80.0).fg, Some(Color::Red));
        assert_eq!(t.progress_style(100.0).fg, Some(Color::Red));
    }

    // ── cost_style thresholds ────────────────────────────────────────────────

    #[test]
    fn test_cost_style_below_50() {
        let t = Theme::dark();
        assert_eq!(t.cost_style(0.0).fg, Some(Color::Green));
        assert_eq!(t.cost_style(49.9).fg, Some(Color::Green));
    }

    #[test]
    fn test_cost_style_50_to_80() {
        let t = Theme::dark();
        assert_eq!(t.cost_style(50.0).fg, Some(Color::Yellow));
        assert_eq!(t.cost_style(79.9).fg, Some(Color::Yellow));
    }

    #[test]
    fn test_cost_style_at_80_and_above() {
        let t = Theme::dark();
        assert_eq!(t.cost_style(80.0).fg, Some(Color::Red));
        assert_eq!(t.cost_style(100.0).fg, Some(Color::Red));
    }

    // ── model_style ──────────────────────────────────────────────────────────

    #[test]
    fn test_model_style_opus() {
        let t = Theme::dark();
        assert_eq!(t.model_style("claude-3-opus").fg, Some(Color::Magenta));
        assert_eq!(
            t.model_style("claude-opus-4-20250514").fg,
            Some(Color::Magenta)
        );
    }

    #[test]
    fn test_model_style_sonnet() {
        let t = Theme::dark();
        assert_eq!(t.model_style("claude-3-5-sonnet").fg, Some(Color::Cyan));
        assert_eq!(t.model_style("Claude-Sonnet-4").fg, Some(Color::Cyan));
    }

    #[test]
    fn test_model_style_haiku() {
        let t = Theme::dark();
        assert_eq!(t.model_style("claude-3-haiku").fg, Some(Color::Green));
    }

    #[test]
    fn test_model_style_unknown() {
        let t = Theme::dark();
        assert_eq!(t.model_style("gpt-4").fg, Some(Color::Gray));
        assert_eq!(t.model_style("").fg, Some(Color::Gray));
    }

    // ── velocity_style ───────────────────────────────────────────────────────

    #[test]
    fn test_velocity_style_slow() {
        let t = Theme::dark();
        assert_eq!(t.velocity_style(0.0).fg, Some(Color::Green));
        assert_eq!(t.velocity_style(99.9).fg, Some(Color::Green));
    }

    #[test]
    fn test_velocity_style_normal() {
        let t = Theme::dark();
        assert_eq!(t.velocity_style(100.0).fg, Some(Color::Cyan));
        assert_eq!(t.velocity_style(499.9).fg, Some(Color::Cyan));
    }

    #[test]
    fn test_velocity_style_fast() {
        let t = Theme::dark();
        assert_eq!(t.velocity_style(500.0).fg, Some(Color::Yellow));
        assert_eq!(t.velocity_style(999.9).fg, Some(Color::Yellow));
    }

    #[test]
    fn test_velocity_style_extreme() {
        let t = Theme::dark();
        assert_eq!(t.velocity_style(1000.0).fg, Some(Color::Red));
        assert_eq!(t.velocity_style(9999.0).fg, Some(Color::Red));
    }
}
