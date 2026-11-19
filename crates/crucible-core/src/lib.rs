//! Core types for `crucible`: scenario specifications, experimental arms,
//! cache-aware token/cost ledgers, and trial records.

pub mod arm;
pub mod ledger;
pub mod result;
pub mod spec;

pub use arm::{ArmKind, ArmSpec, MemoryMode};
pub use ledger::{ModelPricing, ToolCallSummary, UsageMetrics};
pub use result::{RunManifest, TrialRecord, VerificationOutcome};
pub use spec::{
    EpisodeRole, EpisodeSpec, MemoryOp, ScenarioCategory, ScenarioSpec, SeedMemory, SuiteManifest,
    VerifierSpec, load_scenario, load_suite,
};
