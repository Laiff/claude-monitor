use crate::themes::Theme;
use ratatui::text::{Line, Span};

// ── VelocityIndicator ────────────────────────────────────────────────────────

/// Displays the current token burn rate with a tiered emoji and colour.
///
/// | Tokens / min | Emoji | Tier     |
/// |-------------|-------|----------|
/// | ≥ 1 000      | ⚡    | extreme  |
/// | ≥ 500        | 🚀    | fast     |
/// | ≥ 100        | ➡️    | normal   |
/// | < 100        | 🐌    | slow     |
pub struct VelocityIndicator<'a> {
    /// Measured token consumption rate in tokens per minute.
    pub tokens_per_minute: f64,
    /// Theme providing colour styles.
    pub theme: &'a Theme,
}

impl<'a> VelocityIndicator<'a> {
    /// Construct a new indicator.
    pub fn new(tokens_per_minute: f64, theme: &'a Theme) -> Self {
        Self {
            tokens_per_minute,
            theme,
        }
    }

    /// Select the tier emoji for the current burn rate.
    pub fn emoji(&self) -> &'static str {
        if self.tokens_per_minute >= 1000.0 {
            "⚡"
        } else if self.tokens_per_minute >= 500.0 {
            "🚀"
        } else if self.tokens_per_minute >= 100.0 {
            "➡️"
        } else {
            "🐌"
        }
    }

    /// Render the indicator as a [`Line`].
    ///
    /// Format: `"🔥 Burn rate: 123.4 tok/min ➡️"`
    pub fn to_line(&self) -> Line<'a> {
        let style = self.theme.velocity_style(self.tokens_per_minute);
        Line::from(vec![
            Span::styled("🔥 Burn rate: ", self.theme.label),
            Span::styled(format!("{:.1} tok/min", self.tokens_per_minute), style),
            Span::raw(" "),
            Span::raw(self.emoji()),
        ])
    }
}

// ── CostIndicator ────────────────────────────────────────────────────────────

/// Displays the current session cost relative to a configured limit.
///
/// An optional hourly rate is shown in parentheses when provided.
pub struct CostIndicator<'a> {
    /// Cost accrued so far in USD.
    pub current_cost: f64,
    /// Configured cost limit in USD.
    pub cost_limit: f64,
    /// Optional cost-per-hour rate for the current burn velocity.
    pub cost_per_hour: Option<f64>,
    /// Theme providing colour styles.
    pub theme: &'a Theme,
}

impl<'a> CostIndicator<'a> {
    /// Construct a new cost indicator.
    pub fn new(current: f64, limit: f64, per_hour: Option<f64>, theme: &'a Theme) -> Self {
        Self {
            current_cost: current,
            cost_limit: limit,
            cost_per_hour: per_hour,
            theme,
        }
    }

    /// Render the indicator as a [`Line`].
    ///
    /// Format: `"💲 Cost: $1.23 / $10.00  ($0.45/hr)"`
    pub fn to_line(&self) -> Line<'a> {
        let pct = if self.cost_limit > 0.0 {
            (self.current_cost / self.cost_limit) * 100.0
        } else {
            0.0
        };

        let cost_style = self.theme.cost_style(pct);

        let mut spans = vec![
            Span::styled("💲 Cost: ", self.theme.label),
            Span::styled(
                monitor_core::formatting::format_currency(self.current_cost),
                cost_style,
            ),
            Span::styled(
                format!(
                    " / {}",
                    monitor_core::formatting::format_currency(self.cost_limit)
                ),
                self.theme.dim,
            ),
        ];

        if let Some(rate) = self.cost_per_hour {
            spans.push(Span::styled(
                format!("  ({}/hr)", monitor_core::formatting::format_currency(rate)),
                self.theme.dim,
            ));
        }

        Line::from(spans)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::Theme;

    // ── VelocityIndicator ────────────────────────────────────────────────────

    #[test]
    fn test_velocity_emoji_slow() {
        let theme = Theme::dark();
        assert_eq!(VelocityIndicator::new(0.0, &theme).emoji(), "🐌");
        assert_eq!(VelocityIndicator::new(99.9, &theme).emoji(), "🐌");
    }

    #[test]
    fn test_velocity_emoji_normal() {
        let theme = Theme::dark();
        assert_eq!(VelocityIndicator::new(100.0, &theme).emoji(), "➡️");
        assert_eq!(VelocityIndicator::new(499.9, &theme).emoji(), "➡️");
    }

    #[test]
    fn test_velocity_emoji_fast() {
        let theme = Theme::dark();
        assert_eq!(VelocityIndicator::new(500.0, &theme).emoji(), "🚀");
        assert_eq!(VelocityIndicator::new(999.9, &theme).emoji(), "🚀");
    }

    #[test]
    fn test_velocity_emoji_extreme() {
        let theme = Theme::dark();
        assert_eq!(VelocityIndicator::new(1000.0, &theme).emoji(), "⚡");
        assert_eq!(VelocityIndicator::new(5000.0, &theme).emoji(), "⚡");
    }

    #[test]
    fn test_velocity_to_line_span_count() {
        let theme = Theme::dark();
        let indicator = VelocityIndicator::new(250.0, &theme);
        let line = indicator.to_line();
        // 4 spans: label, rate, space, emoji
        assert_eq!(line.spans.len(), 4, "got {} spans", line.spans.len());
    }

    #[test]
    fn test_velocity_to_line_content() {
        let theme = Theme::dark();
        let indicator = VelocityIndicator::new(250.0, &theme);
        let line = indicator.to_line();

        let full_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            full_text.contains("250.0 tok/min"),
            "unexpected content: {full_text}"
        );
        assert!(
            full_text.contains("Burn rate"),
            "unexpected content: {full_text}"
        );
        // 250 tok/min is the normal tier.
        assert!(full_text.contains("➡️"), "unexpected content: {full_text}");
    }

    // ── CostIndicator ────────────────────────────────────────────────────────

    #[test]
    fn test_cost_indicator_to_line_without_rate() {
        let theme = Theme::dark();
        let indicator = CostIndicator::new(2.50, 10.0, None, &theme);
        let line = indicator.to_line();

        // Without hourly rate: 3 spans – label, cost, limit.
        assert_eq!(line.spans.len(), 3, "got {} spans", line.spans.len());

        let full_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            full_text.contains("$2.50"),
            "unexpected content: {full_text}"
        );
        assert!(
            full_text.contains("$10.00"),
            "unexpected content: {full_text}"
        );
    }

    #[test]
    fn test_cost_indicator_to_line_with_rate() {
        let theme = Theme::dark();
        let indicator = CostIndicator::new(5.0, 10.0, Some(1.5), &theme);
        let line = indicator.to_line();

        // With hourly rate: 4 spans.
        assert_eq!(line.spans.len(), 4, "got {} spans", line.spans.len());

        let full_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            full_text.contains("$1.50"),
            "unexpected content: {full_text}"
        );
        assert!(full_text.contains("/hr"), "unexpected content: {full_text}");
    }

    #[test]
    fn test_cost_indicator_zero_limit() {
        // limit == 0 must not panic; percentage defaults to 0.
        let theme = Theme::dark();
        let indicator = CostIndicator::new(1.0, 0.0, None, &theme);
        let line = indicator.to_line();
        assert_eq!(line.spans.len(), 3);
    }

    #[test]
    fn test_cost_indicator_high_percentage_uses_high_style() {
        use ratatui::style::Color;

        let theme = Theme::dark();
        // 90 % of limit → cost_high → Red.
        let indicator = CostIndicator::new(9.0, 10.0, None, &theme);
        let line = indicator.to_line();
        // The second span is the current cost and should carry the high style.
        assert_eq!(line.spans[1].style.fg, Some(Color::Red));
    }
}
