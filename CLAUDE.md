# claude-monitor

Real-time TUI for monitoring Claude Code token usage, costs, and session limits. Rust 1.75+, Cargo workspace (5 crates), tokio async, ratatui terminal UI.

## Commands

```bash
cargo build                          # Build workspace
cargo run -- --plan pro              # Run with Pro plan
cargo test --workspace               # Test all crates
cargo test test_name --workspace     # Run single test by name
cargo test -p monitor-core           # Test one crate
cargo clippy --workspace             # Lint
cargo build --release                # Optimized release build
```

## Architecture

Five-crate workspace. Dependencies flow downward:

```
claude-monitor          CLI entrypoint, bootstrap
  → monitor-runtime     async orchestration, background data refresh via tokio mpsc
    → monitor-ui        ratatui TUI: app event loop, views, themes, components
    → monitor-data      JSONL file discovery with walkdir, session blocking, aggregation
      → monitor-core    domain models, pricing, plans, settings, calculations, errors
```

### Key flows

**Realtime view:** `main.rs` loads settings → `bootstrap` discovers Claude data path → spawns `MonitoringOrchestrator` (tokio background task) → sends `MonitoringData` snapshots over mpsc channel → `App::run_realtime()` renders TUI.

**Table views (daily/monthly):** `main.rs` → `analyze_usage()` pipeline → `aggregator` rolls up session blocks by period → `App::run_table()` renders table.

**Data pipeline:** `reader::find_jsonl_files()` walks `~/.claude/projects/` → parses `UsageEntry` from JSONL → `analyzer` groups into 5-hour `SessionBlock`s → `aggregator` summarizes by day/month.

### Key types

- `UsageEntry` — single API call record (timestamp, tokens, cost, model)
- `SessionBlock` — 5-hour rolling window of entries with aggregated totals
- `TokenCounts` — input/output/cache_creation/cache_read breakdown
- `PlanConfig` — token/cost/message limits per plan (Pro, Max5, Max20, Custom)
- `MonitorError` — domain error enum via thiserror with `Result<T>` alias
- `Settings` — clap-derived CLI args, merged with persisted `LastUsedParams`

## Conventions

- Use `MonitorError` from `monitor-core/src/error.rs` with thiserror. Return `Result<T>` from `monitor_core::error` in all fallible functions.
- Use tokio with `#[tokio::main]`. Coordinate background work through mpsc channels.
- Place tests in inline `#[cfg(test)]` modules. Use `tempfile::TempDir` for filesystem tests. Build test data with helpers like `make_entry()`.
- Persist config to `~/.claude-monitor/last_used.json` via `LastUsedParams`. CLI flags always override persisted values. Plan is never persisted.
- Render TUI with ratatui + crossterm. Place views in `monitor-ui/src/session_view.rs` and `table_view.rs`. Place reusable widgets in `components/`.

## Constraints

- NEVER use shared mutable state for cross-task communication; use tokio mpsc channels instead.
- NEVER add system dependencies beyond Rust toolchain; keep all deps Cargo-managed.
- NEVER put module-specific logic in `monitor-core`; it holds only domain-wide models and utilities.
- NEVER bypass `MonitorError`; use `anyhow` only at the binary crate boundary in `main.rs`.
