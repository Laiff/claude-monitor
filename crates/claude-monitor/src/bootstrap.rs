use std::path::PathBuf;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

// ── Directory bootstrap ────────────────────────────────────────────────────────

/// Ensure the standard `~/.claude-monitor/` directory hierarchy exists.
///
/// Creates the following directories if absent (including any missing parents):
/// - `~/.claude-monitor/`
/// - `~/.claude-monitor/logs/`
/// - `~/.claude-monitor/cache/`
pub fn ensure_directories() -> anyhow::Result<()> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let monitor_dir = home.join(".claude-monitor");
    std::fs::create_dir_all(&monitor_dir)?;
    std::fs::create_dir_all(monitor_dir.join("logs"))?;
    std::fs::create_dir_all(monitor_dir.join("cache"))?;
    Ok(())
}

// ── Logging bootstrap ──────────────────────────────────────────────────────────

/// Initialise the global `tracing` subscriber.
///
/// `log_level` is mapped to a [`tracing_subscriber::EnvFilter`] directive.
/// Falls back to `"info"` if the level string is not recognised.
///
/// The `log_file` parameter is accepted for forward-compatibility but file
/// logging is not yet wired – all output currently goes to stderr.
pub fn setup_logging(log_level: &str, _log_file: Option<&PathBuf>) -> anyhow::Result<()> {
    // Map Python log-level names to tracing level names (tracing uses lowercase).
    let upper = log_level.to_uppercase();
    let normalised = match upper.as_str() {
        "DEBUG" | "CRITICAL" => "debug",
        "INFO" => "info",
        "WARNING" => "warn",
        "ERROR" => "error",
        other => other,
    };

    let filter = EnvFilter::try_new(normalised).unwrap_or_else(|_| EnvFilter::new("info"));

    let subscriber = fmt::layer().with_target(false).with_thread_ids(false);

    tracing_subscriber::registry()
        .with(filter)
        .with(subscriber)
        .init();

    Ok(())
}

// ── Data-path discovery ────────────────────────────────────────────────────────

/// Attempt to locate the Claude AI data directory on the local system.
///
/// Checks the following paths in order and returns the first that exists:
/// 1. `~/.claude/projects/`
/// 2. `~/.config/claude/projects/`
///
/// Returns `None` when neither path exists.
pub fn discover_data_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let candidates = [
        home.join(".claude").join("projects"),
        home.join(".config").join("claude").join("projects"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ── test_ensure_directories ───────────────────────────────────────────────

    #[test]
    fn test_ensure_directories() {
        let tmp = TempDir::new().expect("tempdir");

        // Override HOME so that dirs::home_dir() resolves to our temp dir.
        let original_home = std::env::var_os("HOME");
        std::env::set_var("HOME", tmp.path());

        let result = ensure_directories();

        // Restore HOME.
        match original_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }

        result.expect("ensure_directories should succeed");

        let monitor_dir = tmp.path().join(".claude-monitor");
        assert!(monitor_dir.is_dir(), ".claude-monitor dir must exist");
        assert!(monitor_dir.join("logs").is_dir(), "logs subdir must exist");
        assert!(
            monitor_dir.join("cache").is_dir(),
            "cache subdir must exist"
        );
    }

    // ── test_discover_data_path ───────────────────────────────────────────────

    #[test]
    fn test_discover_data_path_returns_none_when_absent() {
        let tmp = TempDir::new().expect("tempdir");

        // Point HOME at a directory that has neither candidate path.
        let original_home = std::env::var_os("HOME");
        std::env::set_var("HOME", tmp.path());

        let path = discover_data_path();

        // Restore HOME.
        match original_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }

        assert!(
            path.is_none(),
            "should return None when neither path exists"
        );
    }

    #[test]
    fn test_discover_data_path_finds_dot_claude() {
        let tmp = TempDir::new().expect("tempdir");
        let projects = tmp.path().join(".claude").join("projects");
        std::fs::create_dir_all(&projects).expect("create projects dir");

        let original_home = std::env::var_os("HOME");
        std::env::set_var("HOME", tmp.path());

        let path = discover_data_path();

        match original_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }

        assert_eq!(path, Some(projects));
    }

    #[test]
    fn test_discover_data_path_finds_dot_config_claude() {
        let tmp = TempDir::new().expect("tempdir");
        // Create only the .config/claude/projects path (not the .claude one).
        let projects = tmp.path().join(".config").join("claude").join("projects");
        std::fs::create_dir_all(&projects).expect("create projects dir");

        let original_home = std::env::var_os("HOME");
        std::env::set_var("HOME", tmp.path());

        let path = discover_data_path();

        match original_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }

        assert_eq!(path, Some(projects));
    }
}
