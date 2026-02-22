use std::path::PathBuf;
use thiserror::Error;

/// All errors produced by the Claude Monitor.
#[derive(Error, Debug)]
pub enum MonitorError {
    /// A file could not be opened or read from disk.
    #[error("Failed to read file {path}: {source}")]
    FileRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A JSON document could not be parsed.
    #[error("Failed to parse JSON: {0}")]
    JsonParse(#[from] serde_json::Error),

    /// A timestamp string did not match any recognised format.
    #[error("Invalid timestamp format: {0}")]
    TimestampParse(String),

    /// A model identifier could not be resolved.
    #[error("Unknown model: {0}")]
    UnknownModel(String),

    /// A plan name string is not one of the recognised plan types.
    #[error("Invalid plan type: {0}")]
    InvalidPlan(String),

    /// The expected data directory does not exist.
    #[error("Data path not found: {0}")]
    DataPathNotFound(PathBuf),

    /// No JSONL usage files were found under the given directory.
    #[error("No JSONL files found in {0}")]
    NoDataFiles(PathBuf),

    /// An error originating from the terminal / TUI layer.
    #[error("Terminal error: {0}")]
    Terminal(String),

    /// A configuration value is missing or invalid.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Pass-through for any raw I/O error that does not carry a path.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Catch-all for errors from third-party crates via `anyhow`.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Convenience alias used throughout the monitor crates.
pub type Result<T> = std::result::Result<T, MonitorError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_file_read() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        let err = MonitorError::FileRead {
            path: PathBuf::from("/some/path.jsonl"),
            source: io_err,
        };
        let msg = err.to_string();
        assert!(msg.contains("Failed to read file"));
        assert!(msg.contains("/some/path.jsonl"));
        assert!(msg.contains("no such file"));
    }

    #[test]
    fn test_error_display_timestamp_parse() {
        let err = MonitorError::TimestampParse("not-a-timestamp".to_string());
        let msg = err.to_string();
        assert_eq!(msg, "Invalid timestamp format: not-a-timestamp");
    }

    #[test]
    fn test_error_display_unknown_model() {
        let err = MonitorError::UnknownModel("gpt-99".to_string());
        let msg = err.to_string();
        assert_eq!(msg, "Unknown model: gpt-99");
    }

    #[test]
    fn test_error_display_invalid_plan() {
        let err = MonitorError::InvalidPlan("enterprise".to_string());
        let msg = err.to_string();
        assert_eq!(msg, "Invalid plan type: enterprise");
    }

    #[test]
    fn test_error_display_data_path_not_found() {
        let err = MonitorError::DataPathNotFound(PathBuf::from("/missing/dir"));
        let msg = err.to_string();
        assert_eq!(msg, "Data path not found: /missing/dir");
    }

    #[test]
    fn test_error_display_no_data_files() {
        let err = MonitorError::NoDataFiles(PathBuf::from("/empty/dir"));
        let msg = err.to_string();
        assert_eq!(msg, "No JSONL files found in /empty/dir");
    }

    #[test]
    fn test_error_display_terminal() {
        let err = MonitorError::Terminal("crossterm failure".to_string());
        let msg = err.to_string();
        assert_eq!(msg, "Terminal error: crossterm failure");
    }

    #[test]
    fn test_error_display_config() {
        let err = MonitorError::Config("missing api key".to_string());
        let msg = err.to_string();
        assert_eq!(msg, "Configuration error: missing api key");
    }

    #[test]
    fn test_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err: MonitorError = io_err.into();
        let msg = err.to_string();
        assert!(msg.contains("denied"));
    }

    #[test]
    fn test_error_from_serde_json() {
        let json_err = serde_json::from_str::<serde_json::Value>("{invalid}").unwrap_err();
        let err: MonitorError = json_err.into();
        let msg = err.to_string();
        assert!(msg.contains("Failed to parse JSON"));
    }
}
