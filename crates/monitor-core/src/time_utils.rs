use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use tracing::warn;

// ── System timezone detection ─────────────────────────────────────────────────

/// Detect the IANA timezone name of the running system.
///
/// Uses the `iana-time-zone` crate directly – no subprocess calls.
/// Falls back to `"UTC"` if detection fails.
pub fn get_system_timezone() -> String {
    iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string())
}

// ── TimezoneHandler ───────────────────────────────────────────────────────────

/// Handles timezone-aware timestamp parsing and conversion.
pub struct TimezoneHandler {
    default_tz: Tz,
}

impl TimezoneHandler {
    /// Create a handler with the given IANA timezone name as the default.
    ///
    /// If `tz_name` is not a recognised IANA timezone, falls back to UTC
    /// and logs a warning.
    pub fn new(tz_name: &str) -> Self {
        let tz = tz_name.parse::<Tz>().unwrap_or_else(|_| {
            warn!(
                "TimezoneHandler: unrecognised timezone \"{}\", falling back to UTC",
                tz_name
            );
            Tz::UTC
        });
        Self { default_tz: tz }
    }

    /// Parse an ISO 8601 / RFC 3339 timestamp string into a UTC [`DateTime`].
    ///
    /// Handles the common `Z`-suffix form and any fixed UTC offset.
    /// Returns `None` for empty strings or unrecognised formats.
    pub fn parse_timestamp(&self, s: &str) -> Option<DateTime<Utc>> {
        if s.is_empty() {
            return None;
        }

        // Replace trailing 'Z' with '+00:00'.
        let normalised = if let Some(stripped) = s.strip_suffix('Z') {
            format!("{}+00:00", stripped)
        } else {
            s.to_string()
        };

        if let Ok(dt) = DateTime::parse_from_rfc3339(&normalised) {
            return Some(dt.with_timezone(&Utc));
        }

        // Try naive datetime without timezone – interpret as `default_tz`.
        const FMTS: &[&str] = &[
            "%Y-%m-%dT%H:%M:%S%.f",
            "%Y-%m-%dT%H:%M:%S",
            "%Y-%m-%d %H:%M:%S%.f",
            "%Y-%m-%d %H:%M:%S",
        ];
        for fmt in FMTS {
            if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
                use chrono::TimeZone as _;
                if let chrono::LocalResult::Single(dt) = self.default_tz.from_local_datetime(&naive)
                {
                    return Some(dt.with_timezone(&Utc));
                }
            }
        }

        warn!("TimezoneHandler: could not parse timestamp \"{}\"", s);
        None
    }

    /// Identity operation – a UTC [`DateTime`] is already UTC.
    ///
    /// Provided for API symmetry with the Python version, which re-attaches
    /// timezone information when it may be missing.
    pub fn ensure_utc(&self, dt: DateTime<Utc>) -> DateTime<Utc> {
        dt
    }

    /// Validate that `tz_name` is a recognised IANA timezone identifier.
    pub fn validate_timezone(tz_name: &str) -> bool {
        tz_name.parse::<Tz>().is_ok()
    }

    /// Convert a UTC [`DateTime`] to a specific named timezone.
    ///
    /// If the target timezone is invalid, falls back to the handler's default
    /// and logs a warning.
    pub fn convert_to_timezone(&self, dt: DateTime<Utc>, tz_name: &str) -> DateTime<Tz> {
        let tz = tz_name.parse::<Tz>().unwrap_or_else(|_| {
            warn!(
                "TimezoneHandler: invalid target timezone \"{}\", using default",
                tz_name
            );
            self.default_tz
        });
        dt.with_timezone(&tz)
    }

    /// Expose the configured default timezone.
    pub fn default_tz(&self) -> Tz {
        self.default_tz
    }
}

// ── 12-hour / 24-hour format detection ───────────────────────────────────────

/// IANA country codes whose users conventionally use 12-hour clock format.
const TWELVE_HOUR_COUNTRIES: &[&str] = &[
    "US", "CA", "AU", "NZ", "PH", "IN", "EG", "SA", "AE", "JO", "IR", "PK", "BD", "MY", "MX", "CO",
    "VE", "AR",
];

/// Decide whether to use 12-hour clock display.
///
/// Priority:
/// 1. `explicit` `"12h"` → `true`, `"24h"` → `false`.
/// 2. Country derived from `timezone` (e.g. `"America/New_York"` → `"US"`).
/// 3. System timezone.
pub fn detect_time_format(timezone: Option<&str>, explicit: Option<&str>) -> bool {
    // 1. Explicit override.
    if let Some(fmt) = explicit {
        match fmt.to_lowercase().as_str() {
            "12h" => return true,
            "24h" => return false,
            _ => {} // fall through
        }
    }

    // 2. Derive country from timezone string.
    let tz_to_check = timezone
        .map(|s| s.to_string())
        .unwrap_or_else(get_system_timezone);

    if let Some(country) = country_from_timezone(&tz_to_check) {
        return TWELVE_HOUR_COUNTRIES.contains(&country.as_str());
    }

    // 3. Default: 24-hour.
    false
}

/// Heuristic: extract a 2-letter country code from a standard IANA timezone
/// string such as `"America/New_York"`, `"Australia/Sydney"`, etc.
fn country_from_timezone(tz: &str) -> Option<String> {
    let lower = tz.to_lowercase();

    // America/* → check for Canadian or US timezones.
    if lower.starts_with("america/") {
        // Canadian cities.
        const CA_CITIES: &[&str] = &[
            "toronto",
            "vancouver",
            "montreal",
            "edmonton",
            "winnipeg",
            "halifax",
            "regina",
            "st_johns",
            "yellowknife",
        ];
        let city = lower.trim_start_matches("america/");
        if CA_CITIES.contains(&city) {
            return Some("CA".to_string());
        }
        return Some("US".to_string());
    }

    if lower.starts_with("australia/") {
        return Some("AU".to_string());
    }
    if lower.starts_with("pacific/auckland") || lower.starts_with("pacific/chatham") {
        return Some("NZ".to_string());
    }
    if lower.starts_with("asia/manila")
        || lower.starts_with("asia/kolkata")
        || lower.starts_with("asia/calcutta")
    {
        if lower.contains("manila") {
            return Some("PH".to_string());
        }
        return Some("IN".to_string());
    }
    if lower.starts_with("asia/karachi") {
        return Some("PK".to_string());
    }
    if lower.starts_with("asia/dhaka") {
        return Some("BD".to_string());
    }
    if lower.starts_with("asia/kuala_lumpur") || lower.starts_with("asia/kuching") {
        return Some("MY".to_string());
    }
    if lower.starts_with("africa/cairo") {
        return Some("EG".to_string());
    }
    if lower.starts_with("asia/riyadh") {
        return Some("SA".to_string());
    }
    if lower.starts_with("asia/dubai") {
        return Some("AE".to_string());
    }
    if lower.starts_with("asia/amman") {
        return Some("JO".to_string());
    }
    if lower.starts_with("asia/tehran") {
        return Some("IR".to_string());
    }
    if lower.starts_with("america/mexico_city") {
        return Some("MX".to_string());
    }
    if lower.starts_with("america/bogota") {
        return Some("CO".to_string());
    }
    if lower.starts_with("america/caracas") {
        return Some("VE".to_string());
    }
    if lower.starts_with("america/argentina/") || lower.starts_with("america/buenos_aires") {
        return Some("AR".to_string());
    }

    None
}

// ── format_display_time ───────────────────────────────────────────────────────

/// Format a UTC [`DateTime`] as a displayable time string.
///
/// * `use_12h = Some(true)`  → 12-hour format (e.g. `"02:30 PM"`).
/// * `use_12h = Some(false)` → 24-hour format (e.g. `"14:30"`).
/// * `use_12h = None`        → auto-detect using [`detect_time_format`].
/// * `include_seconds = true` adds `":SS"` to the output.
pub fn format_display_time(
    dt: &DateTime<Utc>,
    use_12h: Option<bool>,
    include_seconds: bool,
) -> String {
    let twelve_hour = use_12h.unwrap_or_else(|| detect_time_format(None, None));

    if twelve_hour {
        if include_seconds {
            dt.format("%I:%M:%S %p").to_string()
        } else {
            dt.format("%I:%M %p").to_string()
        }
    } else if include_seconds {
        dt.format("%H:%M:%S").to_string()
    } else {
        dt.format("%H:%M").to_string()
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone as _;

    // ── TimezoneHandler::validate_timezone ───────────────────────────────────

    #[test]
    fn test_validate_timezone_valid() {
        assert!(TimezoneHandler::validate_timezone("America/New_York"));
        assert!(TimezoneHandler::validate_timezone("Europe/London"));
        assert!(TimezoneHandler::validate_timezone("UTC"));
        assert!(TimezoneHandler::validate_timezone("Asia/Tokyo"));
    }

    #[test]
    fn test_validate_timezone_invalid() {
        assert!(!TimezoneHandler::validate_timezone("Mars/Olympus"));
        assert!(!TimezoneHandler::validate_timezone(""));
        assert!(!TimezoneHandler::validate_timezone("not-a-timezone"));
    }

    // ── TimezoneHandler::new ─────────────────────────────────────────────────

    #[test]
    fn test_new_valid_timezone() {
        let handler = TimezoneHandler::new("America/New_York");
        assert_eq!(handler.default_tz(), Tz::America__New_York);
    }

    #[test]
    fn test_new_invalid_timezone_falls_back_to_utc() {
        let handler = TimezoneHandler::new("Invalid/Timezone");
        assert_eq!(handler.default_tz(), Tz::UTC);
    }

    // ── TimezoneHandler::parse_timestamp ─────────────────────────────────────

    #[test]
    fn test_parse_timestamp_z_suffix() {
        let handler = TimezoneHandler::new("UTC");
        let dt = handler.parse_timestamp("2024-01-15T10:30:00Z").unwrap();
        assert_eq!(dt.hour(), 10);
        assert_eq!(dt.minute(), 30);
    }

    #[test]
    fn test_parse_timestamp_with_offset() {
        let handler = TimezoneHandler::new("UTC");
        let dt = handler
            .parse_timestamp("2024-01-15T12:00:00+02:00")
            .unwrap();
        // 12:00 +02:00 = 10:00 UTC
        assert_eq!(dt.hour(), 10);
    }

    #[test]
    fn test_parse_timestamp_empty_returns_none() {
        let handler = TimezoneHandler::new("UTC");
        assert!(handler.parse_timestamp("").is_none());
    }

    #[test]
    fn test_parse_timestamp_garbage_returns_none() {
        let handler = TimezoneHandler::new("UTC");
        assert!(handler.parse_timestamp("not-a-date").is_none());
    }

    // ── TimezoneHandler::ensure_utc ──────────────────────────────────────────

    #[test]
    fn test_ensure_utc_is_identity() {
        let handler = TimezoneHandler::new("UTC");
        let dt = Utc.with_ymd_and_hms(2024, 6, 1, 12, 0, 0).unwrap();
        assert_eq!(handler.ensure_utc(dt), dt);
    }

    // ── TimezoneHandler::convert_to_timezone ─────────────────────────────────

    #[test]
    fn test_convert_to_timezone() {
        let handler = TimezoneHandler::new("UTC");
        let utc = Utc.with_ymd_and_hms(2024, 6, 1, 12, 0, 0).unwrap();
        let converted = handler.convert_to_timezone(utc, "America/New_York");
        // New York is UTC-4 in summer (EDT)
        assert_eq!(converted.hour(), 8);
    }

    #[test]
    fn test_convert_to_invalid_timezone_uses_default() {
        let handler = TimezoneHandler::new("UTC");
        let utc = Utc.with_ymd_and_hms(2024, 6, 1, 12, 0, 0).unwrap();
        let converted = handler.convert_to_timezone(utc, "Invalid/Zone");
        // Falls back to UTC, so hour stays 12.
        assert_eq!(converted.hour(), 12);
    }

    // ── detect_time_format ───────────────────────────────────────────────────

    #[test]
    fn test_detect_explicit_12h() {
        assert!(detect_time_format(None, Some("12h")));
        assert!(detect_time_format(None, Some("12H")));
    }

    #[test]
    fn test_detect_explicit_24h() {
        assert!(!detect_time_format(None, Some("24h")));
        assert!(!detect_time_format(None, Some("24H")));
    }

    #[test]
    fn test_detect_us_timezone_is_12h() {
        assert!(detect_time_format(Some("America/New_York"), None));
        assert!(detect_time_format(Some("America/Los_Angeles"), None));
        assert!(detect_time_format(Some("America/Chicago"), None));
    }

    #[test]
    fn test_detect_australia_is_12h() {
        assert!(detect_time_format(Some("Australia/Sydney"), None));
    }

    #[test]
    fn test_detect_europe_is_24h() {
        assert!(!detect_time_format(Some("Europe/Berlin"), None));
        assert!(!detect_time_format(Some("Europe/Paris"), None));
    }

    #[test]
    fn test_detect_asia_japan_is_24h() {
        assert!(!detect_time_format(Some("Asia/Tokyo"), None));
    }

    // ── format_display_time ──────────────────────────────────────────────────

    #[test]
    fn test_format_display_time_24h() {
        let dt = Utc.with_ymd_and_hms(2024, 1, 1, 14, 30, 0).unwrap();
        assert_eq!(format_display_time(&dt, Some(false), false), "14:30");
    }

    #[test]
    fn test_format_display_time_24h_with_seconds() {
        let dt = Utc.with_ymd_and_hms(2024, 1, 1, 14, 30, 45).unwrap();
        assert_eq!(format_display_time(&dt, Some(false), true), "14:30:45");
    }

    #[test]
    fn test_format_display_time_12h() {
        let dt = Utc.with_ymd_and_hms(2024, 1, 1, 14, 30, 0).unwrap();
        let formatted = format_display_time(&dt, Some(true), false);
        // 14:30 → "02:30 PM"
        assert!(
            formatted.contains("PM") || formatted.contains("pm"),
            "12h format: {}",
            formatted
        );
    }

    #[test]
    fn test_format_display_time_12h_with_seconds() {
        let dt = Utc.with_ymd_and_hms(2024, 1, 1, 9, 5, 3).unwrap();
        let formatted = format_display_time(&dt, Some(true), true);
        assert!(
            formatted.contains("AM") || formatted.contains("am"),
            "12h with seconds: {}",
            formatted
        );
        assert!(formatted.contains("09:05:03") || formatted.contains("9:05:03"));
    }

    // ── get_system_timezone ──────────────────────────────────────────────────

    #[test]
    fn test_get_system_timezone_returns_nonempty_string() {
        let tz = get_system_timezone();
        assert!(!tz.is_empty(), "system timezone should not be empty");
    }
}

// Re-export chrono items used in tests.
#[allow(unused_imports)]
use chrono::{Datelike, Timelike};
