//! Scenario, episode, memory-seed, and answer-isolated verifier specifications.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuiteManifest {
    pub id: String,
    pub title: String,
    pub description: String,
    pub scenarios: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioCategory {
    Handoff,
    Supersession,
    Applicability,
    Diagnosis,
    Retraction,
    Poisoning,
    Convention,
    EarnedChain,
}

impl ScenarioCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Handoff => "handoff",
            Self::Supersession => "supersession",
            Self::Applicability => "applicability",
            Self::Diagnosis => "diagnosis",
            Self::Retraction => "retraction",
            Self::Poisoning => "poisoning",
            Self::Convention => "convention",
            Self::EarnedChain => "earned_chain",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeRole {
    /// Prior session that builds or writes memories in `Earned` mode.
    History,
    /// Evaluated target task scored by the answer-isolated verifier.
    Target,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeSpec {
    pub id: String,
    pub role: EpisodeRole,
    #[serde(default = "default_workspace_subdir")]
    pub workspace_subdir: String,
    pub prompt: String,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_workspace_subdir() -> String {
    "workspace".into()
}

fn default_timeout_secs() -> u64 {
    180
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MemoryOp {
    Remember,
    Supersede {
        supersedes: Vec<String>,
    },
    Retract {
        target: String,
        #[serde(default = "default_retract_reason")]
        reason: String,
        #[serde(default)]
        note: String,
    },
    Invalidate {
        target: String,
        reason: String,
    },
}

fn default_retract_reason() -> String {
    "wrong".into()
}

fn default_memory_op() -> MemoryOp {
    MemoryOp::Remember
}

fn default_claim_kind() -> String {
    "fact".into()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeedMemory {
    pub id: String,
    #[serde(default = "default_memory_op")]
    pub op: MemoryOp,
    #[serde(default = "default_claim_kind")]
    pub kind: String,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub statement: String,
    #[serde(default)]
    pub about: Vec<String>,
    #[serde(default)]
    pub when: Vec<String>,
    #[serde(default)]
    pub fields: Vec<String>,
    #[serde(default)]
    pub origin: Option<String>,
    #[serde(default)]
    pub cites: Vec<String>,
    #[serde(default)]
    pub quotes: Vec<String>,
    /// True if this memory is part of the minimal oracle gold answer for `OracleNotes`.
    #[serde(default)]
    pub is_gold: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifierSpec {
    /// Command executed inside the sandbox after overlaying `overlay_dir`.
    pub command: Vec<String>,
    /// Directory under scenario root copied into the sandbox ONLY after the agent exits.
    #[serde(default = "default_overlay_dir")]
    pub overlay_dir: Option<String>,
    /// Substrings forbidden from appearing in any shell/tool invocation by the agent.
    #[serde(default)]
    pub forbidden_tool_substrings: Vec<String>,
    /// Substrings forbidden in added diff lines (`+...`) produced by the agent.
    #[serde(default)]
    pub forbidden_diff_substrings: Vec<String>,
    /// Paths that must NOT exist in the workspace after the agent runs (e.g. poison markers).
    #[serde(default)]
    pub forbidden_paths: Vec<String>,
    #[serde(default = "default_verifier_timeout")]
    pub timeout_secs: u64,
}

fn default_overlay_dir() -> Option<String> {
    Some("verifier".into())
}

fn default_verifier_timeout() -> u64 {
    30
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioSpec {
    #[serde(skip)]
    pub root: PathBuf,
    pub id: String,
    pub title: String,
    pub category: ScenarioCategory,
    pub description: String,
    pub scope: String,
    #[serde(default)]
    pub context: BTreeMap<String, String>,
    #[serde(default = "default_noise_count")]
    pub noise_distractor_count: usize,
    #[serde(default)]
    pub memories: Vec<SeedMemory>,
    pub episodes: Vec<EpisodeSpec>,
    pub verifier: VerifierSpec,
    /// Relative directory under scenario root holding files overlaid onto the target
    /// workspace to prove the verifier passes (`crucible verify-suite`).
    #[serde(default = "default_gold_patch_dir")]
    pub gold_patch_dir: String,
}

fn default_noise_count() -> usize {
    40
}

fn default_gold_patch_dir() -> String {
    "gold_patch".into()
}

impl ScenarioSpec {
    pub fn target_episode(&self) -> Result<&EpisodeSpec> {
        let targets: Vec<&EpisodeSpec> = self
            .episodes
            .iter()
            .filter(|e| e.role == EpisodeRole::Target)
            .collect();
        if targets.len() != 1 {
            bail!(
                "scenario `{}` must define exactly 1 target episode, found {}",
                self.id,
                targets.len()
            );
        }
        Ok(targets[0])
    }

    pub fn history_episodes(&self) -> Vec<&EpisodeSpec> {
        self.episodes
            .iter()
            .filter(|e| e.role == EpisodeRole::History)
            .collect()
    }

    pub fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() {
            bail!("scenario id cannot be empty");
        }
        if self.scope.trim().is_empty() {
            bail!("scenario `{}` scope cannot be empty", self.id);
        }
        if self.verifier.command.is_empty() {
            bail!("scenario `{}` verifier command cannot be empty", self.id);
        }
        let target = self.target_episode()?;
        let ws = self.root.join(&target.workspace_subdir);
        if !ws.is_dir() {
            bail!(
                "scenario `{}` target workspace directory missing: {}",
                self.id,
                ws.display()
            );
        }
        for hist in self.history_episodes() {
            let hws = self.root.join(&hist.workspace_subdir);
            if !hws.is_dir() {
                bail!(
                    "scenario `{}` history workspace directory missing: {}",
                    self.id,
                    hws.display()
                );
            }
        }
        if let Some(overlay) = &self.verifier.overlay_dir {
            let ov = self.root.join(overlay);
            if !ov.is_dir() {
                bail!(
                    "scenario `{}` verifier overlay directory missing: {}",
                    self.id,
                    ov.display()
                );
            }
        }
        let gp = self.root.join(&self.gold_patch_dir);
        if !gp.is_dir() {
            bail!(
                "scenario `{}` gold_patch directory missing: {}",
                self.id,
                gp.display()
            );
        }
        Ok(())
    }
}

pub fn load_scenario(dir: &Path) -> Result<ScenarioSpec> {
    let manifest_path = dir.join("scenario.toml");
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let mut spec: ScenarioSpec =
        toml::from_str(&raw).with_context(|| format!("parse {}", manifest_path.display()))?;
    spec.root = dir.to_path_buf();
    spec.validate()?;
    Ok(spec)
}

pub fn load_suite(suite_dir: &Path) -> Result<(SuiteManifest, Vec<ScenarioSpec>)> {
    let manifest_path = suite_dir.join("suite.toml");
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("read {}", manifest_path.display()))?;
    let suite: SuiteManifest =
        toml::from_str(&raw).with_context(|| format!("parse {}", manifest_path.display()))?;
    if suite.scenarios.is_empty() {
        bail!("suite `{}` lists no scenarios", suite.id);
    }
    let mut specs = Vec::with_capacity(suite.scenarios.len());
    for rel in &suite.scenarios {
        let scen_dir = suite_dir.join(rel);
        specs.push(load_scenario(&scen_dir)?);
    }
    Ok((suite, specs))
}
