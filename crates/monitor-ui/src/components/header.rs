use crate::themes::Theme;
use ratatui::text::{Line, Span};

/// Decorative sparkle string placed either side of the application title.
pub const SPARKLES: &str = "✦ ✧ ✦ ✧";

/// Monitor dashboard header rendering four lines:
///
/// 1. Application title with sparkle decorations (ALL CAPS).
/// 2. A 60-column `=` separator.
/// 3. Plan and timezone information in `[ plan | timezone ]` format.
/// 4. An empty line.
pub struct Header<'a> {
    /// Claude plan name (e.g. "pro", "max", "team").
    pub plan: &'a str,
    /// Human-readable timezone string (e.g. "UTC", "America/New_York").
    pub timezone: &'a str,
    /// Theme providing colour styles for each part of the header.
    pub theme: &'a Theme,
}

impl<'a> Header<'a> {
    /// Construct a new header.
    pub fn new(plan: &'a str, timezone: &'a str, theme: &'a Theme) -> Self {
        Self {
            plan,
            timezone,
            theme,
        }
    }

    /// Render the header as a `Vec<Line>` containing exactly four lines.
    ///
    /// The returned lines are:
    ///
    /// 1. `"✦ ✧ ✦ ✧ CLAUDE CODE USAGE MONITOR ✦ ✧ ✦ ✧"`
    /// 2. `"============================================================"` (60 `=` chars)
    /// 3. `"[ pro | UTC ]"`
    /// 4. `""`
    pub fn to_lines(&self) -> Vec<Line<'a>> {
        let separator = "=".repeat(60);

        vec![
            // Title line.
            Line::from(vec![
                Span::styled(SPARKLES, self.theme.header_sparkle),
                Span::styled(" CLAUDE CODE USAGE MONITOR ", self.theme.header),
                Span::styled(SPARKLES, self.theme.header_sparkle),
            ]),
            // Separator line.
            Line::from(Span::styled(separator, self.theme.separator)),
            // Plan / timezone info line.
            Line::from(vec![
                Span::styled("[ ", self.theme.label),
                Span::styled(self.plan.to_lowercase(), self.theme.value),
                Span::styled(" | ", self.theme.label),
                Span::styled(self.timezone.to_lowercase(), self.theme.value),
                Span::styled(" ]", self.theme.label),
            ]),
            // Empty line.
            Line::from(""),
        ]
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::Theme;

    #[test]
    fn test_header_to_lines_count() {
        let theme = Theme::dark();
        let header = Header::new("pro", "UTC", &theme);
        let lines = header.to_lines();
        assert_eq!(lines.len(), 4, "header must produce exactly 4 lines");
    }

    #[test]
    fn test_header_title_line_content() {
        let theme = Theme::dark();
        let header = Header::new("pro", "UTC", &theme);
        let lines = header.to_lines();

        // Reconstruct the text of the first line.
        let title_text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(
            title_text.contains("CLAUDE CODE USAGE MONITOR"),
            "title line must contain 'CLAUDE CODE USAGE MONITOR', got: {title_text}"
        );
        assert!(
            title_text.contains(SPARKLES),
            "title line must contain sparkles, got: {title_text}"
        );
    }

    #[test]
    fn test_header_info_line_plan_lowercased() {
        let theme = Theme::dark();
        let header = Header::new("PRO", "America/New_York", &theme);
        let lines = header.to_lines();

        let info_text: String = lines[2].spans.iter().map(|s| s.content.as_ref()).collect();

        // Plan should be lowercased in the output.
        assert!(
            info_text.contains("pro"),
            "plan must be lowercased, got: {info_text}"
        );
        assert!(
            info_text.contains("america/new_york"),
            "timezone must appear lowercased, got: {info_text}"
        );
        assert!(
            info_text.contains("[ ") && info_text.contains(" | ") && info_text.contains(" ]"),
            "format must be '[ plan | timezone ]', got: {info_text}"
        );
    }

    #[test]
    fn test_header_separator_line() {
        let theme = Theme::dark();
        let header = Header::new("max", "Europe/London", &theme);
        let lines = header.to_lines();

        // Second line must be a 60-column `=` separator.
        let sep_text: String = lines[1].spans.iter().map(|s| s.content.as_ref()).collect();

        assert_eq!(
            sep_text.chars().count(),
            60,
            "separator must be 60 chars wide"
        );
        assert!(
            sep_text.chars().all(|c| c == '='),
            "separator must consist of '=' characters, got: {sep_text}"
        );
    }

    #[test]
    fn test_header_info_line_span_count() {
        let theme = Theme::dark();
        let header = Header::new("team", "Asia/Tokyo", &theme);
        let lines = header.to_lines();

        // Info line: "[ " + plan + " | " + tz + " ]" = 5 spans.
        assert_eq!(
            lines[2].spans.len(),
            5,
            "info line must have 5 spans, got {}",
            lines[2].spans.len()
        );
    }

    #[test]
    fn test_header_empty_fourth_line() {
        let theme = Theme::dark();
        let header = Header::new("pro", "UTC", &theme);
        let lines = header.to_lines();

        let empty_text: String = lines[3].spans.iter().map(|s| s.content.as_ref()).collect();

        assert!(
            empty_text.is_empty(),
            "fourth line must be empty, got: {empty_text:?}"
        );
    }
}
