mod bootstrap;

use anyhow::Result;
use monitor_core::settings::Settings;
use monitor_data::aggregator::UsageAggregator;
use monitor_data::analysis::analyze_usage;
use monitor_runtime::orchestrator::MonitoringOrchestrator;
use monitor_ui::app::{App, ViewMode};
use monitor_ui::table_view::{TableRowData, TableTotals};

#[tokio::main]
async fn main() -> Result<()> {
    let settings = Settings::load_with_last_used();

    bootstrap::ensure_directories()?;
    bootstrap::setup_logging(&settings.log_level, settings.log_file.as_ref())?;

    tracing::info!("Claude Monitor v{} starting", env!("CARGO_PKG_VERSION"));
    tracing::info!(
        "Plan: {}, View: {}, Theme: {}",
        settings.plan,
        settings.view,
        settings.theme
    );

    let data_path = bootstrap::discover_data_path();

    match settings.view.as_str() {
        "realtime" | "session" => {
            tracing::info!("Starting real-time monitoring...");

            let data_path_str = data_path.map(|p| p.to_string_lossy().to_string());

            let orchestrator = MonitoringOrchestrator::new(
                u64::from(settings.refresh_rate),
                data_path_str,
                settings.plan.clone(),
            );

            let (rx, handle) = orchestrator.start();

            let app = App::new(
                &settings.theme,
                ViewMode::Realtime,
                settings.plan.clone(),
                settings.timezone.clone(),
            );

            // Run the TUI event loop. The loop exits on 'q' / Ctrl+C inside the TUI.
            // We also listen for Ctrl+C at the OS level so that signals received
            // while the terminal is in raw mode are handled cleanly.
            tokio::select! {
                result = app.run_realtime(rx) => {
                    handle.abort();
                    result?;
                }
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("Ctrl+C received; shutting down monitoring task");
                    handle.abort();
                }
            }
        }

        "daily" | "monthly" => {
            tracing::info!("Running {} view...", settings.view);

            let data_path_str = data_path.map(|p| p.to_string_lossy().to_string());

            // Run the full analysis pipeline to get all session blocks.
            let analysis = analyze_usage(None, false, data_path_str.as_deref());

            // Aggregate the blocks into per-period rows.
            let periods = UsageAggregator::aggregate_from_blocks(&analysis.blocks, &settings.view);

            // Compute cross-period totals.
            let agg_totals = UsageAggregator::calculate_totals(&periods);

            // Convert AggregatedPeriod → TableRowData.
            let rows: Vec<TableRowData> = periods
                .into_iter()
                .map(|p| {
                    let total_tokens = p.stats.total_tokens();
                    let mut models: Vec<String> = p.models_used.into_iter().collect();
                    models.sort();
                    TableRowData {
                        period: p.period_key,
                        models,
                        input_tokens: p.stats.input_tokens,
                        output_tokens: p.stats.output_tokens,
                        cache_creation: p.stats.cache_creation_tokens,
                        cache_read: p.stats.cache_read_tokens,
                        total_tokens,
                        cost: p.stats.cost,
                    }
                })
                .collect();

            // Build the totals row shown at the bottom of the table.
            let totals = TableTotals {
                input_tokens: agg_totals.input_tokens,
                output_tokens: agg_totals.output_tokens,
                cache_creation: agg_totals.cache_creation_tokens,
                cache_read: agg_totals.cache_read_tokens,
                total_tokens: agg_totals.total_tokens(),
                total_cost: agg_totals.cost,
                entries_count: agg_totals.count,
            };

            let view_mode = if settings.view == "monthly" {
                ViewMode::Monthly
            } else {
                ViewMode::Daily
            };

            let app = App::new(
                &settings.theme,
                view_mode,
                settings.plan.clone(),
                settings.timezone.clone(),
            );

            app.run_table(rows, totals).await?;
        }

        unknown => {
            eprintln!("Unknown view mode: {}", unknown);
        }
    }

    Ok(())
}
