use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use tracing::warn;

// ── TimestampProcessor ────────────────────────────────────────────────────────

/// Parses timestamps from the variety of formats found in JSONL usage files.
pub struct TimestampProcessor;

impl TimestampProcessor {
    /// Attempt to parse a [`serde_json::Value`] into a UTC [`DateTime`].
    ///
    /// Handles:
    /// * `null`       → `None`
    /// * JSON string  → ISO 8601 / RFC 3339 (including `Z`-suffix) or common
    ///   date-time patterns.
    /// * JSON number  → Unix timestamp (integer or float seconds).
    pub fn parse(value: &Value) -> Option<DateTime<Utc>> {
        match value {
            Value::Null => None,
            Value::String(s) => Self::parse_str(s.as_str()),
            Value::Number(n) => {
                if let Some(secs) = n.as_i64() {
                    DateTime::from_timestamp(secs, 0)
                } else if let Some(f) = n.as_f64() {
                    let secs = f.trunc() as i64;
                    let nanos = (f.fract() * 1_000_000_000.0).round() as u32;
                    DateTime::from_timestamp(secs, nanos)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn parse_str(s: &str) -> Option<DateTime<Utc>> {
        if s.is_empty() {
            return None;
        }

        // Replace trailing 'Z' with '+00:00' for RFC 3339 compatibility.
        let normalised = if let Some(stripped) = s.strip_suffix('Z') {
            format!("{}+00:00", stripped)
        } else {
            s.to_string()
        };

        // Try RFC 3339 / ISO 8601 with offset.
        if let Ok(dt) = DateTime::parse_from_rfc3339(&normalised) {
            return Some(dt.with_timezone(&Utc));
        }

        // Try RFC 2822 (email date format).
        if let Ok(dt) = DateTime::parse_from_rfc2822(s) {
            return Some(dt.with_timezone(&Utc));
        }

        // Try a series of common strftime-like patterns.
        const FORMATS: &[&str] = &[
            "%Y-%m-%dT%H:%M:%S%.f",
            "%Y-%m-%dT%H:%M:%S",
            "%Y-%m-%d %H:%M:%S%.f",
            "%Y-%m-%d %H:%M:%S",
            "%Y-%m-%d",
            "%d/%m/%Y %H:%M:%S",
            "%m/%d/%Y %H:%M:%S",
        ];

        for fmt in FORMATS {
            if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
                return Some(Utc.from_utc_datetime(&naive));
            }
            // date-only patterns use NaiveDate.
            if let Ok(date) = chrono::NaiveDate::parse_from_str(s, fmt) {
                let naive = date.and_hms_opt(0, 0, 0)?;
                return Some(Utc.from_utc_datetime(&naive));
            }
        }

        warn!(
            "TimestampProcessor: could not parse timestamp string \"{}\"",
            s
        );
        None
    }
}

// ── ExtractedTokens ───────────────────────────────────────────────────────────

/// Token counts extracted from a raw JSON usage entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExtractedTokens {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub total_tokens: u64,
}

// ── TokenExtractor ────────────────────────────────────────────────────────────

/// Extracts token counts from a raw JSON entry, handling the many different
/// key-name and nesting conventions used by the Claude API and its wrappers.
pub struct TokenExtractor;

impl TokenExtractor {
    /// Extract token counts from any shape of JSON entry.
    ///
    /// For `type == "assistant"` entries the lookup order is:
    ///   `message.usage` → `usage` → root object.
    ///
    /// For all other entries:
    ///   `usage` → `message.usage` → root object.
    ///
    /// Within each candidate object, the first alternative key that yields a
    /// non-zero value wins.
    pub fn extract(data: &Value) -> ExtractedTokens {
        let is_assistant = data
            .get("type")
            .and_then(|v| v.as_str())
            .map(|s| s == "assistant")
            .unwrap_or(false);

        // Build the ordered list of source objects to probe.
        let message_usage = data.get("message").and_then(|m| m.get("usage"));
        let usage = data.get("usage");

        let sources: Vec<Option<&Value>> = if is_assistant {
            vec![message_usage, usage, Some(data)]
        } else {
            vec![usage, message_usage, Some(data)]
        };

        for source_opt in sources {
            let Some(source) = source_opt else { continue };

            let input = Self::find_u64(source, &["input_tokens", "inputTokens", "prompt_tokens"]);
            let output = Self::find_u64(
                source,
                &["output_tokens", "outputTokens", "completion_tokens"],
            );

            if input > 0 || output > 0 {
                let cache_create = Self::find_u64(
                    source,
                    &[
                        "cache_creation_tokens",
                        "cache_creation_input_tokens",
                        "cacheCreationInputTokens",
                    ],
                );
                let cache_read = Self::find_u64(
                    source,
                    &[
                        "cache_read_input_tokens",
                        "cache_read_tokens",
                        "cacheReadInputTokens",
                    ],
                );
                let total = input + output + cache_create + cache_read;
                return ExtractedTokens {
                    input_tokens: input,
                    output_tokens: output,
                    cache_creation_input_tokens: cache_create,
                    cache_read_input_tokens: cache_read,
                    total_tokens: total,
                };
            }
        }

        ExtractedTokens::default()
    }

    fn find_u64(obj: &Value, keys: &[&str]) -> u64 {
        for &key in keys {
            if let Some(v) = obj.get(key).and_then(|v| v.as_u64()) {
                return v;
            }
        }
        0
    }
}

// ── DataConverter ─────────────────────────────────────────────────────────────

/// Utility helpers for transforming raw JSON entry data.
pub struct DataConverter;

impl DataConverter {
    /// Flatten a nested JSON object into a single-level map with dotted keys.
    ///
    /// For example, `{"a": {"b": 1}}` with prefix `""` becomes
    /// `{"a.b": 1}` (or `{"prefix.a.b": 1}` when `prefix` is non-empty).
    pub fn flatten_nested(data: &Value, prefix: &str) -> serde_json::Map<String, Value> {
        let mut result = serde_json::Map::new();
        Self::flatten_inner(data, prefix, &mut result);
        result
    }

    fn flatten_inner(value: &Value, prefix: &str, output: &mut serde_json::Map<String, Value>) {
        match value {
            Value::Object(map) => {
                for (key, val) in map {
                    let new_key = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{}.{}", prefix, key)
                    };
                    Self::flatten_inner(val, &new_key, output);
                }
            }
            Value::Array(arr) => {
                for (i, item) in arr.iter().enumerate() {
                    let new_key = if prefix.is_empty() {
                        i.to_string()
                    } else {
                        format!("{}.{}", prefix, i)
                    };
                    Self::flatten_inner(item, &new_key, output);
                }
            }
            _ => {
                output.insert(prefix.to_string(), value.clone());
            }
        }
    }

    /// Extract the model name from a JSON entry.
    ///
    /// Tries `data["model"]`, then `data["message"]["model"]`.
    /// Falls back to `"claude-3-5-sonnet"` if neither is present.
    pub fn extract_model_name(data: &Value) -> String {
        if let Some(s) = data.get("model").and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
        if let Some(s) = data
            .get("message")
            .and_then(|m| m.get("model"))
            .and_then(|v| v.as_str())
        {
            if !s.is_empty() {
                return s.to_string();
            }
        }
        "claude-3-5-sonnet".to_string()
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── TimestampProcessor ───────────────────────────────────────────────────

    #[test]
    fn test_parse_null_returns_none() {
        assert!(TimestampProcessor::parse(&json!(null)).is_none());
    }

    #[test]
    fn test_parse_z_suffix_iso() {
        let v = json!("2024-01-15T10:30:00Z");
        let dt = TimestampProcessor::parse(&v).unwrap();
        assert_eq!(dt.year(), 2024);
        assert_eq!(dt.month(), 1);
        assert_eq!(dt.day(), 15);
        assert_eq!(dt.hour(), 10);
    }

    #[test]
    fn test_parse_rfc3339_with_offset() {
        let v = json!("2024-03-20T14:00:00+05:00");
        let dt = TimestampProcessor::parse(&v).unwrap();
        // 14:00 +05:00 = 09:00 UTC
        assert_eq!(dt.hour(), 9);
    }

    #[test]
    fn test_parse_integer_unix_timestamp() {
        let v = json!(0i64);
        let dt = TimestampProcessor::parse(&v).unwrap();
        assert_eq!(dt.year(), 1970);
    }

    #[test]
    fn test_parse_float_unix_timestamp() {
        let v = json!(1_700_000_000.5f64);
        let dt = TimestampProcessor::parse(&v).unwrap();
        assert_eq!(dt.year(), 2023);
    }

    #[test]
    fn test_parse_date_only_string() {
        let v = json!("2024-06-01");
        let dt = TimestampProcessor::parse(&v).unwrap();
        assert_eq!(dt.year(), 2024);
        assert_eq!(dt.month(), 6);
        assert_eq!(dt.day(), 1);
        assert_eq!(dt.hour(), 0);
    }

    #[test]
    fn test_parse_naive_datetime_no_tz() {
        let v = json!("2024-01-15 12:30:45");
        let dt = TimestampProcessor::parse(&v).unwrap();
        assert_eq!(dt.hour(), 12);
        assert_eq!(dt.minute(), 30);
    }

    #[test]
    fn test_parse_empty_string_returns_none() {
        assert!(TimestampProcessor::parse(&json!("")).is_none());
    }

    #[test]
    fn test_parse_garbage_string_returns_none() {
        assert!(TimestampProcessor::parse(&json!("not-a-timestamp")).is_none());
    }

    // ── TokenExtractor ───────────────────────────────────────────────────────

    #[test]
    fn test_extract_flat_snake_case() {
        let data = json!({
            "input_tokens": 100u64,
            "output_tokens": 50u64,
        });
        let t = TokenExtractor::extract(&data);
        assert_eq!(t.input_tokens, 100);
        assert_eq!(t.output_tokens, 50);
        assert_eq!(t.total_tokens, 150);
    }

    #[test]
    fn test_extract_usage_nesting() {
        let data = json!({
            "usage": {
                "input_tokens": 200u64,
                "output_tokens": 100u64,
            }
        });
        let t = TokenExtractor::extract(&data);
        assert_eq!(t.input_tokens, 200);
        assert_eq!(t.output_tokens, 100);
    }

    #[test]
    fn test_extract_assistant_message_usage_priority() {
        let data = json!({
            "type": "assistant",
            "message": {
                "usage": {
                    "input_tokens": 300u64,
                    "output_tokens": 150u64,
                }
            },
            "usage": {
                "input_tokens": 999u64,
                "output_tokens": 999u64,
            }
        });
        let t = TokenExtractor::extract(&data);
        // For assistant type, message.usage takes priority.
        assert_eq!(t.input_tokens, 300);
        assert_eq!(t.output_tokens, 150);
    }

    #[test]
    fn test_extract_camel_case_keys() {
        let data = json!({
            "inputTokens": 400u64,
            "outputTokens": 200u64,
            "cacheCreationInputTokens": 50u64,
            "cacheReadInputTokens": 25u64,
        });
        let t = TokenExtractor::extract(&data);
        assert_eq!(t.input_tokens, 400);
        assert_eq!(t.output_tokens, 200);
        assert_eq!(t.cache_creation_input_tokens, 50);
        assert_eq!(t.cache_read_input_tokens, 25);
        assert_eq!(t.total_tokens, 675);
    }

    #[test]
    fn test_extract_empty_returns_default() {
        let data = json!({});
        let t = TokenExtractor::extract(&data);
        assert_eq!(t, ExtractedTokens::default());
    }

    #[test]
    fn test_extract_cache_tokens_alternate_keys() {
        let data = json!({
            "input_tokens": 100u64,
            "output_tokens": 50u64,
            "cache_creation_input_tokens": 10u64,
            "cache_read_input_tokens": 5u64,
        });
        let t = TokenExtractor::extract(&data);
        assert_eq!(t.cache_creation_input_tokens, 10);
        assert_eq!(t.cache_read_input_tokens, 5);
    }

    // ── DataConverter::flatten_nested ────────────────────────────────────────

    #[test]
    fn test_flatten_simple_object() {
        let data = json!({"a": 1, "b": 2});
        let flat = DataConverter::flatten_nested(&data, "");
        assert_eq!(flat["a"], json!(1));
        assert_eq!(flat["b"], json!(2));
    }

    #[test]
    fn test_flatten_nested_object() {
        let data = json!({"outer": {"inner": 42}});
        let flat = DataConverter::flatten_nested(&data, "");
        assert_eq!(flat["outer.inner"], json!(42));
    }

    #[test]
    fn test_flatten_with_prefix() {
        let data = json!({"key": "value"});
        let flat = DataConverter::flatten_nested(&data, "root");
        assert_eq!(flat["root.key"], json!("value"));
    }

    #[test]
    fn test_flatten_array() {
        let data = json!({"items": [10, 20]});
        let flat = DataConverter::flatten_nested(&data, "");
        assert_eq!(flat["items.0"], json!(10));
        assert_eq!(flat["items.1"], json!(20));
    }

    #[test]
    fn test_flatten_deeply_nested() {
        let data = json!({"a": {"b": {"c": "deep"}}});
        let flat = DataConverter::flatten_nested(&data, "");
        assert_eq!(flat["a.b.c"], json!("deep"));
    }

    // ── DataConverter::extract_model_name ────────────────────────────────────

    #[test]
    fn test_extract_model_name_from_root() {
        let data = json!({"model": "claude-3-5-sonnet-20241022"});
        assert_eq!(
            DataConverter::extract_model_name(&data),
            "claude-3-5-sonnet-20241022"
        );
    }

    #[test]
    fn test_extract_model_name_from_message() {
        let data = json!({
            "message": {"model": "claude-3-haiku-20240307"}
        });
        assert_eq!(
            DataConverter::extract_model_name(&data),
            "claude-3-haiku-20240307"
        );
    }

    #[test]
    fn test_extract_model_name_fallback() {
        let data = json!({});
        assert_eq!(
            DataConverter::extract_model_name(&data),
            "claude-3-5-sonnet"
        );
    }

    #[test]
    fn test_extract_model_name_empty_string_falls_back() {
        let data = json!({"model": ""});
        assert_eq!(
            DataConverter::extract_model_name(&data),
            "claude-3-5-sonnet"
        );
    }
}

// Re-export chrono items used in tests for brevity.
#[allow(unused_imports)]
use chrono::{Datelike, Timelike};
