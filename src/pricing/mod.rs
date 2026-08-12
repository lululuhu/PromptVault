//! LLM model pricing (USD per 1M tokens) and cost estimation.
//!
//! Prices are public catalog values as of 2025-2026. They are best-effort;
//! users should always verify against the provider's own pricing page before
//! relying on the estimate for billing decisions.

use serde::Serialize;

/// Per-token pricing for a single model, in USD per 1M tokens.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ModelPricing {
    /// Catalog id, e.g. `"gpt-4o"`.
    pub id: &'static str,
    /// Vendor: `openai` | `anthropic` | `ollama` | `meta`.
    pub vendor: &'static str,
    /// USD per 1M input (prompt) tokens.
    pub input_per_1m: f64,
    /// USD per 1M output (completion) tokens.
    pub output_per_1m: f64,
    /// Max context window in tokens.
    pub context_window: usize,
}

impl ModelPricing {
    pub fn cost(&self, input_tokens: usize, output_tokens: usize) -> (f64, f64) {
        let in_cost = (input_tokens as f64 / 1_000_000.0) * self.input_per_1m;
        let out_cost = (output_tokens as f64 / 1_000_000.0) * self.output_per_1m;
        (in_cost, out_cost)
    }
}

/// All known models. Sorted for stable display.
pub fn all_models() -> &'static [ModelPricing] {
    &MODELS
}

pub fn find(id: &str) -> Option<&'static ModelPricing> {
    MODELS.iter().find(|m| m.id.eq_ignore_ascii_case(id))
}

/// Estimated cost for a single (input, output) call against `model_id`.
/// Returns `None` if the model is unknown.
pub fn estimate(model_id: &str, input_tokens: usize, output_tokens: usize) -> Option<CostEstimate> {
    let m = find(model_id)?;
    let (in_cost, out_cost) = m.cost(input_tokens, output_tokens);
    Some(CostEstimate {
        model_id: m.id,
        input_tokens,
        output_tokens,
        input_cost_usd: in_cost,
        output_cost_usd: out_cost,
        total_cost_usd: in_cost + out_cost,
        context_window: m.context_window,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct CostEstimate {
    pub model_id: &'static str,
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub input_cost_usd: f64,
    pub output_cost_usd: f64,
    pub total_cost_usd: f64,
    pub context_window: usize,
}

static MODELS: &[ModelPricing] = &[
    // --- OpenAI ---
    ModelPricing { id: "gpt-4o",              vendor: "openai",    input_per_1m: 2.50,  output_per_1m: 10.00, context_window: 128_000 },
    ModelPricing { id: "gpt-4o-mini",          vendor: "openai",    input_per_1m: 0.15,  output_per_1m: 0.60,  context_window: 128_000 },
    ModelPricing { id: "gpt-4-turbo",          vendor: "openai",    input_per_1m: 10.00, output_per_1m: 30.00, context_window: 128_000 },
    ModelPricing { id: "gpt-4",                vendor: "openai",    input_per_1m: 30.00, output_per_1m: 60.00, context_window: 8_192 },
    ModelPricing { id: "gpt-3.5-turbo",        vendor: "openai",    input_per_1m: 0.50,  output_per_1m: 1.50,  context_window: 16_385 },
    ModelPricing { id: "o1",                   vendor: "openai",    input_per_1m: 15.00, output_per_1m: 60.00, context_window: 200_000 },
    ModelPricing { id: "o1-mini",               vendor: "openai",    input_per_1m: 3.00,  output_per_1m: 12.00, context_window: 128_000 },
    ModelPricing { id: "o1-preview",           vendor: "openai",    input_per_1m: 15.00, output_per_1m: 60.00, context_window: 128_000 },

    // --- Anthropic ---
    ModelPricing { id: "claude-3-5-sonnet-latest",  vendor: "anthropic", input_per_1m: 3.00,  output_per_1m: 15.00, context_window: 200_000 },
    ModelPricing { id: "claude-3-5-sonnet",         vendor: "anthropic", input_per_1m: 3.00,  output_per_1m: 15.00, context_window: 200_000 },
    ModelPricing { id: "claude-3-5-haiku-latest",   vendor: "anthropic", input_per_1m: 0.80,  output_per_1m: 4.00,  context_window: 200_000 },
    ModelPricing { id: "claude-3-opus",              vendor: "anthropic", input_per_1m: 15.00, output_per_1m: 75.00, context_window: 200_000 },
    ModelPricing { id: "claude-3-sonnet",            vendor: "anthropic", input_per_1m: 3.00,  output_per_1m: 15.00, context_window: 200_000 },
    ModelPricing { id: "claude-3-haiku",              vendor: "anthropic", input_per_1m: 0.25,  output_per_1m: 1.25,  context_window: 200_000 },

    // --- Meta (Ollama) --- local models; assume $0 cost, show context only.
    ModelPricing { id: "llama3.2",            vendor: "ollama",    input_per_1m: 0.0,   output_per_1m: 0.0,   context_window: 128_000 },
    ModelPricing { id: "llama3.1",            vendor: "ollama",    input_per_1m: 0.0,   output_per_1m: 0.0,   context_window: 128_000 },
    ModelPricing { id: "qwen2.5",             vendor: "ollama",    input_per_1m: 0.0,   output_per_1m: 0.0,   context_window: 32_000 },
];

/// Pretty-print a USD cost. Tiny numbers use more decimals; bigger ones fewer.
pub fn fmt_usd(v: f64) -> String {
    if v == 0.0 {
        "free".to_string()
    } else if v < 0.01 {
        format!("${v:.6}")
    } else if v < 1.0 {
        format!("${v:.4}")
    } else {
        format!("${v:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_known_model_case_insensitive() {
        assert!(find("GPT-4o").is_some());
        assert!(find("claude-3-5-sonnet-latest").is_some());
    }

    #[test]
    fn unknown_model_returns_none() {
        assert!(find("nope-9000").is_none());
    }

    #[test]
    fn cost_is_linear() {
        let e = estimate("gpt-4o-mini", 1_000_000, 1_000_000).unwrap();
        assert!((e.input_cost_usd - 0.15).abs() < 1e-9);
        assert!((e.output_cost_usd - 0.60).abs() < 1e-9);
        assert!((e.total_cost_usd - 0.75).abs() < 1e-9);
    }

    #[test]
    fn local_model_is_free() {
        let e = estimate("llama3.2", 1_000_000, 1_000_000).unwrap();
        assert_eq!(e.total_cost_usd, 0.0);
    }
}
