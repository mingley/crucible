//! `crucible` — Hermetic evaluation harness for coding agents and mutable memory systems.
//!
//! Provides git-isolated scenario sandboxes, answer-isolated post-episode verification,
//! multi-arm memory lifecycle seeding (`B0`, `B1-notes`, `B1-grep`, `Hivemind`, `Oracle`),
//! hermetic agent drivers (`CopilotDriver`, `CommandDriver`, `ScriptedDriver`), and a
//! Latin-square counterbalanced benchmark runner with resumable JSONL checkpoints.

pub mod driver;
pub mod memory;
pub mod runner;
pub mod sandbox;

use std::path::Path;

use anyhow::{Result, bail};
use crucible_core::{ArmSpec, MemoryMode};

pub use driver::{
    AgentDriver, CommandDriver, CopilotDriver, CopilotJsonlSummary, EpisodeRunOutput,
    EpisodeRunRequest, ScriptedDriver, parse_copilot_jsonl, parse_copilot_usage_json,
};
pub use memory::{
    EpisodeMemoryContext, HivemindStoreGuard, PreparedTrialMemory, generate_noise_memories,
    prepare_trial_memory, seed_hivemind_store, shutdown_hivemind_daemon, write_hivemind_mcp_config,
};
pub use runner::{
    ArmsConfigFile, RunConfig, RunOutcome, TrialProgressEvent, run_benchmark, utc_now_iso8601,
};
pub use sandbox::{
    CommandRunResult, DiffStats, Sandbox, copy_dir_all, run_command_with_timeout,
    verify_scenario_integrity,
};

/// Resolve a shorthand arm identifier (`b0`, `b1-notes`, `b1-grep`, `hivemind`, `hivemind-earned`,
/// `oracle`) into a concrete [`ArmSpec`].
pub fn resolve_arm_spec(
    raw_id: &str,
    hivemind_bin: &Path,
    default_memory_mode: MemoryMode,
) -> Result<ArmSpec> {
    let normalized = raw_id.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "b0" | "no-memory" | "no_memory" => Ok(ArmSpec::b0()),
        "b1-notes" | "static-notes" | "agents-md" => Ok(ArmSpec::b1_notes()),
        "b1-grep" | "markdown-tree" => Ok(ArmSpec::b1_grep()),
        "hivemind" => Ok(ArmSpec::hivemind(
            hivemind_bin.to_path_buf(),
            default_memory_mode,
        )),
        "hivemind-given" => Ok(ArmSpec::hivemind(
            hivemind_bin.to_path_buf(),
            MemoryMode::Given,
        )),
        "hivemind-earned" => Ok(ArmSpec::hivemind(
            hivemind_bin.to_path_buf(),
            MemoryMode::Earned,
        )),
        "oracle" => Ok(ArmSpec::oracle()),
        other => bail!(
            "unknown arm `{other}` (expected one of: b0, b1-notes, b1-grep, hivemind, hivemind-given, hivemind-earned, oracle)"
        ),
    }
}
