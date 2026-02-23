# Contributing to claude-monitor

Thanks for your interest in contributing! Here's how to get started.

## Prerequisites

- Rust 1.75 or later — install via [rustup](https://rustup.rs)

## Getting Started

```bash
git clone https://github.com/Laiff/claude-monitor.git
cd claude-monitor
cargo build
```

### Running

```bash
cargo run -- --plan pro
```

### Testing

```bash
cargo test --workspace
```

## Project Structure

This is a Cargo workspace with five crates:

```
crates/
  monitor-core/      # Domain models, pricing, settings, calculations
  monitor-data/      # JSONL reading, session blocking, aggregation
  monitor-ui/        # TUI rendering (ratatui), views, themes
  monitor-runtime/   # Async orchestration, background refresh
  claude-monitor/    # CLI binary entrypoint
```

Dependencies flow downward: `claude-monitor` → `monitor-runtime` → `monitor-ui` → `monitor-data` → `monitor-core`.

## Pull Requests

1. Fork the repo and create a branch from `main`
2. Make your changes
3. Run `cargo test --workspace` and `cargo clippy --workspace` before submitting
4. Keep PRs focused — one feature or fix per PR
5. Write clear commit messages describing the "why"

## Reporting Issues

Open an issue on GitHub with:
- What you expected to happen
- What actually happened
- Steps to reproduce
- Your OS and Rust version (`rustc --version`)

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
