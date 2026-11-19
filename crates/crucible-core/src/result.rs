//! Trial outcomes and persisted run manifests.

use std::fs;
use std::path::Path;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::ledger::{ModelPricing, ToolCallSummary, UsageMetrics};
use crate::spec::ScenarioCategory;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationOutcome {
    pub passed: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    #[serde(default)]
    pub violations: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrialRecord {
    pub run_id: String,
    pub suite_id: String,
    pub scenario_id: String,
    pub category: ScenarioCategory,
    pub arm_id: String,
    pub seed: u32,
    /// Execution index within the `(scenario, seed)` Latin-square block.
    pub order_index: usize,
    pub passed: bool,
    pub verification: VerificationOutcome,
    pub target_usage: UsageMetrics,
    pub history_usage: UsageMetrics,
    pub total_usage: UsageMetrics,
    #[serde(default)]
    pub tool_trace: Vec<ToolCallSummary>,
    pub agent_exit_code: i32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RunManifest {
    pub run_id: String,
    pub created_at_utc: String,
    pub suite_id: String,
    pub driver: String,
    pub model: String,
    pub reasoning_effort: String,
    pub seeds: Vec<u32>,
    pub arms: Vec<String>,
    pub pricing: ModelPricing,
    pub trials: Vec<TrialRecord>,
}

impl RunManifest {
    pub fn save_json(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(self)?;
        fs::write(path, format!("{raw}\n")).with_context(|| format!("write {}", path.display()))
    }

    pub fn load_json(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))
    }
}
