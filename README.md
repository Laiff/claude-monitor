# claude-monitor

Real-time terminal UI for monitoring [Claude Code](https://docs.anthropic.com/en/docs/claude-code) token usage, costs, and session limits.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)

## Features

- **Plan tracking** — supports Pro, Max5, Max20, and Custom plans with per-session token, cost, and message limits
- **Live burn rates** — tokens/min and $/hour calculated from your active session
- **Multi-model pricing** — accurate cost breakdown across Opus, Sonnet, Haiku, and Claude 4 variants (including cache token pricing)
- **Daily & monthly analytics** — aggregated usage tables with totals
- **Themes** — dark, light, and classic color schemes with auto-detection
- **Notifications** — alerts for approaching limits, plan switch recommendations, and exhaustion warnings
- **Smart configuration** — auto-detects timezone and time format; settings persist across sessions

## Installation

### Homebrew

```bash
brew install Laiff/tap/claude-monitor
```

### Cargo

```bash
cargo install claude-monitor
```

### Build from source

```bash
git clone https://github.com/Laiff/claude-monitor.git
cd claude-monitor
cargo build --release
# Binary at target/release/claude-monitor
```

**Requirements:** Rust 1.75+

## Usage

```bash
# Monitor with Pro plan limits
claude-monitor --plan pro

# Max5 plan with dark theme
claude-monitor --plan max5 --theme dark

# Daily usage summary
claude-monitor --plan pro --view daily

# Monthly usage summary
claude-monitor --view monthly

# Custom token limit
claude-monitor --plan custom --custom-limit-tokens 100000
```

### CLI Options

| Flag | Default | Values | Description |
|------|---------|--------|-------------|
| `--plan` | `custom` | `pro`, `max5`, `max20`, `custom` | Subscription plan |
| `--view` | `realtime` | `realtime`, `daily`, `monthly`, `session` | View mode |
| `--theme` | `auto` | `dark`, `light`, `classic`, `auto` | Color theme |
| `--timezone` | `auto` | Any IANA timezone | Display timezone |
| `--time-format` | `auto` | `12h`, `24h`, `auto` | Time format |
| `--custom-limit-tokens` | — | Number | Token limit for custom plan |
| `--refresh-rate` | `10` | `1`–`60` (seconds) | Data refresh interval |
| `--reset-hour` | — | `0`–`23` | Daily limit reset hour |
| `--debug` | — | Flag | Enable debug logging |
| `--clear` | — | Flag | Clear saved configuration |

## Views

**Realtime** (default) — live dashboard showing token/cost progress bars, burn rates, session timing, per-model breakdown, and notifications.

**Daily / Monthly** — tabular summaries with columns for input, output, cache creation, cache read, total tokens, and cost.

## Supported Plans

| Plan | Tokens/session | Cost limit | Messages |
|------|---------------|------------|----------|
| Pro | 19,000 | $18 | 250 |
| Max5 | 88,000 | $35 | 1,000 |
| Max20 | 220,000 | $140 | 2,000 |
| Custom | configurable | $50 | 250 |

All limits apply to a 5-hour rolling session window.

## Configuration

Settings are automatically saved to `~/.claude-monitor/last_used.json` and restored on next run. CLI flags always take priority over saved values.

Auto-detected settings:
- **Timezone** — from system via `iana-time-zone`
- **Time format** — 12h/24h based on locale
- **Theme** — dark/light based on terminal background

Use `--clear` to reset saved configuration to defaults.

## Architecture

The project is a Cargo workspace with five crates:

| Crate | Role |
|-------|------|
| `monitor-core` | Domain models, pricing, plans, calculations, settings |
| `monitor-data` | JSONL data reading, session blocking, aggregation |
| `monitor-ui` | TUI rendering with ratatui (views, themes, components) |
| `monitor-runtime` | Async orchestration, background data refresh |
| `claude-monitor` | CLI entrypoint and bootstrap |

## License

[MIT](LICENSE)
