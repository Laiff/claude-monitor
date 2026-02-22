use clap::{CommandFactory, Parser};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ── Settings (CLI) ─────────────────────────────────────────────────────────────

/// Real-time token usage monitoring for Claude AI
#[derive(Parser, Debug, Clone)]
#[command(
    name = "claude-monitor",
    about = "Real-time token usage monitoring for Claude AI",
    version
)]
pub struct Settings {
    /// Plan type
    #[arg(long, default_value = "custom", value_parser = ["pro", "max5", "max20", "custom"])]
    pub plan: String,

    /// View mode
    #[arg(long, default_value = "realtime", value_parser = ["realtime", "daily", "monthly", "session"])]
    pub view: String,

    /// Timezone (auto-detected if not specified)
    #[arg(long, default_value = "auto")]
    pub timezone: String,

    /// Time format
    #[arg(long, default_value = "auto", value_parser = ["12h", "24h", "auto"])]
    pub time_format: String,

    /// Display theme
    #[arg(long, default_value = "auto", value_parser = ["light", "dark", "classic", "auto"])]
    pub theme: String,

    /// Custom token limit for custom plan
    #[arg(long)]
    pub custom_limit_tokens: Option<u64>,

    /// Refresh rate in seconds (1-60)
    #[arg(long, default_value = "10", value_parser = clap::value_parser!(u32).range(1..=60))]
    pub refresh_rate: u32,

    /// Display refresh rate per second (Hz)
    #[arg(long, default_value = "0.75")]
    pub refresh_per_second: f64,

    /// Reset hour for daily limits (0-23)
    #[arg(long)]
    pub reset_hour: Option<u8>,

    /// Logging level
    #[arg(long, default_value = "INFO", value_parser = ["DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"])]
    pub log_level: String,

    /// Log file path
    #[arg(long)]
    pub log_file: Option<PathBuf>,

    /// Enable debug logging
    #[arg(long)]
    pub debug: bool,

    /// Clear saved configuration
    #[arg(long)]
    pub clear: bool,
}

// ── LastUsedParams ─────────────────────────────────────────────────────────────

/// Persisted last-used parameters saved to `~/.claude-monitor/last_used.json`.
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct LastUsedParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_rate: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_hour: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_limit_tokens: Option<u64>,
}

impl LastUsedParams {
    /// Return the default path to the persisted config file.
    /// Uses `~/.claude-monitor/last_used.json`.
    pub fn config_path() -> PathBuf {
        Self::config_path_in(&dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
    }

    /// Return the config path rooted at `base_dir` (used for testing).
    pub fn config_path_in(base_dir: &std::path::Path) -> PathBuf {
        base_dir.join(".claude-monitor").join("last_used.json")
    }

    /// Load persisted params from the default path.
    /// Returns `Default` when the file is absent or cannot be parsed.
    pub fn load() -> Self {
        Self::load_from(&Self::config_path())
    }

    /// Load persisted params from an explicit path.
    pub fn load_from(path: &std::path::Path) -> Self {
        let Ok(content) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&content).unwrap_or_default()
    }

    /// Atomically write params to the default path, creating parent directories
    /// if needed.
    pub fn save(&self) -> Result<(), std::io::Error> {
        self.save_to(&Self::config_path())
    }

    /// Atomically write params to an explicit path.
    pub fn save_to(&self, path: &std::path::Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;

        // Write to a temp file then rename for atomicity.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)?;
        std::fs::rename(&tmp, path)?;

        Ok(())
    }

    /// Delete the default config file if it exists.
    pub fn clear() -> Result<(), std::io::Error> {
        Self::clear_at(&Self::config_path())
    }

    /// Delete the config file at an explicit path if it exists.
    pub fn clear_at(path: &std::path::Path) -> Result<(), std::io::Error> {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
}

// ── Settings impl ──────────────────────────────────────────────────────────────

impl Settings {
    /// Parse CLI arguments, merge with last-used params where no explicit CLI
    /// value was provided, resolve `"auto"` values, and persist the result.
    pub fn load_with_last_used() -> Self {
        Self::load_with_last_used_impl(
            std::env::args_os().collect(),
            &LastUsedParams::config_path(),
        )
    }

    /// Same as [`load_with_last_used`] but accepts an explicit argument list,
    /// enabling unit-testing without spawning subprocesses.
    pub fn load_with_last_used_from_args(args: Vec<std::ffi::OsString>) -> Self {
        Self::load_with_last_used_impl(args, &LastUsedParams::config_path())
    }

    /// Full implementation – accepts args and an explicit config path so that
    /// tests can redirect to a temporary directory.
    pub fn load_with_last_used_impl(
        args: Vec<std::ffi::OsString>,
        config_path: &std::path::Path,
    ) -> Self {
        // Build raw ArgMatches so we can query ValueSource.
        let matches = Settings::command().get_matches_from(args.clone());

        // Parse into the typed struct using the same args.
        let mut settings = Settings::parse_from(args);

        if settings.clear {
            let _ = LastUsedParams::clear_at(config_path);
            // Resolve auto values and return without re-persisting.
            return Self::resolve_auto_values(settings, &matches);
        }

        let last = LastUsedParams::load_from(config_path);

        // Merge last-used values for fields that were NOT explicitly set on the
        // command line (CLI always wins).  'plan' is never loaded from last-used.
        if !is_arg_explicitly_set(&matches, "view") {
            if let Some(v) = last.view {
                settings.view = v;
            }
        }
        if !is_arg_explicitly_set(&matches, "timezone") {
            if let Some(v) = last.timezone {
                settings.timezone = v;
            }
        }
        // NOTE: clap stores the arg id using the *field name* (underscores),
        // not the long-flag spelling (hyphens).
        if !is_arg_explicitly_set(&matches, "time_format") {
            if let Some(v) = last.time_format {
                settings.time_format = v;
            }
        }
        if !is_arg_explicitly_set(&matches, "theme") {
            if let Some(v) = last.theme {
                settings.theme = v;
            }
        }
        if !is_arg_explicitly_set(&matches, "refresh_rate") {
            if let Some(v) = last.refresh_rate {
                settings.refresh_rate = v;
            }
        }
        if !is_arg_explicitly_set(&matches, "reset_hour") && settings.reset_hour.is_none() {
            settings.reset_hour = last.reset_hour;
        }
        if !is_arg_explicitly_set(&matches, "custom_limit_tokens")
            && settings.custom_limit_tokens.is_none()
        {
            settings.custom_limit_tokens = last.custom_limit_tokens;
        }

        settings = Self::resolve_auto_values(settings, &matches);

        // Persist current settings for next run.
        let params = LastUsedParams::from(&settings);
        let _ = params.save_to(config_path);

        settings
    }

    /// Resolve `"auto"` sentinel values and apply the `--debug` flag.
    fn resolve_auto_values(mut settings: Settings, _matches: &clap::ArgMatches) -> Settings {
        // Resolve "auto" timezone → system timezone.
        if settings.timezone == "auto" {
            settings.timezone = crate::time_utils::get_system_timezone();
        }

        // Resolve "auto" time_format → locale-based heuristic.
        if settings.time_format == "auto" {
            let is_12h = crate::time_utils::detect_time_format(Some(&settings.timezone), None);
            settings.time_format = if is_12h {
                "12h".to_string()
            } else {
                "24h".to_string()
            };
        }

        // --debug overrides log level.
        if settings.debug {
            settings.log_level = "DEBUG".to_string();
        }

        settings
    }
}

// ── Conversion ─────────────────────────────────────────────────────────────────

impl From<&Settings> for LastUsedParams {
    fn from(s: &Settings) -> Self {
        LastUsedParams {
            theme: Some(s.theme.clone()),
            timezone: Some(s.timezone.clone()),
            time_format: Some(s.time_format.clone()),
            refresh_rate: Some(s.refresh_rate),
            reset_hour: s.reset_hour,
            view: Some(s.view.clone()),
            custom_limit_tokens: s.custom_limit_tokens,
        }
    }
}

// ── Helper: check if an arg was explicitly set on the command line ─────────────

/// Returns `true` when `name` was supplied explicitly on the command line
/// (not via default value or environment variable).
fn is_arg_explicitly_set(matches: &clap::ArgMatches, name: &str) -> bool {
    matches.value_source(name) == Some(clap::parser::ValueSource::CommandLine)
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    // ── Helpers ───────────────────────────────────────────────────────────────

    /// Build the config path inside `tmp`.
    fn tmp_config_path(tmp: &TempDir) -> PathBuf {
        LastUsedParams::config_path_in(tmp.path())
    }

    /// Save `params` to `tmp`, then load them back.
    fn round_trip(tmp: &TempDir, params: &LastUsedParams) -> LastUsedParams {
        let path = tmp_config_path(tmp);
        params.save_to(&path).expect("save");
        LastUsedParams::load_from(&path)
    }

    // ── test_last_used_params_save_load ───────────────────────────────────────

    #[test]
    fn test_last_used_params_save_load() {
        let tmp = TempDir::new().expect("tempdir");
        let params = LastUsedParams {
            theme: Some("dark".to_string()),
            timezone: Some("Europe/Berlin".to_string()),
            time_format: Some("24h".to_string()),
            refresh_rate: Some(5),
            reset_hour: Some(9),
            view: Some("daily".to_string()),
            custom_limit_tokens: Some(50_000),
        };

        let loaded = round_trip(&tmp, &params);

        assert_eq!(loaded.theme, Some("dark".to_string()));
        assert_eq!(loaded.timezone, Some("Europe/Berlin".to_string()));
        assert_eq!(loaded.time_format, Some("24h".to_string()));
        assert_eq!(loaded.refresh_rate, Some(5));
        assert_eq!(loaded.reset_hour, Some(9));
        assert_eq!(loaded.view, Some("daily".to_string()));
        assert_eq!(loaded.custom_limit_tokens, Some(50_000));
    }

    // ── test_last_used_params_clear ───────────────────────────────────────────

    #[test]
    fn test_last_used_params_clear() {
        let tmp = TempDir::new().expect("tempdir");
        let path = tmp_config_path(&tmp);

        // Save something first.
        let params = LastUsedParams {
            theme: Some("light".to_string()),
            ..Default::default()
        };
        params.save_to(&path).expect("save");
        assert!(path.exists(), "file must exist after save");

        // Clear it.
        LastUsedParams::clear_at(&path).expect("clear");
        assert!(!path.exists(), "file must be gone after clear");
    }

    // ── test_last_used_params_default_when_missing ────────────────────────────

    #[test]
    fn test_last_used_params_default_when_missing() {
        let tmp = TempDir::new().expect("tempdir");
        // No file created – load should return default.
        let loaded = LastUsedParams::load_from(&tmp_config_path(&tmp));
        assert!(loaded.theme.is_none());
        assert!(loaded.timezone.is_none());
        assert!(loaded.time_format.is_none());
        assert!(loaded.refresh_rate.is_none());
        assert!(loaded.reset_hour.is_none());
        assert!(loaded.view.is_none());
        assert!(loaded.custom_limit_tokens.is_none());
    }

    // ── test_settings_default_values ─────────────────────────────────────────

    #[test]
    fn test_settings_default_values() {
        // Parse with only the binary name (no flags) to get all defaults.
        let settings = Settings::parse_from(["claude-monitor"]);

        assert_eq!(settings.plan, "custom");
        assert_eq!(settings.view, "realtime");
        assert_eq!(settings.timezone, "auto");
        assert_eq!(settings.time_format, "auto");
        assert_eq!(settings.theme, "auto");
        assert!(settings.custom_limit_tokens.is_none());
        assert_eq!(settings.refresh_rate, 10);
        assert!((settings.refresh_per_second - 0.75).abs() < f64::EPSILON);
        assert!(settings.reset_hour.is_none());
        assert_eq!(settings.log_level, "INFO");
        assert!(settings.log_file.is_none());
        assert!(!settings.debug);
        assert!(!settings.clear);
    }

    // ── test_from_settings_to_last_used ──────────────────────────────────────

    #[test]
    fn test_from_settings_to_last_used() {
        let settings = Settings {
            plan: "pro".to_string(),
            view: "daily".to_string(),
            timezone: "America/New_York".to_string(),
            time_format: "12h".to_string(),
            theme: "dark".to_string(),
            custom_limit_tokens: Some(100_000),
            refresh_rate: 30,
            refresh_per_second: 1.0,
            reset_hour: Some(6),
            log_level: "INFO".to_string(),
            log_file: None,
            debug: false,
            clear: false,
        };

        let last = LastUsedParams::from(&settings);

        assert_eq!(last.view, Some("daily".to_string()));
        assert_eq!(last.timezone, Some("America/New_York".to_string()));
        assert_eq!(last.time_format, Some("12h".to_string()));
        assert_eq!(last.theme, Some("dark".to_string()));
        assert_eq!(last.refresh_rate, Some(30));
        assert_eq!(last.reset_hour, Some(6));
        assert_eq!(last.custom_limit_tokens, Some(100_000));
        // 'plan' is NOT stored in LastUsedParams.
    }

    // ── test_settings_cli_parsing ─────────────────────────────────────────────

    #[test]
    fn test_settings_cli_explicit_plan() {
        let settings = Settings::parse_from(["claude-monitor", "--plan", "pro"]);
        assert_eq!(settings.plan, "pro");
    }

    #[test]
    fn test_settings_cli_debug_flag() {
        let settings = Settings::parse_from(["claude-monitor", "--debug"]);
        assert!(settings.debug);
    }

    #[test]
    fn test_settings_cli_custom_limit() {
        let settings = Settings::parse_from(["claude-monitor", "--custom-limit-tokens", "75000"]);
        assert_eq!(settings.custom_limit_tokens, Some(75_000));
    }

    #[test]
    fn test_settings_cli_log_file() {
        let settings = Settings::parse_from(["claude-monitor", "--log-file", "/tmp/monitor.log"]);
        assert_eq!(settings.log_file, Some(PathBuf::from("/tmp/monitor.log")));
    }

    // ── test_load_with_last_used (uses config path injection) ─────────────────

    #[test]
    fn test_load_with_last_used_merges_persisted_theme() {
        let tmp = TempDir::new().expect("tempdir");
        let config_path = tmp_config_path(&tmp);

        // Pre-populate last-used with a theme and resolved timezone/format.
        let params = LastUsedParams {
            theme: Some("dark".to_string()),
            timezone: Some("UTC".to_string()),
            time_format: Some("24h".to_string()),
            view: Some("realtime".to_string()),
            ..Default::default()
        };
        params.save_to(&config_path).expect("save");

        // Parse without --theme flag → should use persisted value.
        let settings =
            Settings::load_with_last_used_impl(vec!["claude-monitor".into()], &config_path);
        assert_eq!(settings.theme, "dark");
    }

    #[test]
    fn test_load_with_last_used_cli_overrides_persisted() {
        let tmp = TempDir::new().expect("tempdir");
        let config_path = tmp_config_path(&tmp);

        // Pre-populate last-used with dark theme.
        let params = LastUsedParams {
            theme: Some("dark".to_string()),
            timezone: Some("UTC".to_string()),
            time_format: Some("24h".to_string()),
            ..Default::default()
        };
        params.save_to(&config_path).expect("save");

        // Explicit --theme light on CLI must win.
        let settings = Settings::load_with_last_used_impl(
            vec!["claude-monitor".into(), "--theme".into(), "light".into()],
            &config_path,
        );
        assert_eq!(settings.theme, "light");
    }

    #[test]
    fn test_load_with_last_used_clear_removes_file() {
        let tmp = TempDir::new().expect("tempdir");
        let config_path = tmp_config_path(&tmp);

        let params = LastUsedParams {
            theme: Some("classic".to_string()),
            ..Default::default()
        };
        params.save_to(&config_path).expect("save");
        assert!(config_path.exists(), "file must exist before clear");

        Settings::load_with_last_used_impl(
            vec!["claude-monitor".into(), "--clear".into()],
            &config_path,
        );

        assert!(!config_path.exists(), "file must be gone after --clear");
    }

    #[test]
    fn test_load_with_last_used_debug_overrides_log_level() {
        let tmp = TempDir::new().expect("tempdir");
        let config_path = tmp_config_path(&tmp);

        let settings = Settings::load_with_last_used_impl(
            vec!["claude-monitor".into(), "--debug".into()],
            &config_path,
        );
        assert_eq!(settings.log_level, "DEBUG");
    }

    #[test]
    fn test_load_with_last_used_plan_not_loaded_from_persisted() {
        let tmp = TempDir::new().expect("tempdir");
        let config_path = tmp_config_path(&tmp);

        // --plan pro should be respected; there is no persisted plan.
        let settings = Settings::load_with_last_used_impl(
            vec!["claude-monitor".into(), "--plan".into(), "pro".into()],
            &config_path,
        );
        assert_eq!(settings.plan, "pro");
    }

    #[test]
    fn test_load_with_last_used_persists_after_run() {
        let tmp = TempDir::new().expect("tempdir");
        let config_path = tmp_config_path(&tmp);

        Settings::load_with_last_used_impl(
            vec!["claude-monitor".into(), "--theme".into(), "classic".into()],
            &config_path,
        );

        // After a run the file should have been created.
        assert!(
            config_path.exists(),
            "config file must be persisted after run"
        );
        let loaded = LastUsedParams::load_from(&config_path);
        assert_eq!(loaded.theme, Some("classic".to_string()));
    }
}
