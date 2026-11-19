//! `crucible` CLI binary.

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use crucible::{
    AgentDriver, ArmsConfigFile, CommandDriver, CopilotDriver, RunConfig, ScriptedDriver,
    prepare_trial_memory, resolve_arm_spec, run_benchmark, verify_scenario_integrity,
};
use crucible_core::{ArmSpec, MemoryMode, RunManifest, load_suite};

#[derive(Debug, Parser)]
#[command(
    name = "crucible",
    version,
    about = "Hermetic benchmark harness for coding agents and mutable memory systems"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// List scenarios, categories, episode counts, and seed memory counts in a suite.
    List {
        /// Path to suite directory containing `suite.toml`.
        suite_dir: PathBuf,
    },
    /// Validate every scenario TOML and verify unmodified fails & `gold_patch` passes.
    VerifySuite {
        /// Path to suite directory containing `suite.toml`.
        suite_dir: PathBuf,
        /// Optional `hivemind` binary to also dry-run seeding every scenario's memories.
        #[arg(long, env = "CRUCIBLE_HIVEMIND_BIN")]
        hivemind_bin: Option<PathBuf>,
    },
    /// Execute a Latin-square counterbalanced benchmark run across scenarios, arms, and seeds.
    Run(Box<RunArgs>),
    /// Recompute statistical comparisons and render Markdown or JSON from a saved `manifest.json`.
    Report {
        /// Path to a saved `manifest.json`.
        manifest_path: PathBuf,
        /// Baseline arm ID for paired delta and McNemar comparisons (defaults to `b0` when present).
        #[arg(long)]
        baseline: Option<String>,
        /// Number of paired cluster bootstrap resamples for 95% confidence intervals.
        #[arg(long, default_value_t = 10_000)]
        resamples: usize,
        /// RNG seed for deterministic bootstrap resampling.
        #[arg(long, default_value_t = 42)]
        rng_seed: u64,
        /// Emit JSON report instead of Markdown.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum CliMemoryMode {
    Given,
    Earned,
}

impl From<CliMemoryMode> for MemoryMode {
    fn from(val: CliMemoryMode) -> Self {
        match val {
            CliMemoryMode::Given => Self::Given,
            CliMemoryMode::Earned => Self::Earned,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum CliDriverKind {
    Copilot,
    Scripted,
    Command,
}

#[derive(Debug, Parser)]
struct RunArgs {
    /// Path to suite directory containing `suite.toml`.
    suite_dir: PathBuf,

    /// Output directory for `trials.jsonl`, `manifest.json`, `report.md`, and `report.json`.
    #[arg(long)]
    out: PathBuf,

    /// Comma-separated list of experimental arms to evaluate.
    #[arg(long, value_delimiter = ',', default_value = "b0,b1-notes,hivemind")]
    arms: Vec<String>,

    /// Optional TOML file defining custom `ArmSpec` entries (overrides `--arms` when set).
    #[arg(long)]
    arms_file: Option<PathBuf>,

    /// Path or binary name for the `hivemind` CLI/MCP executable.
    #[arg(long, env = "CRUCIBLE_HIVEMIND_BIN", default_value = "hivemind")]
    hivemind_bin: PathBuf,

    /// Path or binary name for the `copilot` CLI executable when `--driver copilot`.
    #[arg(long, env = "CRUCIBLE_COPILOT_BIN", default_value = "copilot")]
    copilot_bin: PathBuf,

    /// External executable path when `--driver command`.
    #[arg(long, env = "CRUCIBLE_COMMAND_BIN")]
    command_bin: Option<PathBuf>,

    /// Memory seeding mode (`given` replays structured seed memories; `earned` runs History episodes).
    #[arg(long, value_enum, default_value_t = CliMemoryMode::Given)]
    memory_mode: CliMemoryMode,

    /// Agent execution driver (`copilot`, `scripted`, or `command`).
    #[arg(long, value_enum, default_value_t = CliDriverKind::Copilot)]
    driver: CliDriverKind,

    /// Model identifier passed to the agent driver.
    #[arg(long, default_value = "claude-sonnet-5.5")]
    model: String,

    /// Reasoning effort passed to the agent driver.
    #[arg(long, default_value = "medium")]
    effort: String,

    /// Number of seeds (`0..seeds`) to execute per `(scenario, arm)`.
    #[arg(long, default_value_t = 3)]
    seeds: u32,

    /// Optional subset of scenario IDs to run (repeatable or comma-separated).
    #[arg(long = "scenario", value_delimiter = ',')]
    scenarios: Vec<String>,

    /// Baseline arm ID for statistical comparisons (defaults to `b0` when present).
    #[arg(long)]
    baseline: Option<String>,

    /// Number of paired bootstrap resamples for statistical confidence intervals.
    #[arg(long, default_value_t = 10_000)]
    resamples: usize,

    /// Resume from an existing `<out>/trials.jsonl` checkpoint without re-running completed trials.
    #[arg(long)]
    resume: bool,

    /// Number of independent `(scenario, seed)` blocks to execute concurrently.
    #[arg(long, short = 'j', default_value_t = 1)]
    jobs: usize,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::List { suite_dir } => cmd_list(&suite_dir),
        Commands::VerifySuite {
            suite_dir,
            hivemind_bin,
        } => cmd_verify_suite(&suite_dir, hivemind_bin.as_deref()),
        Commands::Run(args) => cmd_run(*args),
        Commands::Report {
            manifest_path,
            baseline,
            resamples,
            rng_seed,
            json,
        } => cmd_report(
            &manifest_path,
            baseline.as_deref(),
            resamples,
            rng_seed,
            json,
        ),
    }
}

fn cmd_list(suite_dir: &std::path::Path) -> Result<()> {
    let (suite, scenarios) = load_suite(suite_dir)?;
    println!("Suite: {} — {}", suite.id, suite.title);
    println!("{}\n", suite.description.trim());
    println!(
        "{:<28} {:<16} {:>8} {:>8} {:>10} {:>8}",
        "SCENARIO", "CATEGORY", "HISTORY", "TARGET", "MEMORIES", "NOISE"
    );
    println!("{}", "-".repeat(84));

    for scen in &scenarios {
        let hist_count = scen.history_episodes().len();
        let target_count = usize::from(scen.target_episode().is_ok());
        println!(
            "{:<28} {:<16} {:>8} {:>8} {:>10} {:>8}",
            scen.id,
            scen.category.as_str(),
            hist_count,
            target_count,
            scen.memories.len(),
            scen.noise_distractor_count
        );
    }
    println!("\nTotal scenarios: {}", scenarios.len());
    Ok(())
}

fn cmd_verify_suite(
    suite_dir: &std::path::Path,
    hivemind_bin: Option<&std::path::Path>,
) -> Result<()> {
    let (suite, scenarios) = load_suite(suite_dir)?;
    println!(
        "Verifying suite `{}` ({} scenarios)...",
        suite.id,
        scenarios.len()
    );

    let mut failures = Vec::new();
    for scen in &scenarios {
        let check: Result<()> = (|| {
            verify_scenario_integrity(scen)?;
            if let Some(bin) = hivemind_bin {
                let tmp = tempfile::TempDir::new()?;
                let arm = ArmSpec::hivemind(bin.to_path_buf(), MemoryMode::Given);
                let mut mem = prepare_trial_memory(scen, &arm, MemoryMode::Given, 0, tmp.path())?;
                mem.shutdown();
            }
            Ok(())
        })();

        match check {
            Ok(()) => {
                println!(
                    "[PASS] {:<28} ({}) — unmodified failed, gold_patch passed{}",
                    scen.id,
                    scen.category.as_str(),
                    if hivemind_bin.is_some() {
                        ", hivemind seeded"
                    } else {
                        ""
                    }
                );
            }
            Err(err) => {
                println!("[FAIL] {:<28} — {err:#}", scen.id);
                failures.push((scen.id.clone(), format!("{err:#}")));
            }
        }
    }

    if !failures.is_empty() {
        bail!(
            "{} of {} scenario(s) failed suite integrity verification",
            failures.len(),
            scenarios.len()
        );
    }
    println!("All {} scenarios verified cleanly.", scenarios.len());
    Ok(())
}

fn cmd_run(args: RunArgs) -> Result<()> {
    if args.seeds == 0 {
        bail!("--seeds must be at least 1");
    }

    let memory_mode: MemoryMode = args.memory_mode.into();
    let arms = if let Some(path) = &args.arms_file {
        ArmsConfigFile::load_toml(path)?.arms
    } else {
        let mut resolved = Vec::with_capacity(args.arms.len());
        let mut seen_ids = BTreeSet::new();
        for raw in &args.arms {
            let spec = resolve_arm_spec(raw, &args.hivemind_bin, memory_mode)?;
            if !seen_ids.insert(spec.id.clone()) {
                bail!("duplicate arm `{}` in --arms", spec.id);
            }
            resolved.push(spec);
        }
        resolved
    };

    let driver: Box<dyn AgentDriver> = match args.driver {
        CliDriverKind::Copilot => Box::new(CopilotDriver::new(&args.copilot_bin)),
        CliDriverKind::Scripted => Box::new(ScriptedDriver::default()),
        CliDriverKind::Command => {
            let Some(bin) = args.command_bin else {
                bail!("--command-bin <path> is required when --driver command is selected");
            };
            Box::new(CommandDriver::new(bin, Vec::new()))
        }
    };

    let cfg = RunConfig {
        suite_dir: args.suite_dir,
        out_dir: args.out.clone(),
        arms,
        seeds: (0..args.seeds).collect(),
        scenario_filter: args.scenarios,
        model: args.model,
        reasoning_effort: args.effort,
        memory_mode,
        baseline_arm: args.baseline,
        bootstrap_resamples: args.resamples,
        rng_seed: 42,
        resume: args.resume,
        jobs: args.jobs.max(1),
    };

    let outcome = run_benchmark(&cfg, driver.as_ref(), |ev| {
        let status = match (ev.trial.passed, ev.resumed) {
            (true, false) => "PASS",
            (false, false) => "FAIL",
            (true, true) => "PASS*resumed",
            (false, true) => "FAIL*resumed",
        };
        let wall_s = ev.trial.total_usage.wall_duration_ms as f64 / 1_000.0;
        println!(
            "[{status:<12}] ({:>3}/{:<3}) {:<26} | {:<15} | seed={} | {:>6.2}s | tools={:<3} | ${:.4}",
            ev.completed_count,
            ev.total_count,
            ev.trial.scenario_id,
            ev.trial.arm_id,
            ev.trial.seed,
            wall_s,
            ev.trial.total_usage.tool_calls,
            ev.cache_adjusted_usd
        );
    })?;

    println!(
        "\nArtifacts written to {} (`manifest.json`, `trials.jsonl`, `report.md`, `report.json`).\n",
        args.out.display()
    );
    println!("{}", outcome.markdown_report.trim_end());
    Ok(())
}

fn cmd_report(
    manifest_path: &std::path::Path,
    baseline: Option<&str>,
    resamples: usize,
    rng_seed: u64,
    emit_json: bool,
) -> Result<()> {
    let manifest = RunManifest::load_json(manifest_path)?;
    let effective_baseline = baseline.or_else(|| {
        if manifest.arms.iter().any(|a| a == "b0") {
            Some("b0")
        } else {
            manifest.arms.first().map(String::as_str)
        }
    });

    let report =
        crucible_stats::analyze_manifest(&manifest, effective_baseline, resamples, rng_seed);
    if emit_json {
        println!("{}", crucible_stats::render_json_report(&report).trim_end());
    } else {
        println!(
            "{}",
            crucible_stats::render_markdown_report(&report).trim_end()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;

    use super::*;

    #[test]
    fn clap_cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn resolves_all_supported_arm_shorthands() {
        let bin = PathBuf::from("hivemind");
        assert_eq!(
            resolve_arm_spec("b0", &bin, MemoryMode::Given).unwrap().id,
            "b0"
        );
        assert_eq!(
            resolve_arm_spec("b1-notes", &bin, MemoryMode::Given)
                .unwrap()
                .id,
            "b1-notes"
        );
        assert_eq!(
            resolve_arm_spec("b1-grep", &bin, MemoryMode::Given)
                .unwrap()
                .id,
            "b1-grep"
        );
        assert_eq!(
            resolve_arm_spec("hivemind", &bin, MemoryMode::Given)
                .unwrap()
                .id,
            "hivemind"
        );
        assert_eq!(
            resolve_arm_spec("hivemind", &bin, MemoryMode::Earned)
                .unwrap()
                .id,
            "hivemind-earned"
        );
        assert_eq!(
            resolve_arm_spec("oracle", &bin, MemoryMode::Given)
                .unwrap()
                .id,
            "oracle"
        );
        assert!(resolve_arm_spec("nonexistent", &bin, MemoryMode::Given).is_err());
    }
}
