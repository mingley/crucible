//! Latin-square counterbalanced benchmark runner with resumable JSONL trial checkpoints.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail};
use crucible_core::{
    ArmSpec, MemoryMode, ModelPricing, RunManifest, ScenarioSpec, SuiteManifest, TrialRecord,
    UsageMetrics, load_suite,
};
use crucible_stats::EvaluationReport;
use serde::{Deserialize, Serialize};
use tempfile::TempDir;

use crate::driver::{AgentDriver, EpisodeRunRequest};
use crate::memory::prepare_trial_memory;
use crate::sandbox::Sandbox;

/// Optional TOML configuration file defining custom benchmark arms.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArmsConfigFile {
    pub arms: Vec<ArmSpec>,
}

impl ArmsConfigFile {
    pub fn load_toml(path: &Path) -> Result<Self> {
        let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        let cfg: Self =
            toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
        if cfg.arms.is_empty() {
            bail!("arms config `{}` defines zero arms", path.display());
        }
        Ok(cfg)
    }

    pub fn to_toml_string(&self) -> Result<String> {
        toml::to_string_pretty(self).context("serialize ArmsConfigFile to TOML")
    }
}

/// Configuration for [`run_benchmark`].
#[derive(Clone, Debug)]
pub struct RunConfig {
    pub suite_dir: PathBuf,
    pub out_dir: PathBuf,
    pub arms: Vec<ArmSpec>,
    pub seeds: Vec<u32>,
    pub scenario_filter: Vec<String>,
    pub model: String,
    pub reasoning_effort: String,
    pub memory_mode: MemoryMode,
    pub baseline_arm: Option<String>,
    pub bootstrap_resamples: usize,
    pub rng_seed: u64,
    pub resume: bool,
    pub jobs: usize,
}

impl RunConfig {
    pub fn effective_baseline_arm(&self) -> Option<&str> {
        if let Some(explicit) = &self.baseline_arm {
            return Some(explicit.as_str());
        }
        if self.arms.iter().any(|a| a.id == "b0") {
            Some("b0")
        } else {
            self.arms.first().map(|a| a.id.as_str())
        }
    }
}

/// Live progress notification emitted after each executed or resumed trial.
#[derive(Clone, Debug)]
pub struct TrialProgressEvent {
    pub trial: TrialRecord,
    pub resumed: bool,
    pub completed_count: usize,
    pub total_count: usize,
    pub cache_adjusted_usd: f64,
}

/// Persisted manifest and rendered reports returned at the conclusion of [`run_benchmark`].
#[derive(Clone, Debug)]
pub struct RunOutcome {
    pub suite: SuiteManifest,
    pub manifest: RunManifest,
    pub report: EvaluationReport,
    pub markdown_report: String,
    pub json_report: String,
}

/// Execute a paired, Latin-square counterbalanced benchmark suite and write `trials.jsonl`,
/// `manifest.json`, `report.md`, and `report.json` under `config.out_dir`.
pub fn run_benchmark<F>(
    config: &RunConfig,
    driver: &dyn AgentDriver,
    mut on_trial: F,
) -> Result<RunOutcome>
where
    F: FnMut(&TrialProgressEvent) + Send,
{
    if config.arms.is_empty() {
        bail!("benchmark configuration must include at least one arm");
    }
    if config.seeds.is_empty() {
        bail!("benchmark configuration must include at least one seed");
    }

    let (suite, all_scenarios) = load_suite(&config.suite_dir)?;
    let selected_scenarios = filter_scenarios(all_scenarios, &config.scenario_filter)?;

    fs::create_dir_all(&config.out_dir)
        .with_context(|| format!("create output directory {}", config.out_dir.display()))?;

    let trials_jsonl_path = config.out_dir.join("trials.jsonl");
    let manifest_path = config.out_dir.join("manifest.json");
    let report_md_path = config.out_dir.join("report.md");
    let report_json_path = config.out_dir.join("report.json");

    let mut resumed_map: BTreeMap<(String, String, u32), TrialRecord> = BTreeMap::new();
    let mut existing_run_id: Option<String> = None;
    let mut existing_created_at: Option<String> = None;

    if config.resume {
        if manifest_path.is_file()
            && let Ok(prev_manifest) = RunManifest::load_json(&manifest_path)
        {
            existing_run_id = Some(prev_manifest.run_id);
            existing_created_at = Some(prev_manifest.created_at_utc);
        }
        if trials_jsonl_path.is_file() {
            let loaded = load_checkpoint_trials(&trials_jsonl_path)?;
            for rec in loaded {
                if existing_run_id.is_none() {
                    existing_run_id = Some(rec.run_id.clone());
                }
                resumed_map.insert((rec.scenario_id.clone(), rec.arm_id.clone(), rec.seed), rec);
            }
        }
    } else if trials_jsonl_path.exists() {
        fs::write(&trials_jsonl_path, "")
            .with_context(|| format!("truncate {}", trials_jsonl_path.display()))?;
    }

    let created_at_utc = existing_created_at.unwrap_or_else(utc_now_iso8601);
    let run_id = existing_run_id.unwrap_or_else(|| generate_run_id(&suite.id));
    let pricing = ModelPricing::for_model(&config.model);

    let checkpoint_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&trials_jsonl_path)
        .with_context(|| format!("open {} for append", trials_jsonl_path.display()))?;

    let total_count = selected_scenarios.len() * config.seeds.len() * config.arms.len();
    let num_arms = config.arms.len();

    let mut blocks: Vec<(usize, &ScenarioSpec, u32)> =
        Vec::with_capacity(selected_scenarios.len() * config.seeds.len());
    for (scenario_idx, scenario) in selected_scenarios.iter().enumerate() {
        for &seed in &config.seeds {
            blocks.push((scenario_idx, scenario, seed));
        }
    }

    struct SharedSink<'a, F> {
        checkpoint_file: fs::File,
        resumed_map: BTreeMap<(String, String, u32), TrialRecord>,
        completed_trials: Vec<(usize, u32, usize, TrialRecord)>,
        on_trial: &'a mut F,
    }

    let sink = Mutex::new(SharedSink {
        checkpoint_file,
        resumed_map,
        completed_trials: Vec::with_capacity(total_count),
        on_trial: &mut on_trial,
    });
    let first_err: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    let next_block = AtomicUsize::new(0);
    let worker_count = config.jobs.max(1).min(blocks.len().max(1));

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                loop {
                    if first_err.lock().unwrap().is_some() {
                        break;
                    }
                    let b_idx = next_block.fetch_add(1, Ordering::Relaxed);
                    if b_idx >= blocks.len() {
                        break;
                    }
                    let (scenario_idx, scenario, seed) = blocks[b_idx];
                    // Latin-square rotation across (scenario_idx, seed) so prompt-cache warmth
                    // and transient provider latency jitter affect all arms symmetrically.
                    let rotation_offset = (scenario_idx + seed as usize) % num_arms;

                    for order_index in 0..num_arms {
                        if first_err.lock().unwrap().is_some() {
                            break;
                        }
                        let arm_idx = (rotation_offset + order_index) % num_arms;
                        let arm = &config.arms[arm_idx];
                        let key = (scenario.id.clone(), arm.id.clone(), seed);

                        let pre_resumed = {
                            let mut guard = sink.lock().unwrap();
                            guard.resumed_map.remove(&key)
                        };

                        let (trial, resumed) = match pre_resumed {
                            Some(prev) => (prev, true),
                            None => match execute_single_trial(
                                &SingleTrialParams {
                                    run_id: &run_id,
                                    suite_id: &suite.id,
                                    scenario,
                                    arm,
                                    seed,
                                    order_index,
                                    memory_mode: config.memory_mode,
                                    model: &config.model,
                                    reasoning_effort: &config.reasoning_effort,
                                },
                                driver,
                            ) {
                                Ok(fresh) => (fresh, false),
                                Err(err) => {
                                    let mut e_guard = first_err.lock().unwrap();
                                    if e_guard.is_none() {
                                        *e_guard = Some(err);
                                    }
                                    break;
                                }
                            },
                        };

                        let write_res = (|| -> Result<()> {
                            let mut guard = sink.lock().unwrap();
                            if !resumed {
                                let line = serde_json::to_string(&trial)
                                    .context("serialize TrialRecord checkpoint")?;
                                writeln!(guard.checkpoint_file, "{line}")?;
                                guard.checkpoint_file.flush()?;
                                guard.checkpoint_file.sync_data()?;
                            }
                            let usd = trial.total_usage.cache_adjusted_usd(&pricing);
                            guard.completed_trials.push((
                                scenario_idx,
                                seed,
                                order_index,
                                trial.clone(),
                            ));
                            let completed_count = guard.completed_trials.len();
                            (guard.on_trial)(&TrialProgressEvent {
                                trial,
                                resumed,
                                completed_count,
                                total_count,
                                cache_adjusted_usd: usd,
                            });
                            Ok(())
                        })();

                        if let Err(err) = write_res {
                            let mut e_guard = first_err.lock().unwrap();
                            if e_guard.is_none() {
                                *e_guard = Some(err);
                            }
                            break;
                        }
                    }
                }
            });
        }
    });

    if let Some(err) = first_err.into_inner().unwrap() {
        return Err(err);
    }

    let mut indexed_trials = sink.into_inner().unwrap().completed_trials;
    indexed_trials.sort_by_key(|(s_idx, seed, ord_idx, _)| (*s_idx, *seed, *ord_idx));
    let completed_trials: Vec<TrialRecord> =
        indexed_trials.into_iter().map(|(_, _, _, t)| t).collect();

    let manifest = RunManifest {
        run_id,
        created_at_utc,
        suite_id: suite.id.clone(),
        driver: driver.name().to_string(),
        model: config.model.clone(),
        reasoning_effort: config.reasoning_effort.clone(),
        seeds: config.seeds.clone(),
        arms: config.arms.iter().map(|a| a.id.clone()).collect(),
        pricing,
        trials: completed_trials,
    };
    manifest.save_json(&manifest_path)?;

    let report = crucible_stats::analyze_manifest(
        &manifest,
        config.effective_baseline_arm(),
        config.bootstrap_resamples,
        config.rng_seed,
    );
    let markdown_report = crucible_stats::render_markdown_report(&report);
    let json_report = crucible_stats::render_json_report(&report);

    fs::write(&report_md_path, &markdown_report)
        .with_context(|| format!("write {}", report_md_path.display()))?;
    fs::write(&report_json_path, format!("{}\n", json_report.trim_end()))
        .with_context(|| format!("write {}", report_json_path.display()))?;

    Ok(RunOutcome {
        suite,
        manifest,
        report,
        markdown_report,
        json_report,
    })
}

struct SingleTrialParams<'a> {
    run_id: &'a str,
    suite_id: &'a str,
    scenario: &'a ScenarioSpec,
    arm: &'a ArmSpec,
    seed: u32,
    order_index: usize,
    memory_mode: MemoryMode,
    model: &'a str,
    reasoning_effort: &'a str,
}

fn execute_single_trial(
    params: &SingleTrialParams<'_>,
    driver: &dyn AgentDriver,
) -> Result<TrialRecord> {
    let trial_temp_dir = TempDir::new().context("create per-trial TempDir")?;
    let mut prepared_mem = prepare_trial_memory(
        params.scenario,
        params.arm,
        params.memory_mode,
        params.seed,
        trial_temp_dir.path(),
    )?;

    let mut history_usage = UsageMetrics::default();

    if prepared_mem.should_run_history() {
        for hist_ep in params.scenario.history_episodes() {
            let mut hist_sb =
                Sandbox::stage_scenario_episode(params.scenario, &hist_ep.workspace_subdir)?;
            let ep_ctx = prepared_mem.prepare_episode_workspace(hist_ep, hist_sb.repo_dir())?;
            hist_sb.init_git_commit()?;

            let req = EpisodeRunRequest {
                scenario_id: params.scenario.id.clone(),
                episode: hist_ep.clone(),
                arm: params.arm.clone(),
                seed: params.seed,
                workspace_dir: hist_sb.repo_dir().to_path_buf(),
                scratch_dir: hist_sb.scratch_dir().to_path_buf(),
                prompt: ep_ctx.prompt,
                mcp_config_path: ep_ctx.mcp_config_path,
                gold_patch_source_dir: None,
                model: params.model.to_string(),
                reasoning_effort: params.reasoning_effort.to_string(),
            };

            let mut hist_out = driver.run_episode(&req)?;
            if hist_out.usage.lines_added == 0
                && hist_out.usage.lines_removed == 0
                && hist_out.usage.files_modified == 0
                && let Ok(diff) = hist_sb.diff_stats()
            {
                hist_out.usage.lines_added = diff.lines_added;
                hist_out.usage.lines_removed = diff.lines_removed;
                hist_out.usage.files_modified = diff.files_modified;
            }
            history_usage.accumulate(&hist_out.usage);
            prepared_mem.capture_after_history_episode(hist_sb.repo_dir())?;
        }
    }

    let target_ep = params.scenario.target_episode()?;
    let mut target_sb =
        Sandbox::stage_scenario_episode(params.scenario, &target_ep.workspace_subdir)?;
    let target_ctx = prepared_mem.prepare_episode_workspace(target_ep, target_sb.repo_dir())?;
    target_sb.init_git_commit()?;

    let target_req = EpisodeRunRequest {
        scenario_id: params.scenario.id.clone(),
        episode: target_ep.clone(),
        arm: params.arm.clone(),
        seed: params.seed,
        workspace_dir: target_sb.repo_dir().to_path_buf(),
        scratch_dir: target_sb.scratch_dir().to_path_buf(),
        prompt: target_ctx.prompt,
        mcp_config_path: target_ctx.mcp_config_path,
        gold_patch_source_dir: Some(params.scenario.root.join(&params.scenario.gold_patch_dir)),
        model: params.model.to_string(),
        reasoning_effort: params.reasoning_effort.to_string(),
    };

    let mut target_out = driver.run_episode(&target_req)?;
    let (verification, diff_stats) =
        target_sb.verify_with_diff(params.scenario, &target_out.tool_trace)?;

    if target_out.usage.lines_added == 0
        && target_out.usage.lines_removed == 0
        && target_out.usage.files_modified == 0
    {
        target_out.usage.lines_added = diff_stats.lines_added;
        target_out.usage.lines_removed = diff_stats.lines_removed;
        target_out.usage.files_modified = diff_stats.files_modified;
    }

    // Explicitly shut down any background `hivemindd` daemon before `trial_temp_dir` drops.
    prepared_mem.shutdown();

    let mut total_usage = history_usage.clone();
    total_usage.accumulate(&target_out.usage);

    Ok(TrialRecord {
        run_id: params.run_id.to_string(),
        suite_id: params.suite_id.to_string(),
        scenario_id: params.scenario.id.clone(),
        category: params.scenario.category,
        arm_id: params.arm.id.clone(),
        seed: params.seed,
        order_index: params.order_index,
        passed: verification.passed,
        verification,
        target_usage: target_out.usage,
        history_usage,
        total_usage,
        tool_trace: target_out.tool_trace,
        agent_exit_code: target_out.exit_code,
    })
}

fn filter_scenarios(scenarios: Vec<ScenarioSpec>, filter: &[String]) -> Result<Vec<ScenarioSpec>> {
    if filter.is_empty() {
        return Ok(scenarios);
    }
    let requested: BTreeSet<&str> = filter.iter().map(String::as_str).collect();
    let available: BTreeSet<&str> = scenarios.iter().map(|s| s.id.as_str()).collect();
    for req in &requested {
        if !available.contains(req) {
            bail!("unknown scenario id `{req}` (not present in suite.toml)");
        }
    }
    Ok(scenarios
        .into_iter()
        .filter(|s| requested.contains(s.id.as_str()))
        .collect())
}

fn load_checkpoint_trials(path: &Path) -> Result<Vec<TrialRecord>> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut records = Vec::new();
    let lines: Vec<&str> = raw.lines().filter(|l| !l.trim().is_empty()).collect();
    for (idx, line) in lines.iter().enumerate() {
        match serde_json::from_str::<TrialRecord>(line) {
            Ok(rec) => records.push(rec),
            Err(err) if idx + 1 == lines.len() => {
                // Tolerate a truncated final JSONL line if a previous run crashed mid-write.
                eprintln!(
                    "warning: ignoring truncated trailing line in {}: {err}",
                    path.display()
                );
            }
            Err(err) => {
                bail!(
                    "corrupt checkpoint record at {}:{} : {err}",
                    path.display(),
                    idx + 1
                );
            }
        }
    }
    Ok(records)
}

fn generate_run_id(suite_id: &str) -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{suite_id}-{secs}")
}

/// Format current UTC system time as RFC 3339 (`YYYY-MM-DDTHH:MM:SSZ`) without external crates.
pub fn utc_now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format_unix_seconds_utc(secs)
}

fn format_unix_seconds_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let second = rem % 60;

    // Howard Hinnant's civil-from-days algorithm (proleptic Gregorian calendar).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}Z")
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use crate::driver::{EpisodeRunOutput, ScriptedDriver};

    use super::*;

    fn write_mini_suite(suite_dir: &Path) {
        fs::create_dir_all(suite_dir).unwrap();
        let suite_toml = r#"
id = "mini-suite"
title = "Mini Suite"
description = "Synthetic two-scenario suite for Latin-square & resume tests"
scenarios = ["s1", "s2"]
"#;
        fs::write(suite_dir.join("suite.toml"), suite_toml).unwrap();

        for (id, cat) in [("s1", "handoff"), ("s2", "supersession")] {
            let sdir = suite_dir.join(id);
            fs::create_dir_all(sdir.join("workspace")).unwrap();
            fs::create_dir_all(sdir.join("verifier")).unwrap();
            fs::create_dir_all(sdir.join("gold_patch")).unwrap();

            fs::write(sdir.join("workspace/val.txt"), "broken\n").unwrap();
            fs::write(
                sdir.join("verifier/verify.sh"),
                "#!/bin/sh\ngrep -qx 'fixed' val.txt\n",
            )
            .unwrap();
            fs::write(sdir.join("gold_patch/val.txt"), "fixed\n").unwrap();

            let scen_toml = format!(
                r#"
id = "{id}"
title = "Scenario {id}"
category = "{cat}"
description = "Test scenario {id}"
scope = "project:{id}"
noise_distractor_count = 3

[[memories]]
id = "m1"
kind = "fact"
statement = "Set val.txt to fixed."
is_gold = true

[[episodes]]
id = "history-1"
role = "history"
workspace_subdir = "workspace"
prompt = "Investigate val.txt"
timeout_secs = 10

[[episodes]]
id = "target"
role = "target"
workspace_subdir = "workspace"
prompt = "Fix val.txt"
timeout_secs = 10

[verifier]
command = ["sh", "verify.sh"]
overlay_dir = "verifier"
timeout_secs = 5
"#
            );
            fs::write(sdir.join("scenario.toml"), scen_toml).unwrap();
        }
    }

    #[derive(Debug)]
    struct CountingDriver {
        inner: ScriptedDriver,
        calls: Mutex<Vec<(String, String, u32)>>,
    }

    impl CountingDriver {
        fn new(inner: ScriptedDriver) -> Self {
            Self {
                inner,
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl AgentDriver for CountingDriver {
        fn name(&self) -> &str {
            "counting-scripted"
        }

        fn run_episode(&self, req: &EpisodeRunRequest) -> Result<EpisodeRunOutput> {
            self.calls.lock().unwrap().push((
                req.scenario_id.clone(),
                req.arm.id.clone(),
                req.seed,
            ));
            self.inner.run_episode(req)
        }
    }

    #[test]
    fn utc_formatter_matches_known_timestamps() {
        assert_eq!(format_unix_seconds_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(
            format_unix_seconds_utc(1_709_164_800),
            "2024-02-29T00:00:00Z"
        );
    }

    #[test]
    fn arms_config_toml_roundtrips() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("arms.toml");
        let cfg = ArmsConfigFile {
            arms: vec![ArmSpec::b0(), ArmSpec::b1_notes(), ArmSpec::oracle()],
        };
        fs::write(&path, cfg.to_toml_string().unwrap()).unwrap();
        let loaded = ArmsConfigFile::load_toml(&path).unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn runner_executes_latin_square_rotation_checkpoints_resume_and_earned_history() {
        let dir = TempDir::new().unwrap();
        let suite_dir = dir.path().join("suite");
        let out_dir = dir.path().join("out");
        write_mini_suite(&suite_dir);

        let arms = vec![ArmSpec::b0(), ArmSpec::b1_notes(), ArmSpec::oracle()];
        let cfg = RunConfig {
            suite_dir: suite_dir.clone(),
            out_dir: out_dir.clone(),
            arms: arms.clone(),
            seeds: vec![0, 1, 2],
            scenario_filter: Vec::new(),
            model: "claude-sonnet-5.5".into(),
            reasoning_effort: "medium".into(),
            memory_mode: MemoryMode::Given,
            baseline_arm: Some("b0".into()),
            bootstrap_resamples: 500,
            rng_seed: 42,
            resume: false,
            jobs: 1,
        };

        let driver = CountingDriver::new(ScriptedDriver::pass_arms(["oracle"]));
        let outcome = run_benchmark(&cfg, &driver, |_| {}).unwrap();

        assert_eq!(outcome.manifest.trials.len(), 18); // 2 scenarios * 3 seeds * 3 arms
        assert!(out_dir.join("trials.jsonl").is_file());
        assert!(out_dir.join("manifest.json").is_file());
        assert!(out_dir.join("report.md").is_file());
        assert!(out_dir.join("report.json").is_file());
        assert!(!outcome.markdown_report.is_empty());

        // Verify Latin-square counterbalancing: every arm appears at order_index 0, 1, and 2
        // exactly twice across 2 scenarios x 3 seeds!
        for arm in &arms {
            for order_idx in 0..3 {
                let count = outcome
                    .manifest
                    .trials
                    .iter()
                    .filter(|t| t.arm_id == arm.id && t.order_index == order_idx)
                    .count();
                assert_eq!(
                    count, 2,
                    "arm `{}` should run at order_index={order_idx} twice",
                    arm.id
                );
            }
        }

        // Verify oracle passed all 6 trials and b0 failed all 6 trials.
        assert!(
            outcome
                .manifest
                .trials
                .iter()
                .filter(|t| t.arm_id == "oracle")
                .all(|t| t.passed)
        );
        assert!(
            outcome
                .manifest
                .trials
                .iter()
                .filter(|t| t.arm_id == "b0")
                .all(|t| !t.passed)
        );

        // Verify `--resume` reuses all 18 completed trials with ZERO new driver invocations!
        let resume_driver = CountingDriver::new(ScriptedDriver::pass_arms(["oracle"]));
        let mut resume_cfg = cfg.clone();
        resume_cfg.resume = true;
        let mut resumed_events = 0usize;
        let resumed_outcome = run_benchmark(&resume_cfg, &resume_driver, |ev| {
            if ev.resumed {
                resumed_events += 1;
            }
        })
        .unwrap();
        assert_eq!(resumed_events, 18);
        assert!(resume_driver.calls.lock().unwrap().is_empty());
        assert_eq!(resumed_outcome.manifest.run_id, outcome.manifest.run_id);

        // Verify `Earned` mode runs History episode on `b1-notes` and carries forward AGENTS.md!
        let earned_out = dir.path().join("out-earned");
        let earned_cfg = RunConfig {
            out_dir: earned_out,
            arms: vec![ArmSpec::b1_notes()],
            seeds: vec![0],
            scenario_filter: vec!["s1".into()],
            memory_mode: MemoryMode::Earned,
            baseline_arm: None,
            resume: false,
            ..cfg
        };
        let earned_driver = ScriptedDriver::with_predicate(|req| {
            let agents_md =
                fs::read_to_string(req.workspace_dir.join("AGENTS.md")).unwrap_or_default();
            agents_md.contains("Earned note recorded during history episode `history-1`")
        });
        let earned_res = run_benchmark(&earned_cfg, &earned_driver, |_| {}).unwrap();
        assert_eq!(earned_res.manifest.trials.len(), 1);
        let t = &earned_res.manifest.trials[0];
        assert!(
            t.passed,
            "Target episode should see AGENTS.md updated by History episode"
        );
        assert!(t.history_usage.total_input_tokens > 0);
        assert_eq!(
            t.total_usage.total_input_tokens,
            t.history_usage.total_input_tokens + t.target_usage.total_input_tokens
        );
    }
}
