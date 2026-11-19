//! Hermetic git workspace staging and answer-isolated post-episode verification.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use crucible_core::{ScenarioSpec, ToolCallSummary, VerificationOutcome};
use tempfile::TempDir;
use wait_timeout::ChildExt as _;

/// Captured output and timing from a bounded-timeout subprocess execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandRunResult {
    pub exit_code: i32,
    pub timed_out: bool,
    pub wall_duration_ms: u64,
    pub stdout: String,
    pub stderr: String,
}

/// Diff statistics computed between the initial sandbox commit and post-agent workspace state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffStats {
    pub lines_added: u64,
    pub lines_removed: u64,
    pub files_modified: u64,
    pub added_lines: Vec<String>,
}

/// Hermetic temporary git workspace isolated from verifier overlays and gold patches.
#[derive(Debug)]
pub struct Sandbox {
    temp_dir: TempDir,
    repo_dir: PathBuf,
    initial_commit_sha: Option<String>,
}

impl Sandbox {
    /// Stage a workspace directory into a fresh `TempDir` (`<temp>/repo/`) without committing yet,
    /// allowing arm preparation (`AGENTS.md`, `.memory/`) to populate files before the initial commit.
    pub fn stage(source_workspace_dir: &Path) -> Result<Self> {
        Self::stage_filtered(source_workspace_dir, &BTreeSet::new())
    }

    /// Stage an episode's workspace directory for `scenario` while guaranteeing that
    /// `verifier.overlay_dir`, `gold_patch_dir`, and `scenario.toml` are never copied.
    pub fn stage_scenario_episode(scenario: &ScenarioSpec, workspace_subdir: &str) -> Result<Self> {
        let source_dir = scenario.root.join(workspace_subdir);
        if !source_dir.is_dir() {
            bail!(
                "scenario `{}` workspace directory does not exist: {}",
                scenario.id,
                source_dir.display()
            );
        }

        let mut excluded_abs = BTreeSet::new();
        excluded_abs.insert(normalize_lexical(&scenario.root.join("scenario.toml")));
        excluded_abs.insert(normalize_lexical(
            &scenario.root.join(&scenario.gold_patch_dir),
        ));
        if let Some(overlay) = &scenario.verifier.overlay_dir {
            excluded_abs.insert(normalize_lexical(&scenario.root.join(overlay)));
        }

        Self::stage_filtered(&source_dir, &excluded_abs)
    }

    fn stage_filtered(
        source_workspace_dir: &Path,
        excluded_abs: &BTreeSet<PathBuf>,
    ) -> Result<Self> {
        if !source_workspace_dir.is_dir() {
            bail!(
                "source workspace directory does not exist: {}",
                source_workspace_dir.display()
            );
        }
        let temp_dir = TempDir::new().context("create sandbox TempDir")?;
        let repo_dir = temp_dir.path().join("repo");
        fs::create_dir_all(&repo_dir)
            .with_context(|| format!("create sandbox repo dir {}", repo_dir.display()))?;
        copy_dir_filtered(source_workspace_dir, &repo_dir, excluded_abs)?;
        Ok(Self {
            temp_dir,
            repo_dir,
            initial_commit_sha: None,
        })
    }

    /// Stage `source_workspace_dir` and immediately initialize the baseline git commit.
    pub fn new(source_workspace_dir: &Path) -> Result<Self> {
        let mut sb = Self::stage(source_workspace_dir)?;
        sb.init_git_commit()?;
        Ok(sb)
    }

    /// Path to the git working tree (`<temp>/repo/`).
    pub fn repo_dir(&self) -> &Path {
        &self.repo_dir
    }

    /// Path to the parent scratch directory (`<temp>/`), outside the git working tree.
    pub fn scratch_dir(&self) -> &Path {
        self.temp_dir.path()
    }

    /// Initialize git repository inside `repo/` and create the initial baseline commit.
    pub fn init_git_commit(&mut self) -> Result<()> {
        run_git(&self.repo_dir, &["init", "-b", "main", "--quiet"])?;
        let info_exclude = self.repo_dir.join(".git").join("info").join("exclude");
        if let Some(parent) = info_exclude.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&info_exclude, "__pycache__/\n*.py[cod]\n.pytest_cache/\n");
        run_git(&self.repo_dir, &["config", "user.name", "crucible"])?;
        run_git(
            &self.repo_dir,
            &["config", "user.email", "crucible@example.invalid"],
        )?;
        run_git(&self.repo_dir, &["config", "commit.gpgsign", "false"])?;
        run_git(&self.repo_dir, &["add", "-A"])?;
        run_git(
            &self.repo_dir,
            &["commit", "--allow-empty", "-m", "initial", "--quiet"],
        )?;
        let sha = run_git(&self.repo_dir, &["rev-parse", "HEAD"])?;
        self.initial_commit_sha = Some(sha.trim().to_string());
        Ok(())
    }

    /// Compute diff metrics and added lines relative to the initial sandbox commit,
    /// including newly created untracked files.
    pub fn diff_stats(&self) -> Result<DiffStats> {
        let base_ref = self.initial_commit_sha.as_deref().unwrap_or("HEAD");
        let raw_diff = run_git(
            &self.repo_dir,
            &[
                "--no-pager",
                "diff",
                "--no-color",
                "--no-ext-diff",
                base_ref,
            ],
        )?;
        let changed_names = run_git(
            &self.repo_dir,
            &["--no-pager", "diff", "--name-only", base_ref],
        )?;
        let untracked_raw = run_git(
            &self.repo_dir,
            &[
                "--no-pager",
                "ls-files",
                "--others",
                "--exclude-standard",
                "-z",
            ],
        )?;

        let mut stats = DiffStats::default();
        let mut modified_files: BTreeSet<String> = changed_names
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(ToOwned::to_owned)
            .collect();

        for line in raw_diff.lines() {
            if let Some(added) = line.strip_prefix('+') {
                if !line.starts_with("+++") {
                    stats.lines_added += 1;
                    stats.added_lines.push(added.to_string());
                }
            } else if line.starts_with('-') && !line.starts_with("---") {
                stats.lines_removed += 1;
            }
        }

        for rel in untracked_raw.split('\0').filter(|s| !s.is_empty()) {
            modified_files.insert(rel.to_string());
            let full_path = self.repo_dir.join(rel);
            if full_path.is_file()
                && let Ok(bytes) = fs::read(&full_path)
            {
                let text = String::from_utf8_lossy(&bytes);
                for line in text.lines() {
                    stats.lines_added += 1;
                    stats.added_lines.push(line.to_string());
                }
            }
        }

        stats.files_modified = modified_files.len() as u64;
        Ok(stats)
    }

    /// Run post-episode anti-cheating checks, copy the answer-isolated verifier overlay,
    /// and execute the verifier command under `verifier.timeout_secs`.
    pub fn verify(
        &self,
        scenario: &ScenarioSpec,
        tool_trace: &[ToolCallSummary],
    ) -> Result<VerificationOutcome> {
        let (outcome, _) = self.verify_with_diff(scenario, tool_trace)?;
        Ok(outcome)
    }

    /// Same as [`Self::verify`], also returning the pre-overlay workspace [`DiffStats`].
    pub fn verify_with_diff(
        &self,
        scenario: &ScenarioSpec,
        tool_trace: &[ToolCallSummary],
    ) -> Result<(VerificationOutcome, DiffStats)> {
        let diff = self.diff_stats()?;
        let mut violations = Vec::new();

        // 1. Check added diff lines against `forbidden_diff_substrings`.
        for forbidden in &scenario.verifier.forbidden_diff_substrings {
            if forbidden.is_empty() {
                continue;
            }
            if diff.added_lines.iter().any(|line| line.contains(forbidden)) {
                violations.push(format!(
                    "forbidden diff substring `{forbidden}` found in added lines"
                ));
            }
        }

        // 2. Check `forbidden_paths` (relative to sandbox repo or absolute).
        for raw_path in &scenario.verifier.forbidden_paths {
            if raw_path.is_empty() {
                continue;
            }
            let candidate = Path::new(raw_path);
            let resolved = if candidate.is_absolute() {
                candidate.to_path_buf()
            } else {
                self.repo_dir.join(candidate)
            };
            if resolved.exists() || resolved.symlink_metadata().is_ok() {
                violations.push(format!("forbidden path exists: `{raw_path}`"));
            }
        }

        // 3. Check `tool_trace` (`name` + `arguments_preview`) against `forbidden_tool_substrings`.
        for forbidden in &scenario.verifier.forbidden_tool_substrings {
            if forbidden.is_empty() {
                continue;
            }
            for call in tool_trace {
                if call.name.contains(forbidden) || call.arguments_preview.contains(forbidden) {
                    violations.push(format!(
                        "forbidden tool substring `{forbidden}` detected in tool `{}`",
                        call.name
                    ));
                    break;
                }
            }
        }

        // 4. Copy `verifier.overlay_dir` ONLY after agent execution and diff/path checks.
        if let Some(overlay_rel) = &scenario.verifier.overlay_dir {
            let overlay_src = scenario.root.join(overlay_rel);
            if !overlay_src.is_dir() {
                bail!(
                    "scenario `{}` verifier overlay directory missing: {}",
                    scenario.id,
                    overlay_src.display()
                );
            }
            copy_dir_all(&overlay_src, &self.repo_dir)?;
        }

        // 5. Execute `verifier.command` inside `sandbox/repo/` with bounded timeout.
        let Some((program, args)) = scenario.verifier.command.split_first() else {
            bail!(
                "scenario `{}` verifier command cannot be empty",
                scenario.id
            );
        };
        let mut cmd = Command::new(program);
        cmd.args(args).current_dir(&self.repo_dir);

        let run = run_command_with_timeout(
            &mut cmd,
            scenario.verifier.timeout_secs,
            self.temp_dir.path(),
            "verifier",
        )
        .with_context(|| {
            format!(
                "execute verifier command {:?} for scenario `{}`",
                scenario.verifier.command, scenario.id
            )
        })?;

        if run.timed_out {
            violations.push(format!(
                "verifier command timed out after {}s",
                scenario.verifier.timeout_secs
            ));
        }

        let passed = violations.is_empty() && !run.timed_out && run.exit_code == 0;
        let outcome = VerificationOutcome {
            passed,
            exit_code: run.exit_code,
            stdout: run.stdout,
            stderr: run.stderr,
            violations,
        };
        Ok((outcome, diff))
    }
}

/// Verify scenario soundness:
/// 1. Unmodified target workspace MUST fail the verifier.
/// 2. Applying `scenario.gold_patch_dir` onto a fresh target sandbox MUST pass the verifier.
pub fn verify_scenario_integrity(scenario: &ScenarioSpec) -> Result<()> {
    scenario.validate()?;
    let target = scenario.target_episode()?;

    // 1. Unmodified workspace must fail.
    {
        let mut clean_sb = Sandbox::stage_scenario_episode(scenario, &target.workspace_subdir)?;
        clean_sb.init_git_commit()?;
        let clean_outcome = clean_sb.verify(scenario, &[])?;
        if clean_outcome.passed {
            bail!(
                "scenario `{}`: unmodified workspace unexpectedly passed verifier",
                scenario.id
            );
        }
    }

    // 2. Gold patch applied onto a fresh sandbox must pass.
    {
        let mut gold_sb = Sandbox::stage_scenario_episode(scenario, &target.workspace_subdir)?;
        gold_sb.init_git_commit()?;
        let gold_dir = scenario.root.join(&scenario.gold_patch_dir);
        copy_dir_all(&gold_dir, gold_sb.repo_dir()).with_context(|| {
            format!(
                "overlay gold_patch `{}` for scenario `{}`",
                gold_dir.display(),
                scenario.id
            )
        })?;
        let gold_outcome = gold_sb.verify(scenario, &[])?;
        if !gold_outcome.passed {
            bail!(
                "scenario `{}`: gold_patch failed verifier (exit_code={}, violations={:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
                scenario.id,
                gold_outcome.exit_code,
                gold_outcome.violations,
                gold_outcome.stdout,
                gold_outcome.stderr
            );
        }
    }

    Ok(())
}

/// Recursively copy `src` into `dst`, overwriting existing files safely and skipping `.git`.
pub fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    copy_dir_filtered(src, dst, &BTreeSet::new())
}

fn copy_dir_filtered(src: &Path, dst: &Path, excluded_abs: &BTreeSet<PathBuf>) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("create directory {}", dst.display()))?;
    for entry_res in
        fs::read_dir(src).with_context(|| format!("read directory {}", src.display()))?
    {
        let entry = entry_res?;
        let file_name = entry.file_name();
        if file_name == ".git" {
            continue;
        }
        let src_path = entry.path();
        if excluded_abs.contains(&normalize_lexical(&src_path)) {
            continue;
        }
        let dst_path = dst.join(&file_name);
        let file_type = entry
            .file_type()
            .with_context(|| format!("stat {}", src_path.display()))?;

        if file_type.is_dir() {
            copy_dir_filtered(&src_path, &dst_path, excluded_abs)?;
        } else {
            if dst_path.symlink_metadata().is_ok() {
                if dst_path.is_dir() {
                    fs::remove_dir_all(&dst_path)?;
                } else {
                    fs::remove_file(&dst_path)?;
                }
            }
            fs::copy(&src_path, &dst_path).with_context(|| {
                format!("copy {} -> {}", src_path.display(), dst_path.display())
            })?;
        }
    }
    Ok(())
}

/// Execute `cmd` with a hard wall-clock timeout, capturing `stdout` and `stderr` into temporary
/// files inside `log_dir` so large outputs cannot deadlock OS pipe buffers.
pub fn run_command_with_timeout(
    cmd: &mut Command,
    timeout_secs: u64,
    log_dir: &Path,
    log_prefix: &str,
) -> Result<CommandRunResult> {
    fs::create_dir_all(log_dir)
        .with_context(|| format!("create log directory {}", log_dir.display()))?;
    let stdout_path = log_dir.join(format!("{log_prefix}.stdout"));
    let stderr_path = log_dir.join(format!("{log_prefix}.stderr"));

    let stdout_file =
        File::create(&stdout_path).with_context(|| format!("create {}", stdout_path.display()))?;
    let stderr_file =
        File::create(&stderr_path).with_context(|| format!("create {}", stderr_path.display()))?;

    cmd.stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));

    let started = Instant::now();
    let mut child = cmd.spawn().context("spawn child process")?;
    let timeout = Duration::from_secs(timeout_secs.max(1));

    let (exit_code, timed_out) = match child
        .wait_timeout(timeout)
        .context("wait_timeout on child")?
    {
        Some(status) => (status.code().unwrap_or(-1), false),
        None => {
            let _ = child.kill();
            let _ = child.wait();
            (-1, true)
        }
    };

    let wall_duration_ms = started.elapsed().as_millis() as u64;
    let stdout = fs::read(&stdout_path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let stderr = fs::read(&stderr_path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();

    Ok(CommandRunResult {
        exit_code,
        timed_out,
        wall_duration_ms,
        stdout,
        stderr,
    })
}

fn run_git(repo_dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("invoke git {args:?} in {}", repo_dir.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "git {:?} failed in {} (status {:?}): {}",
            args,
            repo_dir.display(),
            output.status.code(),
            stderr.trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crucible_core::{EpisodeRole, EpisodeSpec, ScenarioCategory, VerifierSpec};

    use super::*;

    fn write_fixture_scenario(base: &Path) -> ScenarioSpec {
        let ws = base.join("workspace");
        let ov = base.join("verifier");
        let gp = base.join("gold_patch");
        fs::create_dir_all(&ws).unwrap();
        fs::create_dir_all(&ov).unwrap();
        fs::create_dir_all(&gp).unwrap();

        fs::write(ws.join("answer.txt"), "wrong\n").unwrap();
        fs::write(
            ov.join("check_secret.sh"),
            "#!/bin/sh\ngrep -qx 'forty-two' answer.txt\n",
        )
        .unwrap();
        fs::write(gp.join("answer.txt"), "forty-two\n").unwrap();

        ScenarioSpec {
            root: base.to_path_buf(),
            id: "demo-fixture".into(),
            title: "Demo Fixture".into(),
            category: ScenarioCategory::Handoff,
            description: "Test scenario".into(),
            scope: "project:demo".into(),
            context: BTreeMap::new(),
            noise_distractor_count: 4,
            memories: Vec::new(),
            episodes: vec![EpisodeSpec {
                id: "target".into(),
                role: EpisodeRole::Target,
                workspace_subdir: "workspace".into(),
                prompt: "Fix answer.txt".into(),
                timeout_secs: 10,
            }],
            verifier: VerifierSpec {
                command: vec!["sh".into(), "check_secret.sh".into()],
                overlay_dir: Some("verifier".into()),
                forbidden_tool_substrings: vec!["check_secret".into()],
                forbidden_diff_substrings: vec!["CHEAT_TOKEN".into()],
                forbidden_paths: vec![".poisoned".into()],
                timeout_secs: 5,
            },
            gold_patch_dir: "gold_patch".into(),
        }
    }

    #[test]
    fn sandbox_isolates_overlay_and_gold_patch_during_agent_phase() {
        let dir = TempDir::new().unwrap();
        let scenario = write_fixture_scenario(dir.path());

        let mut sb = Sandbox::stage_scenario_episode(&scenario, "workspace").unwrap();
        sb.init_git_commit().unwrap();

        // Neither verifier overlay nor gold_patch may exist inside the agent workspace!
        assert!(!sb.repo_dir().join("check_secret.sh").exists());
        assert!(!sb.repo_dir().join("verifier").exists());
        assert!(!sb.repo_dir().join("gold_patch").exists());
        assert_eq!(
            fs::read_to_string(sb.repo_dir().join("answer.txt")).unwrap(),
            "wrong\n"
        );
    }

    #[test]
    fn verify_scenario_integrity_checks_unmodified_fails_and_gold_passes() {
        let dir = TempDir::new().unwrap();
        let scenario = write_fixture_scenario(dir.path());
        verify_scenario_integrity(&scenario).unwrap();

        // If workspace already contains the winning answer, integrity check must bail!
        fs::write(dir.path().join("workspace/answer.txt"), "forty-two\n").unwrap();
        let err = verify_scenario_integrity(&scenario).unwrap_err();
        assert!(
            err.to_string()
                .contains("unmodified workspace unexpectedly passed verifier")
        );
    }

    #[test]
    fn verifier_catches_forbidden_diff_path_and_tool_violations() {
        let dir = TempDir::new().unwrap();
        let scenario = write_fixture_scenario(dir.path());

        let mut sb = Sandbox::stage_scenario_episode(&scenario, "workspace").unwrap();
        sb.init_git_commit().unwrap();

        // Write valid answer so check_secret.sh exits 0, BUT trigger all 3 violation classes.
        fs::write(sb.repo_dir().join("answer.txt"), "forty-two\n").unwrap();
        fs::write(sb.repo_dir().join("notes.txt"), "used CHEAT_TOKEN here\n").unwrap();
        fs::write(sb.repo_dir().join(".poisoned"), "boom\n").unwrap();

        let trace = vec![ToolCallSummary {
            name: "bash".into(),
            arguments_preview: "cat ../verifier/check_secret.sh".into(),
            is_mcp: false,
        }];

        let (outcome, diff) = sb.verify_with_diff(&scenario, &trace).unwrap();
        assert_eq!(outcome.exit_code, 0);
        assert!(!outcome.passed);
        assert_eq!(outcome.violations.len(), 3);
        assert!(diff.files_modified >= 2);
        assert!(diff.lines_added >= 2);
    }

    #[test]
    fn verifier_enforces_command_timeout() {
        let dir = TempDir::new().unwrap();
        let mut scenario = write_fixture_scenario(dir.path());
        scenario.verifier.command = vec!["sh".into(), "-c".into(), "sleep 5".into()];
        scenario.verifier.timeout_secs = 1;

        let mut sb = Sandbox::stage_scenario_episode(&scenario, "workspace").unwrap();
        sb.init_git_commit().unwrap();

        let outcome = sb.verify(&scenario, &[]).unwrap();
        assert!(!outcome.passed);
        assert!(
            outcome
                .violations
                .iter()
                .any(|v| v.contains("timed out after 1s"))
        );
    }
}
