//! Provider-reported token, prompt-cache, AIU, tool-call, and USD cost accounting.

use serde::{Deserialize, Serialize};

/// Per-million-token pricing with separate cached vs uncached input rates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelPricing {
    pub model_pattern: String,
    pub uncached_input_per_mtok_usd: f64,
    pub cache_write_per_mtok_usd: f64,
    pub cache_read_per_mtok_usd: f64,
    pub output_per_mtok_usd: f64,
}

impl ModelPricing {
    /// Standard Anthropic Sonnet-tier list rates ($3.00 in, $3.75 cache write, $0.30 cache read, $15.00 out / MTok).
    pub fn sonnet_default() -> Self {
        Self {
            model_pattern: "claude-sonnet".into(),
            uncached_input_per_mtok_usd: 3.00,
            cache_write_per_mtok_usd: 3.75,
            cache_read_per_mtok_usd: 0.30,
            output_per_mtok_usd: 15.00,
        }
    }

    /// Standard Anthropic Haiku-tier list rates ($1.00 in, $1.25 cache write, $0.10 cache read, $5.00 out / MTok).
    pub fn haiku_default() -> Self {
        Self {
            model_pattern: "claude-haiku".into(),
            uncached_input_per_mtok_usd: 1.00,
            cache_write_per_mtok_usd: 1.25,
            cache_read_per_mtok_usd: 0.10,
            output_per_mtok_usd: 5.00,
        }
    }

    pub fn for_model(model: &str) -> Self {
        let lower = model.to_ascii_lowercase();
        if lower.contains("haiku") {
            Self::haiku_default()
        } else {
            Self::sonnet_default()
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallSummary {
    pub name: String,
    pub arguments_preview: String,
    pub is_mcp: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageMetrics {
    pub api_requests: u64,
    pub uncached_input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub total_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub nano_aiu: u64,
    pub api_duration_ms: u64,
    pub wall_duration_ms: u64,
    pub tool_calls: u64,
    pub mcp_tool_calls: u64,
    pub lines_added: u64,
    pub lines_removed: u64,
    pub files_modified: u64,
}

impl UsageMetrics {
    pub fn accumulate(&mut self, other: &Self) {
        self.api_requests += other.api_requests;
        self.uncached_input_tokens += other.uncached_input_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.total_input_tokens += other.total_input_tokens;
        self.output_tokens += other.output_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
        self.nano_aiu += other.nano_aiu;
        self.api_duration_ms += other.api_duration_ms;
        self.wall_duration_ms += other.wall_duration_ms;
        self.tool_calls += other.tool_calls;
        self.mcp_tool_calls += other.mcp_tool_calls;
        self.lines_added += other.lines_added;
        self.lines_removed += other.lines_removed;
        self.files_modified += other.files_modified;
    }

    pub fn aiu(&self) -> f64 {
        self.nano_aiu as f64 / 1_000_000_000.0
    }

    /// Cache-aware USD cost from exact uncached, cache-write, cache-read, and output tokens.
    pub fn cache_adjusted_usd(&self, pricing: &ModelPricing) -> f64 {
        let mtok = 1_000_000.0;
        (self.uncached_input_tokens as f64 / mtok) * pricing.uncached_input_per_mtok_usd
            + (self.cache_write_tokens as f64 / mtok) * pricing.cache_write_per_mtok_usd
            + (self.cache_read_tokens as f64 / mtok) * pricing.cache_read_per_mtok_usd
            + (self.output_tokens as f64 / mtok) * pricing.output_per_mtok_usd
    }

    /// Cache-neutral USD equivalent treating all `total_input_tokens` at a uniform cached/blended
    /// reference rate so warm-vs-cold prompt cache order cannot mask prompt bloat.
    pub fn uncached_equivalent_usd(&self, pricing: &ModelPricing) -> f64 {
        let mtok = 1_000_000.0;
        (self.total_input_tokens as f64 / mtok) * pricing.uncached_input_per_mtok_usd
            + (self.output_tokens as f64 / mtok) * pricing.output_per_mtok_usd
    }
}
