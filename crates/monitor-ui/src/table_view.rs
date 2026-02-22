//! Aggregate table views (daily / monthly) for the Claude Monitor TUI.
//!
//! Renders a bordered [`ratatui::widgets::Table`] with one row per time
//! period plus a highlighted totals row at the bottom.

use ratatui::{
    layout::{Constraint, Rect},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

use monitor_core::formatting;

use crate::themes::Theme;

/// Data for a single row in the aggregate table.
#[derive(Debug, Clone)]
pub struct TableRowData {
    /// Period label, e.g. `"2024-02-22"` (daily) or `"2024-02"` (monthly).
    pub period: String,
    /// Canonical model names seen in this period.
    pub models: Vec<String>,
    /// Accumulated input (prompt) tokens.
    pub input_tokens: u64,
    /// Accumulated output (completion) tokens.
    pub output_tokens: u64,
    /// Accumulated cache-creation tokens.
    pub cache_creation: u64,
    /// Accumulated cache-read tokens.
    pub cache_read: u64,
    /// Sum of all four token categories.
    pub total_tokens: u64,
    /// Total cost in USD.
    pub cost: f64,
}

/// Aggregated totals across all rows in the table.
#[derive(Debug, Clone)]
pub struct TableTotals {
    /// Total input tokens across all periods.
    pub input_tokens: u64,
    /// Total output tokens across all periods.
    pub output_tokens: u64,
    /// Total cache-creation tokens across all periods.
    pub cache_creation: u64,
    /// Total cache-read tokens across all periods.
    pub cache_read: u64,
    /// Total of all token categories across all periods.
    pub total_tokens: u64,
    /// Total cost in USD across all periods.
    pub total_cost: f64,
    /// Number of periods (rows) represented.
    pub entries_count: u32,
}

/// Render the daily or monthly aggregate table into `area`.
///
/// The table has one data row per [`TableRowData`] entry, followed by a
/// highlighted totals row, all within a bordered block titled `title`.
pub fn render_table_view(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    rows: &[TableRowData],
    totals: &TableTotals,
    theme: &Theme,
) {
    let header_cells = [
        "Period",
        "Models",
        "Input",
        "Output",
        "Cache Create",
        "Cache Read",
        "Total",
        "Cost",
    ]
    .iter()
    .map(|h| Cell::from(*h).style(theme.table_header));
    let header = Row::new(header_cells).height(1);

    let data_rows: Vec<Row> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let style = if i % 2 == 0 {
                theme.table_row
            } else {
                theme.table_row_alt
            };
            Row::new(vec![
                Cell::from(row.period.clone()),
                Cell::from(row.models.join(", ")),
                Cell::from(formatting::format_number(row.input_tokens as f64, 0)),
                Cell::from(formatting::format_number(row.output_tokens as f64, 0)),
                Cell::from(formatting::format_number(row.cache_creation as f64, 0)),
                Cell::from(formatting::format_number(row.cache_read as f64, 0)),
                Cell::from(formatting::format_number(row.total_tokens as f64, 0)),
                Cell::from(formatting::format_currency(row.cost)),
            ])
            .style(style)
        })
        .collect();

    // Totals row – styled separately to stand out.
    let total_row = Row::new(vec![
        Cell::from("TOTAL").style(theme.table_total),
        Cell::from(format!("{} periods", totals.entries_count)),
        Cell::from(formatting::format_number(totals.input_tokens as f64, 0)),
        Cell::from(formatting::format_number(totals.output_tokens as f64, 0)),
        Cell::from(formatting::format_number(totals.cache_creation as f64, 0)),
        Cell::from(formatting::format_number(totals.cache_read as f64, 0)),
        Cell::from(formatting::format_number(totals.total_tokens as f64, 0)),
        Cell::from(formatting::format_currency(totals.total_cost)),
    ])
    .style(theme.table_total);

    let mut all_rows = data_rows;
    all_rows.push(total_row);

    let widths = [
        Constraint::Length(12),
        Constraint::Length(25),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(14),
        Constraint::Length(12),
        Constraint::Length(12),
        Constraint::Length(12),
    ];

    let table = Table::new(all_rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", title)),
        )
        .style(theme.text);

    frame.render_widget(table, area);
}

/// Render a "no data" placeholder when there are no periods to show.
pub fn render_no_data(frame: &mut Frame, area: Rect, theme: &Theme) {
    let text = vec![
        Line::from(""),
        Line::from(Span::styled("No usage data found", theme.warning)),
        Line::from(""),
        Line::from(Span::styled(
            "Make sure Claude has been used recently.",
            theme.dim,
        )),
        Line::from(Span::styled("Press 'q' or Ctrl+C to exit", theme.dim)),
    ];
    frame.render_widget(
        Paragraph::new(ratatui::text::Text::from(text)).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Claude Monitor "),
        ),
        area,
    );
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::themes::Theme;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn make_rows() -> Vec<TableRowData> {
        vec![
            TableRowData {
                period: "2024-01-15".to_string(),
                models: vec!["claude-3-5-sonnet".to_string()],
                input_tokens: 10_000,
                output_tokens: 5_000,
                cache_creation: 500,
                cache_read: 200,
                total_tokens: 15_700,
                cost: 1.23,
            },
            TableRowData {
                period: "2024-01-16".to_string(),
                models: vec![
                    "claude-3-5-sonnet".to_string(),
                    "claude-3-haiku".to_string(),
                ],
                input_tokens: 20_000,
                output_tokens: 8_000,
                cache_creation: 1_000,
                cache_read: 400,
                total_tokens: 29_400,
                cost: 2.45,
            },
        ]
    }

    fn make_totals(rows: &[TableRowData]) -> TableTotals {
        TableTotals {
            input_tokens: rows.iter().map(|r| r.input_tokens).sum(),
            output_tokens: rows.iter().map(|r| r.output_tokens).sum(),
            cache_creation: rows.iter().map(|r| r.cache_creation).sum(),
            cache_read: rows.iter().map(|r| r.cache_read).sum(),
            total_tokens: rows.iter().map(|r| r.total_tokens).sum(),
            total_cost: rows.iter().map(|r| r.cost).sum(),
            entries_count: rows.len() as u32,
        }
    }

    // ── Data construction ─────────────────────────────────────────────────────

    #[test]
    fn test_table_row_data_construction() {
        let rows = make_rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].period, "2024-01-15");
        assert_eq!(rows[0].input_tokens, 10_000);
        assert_eq!(rows[1].models.len(), 2);
    }

    #[test]
    fn test_table_totals_construction() {
        let rows = make_rows();
        let totals = make_totals(&rows);
        assert_eq!(totals.input_tokens, 30_000);
        assert_eq!(totals.output_tokens, 13_000);
        assert_eq!(totals.entries_count, 2);
        assert!((totals.total_cost - 3.68).abs() < 1e-9);
    }

    // ── Render (does not panic) ───────────────────────────────────────────────

    #[test]
    fn test_render_table_view_does_not_panic() {
        let backend = TestBackend::new(130, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        let rows = make_rows();
        let totals = make_totals(&rows);

        terminal
            .draw(|frame| {
                let area = frame.area();
                render_table_view(frame, area, "Daily Usage", &rows, &totals, &theme);
            })
            .unwrap();
    }

    #[test]
    fn test_render_table_view_empty_rows_does_not_panic() {
        let backend = TestBackend::new(130, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();
        let rows: Vec<TableRowData> = vec![];
        let totals = TableTotals {
            input_tokens: 0,
            output_tokens: 0,
            cache_creation: 0,
            cache_read: 0,
            total_tokens: 0,
            total_cost: 0.0,
            entries_count: 0,
        };

        terminal
            .draw(|frame| {
                let area = frame.area();
                render_table_view(frame, area, "Daily Usage", &rows, &totals, &theme);
            })
            .unwrap();
    }

    #[test]
    fn test_render_no_data_does_not_panic() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();

        terminal
            .draw(|frame| {
                let area = frame.area();
                render_no_data(frame, area, &theme);
            })
            .unwrap();
    }

    #[test]
    fn test_render_table_view_monthly_title_does_not_panic() {
        let backend = TestBackend::new(130, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::light();
        let rows = vec![TableRowData {
            period: "2024-01".to_string(),
            models: vec!["claude-3-5-sonnet".to_string()],
            input_tokens: 100_000,
            output_tokens: 50_000,
            cache_creation: 5_000,
            cache_read: 2_000,
            total_tokens: 157_000,
            cost: 12.50,
        }];
        let totals = make_totals(&rows);

        terminal
            .draw(|frame| {
                let area = frame.area();
                render_table_view(frame, area, "Monthly Usage", &rows, &totals, &theme);
            })
            .unwrap();
    }
}
