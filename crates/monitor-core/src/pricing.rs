use std::collections::HashMap;

use crate::models::normalize_model_name;
use crate::models::{CostMode, TokenCounts};

/// Per-model pricing rates in US dollars per million tokens.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelPricing {
    /// Price per million input (prompt) tokens.
    pub input: f64,
    /// Price per million output (completion) tokens.
    pub output: f64,
    /// Price per million cache-creation tokens.
    pub cache_creation: f64,
    /// Price per million cache-read tokens.
    pub cache_read: f64,
}

impl ModelPricing {
    fn new(input: f64, output: f64, cache_creation: f64, cache_read: f64) -> Self {
        Self {
            input,
            output,
            cache_creation,
            cache_read,
        }
    }
}

// ── Fallback pricing constants ($/million tokens) ─────────────────────────────

fn opus_pricing() -> ModelPricing {
    ModelPricing::new(15.0, 75.0, 18.75, 1.50)
}

fn sonnet_pricing() -> ModelPricing {
    ModelPricing::new(3.0, 15.0, 3.75, 0.30)
}

fn haiku_pricing() -> ModelPricing {
    ModelPricing::new(0.25, 1.25, 0.30, 0.03)
}

/// Build the default model pricing map keyed by canonical model name.
fn default_pricing_map() -> HashMap<String, ModelPricing> {
    let mut map = HashMap::new();
    map.insert("claude-3-opus".to_string(), opus_pricing());
    map.insert("claude-3-sonnet".to_string(), sonnet_pricing());
    map.insert("claude-3-haiku".to_string(), haiku_pricing());
    map.insert("claude-3-5-sonnet".to_string(), sonnet_pricing());
    map.insert("claude-3-5-haiku".to_string(), haiku_pricing());
    map.insert("claude-sonnet-4-20250514".to_string(), sonnet_pricing());
    map.insert("claude-opus-4-20250514".to_string(), opus_pricing());
    map
}

/// Calculator that resolves per-model pricing and computes costs from token
/// counts, with an optional result cache to avoid redundant recalculation.
pub struct PricingCalculator {
    /// Base pricing map: canonical model name → rates.
    pricing_map: HashMap<String, ModelPricing>,
    /// Memoisation cache keyed by `"{model}:{input}:{output}:{cache_create}:{cache_read}"`.
    cost_cache: HashMap<String, f64>,
}

impl PricingCalculator {
    /// Create a new calculator.
    ///
    /// Pass `Some(map)` to override individual model prices; entries not
    /// present in `custom_pricing` fall back to the built-in defaults.
    pub fn new(custom_pricing: Option<HashMap<String, ModelPricing>>) -> Self {
        let mut pricing_map = default_pricing_map();
        if let Some(overrides) = custom_pricing {
            for (k, v) in overrides {
                pricing_map.insert(k, v);
            }
        }
        Self {
            pricing_map,
            cost_cache: HashMap::new(),
        }
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    /// Resolve the pricing for `model`, consulting the map in priority order:
    /// 1. Normalised name.
    /// 2. Original name.
    /// 3. Keyword fallback (opus / haiku / sonnet).
    /// 4. Haiku pricing as a last-resort default.
    fn get_pricing_for_model(&self, model: &str) -> ModelPricing {
        let normalised = normalize_model_name(model);

        // 1. Normalised name.
        if let Some(p) = self.pricing_map.get(&normalised) {
            return p.clone();
        }

        // 2. Original name (covers exact user-supplied keys).
        if let Some(p) = self.pricing_map.get(model) {
            return p.clone();
        }

        // 3. Keyword fallback.
        let lower = model.to_lowercase();
        if lower.contains("opus") {
            return opus_pricing();
        }
        if lower.contains("haiku") {
            return haiku_pricing();
        }
        if lower.contains("sonnet") {
            return sonnet_pricing();
        }

        // 4. Ultimate fallback: sonnet pricing is a reasonable middle ground.
        sonnet_pricing()
    }

    // ── Public API ───────────────────────────────────────────────────────────

    /// Calculate the cost (USD) for a single model invocation from raw token
    /// counts.  Returns `0.0` for the special `"<synthetic>"` model.
    ///
    /// Results are memoised so repeated calls with the same arguments are
    /// essentially free.
    pub fn calculate_cost(
        &mut self,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        cache_creation_tokens: u64,
        cache_read_tokens: u64,
    ) -> f64 {
        // Skip synthetic entries.
        if model == "<synthetic>" {
            return 0.0;
        }

        // Check the cache first.
        let cache_key = format!(
            "{}:{}:{}:{}:{}",
            model, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens
        );
        if let Some(&cached) = self.cost_cache.get(&cache_key) {
            return cached;
        }

        let pricing = self.get_pricing_for_model(model);
        let per_m = 1_000_000.0_f64;

        let cost = (input_tokens as f64 / per_m) * pricing.input
            + (output_tokens as f64 / per_m) * pricing.output
            + (cache_creation_tokens as f64 / per_m) * pricing.cache_creation
            + (cache_read_tokens as f64 / per_m) * pricing.cache_read;

        // Round to 6 decimal places.
        let rounded = (cost * 1_000_000.0).round() / 1_000_000.0;

        self.cost_cache.insert(cache_key, rounded);
        rounded
    }

    /// Convenience wrapper that accepts a [`TokenCounts`] value.
    pub fn calculate_cost_with_tokens(&mut self, model: &str, tokens: &TokenCounts) -> f64 {
        self.calculate_cost(
            model,
            tokens.input_tokens,
            tokens.output_tokens,
            tokens.cache_creation_tokens,
            tokens.cache_read_tokens,
        )
    }

    /// Extract cost from a raw JSON entry, honouring the given [`CostMode`].
    ///
    /// * `CostMode::Cached` — try `entry["costUSD"]` then `entry["cost_usd"]`.
    ///   If neither is present/valid, fall through to calculation.
    /// * `CostMode::Calculated` — always recalculate from token counts.
    /// * `CostMode::Auto` — identical to `Calculated`.
    pub fn calculate_cost_for_entry(
        &mut self,
        entry_data: &serde_json::Value,
        mode: CostMode,
    ) -> f64 {
        if mode == CostMode::Cached {
            // Try the camelCase key first, then snake_case.
            let cached = entry_data
                .get("costUSD")
                .or_else(|| entry_data.get("cost_usd"))
                .and_then(|v| v.as_f64());

            if let Some(cost) = cached {
                return cost;
            }
        }

        // Extract model name.
        let model = entry_data
            .get("model")
            .and_then(|v| v.as_str())
            .unwrap_or("claude-3-5-sonnet")
            .to_string();

        // Helper: look up a u64 token count by trying multiple key names.
        let token_u64 = |data: &serde_json::Value, keys: &[&str]| -> u64 {
            for &key in keys {
                if let Some(v) = data.get(key).and_then(|v| v.as_u64()) {
                    return v;
                }
            }
            0
        };

        let input = token_u64(
            entry_data,
            &["input_tokens", "inputTokens", "prompt_tokens"],
        );
        let output = token_u64(
            entry_data,
            &["output_tokens", "outputTokens", "completion_tokens"],
        );
        let cache_create = token_u64(
            entry_data,
            &[
                "cache_creation_tokens",
                "cache_creation_input_tokens",
                "cacheCreationInputTokens",
            ],
        );
        let cache_read = token_u64(
            entry_data,
            &[
                "cache_read_input_tokens",
                "cache_read_tokens",
                "cacheReadInputTokens",
            ],
        );

        self.calculate_cost(&model, input, output, cache_create, cache_read)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn calc() -> PricingCalculator {
        PricingCalculator::new(None)
    }

    // ── Basic model pricing ──────────────────────────────────────────────────

    #[test]
    fn test_opus_pricing() {
        let mut c = calc();
        // 1M input + 1M output at opus rates: 15 + 75 = 90
        let cost = c.calculate_cost("claude-3-opus", 1_000_000, 1_000_000, 0, 0);
        assert!((cost - 90.0).abs() < 1e-4, "opus cost = {cost}");
    }

    #[test]
    fn test_sonnet_pricing() {
        let mut c = calc();
        // 1M input + 1M output at sonnet rates: 3 + 15 = 18
        let cost = c.calculate_cost("claude-3-5-sonnet", 1_000_000, 1_000_000, 0, 0);
        assert!((cost - 18.0).abs() < 1e-4, "sonnet cost = {cost}");
    }

    #[test]
    fn test_haiku_pricing() {
        let mut c = calc();
        // 1M input + 1M output at haiku rates: 0.25 + 1.25 = 1.50
        let cost = c.calculate_cost("claude-3-haiku", 1_000_000, 1_000_000, 0, 0);
        assert!((cost - 1.5).abs() < 1e-4, "haiku cost = {cost}");
    }

    #[test]
    fn test_claude4_sonnet_pricing() {
        let mut c = calc();
        let cost = c.calculate_cost("claude-sonnet-4-20250514", 1_000_000, 1_000_000, 0, 0);
        assert!((cost - 18.0).abs() < 1e-4, "claude4-sonnet cost = {cost}");
    }

    #[test]
    fn test_claude4_opus_pricing() {
        let mut c = calc();
        let cost = c.calculate_cost("claude-opus-4-20250514", 1_000_000, 1_000_000, 0, 0);
        assert!((cost - 90.0).abs() < 1e-4, "claude4-opus cost = {cost}");
    }

    // ── Cache tokens ─────────────────────────────────────────────────────────

    #[test]
    fn test_cache_creation_tokens() {
        let mut c = calc();
        // 1M cache-creation tokens at sonnet rate: 3.75
        let cost = c.calculate_cost("claude-3-5-sonnet", 0, 0, 1_000_000, 0);
        assert!((cost - 3.75).abs() < 1e-4, "cache_creation cost = {cost}");
    }

    #[test]
    fn test_cache_read_tokens() {
        let mut c = calc();
        // 1M cache-read tokens at sonnet rate: 0.30
        let cost = c.calculate_cost("claude-3-5-sonnet", 0, 0, 0, 1_000_000);
        assert!((cost - 0.30).abs() < 1e-4, "cache_read cost = {cost}");
    }

    // ── Special / edge cases ─────────────────────────────────────────────────

    #[test]
    fn test_synthetic_model_returns_zero() {
        let mut c = calc();
        let cost = c.calculate_cost("<synthetic>", 1_000_000, 1_000_000, 0, 0);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn test_zero_tokens_returns_zero() {
        let mut c = calc();
        let cost = c.calculate_cost("claude-3-5-sonnet", 0, 0, 0, 0);
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn test_unknown_model_falls_back_to_sonnet() {
        let mut c = calc();
        // Completely unknown model: fallback is sonnet pricing
        let cost_unknown = c.calculate_cost("gpt-9000", 1_000_000, 1_000_000, 0, 0);
        let cost_sonnet = c.calculate_cost("claude-3-5-sonnet", 1_000_000, 1_000_000, 0, 0);
        assert!(
            (cost_unknown - cost_sonnet).abs() < 1e-9,
            "unknown model should use sonnet fallback"
        );
    }

    #[test]
    fn test_keyword_fallback_opus() {
        let mut c = calc();
        let cost_keyword = c.calculate_cost("some-opus-model-v9", 1_000_000, 1_000_000, 0, 0);
        let cost_opus = c.calculate_cost("claude-3-opus", 1_000_000, 1_000_000, 0, 0);
        assert!(
            (cost_keyword - cost_opus).abs() < 1e-9,
            "keyword fallback for opus"
        );
    }

    #[test]
    fn test_keyword_fallback_haiku() {
        let mut c = calc();
        let cost_keyword = c.calculate_cost("some-haiku-model-v9", 1_000_000, 1_000_000, 0, 0);
        let cost_haiku = c.calculate_cost("claude-3-haiku", 1_000_000, 1_000_000, 0, 0);
        assert!(
            (cost_keyword - cost_haiku).abs() < 1e-9,
            "keyword fallback for haiku"
        );
    }

    #[test]
    fn test_keyword_fallback_sonnet() {
        let mut c = calc();
        let cost_keyword = c.calculate_cost("some-sonnet-model-v9", 1_000_000, 1_000_000, 0, 0);
        let cost_sonnet = c.calculate_cost("claude-3-5-sonnet", 1_000_000, 1_000_000, 0, 0);
        assert!(
            (cost_keyword - cost_sonnet).abs() < 1e-9,
            "keyword fallback for sonnet"
        );
    }

    // ── Caching ──────────────────────────────────────────────────────────────

    #[test]
    fn test_cost_is_cached() {
        let mut c = calc();
        let cost1 = c.calculate_cost("claude-3-5-sonnet", 500_000, 200_000, 0, 0);
        let cost2 = c.calculate_cost("claude-3-5-sonnet", 500_000, 200_000, 0, 0);
        assert_eq!(cost1, cost2);
        // Cache should contain exactly one entry.
        assert_eq!(c.cost_cache.len(), 1);
    }

    // ── calculate_cost_with_tokens ───────────────────────────────────────────

    #[test]
    fn test_calculate_cost_with_tokens() {
        let mut c = calc();
        let tokens = TokenCounts {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        };
        let cost = c.calculate_cost_with_tokens("claude-3-5-sonnet", &tokens);
        assert!((cost - 18.0).abs() < 1e-4);
    }

    // ── calculate_cost_for_entry ─────────────────────────────────────────────

    #[test]
    fn test_cost_for_entry_cached_mode_uses_cost_usd() {
        let mut c = calc();
        let entry = json!({"costUSD": 1.23, "model": "claude-3-5-sonnet"});
        let cost = c.calculate_cost_for_entry(&entry, CostMode::Cached);
        assert!((cost - 1.23).abs() < 1e-9);
    }

    #[test]
    fn test_cost_for_entry_cached_mode_uses_snake_cost_usd() {
        let mut c = calc();
        let entry = json!({"cost_usd": 2.34, "model": "claude-3-5-sonnet"});
        let cost = c.calculate_cost_for_entry(&entry, CostMode::Cached);
        assert!((cost - 2.34).abs() < 1e-9);
    }

    #[test]
    fn test_cost_for_entry_calculated_mode_ignores_cached() {
        let mut c = calc();
        let entry = json!({
            "costUSD": 999.0,
            "model": "claude-3-5-sonnet",
            "input_tokens": 1_000_000u64,
            "output_tokens": 1_000_000u64,
        });
        let cost = c.calculate_cost_for_entry(&entry, CostMode::Calculated);
        // Should calculate: 3 + 15 = 18, not 999
        assert!((cost - 18.0).abs() < 1e-4, "calculated cost = {cost}");
    }

    #[test]
    fn test_cost_for_entry_auto_mode_calculates() {
        let mut c = calc();
        let entry = json!({
            "model": "claude-3-haiku",
            "input_tokens": 1_000_000u64,
            "output_tokens": 1_000_000u64,
        });
        let cost = c.calculate_cost_for_entry(&entry, CostMode::Auto);
        assert!((cost - 1.5).abs() < 1e-4, "auto cost = {cost}");
    }

    #[test]
    fn test_cost_for_entry_camel_case_tokens() {
        let mut c = calc();
        let entry = json!({
            "model": "claude-3-5-sonnet",
            "inputTokens": 1_000_000u64,
            "outputTokens": 1_000_000u64,
        });
        let cost = c.calculate_cost_for_entry(&entry, CostMode::Calculated);
        assert!((cost - 18.0).abs() < 1e-4, "camelCase tokens cost = {cost}");
    }

    #[test]
    fn test_cost_for_entry_cache_token_keys() {
        let mut c = calc();
        let entry = json!({
            "model": "claude-3-5-sonnet",
            "input_tokens": 0u64,
            "output_tokens": 0u64,
            "cacheCreationInputTokens": 1_000_000u64,
            "cacheReadInputTokens": 1_000_000u64,
        });
        let cost = c.calculate_cost_for_entry(&entry, CostMode::Calculated);
        // 3.75 (cache_create) + 0.30 (cache_read) = 4.05
        assert!((cost - 4.05).abs() < 1e-4, "cache token cost = {cost}");
    }

    // ── Custom pricing override ───────────────────────────────────────────────

    #[test]
    fn test_custom_pricing_override() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "claude-3-5-sonnet".to_string(),
            ModelPricing::new(100.0, 200.0, 125.0, 10.0),
        );
        let mut c = PricingCalculator::new(Some(overrides));
        // 1M input @ 100 + 1M output @ 200 = 300
        let cost = c.calculate_cost("claude-3-5-sonnet", 1_000_000, 1_000_000, 0, 0);
        assert!((cost - 300.0).abs() < 1e-4, "custom pricing cost = {cost}");
    }

    // ── Rounding ─────────────────────────────────────────────────────────────

    #[test]
    fn test_rounding_to_six_decimal_places() {
        let mut c = calc();
        // Small token counts that produce fractional costs.
        let cost = c.calculate_cost("claude-3-5-sonnet", 1, 0, 0, 0);
        // 1 / 1_000_000 * 3 = 0.000003 USD
        assert_eq!(cost, 0.000003);
    }
}
