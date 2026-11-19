//! Hermetic agent drivers: [`CopilotDriver`], [`CommandDriver`], and [`ScriptedDriver`].

use std::collections::{BTreeSet, HashSet};
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use anyhow::{Context as _, Result};
use crucible_core::{ArmKind, ArmSpec, EpisodeRole, EpisodeSpec, ToolCallSummary, UsageMetrics};
use serde_json::Value;

use crate::sandbox::{copy_dir_all, run_command_with_timeout};

/// Input specification for executing a single episode against a staged workspace.
#[derive(Clone, Debug)]
pub struct EpisodeRunRequest {
    pub scenario_id: String,
    pub episode: EpisodeSpec,
    pub arm: ArmSpec,
    pub seed: u32,
    pub workspace_dir: PathBuf,
    pub scratch_dir: PathBuf,
    pub prompt: String,
    pub mcp_config_path: Option<PathBuf>,
    pub copilot_settings_json: Option<String>,
    ///Scenario gold patch directory, inspected ONLY by offline [`ScriptedDriver`] in tests/dry-runs.
    pub gold_patch_source_dir: Option<PathBuf>,
    pub model: String,
    pub reasoning_effort: String,
}

/// Output captured from a single episode run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EpisodeRunOutput {
    pub exit_code: i32,
    pub timed_out: bool,
    pub usage: UsageMetrics,
    pub tool_trace: Vec<ToolCallSummary>,
    pub raw_stdout: String,
    pub raw_stderr: String,
}

/// Uniform interface implemented by live CLI agent harnesses and deterministic test drivers.
pub trait AgentDriver: fmt::Debug + Send + Sync {
    fn name(&self) -> &str;
    fn run_episode(&self, req: &EpisodeRunRequest) -> Result<EpisodeRunOutput>;
}

/// Parsed tool trace and fallback telemetry extracted from Copilot CLI `--output-format json` JSONL.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CopilotJsonlSummary {
    pub tool_trace: Vec<ToolCallSummary>,
    pub tool_calls: u64,
    pub mcp_tool_calls: u64,
    pub fallback_usage: Option<UsageMetrics>,
}

/// Hermetic GitHub Copilot CLI driver using a fresh per-episode `COPILOT_HOME`.
#[derive(Clone, Debug)]
pub struct CopilotDriver {
    pub copilot_bin: PathBuf,
    /// Optional host `.copilot` directory from which ONLY OAuth login pointers are copied.
    pub auth_source_home: Option<PathBuf>,
}

impl Default for CopilotDriver {
    fn default() -> Self {
        Self::new("copilot")
    }
}

impl CopilotDriver {
    pub fn new(copilot_bin: impl Into<PathBuf>) -> Self {
        let auth_source_home = std::env::var_os("COPILOT_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".copilot")));
        Self {
            copilot_bin: copilot_bin.into(),
            auth_source_home,
        }
    }

    pub fn without_host_auth_seed(mut self) -> Self {
        self.auth_source_home = None;
        self
    }

    fn prepare_hermetic_copilot_home(&self, req: &EpisodeRunRequest) -> Result<PathBuf> {
        let home = req.scratch_dir.join(format!(
            "copilot-home-{}",
            sanitize_file_component(&req.episode.id)
        ));
        fs::create_dir_all(&home)
            .with_context(|| format!("create hermetic COPILOT_HOME {}", home.display()))?;

        // Copy strictly OAuth account identity keys (if present) so OS credential-store lookup
        // succeeds while excluding all user plugins, hooks, trusted folders, and MCP configs.
        if let Some(src_home) = &self.auth_source_home {
            let src_cfg = src_home.join("config.json");
            if src_cfg.is_file()
                && let Ok(raw) = fs::read_to_string(&src_cfg)
                && let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&raw)
            {
                let mut minimal = serde_json::Map::new();
                for key in ["logged_in_users", "last_logged_in_user", "copilot_tokens"] {
                    if let Some(v) = map.get(key) {
                        minimal.insert(key.to_string(), v.clone());
                    }
                }
                if !minimal.is_empty() {
                    let dst_cfg = home.join("config.json");
                    let serialized = serde_json::to_string_pretty(&Value::Object(minimal))?;
                    fs::write(dst_cfg, format!("{serialized}\n"))?;
                }
            }
        }

        if let Some(settings_json) = &req.copilot_settings_json {
            fs::write(home.join("settings.json"), settings_json)
                .with_context(|| format!("write {}/settings.json", home.display()))?;
        }

        Ok(home)
    }
}

impl AgentDriver for CopilotDriver {
    fn name(&self) -> &str {
        "copilot"
    }

    fn run_episode(&self, req: &EpisodeRunRequest) -> Result<EpisodeRunOutput> {
        let copilot_home = self.prepare_hermetic_copilot_home(req)?;
        let ep_tag = sanitize_file_component(&req.episode.id);
        let usage_path = req.scratch_dir.join(format!("usage-{ep_tag}.json"));

        let mut cmd = Command::new(&self.copilot_bin);
        cmd.env("COPILOT_HOME", &copilot_home)
            .env("COPILOT_PLUGIN_DIR_ONLY", "true")
            .env("COPILOT_CUSTOM_INSTRUCTIONS_DIRS", "")
            .env("COPILOT_AUTO_UPDATE", "false")
            .arg("--model")
            .arg(&req.model)
            .arg("--reasoning-effort")
            .arg(&req.reasoning_effort)
            .arg("--context")
            .arg("default")
            .arg("-C")
            .arg(&req.workspace_dir)
            .arg("--disable-builtin-mcps")
            .arg("--sync-session=off")
            .arg("--no-auto-update")
            .arg("--disable-client-telemetry")
            .arg("--no-ask-user")
            .arg("--allow-all-tools")
            .arg("--allow-all-paths")
            .arg("--output-format")
            .arg("json")
            .arg("--usage-output-file")
            .arg(&usage_path);

        if let Some(mcp_cfg) = &req.mcp_config_path {
            cmd.arg("--additional-mcp-config")
                .arg(format!("@{}", mcp_cfg.display()))
                .arg("--allow-all-mcp-server-instructions");
        }

        cmd.arg("-p").arg(&req.prompt);

        let run = run_command_with_timeout(
            &mut cmd,
            req.episode.timeout_secs,
            &req.scratch_dir,
            &format!("copilot-{ep_tag}"),
        )
        .with_context(|| {
            format!(
                "run copilot CLI (`{}`) for scenario `{}` episode `{}`",
                self.copilot_bin.display(),
                req.scenario_id,
                req.episode.id
            )
        })?;

        let jsonl = parse_copilot_jsonl(&run.stdout);
        let mut usage = if usage_path.is_file() {
            let raw_usage = fs::read_to_string(&usage_path)
                .with_context(|| format!("read usage file {}", usage_path.display()))?;
            if raw_usage.trim().is_empty() {
                jsonl.fallback_usage.unwrap_or_default()
            } else {
                parse_copilot_usage_json(&raw_usage)
                    .with_context(|| format!("parse copilot usage JSON {}", usage_path.display()))?
            }
        } else {
            jsonl.fallback_usage.unwrap_or_default()
        };

        usage.wall_duration_ms = run.wall_duration_ms;
        usage.tool_calls = jsonl.tool_calls;
        usage.mcp_tool_calls = jsonl.mcp_tool_calls;

        Ok(EpisodeRunOutput {
            exit_code: run.exit_code,
            timed_out: run.timed_out,
            usage,
            tool_trace: jsonl.tool_trace,
            raw_stdout: run.stdout,
            raw_stderr: run.stderr,
        })
    }
}

/// Generic subprocess agent driver for arbitrary external agent commands.
#[derive(Clone, Debug)]
pub struct CommandDriver {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl CommandDriver {
    pub fn new(program: impl Into<PathBuf>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
        }
    }
}

impl AgentDriver for CommandDriver {
    fn name(&self) -> &str {
        "command"
    }

    fn run_episode(&self, req: &EpisodeRunRequest) -> Result<EpisodeRunOutput> {
        let ep_tag = sanitize_file_component(&req.episode.id);
        let prompt_file = req.scratch_dir.join(format!("prompt-{ep_tag}.txt"));
        let usage_file = req.scratch_dir.join(format!("cmd-usage-{ep_tag}.json"));
        fs::write(&prompt_file, &req.prompt)?;

        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args)
            .current_dir(&req.workspace_dir)
            .env("CRUCIBLE_SCENARIO_ID", &req.scenario_id)
            .env("CRUCIBLE_EPISODE_ID", &req.episode.id)
            .env("CRUCIBLE_ARM_ID", &req.arm.id)
            .env("CRUCIBLE_SEED", req.seed.to_string())
            .env("CRUCIBLE_WORKSPACE_DIR", &req.workspace_dir)
            .env("CRUCIBLE_SCRATCH_DIR", &req.scratch_dir)
            .env("CRUCIBLE_PROMPT_FILE", &prompt_file)
            .env("CRUCIBLE_USAGE_OUT_FILE", &usage_file)
            .env("CRUCIBLE_MODEL", &req.model)
            .env("CRUCIBLE_REASONING_EFFORT", &req.reasoning_effort);

        if let Some(mcp_cfg) = &req.mcp_config_path {
            cmd.env("CRUCIBLE_MCP_CONFIG", mcp_cfg);
        }

        let run = run_command_with_timeout(
            &mut cmd,
            req.episode.timeout_secs,
            &req.scratch_dir,
            &format!("command-{ep_tag}"),
        )
        .with_context(|| format!("run command driver `{}`", self.program.display()))?;

        let jsonl = parse_copilot_jsonl(&run.stdout);
        let mut usage = if usage_file.is_file() {
            let raw = fs::read_to_string(&usage_file)?;
            serde_json::from_str::<UsageMetrics>(&raw)
                .or_else(|_| parse_copilot_usage_json(&raw))
                .unwrap_or_default()
        } else {
            jsonl.fallback_usage.unwrap_or_default()
        };

        usage.wall_duration_ms = run.wall_duration_ms;
        if usage.tool_calls == 0 {
            usage.tool_calls = jsonl.tool_calls;
        }
        if usage.mcp_tool_calls == 0 {
            usage.mcp_tool_calls = jsonl.mcp_tool_calls;
        }

        Ok(EpisodeRunOutput {
            exit_code: run.exit_code,
            timed_out: run.timed_out,
            usage,
            tool_trace: jsonl.tool_trace,
            raw_stdout: run.stdout,
            raw_stderr: run.stderr,
        })
    }
}

type ArmPassPredicate = Arc<dyn Fn(&EpisodeRunRequest) -> bool + Send + Sync>;

/// Deterministic offline driver for integration tests, CI validation, and dry-runs.
#[derive(Clone)]
pub struct ScriptedDriver {
    label: String,
    should_apply_gold: ArmPassPredicate,
}

impl fmt::Debug for ScriptedDriver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScriptedDriver")
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

impl Default for ScriptedDriver {
    fn default() -> Self {
        Self {
            label: "scripted".into(),
            should_apply_gold: Arc::new(|req| {
                matches!(
                    req.arm.kind,
                    ArmKind::HivemindMcp { .. } | ArmKind::OracleNotes
                )
            }),
        }
    }
}

impl ScriptedDriver {
    /// Construct a scripted driver that applies `gold_patch` on `Target` episodes whenever
    /// `arm.id` is in `passing_arm_ids`.
    pub fn pass_arms<I, S>(passing_arm_ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let allowed: BTreeSet<String> = passing_arm_ids.into_iter().map(Into::into).collect();
        Self {
            label: "scripted".into(),
            should_apply_gold: Arc::new(move |req| allowed.contains(&req.arm.id)),
        }
    }

    /// Construct a scripted driver with a custom predicate deciding whether `gold_patch` applies.
    pub fn with_predicate<F>(predicate: F) -> Self
    where
        F: Fn(&EpisodeRunRequest) -> bool + Send + Sync + 'static,
    {
        Self {
            label: "scripted".into(),
            should_apply_gold: Arc::new(predicate),
        }
    }
}

impl AgentDriver for ScriptedDriver {
    fn name(&self) -> &str {
        &self.label
    }

    fn run_episode(&self, req: &EpisodeRunRequest) -> Result<EpisodeRunOutput> {
        let mut tool_trace = Vec::new();
        let (uncached_in, cache_read, cache_write, output_toks, wall_ms): (
            u64,
            u64,
            u64,
            u64,
            u64,
        ) = match &req.arm.kind {
            ArmKind::NoMemory => {
                tool_trace.push(ToolCallSummary {
                    name: "grep".into(),
                    arguments_preview: "{\"pattern\":\"retry\"}".into(),
                    is_mcp: false,
                });
                tool_trace.push(ToolCallSummary {
                    name: "view".into(),
                    arguments_preview: "{\"path\":\"src/lib.rs\"}".into(),
                    is_mcp: false,
                });
                (6_400, 18_000, 4_200, 950, 420)
            }
            ArmKind::StaticNotes { file_name } => {
                if req.episode.role == EpisodeRole::History {
                    let notes_path = req.workspace_dir.join(file_name);
                    let mut existing = fs::read_to_string(&notes_path).unwrap_or_default();
                    existing.push_str(&format!(
                        "- [handoff] Earned note recorded during history episode `{}`.\n",
                        req.episode.id
                    ));
                    fs::write(&notes_path, existing)?;
                }
                tool_trace.push(ToolCallSummary {
                    name: "view".into(),
                    arguments_preview: "{\"path\":\"src/lib.rs\"}".into(),
                    is_mcp: false,
                });
                (9_800, 28_000, 8_500, 820, 360)
            }
            ArmKind::MarkdownTree { dir_name } => {
                if req.episode.role == EpisodeRole::History {
                    let earned_file = req.workspace_dir.join(dir_name).join("earned.md");
                    if let Some(parent) = earned_file.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(
                        &earned_file,
                        format!(
                            "# Earned Notes\n\n- History episode `{}` note.\n",
                            req.episode.id
                        ),
                    )?;
                }
                tool_trace.push(ToolCallSummary {
                    name: "grep".into(),
                    arguments_preview: format!("{{\"paths\":\"{dir_name}\"}}"),
                    is_mcp: false,
                });
                (5_200, 15_000, 3_900, 780, 330)
            }
            ArmKind::HivemindMcp { .. } => {
                let mcp_tool = match req.episode.role {
                    EpisodeRole::History => "hivemind-remember",
                    EpisodeRole::Target => "hivemind-recall",
                };
                tool_trace.push(ToolCallSummary {
                    name: mcp_tool.into(),
                    arguments_preview: format!("{{\"query\":\"{}\"}}", req.scenario_id),
                    is_mcp: true,
                });
                tool_trace.push(ToolCallSummary {
                    name: "edit".into(),
                    arguments_preview: "{\"path\":\"workspace\"}".into(),
                    is_mcp: false,
                });
                (3_100, 9_500, 3_400, 540, 240)
            }
            ArmKind::OracleNotes => {
                tool_trace.push(ToolCallSummary {
                    name: "edit".into(),
                    arguments_preview: "{\"path\":\"workspace\"}".into(),
                    is_mcp: false,
                });
                (2_600, 7_200, 3_000, 460, 190)
            }
        };

        if req.episode.role == EpisodeRole::Target
            && (self.should_apply_gold)(req)
            && let Some(gold_dir) = &req.gold_patch_source_dir
            && gold_dir.is_dir()
        {
            copy_dir_all(gold_dir, &req.workspace_dir)?;
        }

        let seed_jitter = u64::from(req.seed) * 37;
        let uncached_input_tokens = uncached_in + seed_jitter;
        let total_input_tokens = uncached_input_tokens + cache_read + cache_write;
        let tool_calls = tool_trace.len() as u64;
        let mcp_tool_calls = tool_trace.iter().filter(|t| t.is_mcp).count() as u64;

        let usage = UsageMetrics {
            api_requests: tool_calls + 1,
            uncached_input_tokens,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            total_input_tokens,
            output_tokens: output_toks,
            reasoning_tokens: output_toks / 4,
            nano_aiu: (total_input_tokens + output_toks * 5) * 1_000,
            api_duration_ms: wall_ms.saturating_sub(40),
            wall_duration_ms: wall_ms,
            tool_calls,
            mcp_tool_calls,
            lines_added: 0,
            lines_removed: 0,
            files_modified: 0,
        };

        Ok(EpisodeRunOutput {
            exit_code: 0,
            timed_out: false,
            usage,
            tool_trace,
            raw_stdout: String::new(),
            raw_stderr: String::new(),
        })
    }
}

/// Parse Copilot CLI `--usage-output-file` JSON (both direct and `accountingSnapshot`-wrapped).
pub fn parse_copilot_usage_json(raw: &str) -> Result<UsageMetrics> {
    let parsed: Value = serde_json::from_str(raw).context("deserialize copilot usage JSON")?;
    Ok(extract_usage_snapshot(&parsed))
}

fn extract_usage_snapshot(parsed: &Value) -> UsageMetrics {
    let root = parsed.get("accountingSnapshot").unwrap_or(parsed);

    let td_input = root
        .pointer("/tokenDetails/input/tokenCount")
        .and_then(json_u64);
    let td_cache_read = root
        .pointer("/tokenDetails/cache_read/tokenCount")
        .and_then(json_u64);
    let td_cache_write = root
        .pointer("/tokenDetails/cache_write/tokenCount")
        .and_then(json_u64);
    let td_output = root
        .pointer("/tokenDetails/output/tokenCount")
        .and_then(json_u64);

    let mut api_requests: u64 = 0;
    let mut mm_input_tokens: u64 = 0;
    let mut mm_output_tokens: u64 = 0;
    let mut mm_cache_read: u64 = 0;
    let mut mm_cache_write: u64 = 0;
    let mut reasoning_tokens: u64 = 0;

    if let Some(models) = root.get("modelMetrics").and_then(Value::as_object) {
        for entry in models.values() {
            api_requests += entry
                .pointer("/requests/count")
                .and_then(json_u64)
                .unwrap_or(0);
            mm_input_tokens += entry
                .pointer("/usage/inputTokens")
                .and_then(json_u64)
                .unwrap_or(0);
            mm_output_tokens += entry
                .pointer("/usage/outputTokens")
                .and_then(json_u64)
                .unwrap_or(0);
            mm_cache_read += entry
                .pointer("/usage/cacheReadTokens")
                .and_then(json_u64)
                .unwrap_or(0);
            mm_cache_write += entry
                .pointer("/usage/cacheWriteTokens")
                .and_then(json_u64)
                .unwrap_or(0);
            reasoning_tokens += entry
                .pointer("/usage/reasoningTokens")
                .and_then(json_u64)
                .unwrap_or(0);
        }
    }

    let cache_read_tokens = td_cache_read.unwrap_or(mm_cache_read);
    let cache_write_tokens = td_cache_write.unwrap_or(mm_cache_write);
    let output_tokens = td_output.unwrap_or(mm_output_tokens);

    let uncached_input_tokens = match td_input {
        Some(v) => v,
        None => mm_input_tokens.saturating_sub(cache_read_tokens + cache_write_tokens),
    };

    let sum_components = uncached_input_tokens + cache_read_tokens + cache_write_tokens;
    let total_input_tokens = if mm_input_tokens > 0 {
        mm_input_tokens.max(sum_components)
    } else {
        sum_components
    };

    let nano_aiu = root.get("totalNanoAiu").and_then(json_u64).unwrap_or(0);
    let api_duration_ms = root
        .get("totalApiDurationMs")
        .and_then(json_u64)
        .unwrap_or(0);

    let lines_added = root
        .pointer("/codeChanges/linesAdded")
        .and_then(json_u64)
        .unwrap_or(0);
    let lines_removed = root
        .pointer("/codeChanges/linesRemoved")
        .and_then(json_u64)
        .unwrap_or(0);
    let files_modified = root
        .pointer("/codeChanges/filesModifiedCount")
        .and_then(json_u64)
        .or_else(|| {
            root.pointer("/codeChanges/filesModified")
                .and_then(Value::as_array)
                .map(|a| a.len() as u64)
        })
        .unwrap_or(0);

    UsageMetrics {
        api_requests,
        uncached_input_tokens,
        cache_read_tokens,
        cache_write_tokens,
        total_input_tokens,
        output_tokens,
        reasoning_tokens,
        nano_aiu,
        api_duration_ms,
        wall_duration_ms: 0,
        tool_calls: 0,
        mcp_tool_calls: 0,
        lines_added,
        lines_removed,
        files_modified,
    }
}

/// Parse Copilot CLI `--output-format json` JSONL lines into a deduplicated tool trace and counts,
/// tolerating malformed or truncated lines.
pub fn parse_copilot_jsonl(raw_jsonl: &str) -> CopilotJsonlSummary {
    let mut tool_trace = Vec::new();
    let mut seen_call_ids: HashSet<String> = HashSet::new();
    let mut shutdown_usage: Option<UsageMetrics> = None;
    let mut streamed_usage = UsageMetrics::default();
    let mut saw_streamed_usage = false;

    for raw_line in raw_jsonl.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };

        let Some(event_type) = event.get("type").and_then(Value::as_str) else {
            continue;
        };

        match event_type {
            "assistant.message" => {
                if let Some(reqs) = event
                    .pointer("/data/toolRequests")
                    .and_then(Value::as_array)
                {
                    for tr in reqs {
                        let call_id = tr
                            .get("toolCallId")
                            .or_else(|| tr.get("id"))
                            .and_then(Value::as_str);
                        if let Some(cid) = call_id
                            && !cid.is_empty()
                            && !seen_call_ids.insert(cid.to_string())
                        {
                            continue;
                        }
                        let Some(name) = tr
                            .get("name")
                            .or_else(|| tr.get("toolName"))
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                        else {
                            continue;
                        };
                        let arguments_preview = format_arguments_preview(tr.get("arguments"));
                        let is_mcp = is_mcp_tool(name, tr.get("mcpServerName"));
                        tool_trace.push(ToolCallSummary {
                            name: name.to_string(),
                            arguments_preview,
                            is_mcp,
                        });
                    }
                }
            }
            "tool.execution_start" => {
                if let Some(data) = event.get("data") {
                    let call_id = data
                        .get("toolCallId")
                        .or_else(|| data.get("id"))
                        .and_then(Value::as_str);
                    if let Some(cid) = call_id
                        && !cid.is_empty()
                        && !seen_call_ids.insert(cid.to_string())
                    {
                        continue;
                    }
                    let Some(name) = data
                        .get("toolName")
                        .or_else(|| data.get("name"))
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    else {
                        continue;
                    };
                    let arguments_preview = format_arguments_preview(data.get("arguments"));
                    let is_mcp = is_mcp_tool(name, data.get("mcpServerName"));
                    tool_trace.push(ToolCallSummary {
                        name: name.to_string(),
                        arguments_preview,
                        is_mcp,
                    });
                }
            }
            "session.shutdown" => {
                if let Some(data) = event.get("data") {
                    shutdown_usage = Some(extract_usage_snapshot(data));
                }
            }
            "session.usage_record" => {
                if let Some(u) = event.pointer("/data/usage") {
                    saw_streamed_usage = true;
                    let total_in = u.get("inputTokens").and_then(json_u64).unwrap_or(0);
                    let cache_read = u.get("cacheReadTokens").and_then(json_u64).unwrap_or(0);
                    let cache_write = u.get("cacheWriteTokens").and_then(json_u64).unwrap_or(0);
                    let uncached = total_in.saturating_sub(cache_read + cache_write);
                    streamed_usage.api_requests += 1;
                    streamed_usage.total_input_tokens += total_in;
                    streamed_usage.uncached_input_tokens += uncached;
                    streamed_usage.cache_read_tokens += cache_read;
                    streamed_usage.cache_write_tokens += cache_write;
                    streamed_usage.output_tokens +=
                        u.get("outputTokens").and_then(json_u64).unwrap_or(0);
                    streamed_usage.reasoning_tokens +=
                        u.get("reasoningTokens").and_then(json_u64).unwrap_or(0);
                    streamed_usage.api_duration_ms +=
                        u.get("duration").and_then(json_u64).unwrap_or(0);
                }
            }
            _ => {}
        }
    }

    let tool_calls = tool_trace.len() as u64;
    let mcp_tool_calls = tool_trace.iter().filter(|t| t.is_mcp).count() as u64;
    let fallback_usage = shutdown_usage.or({
        if saw_streamed_usage {
            Some(streamed_usage)
        } else {
            None
        }
    });

    CopilotJsonlSummary {
        tool_trace,
        tool_calls,
        mcp_tool_calls,
        fallback_usage,
    }
}

fn is_mcp_tool(name: &str, mcp_server_field: Option<&Value>) -> bool {
    name.starts_with("hivemind")
        || mcp_server_field
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
}

fn format_arguments_preview(val: Option<&Value>) -> String {
    const MAX_PREVIEW_CHARS: usize = 4096;
    let rendered = match val {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => serde_json::to_string(other).unwrap_or_default(),
    };
    truncate_utf8(&rendered, MAX_PREVIEW_CHARS)
}

fn truncate_utf8(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => format!("{}...", &s[..byte_idx]),
        None => s.to_string(),
    }
}

fn json_u64(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| {
        v.as_f64()
            .filter(|f| f.is_finite() && *f >= 0.0)
            .map(|f| f.round() as u64)
    })
}

fn sanitize_file_component(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "episode".into()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn parses_real_copilot_usage_json_including_float_nano_aiu_and_multi_model_metrics() {
        let sample = r#"{
          "totalPremiumRequestCost": 2.0,
          "totalUserRequests": 1,
          "totalNanoAiu": 69326140000.0,
          "aiCreditsStatus": "complete",
          "tokenDetails": {
            "input": { "tokenCount": 420 },
            "cache_read": { "tokenCount": 125000 },
            "cache_write": { "tokenCount": 18500 },
            "output": { "tokenCount": 1430 }
          },
          "totalApiDurationMs": 14820,
          "codeChanges": {
            "linesAdded": 19,
            "linesRemoved": 4,
            "filesModifiedCount": 2,
            "filesModified": ["src/lib.rs", "src/retry.rs"]
          },
          "modelMetrics": {
            "claude-sonnet-5.5": {
              "requests": { "count": 4, "cost": 2.0 },
              "usage": {
                "inputTokens": 140000,
                "outputTokens": 1300,
                "cacheReadTokens": 122000,
                "cacheWriteTokens": 17600,
                "reasoningTokens": 310
              }
            },
            "claude-haiku-5.5": {
              "requests": { "count": 1, "cost": 0.0 },
              "usage": {
                "inputTokens": 3920,
                "outputTokens": 130,
                "cacheReadTokens": 3000,
                "cacheWriteTokens": 900,
                "reasoningTokens": 0
              }
            }
          }
        }"#;

        let u = parse_copilot_usage_json(sample).unwrap();
        assert_eq!(u.api_requests, 5);
        assert_eq!(u.uncached_input_tokens, 420);
        assert_eq!(u.cache_read_tokens, 125_000);
        assert_eq!(u.cache_write_tokens, 18_500);
        assert_eq!(u.total_input_tokens, 143_920);
        assert_eq!(u.output_tokens, 1_430);
        assert_eq!(u.reasoning_tokens, 310);
        assert_eq!(u.nano_aiu, 69_326_140_000);
        assert_eq!(u.api_duration_ms, 14_820);
        assert_eq!(u.lines_added, 19);
        assert_eq!(u.lines_removed, 4);
        assert_eq!(u.files_modified, 2);
    }

    #[test]
    fn parses_copilot_jsonl_deduplicating_call_ids_and_ignoring_truncated_lines() {
        let sample_jsonl = concat!(
            "{\"type\":\"assistant.message\",\"data\":{\"toolRequests\":[",
            "{\"toolCallId\":\"call_1\",\"name\":\"hivemind-recall\",\"arguments\":{\"query\":\"retry backoff\"},\"mcpServerName\":\"hivemind\"},",
            "{\"toolCallId\":\"call_2\",\"name\":\"view\",\"arguments\":{\"path\":\"src/lib.rs\"}}",
            "]}}\n",
            "{\"type\":\"tool.execution_start\",\"data\":{\"toolCallId\":\"call_1\",\"toolName\":\"hivemind-recall\",\"arguments\":{\"query\":\"retry backoff\"}}}\n",
            "{\"type\":\"tool.execution_start\",\"data\":{\"toolCallId\":\"call_3\",\"toolName\":\"edit\",\"arguments\":{\"path\":\"src/lib.rs\"}}}\n",
            "{\"type\":\"assistant.message\",\"truncated_json...\n",
            "\n"
        );

        let summary = parse_copilot_jsonl(sample_jsonl);
        assert_eq!(summary.tool_calls, 3);
        assert_eq!(summary.mcp_tool_calls, 1);
        assert_eq!(summary.tool_trace[0].name, "hivemind-recall");
        assert!(summary.tool_trace[0].is_mcp);
        assert!(
            summary.tool_trace[0]
                .arguments_preview
                .contains("retry backoff")
        );
        assert_eq!(summary.tool_trace[1].name, "view");
        assert!(!summary.tool_trace[1].is_mcp);
        assert_eq!(summary.tool_trace[2].name, "edit");
    }

    #[cfg(unix)]
    #[test]
    fn copilot_driver_invokes_hermetic_flags_and_parses_outputs() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = TempDir::new().unwrap();
        let ws = dir.path().join("ws");
        let scratch = dir.path().join("scratch");
        fs::create_dir_all(&ws).unwrap();
        fs::create_dir_all(&scratch).unwrap();

        let mcp_cfg = scratch.join("mcp-config.json");
        fs::write(&mcp_cfg, "{\"mcpServers\":{}}\n").unwrap();

        let captured_args = dir.path().join("args.txt");
        let captured_home = dir.path().join("copilot_home.txt");
        let mock_copilot = dir.path().join("mock-copilot.sh");
        let script = format!(
            r#"#!/bin/sh
printf '%s\n' "$@" > "{args_out}"
printf '%s' "$COPILOT_HOME" > "{home_out}"
usage_out=""
prev=""
for arg in "$@"; do
  if [ "$prev" = "--usage-output-file" ]; then
    usage_out="$arg"
  fi
  prev="$arg"
done
cat <<'EOF' > "$usage_out"
{{"totalNanoAiu":12000000,"totalApiDurationMs":450,"tokenDetails":{{"input":{{"tokenCount":100}},"cache_read":{{"tokenCount":900}},"cache_write":{{"tokenCount":200}},"output":{{"tokenCount":50}}}},"codeChanges":{{"linesAdded":3,"linesRemoved":1,"filesModifiedCount":1}},"modelMetrics":{{"claude-sonnet-5.5":{{"requests":{{"count":2}},"usage":{{"inputTokens":1200,"outputTokens":50,"reasoningTokens":10}}}}}}}}
EOF
printf '%s\n' '{{"type":"assistant.message","data":{{"toolRequests":[{{"toolCallId":"c1","name":"hivemind-recall","arguments":{{"query":"timeout"}}}}]}}}}'
"#,
            args_out = captured_args.display(),
            home_out = captured_home.display(),
        );
        fs::write(&mock_copilot, script).unwrap();
        fs::set_permissions(&mock_copilot, fs::Permissions::from_mode(0o755)).unwrap();

        let driver = CopilotDriver::new(&mock_copilot).without_host_auth_seed();
        let req = EpisodeRunRequest {
            scenario_id: "scen-1".into(),
            episode: EpisodeSpec {
                id: "target".into(),
                role: EpisodeRole::Target,
                workspace_subdir: "workspace".into(),
                prompt: "Solve the bug.".into(),
                timeout_secs: 15,
            },
            arm: ArmSpec::b1_notes(),
            seed: 0,
            workspace_dir: ws,
            scratch_dir: scratch.clone(),
            prompt: "Solve the bug.".into(),
            mcp_config_path: Some(mcp_cfg.clone()),
            copilot_settings_json: None,
            gold_patch_source_dir: None,
            model: "claude-sonnet-5.5".into(),
            reasoning_effort: "medium".into(),
        };

        let out = driver.run_episode(&req).unwrap();
        assert_eq!(out.exit_code, 0);
        assert!(!out.timed_out);
        assert_eq!(out.usage.api_requests, 2);
        assert_eq!(out.usage.uncached_input_tokens, 100);
        assert_eq!(out.usage.cache_read_tokens, 900);
        assert_eq!(out.usage.total_input_tokens, 1200);
        assert_eq!(out.usage.tool_calls, 1);
        assert_eq!(out.usage.mcp_tool_calls, 1);

        let args_text = fs::read_to_string(&captured_args).unwrap();
        assert!(args_text.contains("--disable-builtin-mcps"));
        assert!(args_text.contains("--sync-session=off"));
        assert!(args_text.contains("--allow-all-mcp-server-instructions"));
        assert!(args_text.contains(&format!("@{}", mcp_cfg.display())));
        // Must NOT pass --no-custom-instructions so workspace AGENTS.md loads natively!
        assert!(!args_text.contains("--no-custom-instructions"));

        let used_home = PathBuf::from(fs::read_to_string(&captured_home).unwrap());
        assert!(used_home.starts_with(&scratch));
    }
}
