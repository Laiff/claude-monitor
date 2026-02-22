/// Format a floating-point number with thousands separators and a fixed number
/// of decimal places.
///
/// # Examples
///
/// ```
/// use monitor_core::formatting::format_number;
///
/// assert_eq!(format_number(1234.5,  1), "1,234.5");
/// assert_eq!(format_number(1234567.0, 0), "1,234,567");
/// assert_eq!(format_number(0.0, 2), "0.00");
/// assert_eq!(format_number(-9876.5, 1), "-9,876.5");
/// ```
pub fn format_number(value: f64, decimals: u32) -> String {
    // Handle the sign separately so the thousands grouping works on the
    // absolute value.
    let negative = value < 0.0;
    let abs_value = value.abs();

    // Round to the requested decimal places.
    // Add a tiny epsilon (half ULP at the target precision) before rounding
    // to avoid IEEE 754 binary-representation issues at exact midpoints.
    let factor = 10_f64.powi(decimals as i32);
    let epsilon = f64::EPSILON * abs_value * factor;
    let rounded = ((abs_value * factor) + epsilon).round() / factor;

    let integer_part = rounded.trunc() as u64;
    let frac_part = rounded - rounded.trunc();

    // Build the thousands-separated integer portion.
    let int_str = integer_part.to_string();
    let grouped = group_thousands(&int_str);

    let result = if decimals == 0 {
        grouped
    } else {
        // Format the fractional part to the exact number of decimals.
        let frac_str = format!("{:.prec$}", frac_part, prec = decimals as usize);
        // `frac_str` starts with "0.", e.g. "0.50". Strip the leading "0".
        let decimal_digits = &frac_str[1..]; // ".50"
        format!("{}{}", grouped, decimal_digits)
    };

    if negative {
        format!("-{}", result)
    } else {
        result
    }
}

/// Format a monetary amount as a USD string with two decimal places and
/// thousands separators.
///
/// # Examples
///
/// ```
/// use monitor_core::formatting::format_currency;
///
/// assert_eq!(format_currency(1234.56),  "$1,234.56");
/// assert_eq!(format_currency(0.0),      "$0.00");
/// assert_eq!(format_currency(-9.99),    "$-9.99");
/// ```
pub fn format_currency(amount: f64) -> String {
    if amount < 0.0 {
        format!("$-{}", format_number(amount.abs(), 2))
    } else {
        format!("${}", format_number(amount, 2))
    }
}

/// Format a duration in minutes as a human-readable string.
///
/// * `< 60` minutes → `"45m"`
/// * `≥ 60` minutes, no remainder → `"3h"`
/// * `≥ 60` minutes, with remainder → `"3h 45m"`
///
/// # Examples
///
/// ```
/// use monitor_core::formatting::format_time;
///
/// assert_eq!(format_time(45.0),  "45m");
/// assert_eq!(format_time(60.0),  "1h");
/// assert_eq!(format_time(180.0), "3h");
/// assert_eq!(format_time(225.0), "3h 45m");
/// assert_eq!(format_time(0.0),   "0m");
/// ```
pub fn format_time(minutes: f64) -> String {
    let total_mins = minutes.round() as i64;
    if total_mins < 60 {
        format!("{}m", total_mins)
    } else {
        let hours = total_mins / 60;
        let mins = total_mins % 60;
        if mins == 0 {
            format!("{}h", hours)
        } else {
            format!("{}h {}m", hours, mins)
        }
    }
}

/// Calculate `(part / whole) * 100`, rounded to `decimal_places`.
///
/// Returns `0.0` if `whole` is zero to avoid division by zero.
///
/// # Examples
///
/// ```
/// use monitor_core::formatting::percentage;
///
/// assert!((percentage(50.0, 200.0, 1) - 25.0).abs() < 1e-9);
/// assert_eq!(percentage(0.0, 0.0, 2), 0.0);
/// ```
pub fn percentage(part: f64, whole: f64, decimal_places: u32) -> f64 {
    if whole == 0.0 {
        return 0.0;
    }
    let raw = (part / whole) * 100.0;
    let factor = 10_f64.powi(decimal_places as i32);
    (raw * factor).round() / factor
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Insert commas every three digits from the right of an integer string.
fn group_thousands(s: &str) -> String {
    if s.len() <= 3 {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    let remainder = chars.len() % 3;
    for (i, &c) in chars.iter().enumerate() {
        if i != 0 && (i % 3 == remainder) {
            result.push(',');
        }
        result.push(c);
    }
    result
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── format_number ────────────────────────────────────────────────────────

    #[test]
    fn test_format_number_zero() {
        assert_eq!(format_number(0.0, 0), "0");
        assert_eq!(format_number(0.0, 2), "0.00");
    }

    #[test]
    fn test_format_number_no_thousands() {
        assert_eq!(format_number(123.456, 2), "123.46");
    }

    #[test]
    fn test_format_number_with_thousands() {
        assert_eq!(format_number(1_234.5, 1), "1,234.5");
    }

    #[test]
    fn test_format_number_millions() {
        assert_eq!(format_number(1_234_567.0, 0), "1,234,567");
    }

    #[test]
    fn test_format_number_negative() {
        assert_eq!(format_number(-9_876.5, 1), "-9,876.5");
    }

    #[test]
    fn test_format_number_exact_thousands() {
        assert_eq!(format_number(1_000.0, 0), "1,000");
    }

    #[test]
    fn test_format_number_small_decimals() {
        assert_eq!(format_number(0.001, 3), "0.001");
    }

    #[test]
    fn test_format_number_rounds_up() {
        assert_eq!(format_number(1.005, 2), "1.01");
    }

    // ── format_currency ──────────────────────────────────────────────────────

    #[test]
    fn test_format_currency_positive() {
        assert_eq!(format_currency(1_234.56), "$1,234.56");
    }

    #[test]
    fn test_format_currency_zero() {
        assert_eq!(format_currency(0.0), "$0.00");
    }

    #[test]
    fn test_format_currency_negative() {
        assert_eq!(format_currency(-9.99), "$-9.99");
    }

    #[test]
    fn test_format_currency_large() {
        assert_eq!(format_currency(1_000_000.0), "$1,000,000.00");
    }

    // ── format_time ──────────────────────────────────────────────────────────

    #[test]
    fn test_format_time_zero() {
        assert_eq!(format_time(0.0), "0m");
    }

    #[test]
    fn test_format_time_under_hour() {
        assert_eq!(format_time(45.0), "45m");
        assert_eq!(format_time(1.0), "1m");
        assert_eq!(format_time(59.0), "59m");
    }

    #[test]
    fn test_format_time_exact_hours() {
        assert_eq!(format_time(60.0), "1h");
        assert_eq!(format_time(120.0), "2h");
        assert_eq!(format_time(180.0), "3h");
    }

    #[test]
    fn test_format_time_hours_and_minutes() {
        assert_eq!(format_time(90.0), "1h 30m");
        assert_eq!(format_time(225.0), "3h 45m");
        assert_eq!(format_time(61.0), "1h 1m");
    }

    #[test]
    fn test_format_time_fractional_rounds() {
        // 60.5 rounds to 61 minutes → "1h 1m"
        assert_eq!(format_time(60.5), "1h 1m");
    }

    // ── percentage ───────────────────────────────────────────────────────────

    #[test]
    fn test_percentage_basic() {
        let p = percentage(50.0, 200.0, 1);
        assert!((p - 25.0).abs() < 1e-9, "percentage = {p}");
    }

    #[test]
    fn test_percentage_zero_whole() {
        assert_eq!(percentage(10.0, 0.0, 2), 0.0);
    }

    #[test]
    fn test_percentage_full() {
        let p = percentage(100.0, 100.0, 0);
        assert!((p - 100.0).abs() < 1e-9);
    }

    #[test]
    fn test_percentage_rounding() {
        let p = percentage(1.0, 3.0, 2);
        assert!((p - 33.33).abs() < 1e-2, "percentage = {p}");
    }

    #[test]
    fn test_percentage_zero_part() {
        assert_eq!(percentage(0.0, 100.0, 2), 0.0);
    }

    // ── group_thousands (via format_number) ──────────────────────────────────

    #[test]
    fn test_group_thousands_one_digit() {
        assert_eq!(format_number(5.0, 0), "5");
    }

    #[test]
    fn test_group_thousands_four_digits() {
        assert_eq!(format_number(1234.0, 0), "1,234");
    }

    #[test]
    fn test_group_thousands_seven_digits() {
        assert_eq!(format_number(1_234_567.0, 0), "1,234,567");
    }
}
