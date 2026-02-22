use crate::error::{MonitorError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

/// Available Claude subscription plan types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanType {
    /// Claude Pro plan (~$20/month).
    Pro,
    /// Claude Max plan at the $50/month tier (formerly "Max 5x").
    Max5,
    /// Claude Max plan at the $200/month tier (formerly "Max 20x").
    Max20,
    /// User-defined custom plan (limits computed at runtime).
    Custom,
}

impl FromStr for PlanType {
    type Err = MonitorError;

    /// Case-insensitive construction from a string slice.
    ///
    /// Accepts `"pro"`, `"max5"`, `"max20"`, and `"custom"` (case-insensitive).
    /// Returns [`MonitorError::InvalidPlan`] for unrecognised strings.
    fn from_str(value: &str) -> Result<Self> {
        match value.to_lowercase().as_str() {
            "pro" => Ok(PlanType::Pro),
            "max5" => Ok(PlanType::Max5),
            "max20" => Ok(PlanType::Max20),
            "custom" => Ok(PlanType::Custom),
            other => Err(MonitorError::InvalidPlan(other.to_string())),
        }
    }
}

impl PlanType {
    /// The canonical lowercase string identifier for this plan.
    pub fn as_str(&self) -> &'static str {
        match self {
            PlanType::Pro => "pro",
            PlanType::Max5 => "max5",
            PlanType::Max20 => "max20",
            PlanType::Custom => "custom",
        }
    }
}

/// Immutable configuration record for a single Claude subscription plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanConfig {
    /// Canonical lowercase name that matches [`PlanType::as_str`].
    pub name: String,
    /// Maximum number of tokens per 5-hour session window.
    pub token_limit: u64,
    /// Maximum monetary cost (USD) per 5-hour session window.
    pub cost_limit: f64,
    /// Maximum number of user messages per 5-hour session window.
    pub message_limit: u32,
    /// Human-readable plan name for display purposes.
    pub display_name: String,
}

impl PlanConfig {
    /// Compact human-readable token limit string (e.g. `"19k"`, `"220k"`).
    pub fn formatted_token_limit(&self) -> String {
        if self.token_limit >= 1_000 {
            format!("{}k", self.token_limit / 1_000)
        } else {
            self.token_limit.to_string()
        }
    }
}

// ── Shared constants ──────────────────────────────────────────────────────────

/// Fallback token limit used when no plan is recognised (mirrors Pro limit).
pub const DEFAULT_TOKEN_LIMIT: u64 = 19_000;

/// Fallback cost limit used when no plan is recognised (mirrors Custom limit).
pub const DEFAULT_COST_LIMIT: f64 = 50.0;

/// Fallback message limit used when no plan is recognised (mirrors Pro limit).
pub const DEFAULT_MESSAGE_LIMIT: u32 = 250;

/// Well-known token limits, in ascending order, used for auto-detection.
pub const COMMON_TOKEN_LIMITS: &[u64] = &[19_000, 88_000, 220_000, 880_000];

/// Fraction of a limit at which the monitor considers it "reached".
pub const LIMIT_DETECTION_THRESHOLD: f64 = 0.95;

// ── Plan data ─────────────────────────────────────────────────────────────────

fn plan_configs() -> HashMap<PlanType, PlanConfig> {
    let mut map = HashMap::new();
    map.insert(
        PlanType::Pro,
        PlanConfig {
            name: "pro".to_string(),
            token_limit: 19_000,
            cost_limit: 18.0,
            message_limit: 250,
            display_name: "Pro".to_string(),
        },
    );
    map.insert(
        PlanType::Max5,
        PlanConfig {
            name: "max5".to_string(),
            token_limit: 88_000,
            cost_limit: 35.0,
            message_limit: 1_000,
            display_name: "Max5".to_string(),
        },
    );
    map.insert(
        PlanType::Max20,
        PlanConfig {
            name: "max20".to_string(),
            token_limit: 220_000,
            cost_limit: 140.0,
            message_limit: 2_000,
            display_name: "Max20".to_string(),
        },
    );
    map.insert(
        PlanType::Custom,
        PlanConfig {
            name: "custom".to_string(),
            token_limit: 44_000,
            cost_limit: 50.0,
            message_limit: 250,
            display_name: "Custom".to_string(),
        },
    );
    map
}

/// Registry of all plan configurations with static helper methods.
pub struct Plans;

impl Plans {
    /// The default token limit (Pro plan value).
    pub const DEFAULT_TOKEN_LIMIT: u64 = DEFAULT_TOKEN_LIMIT;
    /// The default cost limit (Custom plan value).
    pub const DEFAULT_COST_LIMIT: f64 = DEFAULT_COST_LIMIT;
    /// The default message limit (Pro plan value).
    pub const DEFAULT_MESSAGE_LIMIT: u32 = DEFAULT_MESSAGE_LIMIT;
    /// Well-known token limit steps for limit auto-detection.
    pub const COMMON_TOKEN_LIMITS: &'static [u64] = COMMON_TOKEN_LIMITS;
    /// Fraction of limit at which a session is considered "at limit".
    pub const LIMIT_DETECTION_THRESHOLD: f64 = LIMIT_DETECTION_THRESHOLD;

    /// Return all plan configurations keyed by [`PlanType`].
    pub fn all_plans() -> HashMap<PlanType, PlanConfig> {
        plan_configs()
    }

    /// Return the configuration for a specific [`PlanType`].
    pub fn get_plan(plan_type: PlanType) -> PlanConfig {
        plan_configs()
            .remove(&plan_type)
            .expect("all PlanType variants are present in plan_configs")
    }

    /// Return the configuration for a plan identified by its string name.
    ///
    /// Returns `None` if the name is not recognised.
    pub fn get_plan_by_name(name: &str) -> Option<PlanConfig> {
        let pt = name.parse::<PlanType>().ok()?;
        Some(Self::get_plan(pt))
    }

    /// Token limit for the named plan, or [`DEFAULT_TOKEN_LIMIT`] if unknown.
    pub fn get_token_limit(plan: &str) -> u64 {
        Self::get_plan_by_name(plan)
            .map(|c| c.token_limit)
            .unwrap_or(DEFAULT_TOKEN_LIMIT)
    }

    /// Cost limit for the named plan, or [`DEFAULT_COST_LIMIT`] if unknown.
    pub fn get_cost_limit(plan: &str) -> f64 {
        Self::get_plan_by_name(plan)
            .map(|c| c.cost_limit)
            .unwrap_or(DEFAULT_COST_LIMIT)
    }

    /// Message limit for the named plan, or [`DEFAULT_MESSAGE_LIMIT`] if unknown.
    pub fn get_message_limit(plan: &str) -> u32 {
        Self::get_plan_by_name(plan)
            .map(|c| c.message_limit)
            .unwrap_or(DEFAULT_MESSAGE_LIMIT)
    }

    /// Returns `true` if `plan` is a recognised plan name.
    pub fn is_valid_plan(plan: &str) -> bool {
        Self::get_plan_by_name(plan).is_some()
    }
}

// ── Module-level free functions (mirror Python module-level helpers) ───────────

/// Token limit for the named plan, or [`DEFAULT_TOKEN_LIMIT`] if unknown.
///
/// Wraps [`Plans::get_token_limit`] as a free function for ergonomic use.
pub fn get_token_limit(plan: &str) -> u64 {
    Plans::get_token_limit(plan)
}

/// Cost limit for the named plan, or [`DEFAULT_COST_LIMIT`] if unknown.
///
/// Wraps [`Plans::get_cost_limit`] as a free function for ergonomic use.
pub fn get_cost_limit(plan: &str) -> f64 {
    Plans::get_cost_limit(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── PlanType::from_str (via std::str::FromStr) ─────────────────────────

    #[test]
    fn test_plan_type_from_str_all_valid() {
        assert_eq!("pro".parse::<PlanType>().unwrap(), PlanType::Pro);
        assert_eq!("PRO".parse::<PlanType>().unwrap(), PlanType::Pro);
        assert_eq!("Pro".parse::<PlanType>().unwrap(), PlanType::Pro);

        assert_eq!("max5".parse::<PlanType>().unwrap(), PlanType::Max5);
        assert_eq!("MAX5".parse::<PlanType>().unwrap(), PlanType::Max5);

        assert_eq!("max20".parse::<PlanType>().unwrap(), PlanType::Max20);
        assert_eq!("MAX20".parse::<PlanType>().unwrap(), PlanType::Max20);

        assert_eq!("custom".parse::<PlanType>().unwrap(), PlanType::Custom);
        assert_eq!("CUSTOM".parse::<PlanType>().unwrap(), PlanType::Custom);
    }

    #[test]
    fn test_plan_type_from_str_invalid() {
        let err = "enterprise".parse::<PlanType>().unwrap_err();
        assert!(matches!(err, MonitorError::InvalidPlan(_)));
        assert!(err.to_string().contains("enterprise"));
    }

    #[test]
    fn test_plan_type_from_str_empty() {
        let err = "".parse::<PlanType>().unwrap_err();
        assert!(matches!(err, MonitorError::InvalidPlan(_)));
    }

    // ── Plans::get_plan ────────────────────────────────────────────────────

    #[test]
    fn test_get_plan_pro() {
        let cfg = Plans::get_plan(PlanType::Pro);
        assert_eq!(cfg.name, "pro");
        assert_eq!(cfg.token_limit, 19_000);
        assert!((cfg.cost_limit - 18.0).abs() < f64::EPSILON);
        assert_eq!(cfg.message_limit, 250);
        assert_eq!(cfg.display_name, "Pro");
    }

    #[test]
    fn test_get_plan_max5() {
        let cfg = Plans::get_plan(PlanType::Max5);
        assert_eq!(cfg.name, "max5");
        assert_eq!(cfg.token_limit, 88_000);
        assert!((cfg.cost_limit - 35.0).abs() < f64::EPSILON);
        assert_eq!(cfg.message_limit, 1_000);
        assert_eq!(cfg.display_name, "Max5");
    }

    #[test]
    fn test_get_plan_max20() {
        let cfg = Plans::get_plan(PlanType::Max20);
        assert_eq!(cfg.name, "max20");
        assert_eq!(cfg.token_limit, 220_000);
        assert!((cfg.cost_limit - 140.0).abs() < f64::EPSILON);
        assert_eq!(cfg.message_limit, 2_000);
        assert_eq!(cfg.display_name, "Max20");
    }

    #[test]
    fn test_get_plan_custom() {
        let cfg = Plans::get_plan(PlanType::Custom);
        assert_eq!(cfg.name, "custom");
        assert_eq!(cfg.token_limit, 44_000);
        assert!((cfg.cost_limit - 50.0).abs() < f64::EPSILON);
        assert_eq!(cfg.message_limit, 250);
        assert_eq!(cfg.display_name, "Custom");
    }

    // ── get_token_limit ────────────────────────────────────────────────────

    #[test]
    fn test_get_token_limit_all_plans() {
        assert_eq!(get_token_limit("pro"), 19_000);
        assert_eq!(get_token_limit("max5"), 88_000);
        assert_eq!(get_token_limit("max20"), 220_000);
        assert_eq!(get_token_limit("custom"), 44_000);
    }

    #[test]
    fn test_get_token_limit_unknown_returns_default() {
        assert_eq!(get_token_limit("unknown"), DEFAULT_TOKEN_LIMIT);
    }

    // ── get_cost_limit ─────────────────────────────────────────────────────

    #[test]
    fn test_get_cost_limit_all_plans() {
        assert!((get_cost_limit("pro") - 18.0).abs() < f64::EPSILON);
        assert!((get_cost_limit("max5") - 35.0).abs() < f64::EPSILON);
        assert!((get_cost_limit("max20") - 140.0).abs() < f64::EPSILON);
        assert!((get_cost_limit("custom") - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_get_cost_limit_unknown_returns_default() {
        assert!((get_cost_limit("unknown") - DEFAULT_COST_LIMIT).abs() < f64::EPSILON);
    }

    // ── is_valid_plan ──────────────────────────────────────────────────────

    #[test]
    fn test_is_valid_plan() {
        assert!(Plans::is_valid_plan("pro"));
        assert!(Plans::is_valid_plan("PRO"));
        assert!(Plans::is_valid_plan("max5"));
        assert!(Plans::is_valid_plan("max20"));
        assert!(Plans::is_valid_plan("custom"));
        assert!(!Plans::is_valid_plan("enterprise"));
        assert!(!Plans::is_valid_plan(""));
    }

    // ── formatted_token_limit ──────────────────────────────────────────────

    #[test]
    fn test_formatted_token_limit_pro() {
        let cfg = Plans::get_plan(PlanType::Pro);
        assert_eq!(cfg.formatted_token_limit(), "19k");
    }

    #[test]
    fn test_formatted_token_limit_max5() {
        let cfg = Plans::get_plan(PlanType::Max5);
        assert_eq!(cfg.formatted_token_limit(), "88k");
    }

    #[test]
    fn test_formatted_token_limit_max20() {
        let cfg = Plans::get_plan(PlanType::Max20);
        assert_eq!(cfg.formatted_token_limit(), "220k");
    }

    #[test]
    fn test_formatted_token_limit_custom() {
        let cfg = Plans::get_plan(PlanType::Custom);
        assert_eq!(cfg.formatted_token_limit(), "44k");
    }

    #[test]
    fn test_formatted_token_limit_small() {
        // Edge case: token_limit < 1000 should give raw number string.
        let cfg = PlanConfig {
            name: "tiny".to_string(),
            token_limit: 500,
            cost_limit: 1.0,
            message_limit: 10,
            display_name: "Tiny".to_string(),
        };
        assert_eq!(cfg.formatted_token_limit(), "500");
    }

    // ── all_plans ──────────────────────────────────────────────────────────

    #[test]
    fn test_all_plans_contains_all_variants() {
        let all = Plans::all_plans();
        assert!(all.contains_key(&PlanType::Pro));
        assert!(all.contains_key(&PlanType::Max5));
        assert!(all.contains_key(&PlanType::Max20));
        assert!(all.contains_key(&PlanType::Custom));
        assert_eq!(all.len(), 4);
    }

    // ── get_plan_by_name ───────────────────────────────────────────────────

    #[test]
    fn test_get_plan_by_name_valid() {
        let cfg = Plans::get_plan_by_name("max20").unwrap();
        assert_eq!(cfg.name, "max20");
    }

    #[test]
    fn test_get_plan_by_name_invalid() {
        assert!(Plans::get_plan_by_name("nonsense").is_none());
    }

    // ── Plans::get_message_limit ───────────────────────────────────────────

    #[test]
    fn test_get_message_limit_all_plans() {
        assert_eq!(Plans::get_message_limit("pro"), 250);
        assert_eq!(Plans::get_message_limit("max5"), 1_000);
        assert_eq!(Plans::get_message_limit("max20"), 2_000);
        assert_eq!(Plans::get_message_limit("custom"), 250);
    }

    #[test]
    fn test_get_message_limit_unknown_returns_default() {
        assert_eq!(Plans::get_message_limit("ghost"), DEFAULT_MESSAGE_LIMIT);
    }

    // ── Constants ─────────────────────────────────────────────────────────

    #[test]
    fn test_constants() {
        assert_eq!(DEFAULT_TOKEN_LIMIT, 19_000);
        assert!((DEFAULT_COST_LIMIT - 50.0).abs() < f64::EPSILON);
        assert_eq!(DEFAULT_MESSAGE_LIMIT, 250);
        assert_eq!(COMMON_TOKEN_LIMITS, &[19_000u64, 88_000, 220_000, 880_000]);
        assert!((LIMIT_DETECTION_THRESHOLD - 0.95).abs() < f64::EPSILON);
    }
}
