//! Experimental arm setup, deterministic distractor generation, and isolated `hivemind`
//! store lifecycle seeding and teardown.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
use crucible_core::{
    ArmKind, ArmSpec, EpisodeRole, EpisodeSpec, MemoryMode, MemoryOp, ScenarioSpec, SeedMemory,
};
use serde_json::json;

use crate::sandbox::copy_dir_all;

/// Per-episode workspace and prompt configuration produced by [`PreparedTrialMemory`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpisodeMemoryContext {
    pub prompt: String,
    pub mcp_config_path: Option<PathBuf>,
    pub copilot_settings_json: Option<String>,
}

/// RAII guard that shuts down an isolated `hivemindd` daemon via `<store>/run/hivemindd.sock`
/// before its enclosing temporary directory is deleted.
#[derive(Debug)]
pub struct HivemindStoreGuard {
    store_dir: PathBuf,
    active: bool,
}

impl HivemindStoreGuard {
    pub fn new(store_dir: PathBuf) -> Self {
        Self {
            store_dir,
            active: true,
        }
    }

    pub fn store_dir(&self) -> &Path {
        &self.store_dir
    }

    /// Explicitly request graceful `hivemindd` shutdown over `<store>/run/hivemindd.sock`.
    pub fn shutdown_now(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        shutdown_hivemind_daemon(&self.store_dir);
    }
}

impl Drop for HivemindStoreGuard {
    fn drop(&mut self) {
        self.shutdown_now();
    }
}

#[derive(Debug)]
enum ArmRuntimeState {
    NoMemory,
    StaticNotes {
        file_name: String,
        current_content: String,
        run_history: bool,
    },
    MarkdownTree {
        dir_name: String,
        staged_tree_dir: PathBuf,
        run_history: bool,
    },
    OracleNotes {
        gold_statements: Vec<String>,
    },
    HivemindMcp {
        hivemind_bin: PathBuf,
        mcp_config_path: PathBuf,
        use_applicability: bool,
        run_history: bool,
        claim_id_map: BTreeMap<String, String>,
        guard: HivemindStoreGuard,
    },
}

/// Prepared memory state across optional `History` episodes and the scored `Target` episode
/// for a single `(scenario, arm, seed)` trial.
#[derive(Debug)]
pub struct PreparedTrialMemory {
    scenario_scope: String,
    scenario_context: BTreeMap<String, String>,
    state: ArmRuntimeState,
}

impl PreparedTrialMemory {
    /// Whether `History` episodes should be executed prior to `Target` for this trial.
    pub fn should_run_history(&self) -> bool {
        match &self.state {
            ArmRuntimeState::NoMemory | ArmRuntimeState::OracleNotes { .. } => false,
            ArmRuntimeState::StaticNotes { run_history, .. }
            | ArmRuntimeState::MarkdownTree { run_history, .. }
            | ArmRuntimeState::HivemindMcp { run_history, .. } => *run_history,
        }
    }

    /// Logical-to-physical (`clm_...`) claim ID map when seeded into `hivemind`.
    pub fn hivemind_claim_ids(&self) -> Option<&BTreeMap<String, String>> {
        match &self.state {
            ArmRuntimeState::HivemindMcp { claim_id_map, .. } => Some(claim_id_map),
            _ => None,
        }
    }

    /// Populate any pre-commit workspace memory files (`AGENTS.md`, `.memory/`) inside `repo_dir`
    /// and construct the arm-adjusted prompt and optional MCP config path for `episode`.
    pub fn prepare_episode_workspace(
        &self,
        episode: &EpisodeSpec,
        repo_dir: &Path,
    ) -> Result<EpisodeMemoryContext> {
        match &self.state {
            ArmRuntimeState::NoMemory => Ok(EpisodeMemoryContext {
                prompt: episode.prompt.clone(),
                mcp_config_path: None,
                copilot_settings_json: None,
            }),
            ArmRuntimeState::StaticNotes {
                file_name,
                current_content,
                ..
            } => {
                let notes_path = repo_dir.join(file_name);
                if let Some(parent) = notes_path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&notes_path, current_content)
                    .with_context(|| format!("write static notes {}", notes_path.display()))?;

                let prompt = match episode.role {
                    EpisodeRole::History => format!(
                        "{}\n\nAfter solving this task, update `{file_name}` in the workspace root \
with concise bullet notes of verified root causes, architectural invariants, and gotchas for \
future sessions.",
                        episode.prompt.trim_end()
                    ),
                    EpisodeRole::Target => episode.prompt.clone(),
                };
                Ok(EpisodeMemoryContext {
                    prompt,
                    mcp_config_path: None,
                    copilot_settings_json: None,
                })
            }
            ArmRuntimeState::MarkdownTree {
                dir_name,
                staged_tree_dir,
                ..
            } => {
                let dst_tree = repo_dir.join(dir_name);
                copy_dir_all(staged_tree_dir, &dst_tree)?;
                let prompt = match episode.role {
                    EpisodeRole::History => format!(
                        "{}\n\nProject notes live in `{dir_name}/` (indexed in `{dir_name}/INDEX.md`). \
After solving this task, record verified root causes, architectural invariants, and gotchas in `{dir_name}/` \
and keep `{dir_name}/INDEX.md` updated.",
                        episode.prompt.trim_end()
                    ),
                    EpisodeRole::Target => format!(
                        "{}\n\nProject notes from prior sessions are stored in `{dir_name}/` (see `{dir_name}/INDEX.md`).\n\
Search or read `{dir_name}/` via `grep` or `view` before investigating from scratch.",
                        episode.prompt.trim_end()
                    ),
                };
                Ok(EpisodeMemoryContext {
                    prompt,
                    mcp_config_path: None,
                    copilot_settings_json: None,
                })
            }
            ArmRuntimeState::OracleNotes { gold_statements } => {
                let prompt = if gold_statements.is_empty() {
                    episode.prompt.clone()
                } else {
                    let bullets = gold_statements
                        .iter()
                        .map(|s| format!("- {}", s.trim()))
                        .collect::<Vec<_>>()
                        .join("\n");
                    format!(
                        "{}\n\nVerified reference notes:\n{bullets}",
                        episode.prompt.trim_end()
                    )
                };
                Ok(EpisodeMemoryContext {
                    prompt,
                    mcp_config_path: None,
                    copilot_settings_json: None,
                })
            }
            ArmRuntimeState::HivemindMcp {
                hivemind_bin,
                mcp_config_path,
                use_applicability,
                guard,
                ..
            } => {
                let hook_settings = build_hivemind_hook_settings_json(
                    hivemind_bin,
                    guard.store_dir(),
                    &self.scenario_scope,
                    if *use_applicability {
                        Some(&self.scenario_context)
                    } else {
                        None
                    },
                )?;
                let prompt = match episode.role {
                    EpisodeRole::History => format!(
                        "{}\n\nYou have the hivemind MCP memory tools connected (scope: {scope}).\n\
After solving this task, call hivemind remember to record verified root causes, architectural invariants, \
and gotchas for future sessions on {scope} (citing relevant spec docs and source files in `cites`).",
                        episode.prompt.trim_end(),
                        scope = self.scenario_scope
                    ),
                    EpisodeRole::Target => episode.prompt.clone(),
                };
                let ep_mcp_cfg = mcp_config_path
                    .with_file_name(format!("mcp-config-{}.json", episode.id.replace('/', "_")));
                write_hivemind_mcp_config(
                    hivemind_bin,
                    guard.store_dir(),
                    Some(repo_dir),
                    &ep_mcp_cfg,
                )?;
                Ok(EpisodeMemoryContext {
                    prompt,
                    mcp_config_path: Some(ep_mcp_cfg),
                    copilot_settings_json: Some(hook_settings),
                })
            }
        }
    }

    /// Capture updated workspace-resident memory artifacts (`AGENTS.md` or `.memory/`) after a
    /// `History` episode finishes so they carry forward into subsequent episodes.
    pub fn capture_after_history_episode(&mut self, history_repo_dir: &Path) -> Result<()> {
        match &mut self.state {
            ArmRuntimeState::StaticNotes {
                file_name,
                current_content,
                ..
            } => {
                let candidate = history_repo_dir.join(file_name);
                if candidate.is_file() {
                    *current_content = fs::read_to_string(&candidate)
                        .with_context(|| format!("read updated {}", candidate.display()))?;
                }
            }
            ArmRuntimeState::MarkdownTree {
                dir_name,
                staged_tree_dir,
                ..
            } => {
                let candidate = history_repo_dir.join(dir_name);
                if candidate.is_dir() {
                    if staged_tree_dir.exists() {
                        fs::remove_dir_all(&*staged_tree_dir)?;
                    }
                    copy_dir_all(&candidate, staged_tree_dir)?;
                }
            }
            ArmRuntimeState::NoMemory
            | ArmRuntimeState::OracleNotes { .. }
            | ArmRuntimeState::HivemindMcp { .. } => {}
        }
        Ok(())
    }

    /// Explicitly shut down any background `hivemindd` daemon before removing temporary directories.
    pub fn shutdown(&mut self) {
        if let ArmRuntimeState::HivemindMcp { guard, .. } = &mut self.state {
            guard.shutdown_now();
        }
    }
}

/// Prepare per-trial memory substrate inside `trial_temp_dir` for `(scenario, arm, seed)`.
pub fn prepare_trial_memory(
    scenario: &ScenarioSpec,
    arm: &ArmSpec,
    default_memory_mode: MemoryMode,
    seed: u32,
    trial_temp_dir: &Path,
) -> Result<PreparedTrialMemory> {
    fs::create_dir_all(trial_temp_dir)
        .with_context(|| format!("create trial temp dir {}", trial_temp_dir.display()))?;

    let has_history = !scenario.history_episodes().is_empty();
    let noise = generate_noise_memories(scenario.noise_distractor_count, seed, &scenario.scope);

    let state = match &arm.kind {
        ArmKind::NoMemory => ArmRuntimeState::NoMemory,
        ArmKind::StaticNotes { file_name } => {
            let run_history = has_history
                && (default_memory_mode == MemoryMode::Earned || scenario.memories.is_empty());
            let seed_entries: &[SeedMemory] = if run_history { &[] } else { &scenario.memories };
            let content = render_static_notes_markdown(&scenario.scope, seed_entries, &noise, seed);
            ArmRuntimeState::StaticNotes {
                file_name: file_name.clone(),
                current_content: content,
                run_history,
            }
        }
        ArmKind::MarkdownTree { dir_name } => {
            let run_history = has_history
                && (default_memory_mode == MemoryMode::Earned || scenario.memories.is_empty());
            let seed_entries: &[SeedMemory] = if run_history { &[] } else { &scenario.memories };
            let staged_tree_dir = trial_temp_dir.join("staged-markdown-tree");
            populate_markdown_tree(&staged_tree_dir, &scenario.scope, seed_entries, &noise)?;
            ArmRuntimeState::MarkdownTree {
                dir_name: dir_name.clone(),
                staged_tree_dir,
                run_history,
            }
        }
        ArmKind::OracleNotes => {
            let gold_statements = scenario
                .memories
                .iter()
                .filter(|m| m.is_gold && !m.statement.trim().is_empty())
                .map(|m| m.statement.clone())
                .collect();
            ArmRuntimeState::OracleNotes { gold_statements }
        }
        ArmKind::HivemindMcp {
            hivemind_bin,
            mode,
            use_applicability,
        } => {
            let store_dir = trial_temp_dir.join("hivemind-store");
            fs::create_dir_all(&store_dir)
                .with_context(|| format!("create hivemind store {}", store_dir.display()))?;
            let guard = HivemindStoreGuard::new(store_dir.clone());

            let run_history =
                has_history && (*mode == MemoryMode::Earned || scenario.memories.is_empty());
            let should_seed_scenario_memories =
                !scenario.memories.is_empty() && (*mode == MemoryMode::Given || !has_history);

            let seed_slice: &[SeedMemory] = if should_seed_scenario_memories {
                &scenario.memories
            } else {
                &[]
            };

            let claim_id_map = seed_hivemind_store(
                hivemind_bin,
                &store_dir,
                &scenario.scope,
                seed_slice,
                &noise,
            )?;

            let mcp_config_path = trial_temp_dir.join("mcp-config.json");
            write_hivemind_mcp_config(hivemind_bin, &store_dir, None, &mcp_config_path)?;

            ArmRuntimeState::HivemindMcp {
                hivemind_bin: hivemind_bin.clone(),
                mcp_config_path,
                use_applicability: *use_applicability,
                run_history,
                claim_id_map,
                guard,
            }
        }
    };

    Ok(PreparedTrialMemory {
        scenario_scope: scenario.scope.clone(),
        scenario_context: scenario.context.clone(),
        state,
    })
}

/// Deterministically generate `count` plausible engineering distractor memories for `(seed, default_scope)`.
pub fn generate_noise_memories(count: usize, seed: u32, default_scope: &str) -> Vec<SeedMemory> {
    const TOPICS: &[(&str, &str, &str, &str)] = &[
        (
            "decision",
            "mod:http_pool",
            "src/net/pool.rs:42-68",
            "Outbound HTTP connection pools keep idle sockets open for {n}s with max_idle_per_host={m} to avoid TCP handshake spikes.",
        ),
        (
            "fact",
            "mod:cache_ttl",
            "src/cache/policy.rs:15-39",
            "Read-through metadata cache entries use a {n}s base TTL plus +/-{m}% uniform jitter to prevent synchronized cache stampedes.",
        ),
        (
            "preference",
            "mod:pagination",
            "src/api/cursor.rs:11-34",
            "List endpoints encode opaque base64url pagination cursors `(sort_key, last_id)` with default page_size={m} and max_page_size={n}.",
        ),
        (
            "decision",
            "mod:retry_backoff",
            "src/rpc/retry.rs:24-57",
            "Idempotent upstream GET calls retry at most {m} times using decorrelated jitter backoff capped at {n}0ms; non-idempotent POSTs never auto-retry.",
        ),
        (
            "preference",
            "mod:metrics",
            "src/telemetry/metrics.rs:8-29",
            "Service latency histograms export snake_case metric names suffixed with `_seconds` and {m} exponential buckets starting at 0.00{n}s.",
        ),
        (
            "fact",
            "mod:tls_config",
            "src/net/tls.rs:19-44",
            "Internal TLS listeners require TLS 1.3 cipher suites (`TLS_AES_256_GCM_SHA384`, `TLS_CHACHA20_POLY1305_SHA256`) and reload certificates every {n}m.",
        ),
        (
            "warning",
            "mod:db_migration",
            "migrations/README.md:1-22",
            "Online schema migrations set `lock_timeout = '{m}s'` and `statement_timeout = '{n}s'` so DDL never queues transactions behind long analytics reads.",
        ),
        (
            "procedure",
            "mod:log_redaction",
            "src/logging/redact.rs:30-61",
            "Structured JSON access logs scrub `authorization`, `cookie`, and `x-api-key` headers before serialization and cap field previews at {n} bytes.",
        ),
        (
            "fact",
            "mod:grpc_keepalive",
            "src/rpc/transport.rs:50-73",
            "gRPC channels send HTTP/2 keepalive pings every {n}s with a {m}s timeout and `keepalive_while_idle = false`.",
        ),
        (
            "warning",
            "mod:webhook_hmac",
            "src/ingest/webhook.rs:14-48",
            "Inbound webhook HMAC-SHA256 verification rejects payloads whose `X-Signature-Timestamp` drifts by more than {n}s or reuses a nonce within {m}m.",
        ),
        (
            "decision",
            "mod:rate_limiter",
            "src/gateway/ratelimit.rs:21-55",
            "Per-tenant token buckets refill at {n} requests/sec with burst capacity {m}x steady rate, keyed by normalized tenant URN.",
        ),
        (
            "procedure",
            "mod:worker_drain",
            "src/runtime/shutdown.rs:12-40",
            "On SIGTERM, background queue workers stop polling new batches immediately and allow {n}s for in-flight jobs before forced cancellation.",
        ),
        (
            "fact",
            "mod:blob_upload",
            "src/storage/multipart.rs:18-47",
            "Object-store uploads larger than {n} MiB switch to multipart transfer using {m} MiB part chunks and verify SHA-256 checksums per part.",
        ),
        (
            "preference",
            "mod:config_reload",
            "src/config/watcher.rs:9-33",
            "Hot-reloaded TOML configuration snapshots validate into an immutable `ArcSwap` struct every {n}s; malformed edits retain the last known-good snapshot.",
        ),
        (
            "warning",
            "mod:clock_skew",
            "src/auth/jwt.rs:27-59",
            "Short-lived service tokens allow {m}s of `nbf`/`exp` clock-skew leeway and reject `alg = none` or symmetric HMAC keys in verification paths.",
        ),
        (
            "fact",
            "mod:batch_compaction",
            "src/index/compact.rs:63-98",
            "Segment compaction merges L0 runs once {m} segments accumulate or total uncompacted WAL bytes exceed {n} MiB.",
        ),
    ];

    let mut rng = DeterministicRng::new(seed, default_scope);
    let mut out = Vec::with_capacity(count);

    for idx in 0..count {
        let topic_idx = (rng.next_u64() as usize + idx) % TOPICS.len();
        let (kind, about_tag, cite_path, template) = TOPICS[topic_idx];
        let n = 15 + (rng.next_u64() % 75);
        let m = 2 + (rng.next_u64() % 11);
        let statement = template
            .replace("{n}", &n.to_string())
            .replace("{m}", &m.to_string());

        let scope = if rng.next_u64().is_multiple_of(4) {
            "global".to_string()
        } else {
            default_scope.to_string()
        };

        out.push(SeedMemory {
            id: format!("noise-{seed}-{idx:03}"),
            op: MemoryOp::Remember,
            kind: kind.to_string(),
            scope: Some(scope),
            statement,
            about: vec![about_tag.to_string()],
            when: Vec::new(),
            fields: Vec::new(),
            origin: Some("agent".into()),
            cites: vec![cite_path.to_string()],
            quotes: Vec::new(),
            is_gold: false,
        });
    }

    out
}

/// Replay structured `memories` (including `Supersede`, `Retract`, and `Invalidate`) and `noise_memories`
/// into an isolated `hivemind` store directory, returning logical-ID -> `clm_...` mappings.
pub fn seed_hivemind_store(
    hivemind_bin: &Path,
    store_dir: &Path,
    default_scope: &str,
    memories: &[SeedMemory],
    noise_memories: &[SeedMemory],
) -> Result<BTreeMap<String, String>> {
    fs::create_dir_all(store_dir)
        .with_context(|| format!("create hivemind store {}", store_dir.display()))?;

    let mut id_map: BTreeMap<String, String> = BTreeMap::new();

    // Ingest background noise distractors first so scenario memories have realistic store density.
    for noise in noise_memories {
        let clm_id = run_hivemind_remember(hivemind_bin, store_dir, default_scope, noise, &[])?;
        id_map.insert(noise.id.clone(), clm_id);
    }

    // Replay structured scenario lifecycle operations in exact chronological order.
    for mem in memories {
        match &mem.op {
            MemoryOp::Remember => {
                let clm_id =
                    run_hivemind_remember(hivemind_bin, store_dir, default_scope, mem, &[])?;
                id_map.insert(mem.id.clone(), clm_id);
            }
            MemoryOp::Supersede { supersedes } => {
                let mut mapped_targets = Vec::with_capacity(supersedes.len());
                for logical in supersedes {
                    let resolved = resolve_claim_id(&id_map, logical)?;
                    mapped_targets.push(resolved);
                }
                let clm_id = run_hivemind_remember(
                    hivemind_bin,
                    store_dir,
                    default_scope,
                    mem,
                    &mapped_targets,
                )?;
                id_map.insert(mem.id.clone(), clm_id);
            }
            MemoryOp::Retract {
                target,
                reason,
                note,
            } => {
                let resolved = resolve_claim_id(&id_map, target)?;
                let mut cmd = base_hivemind_cmd(hivemind_bin, store_dir);
                cmd.arg("retract")
                    .arg(&resolved)
                    .arg("--reason")
                    .arg(reason);
                if !note.is_empty() {
                    cmd.arg("--note").arg(note);
                }
                run_hivemind_json(&mut cmd, &format!("retract `{}` ({resolved})", mem.id))?;
            }
            MemoryOp::Invalidate { target, reason } => {
                let resolved = resolve_claim_id(&id_map, target)?;
                let mut cmd = base_hivemind_cmd(hivemind_bin, store_dir);
                cmd.arg("invalidate")
                    .arg(&resolved)
                    .arg("--reason")
                    .arg(reason);
                run_hivemind_json(&mut cmd, &format!("invalidate `{}` ({resolved})", mem.id))?;
            }
        }
    }

    Ok(id_map)
}

/// Write an isolated MCP server configuration JSON file for `hivemind`.
pub fn write_hivemind_mcp_config(
    hivemind_bin: &Path,
    store_dir: &Path,
    workspace_dir: Option<&Path>,
    out_path: &Path,
) -> Result<()> {
    let store_str = store_dir.to_string_lossy().into_owned();
    let bin_str = hivemind_bin.to_string_lossy().into_owned();
    let mut env_map = serde_json::Map::new();
    env_map.insert(
        "HIVEMIND_DIR".into(),
        serde_json::Value::String(store_dir.to_string_lossy().into_owned()),
    );
    env_map.insert(
        "HIVEMIND_AGENT".into(),
        serde_json::Value::String("crucible".into()),
    );
    env_map.insert(
        "HIVEMIND_IDLE_TIMEOUT".into(),
        serde_json::Value::String("30".into()),
    );
    if let Some(ws) = workspace_dir {
        env_map.insert(
            "HIVEMIND_WORKSPACE".into(),
            serde_json::Value::String(ws.to_string_lossy().into_owned()),
        );
    }
    let payload = json!({
        "mcpServers": {
            "hivemind": {
                "type": "local",
                "command": bin_str,
                "args": ["mcp", "--dir", store_str],
                "tools": ["*"],
                "env": env_map
            }
        }
    });
    let serialized = serde_json::to_string_pretty(&payload)?;
    fs::write(out_path, format!("{serialized}\n"))
        .with_context(|| format!("write MCP config {}", out_path.display()))
}

fn shell_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

fn build_hivemind_hook_settings_json(
    hivemind_bin: &Path,
    store_dir: &Path,
    scope: &str,
    ctx: Option<&BTreeMap<String, String>>,
) -> Result<String> {
    let mut parts = vec![
        shell_single_quote(&hivemind_bin.to_string_lossy()),
        "--dir".to_string(),
        shell_single_quote(&store_dir.to_string_lossy()),
        "brief".to_string(),
        "--scope".to_string(),
        shell_single_quote(scope),
        "--scope".to_string(),
        "'global'".to_string(),
    ];
    if let Some(ctx_map) = ctx {
        for (k, v) in ctx_map {
            parts.push("--ctx".to_string());
            parts.push(shell_single_quote(&format!("{k}={v}")));
        }
    }
    parts.push("--hook-json".to_string());
    let bash_cmd = parts.join(" ");

    let settings = json!({
        "hooks": {
            "userPromptSubmitted": [
                {
                    "type": "command",
                    "bash": bash_cmd,
                    "timeoutSec": 5
                }
            ]
        }
    });
    Ok(format!("{}\n", serde_json::to_string_pretty(&settings)?))
}

fn resolve_claim_id(id_map: &BTreeMap<String, String>, target: &str) -> Result<String> {
    if let Some(mapped) = id_map.get(target) {
        return Ok(mapped.clone());
    }
    if target.starts_with("clm_") {
        return Ok(target.to_string());
    }
    bail!("seed memory references unknown prior memory id `{target}`")
}

fn base_hivemind_cmd(hivemind_bin: &Path, store_dir: &Path) -> Command {
    let mut cmd = Command::new(hivemind_bin);
    cmd.arg("--dir")
        .arg(store_dir)
        .arg("--no-fsync")
        .arg("--json")
        .env("HIVEMIND_DIR", store_dir)
        .env("HIVEMIND_AGENT", "crucible-seed")
        .stdin(Stdio::null());
    cmd
}

fn run_hivemind_remember(
    hivemind_bin: &Path,
    store_dir: &Path,
    default_scope: &str,
    mem: &SeedMemory,
    supersedes_ids: &[String],
) -> Result<String> {
    if mem.statement.trim().is_empty() {
        bail!(
            "seed memory `{}` remember/supersede statement cannot be empty",
            mem.id
        );
    }
    let scope = mem.scope.as_deref().unwrap_or(default_scope);
    let kind = if mem.kind.trim().is_empty() {
        "fact"
    } else {
        mem.kind.trim()
    };

    let mut cmd = base_hivemind_cmd(hivemind_bin, store_dir);
    cmd.arg("remember")
        .arg("--kind")
        .arg(kind)
        .arg("--scope")
        .arg(scope);

    for about in &mem.about {
        cmd.arg("--about").arg(about);
    }
    for cond in &mem.when {
        cmd.arg("--when").arg(cond);
    }
    for field in &mem.fields {
        cmd.arg("--field").arg(field);
    }
    if let Some(origin) = &mem.origin {
        cmd.arg("--origin").arg(origin);
    }
    for cite in &mem.cites {
        cmd.arg("--cite").arg(cite);
    }
    for quote in &mem.quotes {
        cmd.arg("--quote").arg(quote);
    }
    for sup in supersedes_ids {
        cmd.arg("--supersedes").arg(sup);
    }
    cmd.arg("--").arg(&mem.statement);

    let val = run_hivemind_json(&mut cmd, &format!("remember `{}`", mem.id))?;
    let claim = val
        .get("claim")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .with_context(|| {
            format!(
                "hivemind remember `{}` JSON response missing `claim` field: {val}",
                mem.id
            )
        })?;
    Ok(claim.to_string())
}

fn run_hivemind_json(cmd: &mut Command, context_label: &str) -> Result<serde_json::Value> {
    let out = cmd
        .output()
        .with_context(|| format!("spawn hivemind for {context_label}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        bail!(
            "hivemind {context_label} failed (status {:?}): {} {}",
            out.status.code(),
            stderr.trim(),
            stdout.trim()
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(stdout.trim())
        .with_context(|| format!("parse hivemind JSON output for {context_label}: `{stdout}`"))
}

fn render_static_notes_markdown(
    scope: &str,
    scenario_memories: &[SeedMemory],
    noise_memories: &[SeedMemory],
    seed: u32,
) -> String {
    let active_scenario_notes: Vec<&SeedMemory> = scenario_memories
        .iter()
        .filter(|m| !m.statement.trim().is_empty())
        .collect();

    // Interleave distractor notes while strictly preserving chronological order of `scenario_memories`.
    let mut combined: Vec<&SeedMemory> =
        Vec::with_capacity(active_scenario_notes.len() + noise_memories.len());
    if active_scenario_notes.is_empty() {
        combined.extend(noise_memories.iter());
    } else {
        let slots = active_scenario_notes.len() + 1;
        let mut buckets: Vec<Vec<&SeedMemory>> = vec![Vec::new(); slots];
        let mut rng = DeterministicRng::new(seed ^ 0x9E37_79B9, scope);
        for n in noise_memories {
            let bucket = (rng.next_u64() as usize) % slots;
            buckets[bucket].push(n);
        }
        for (idx, scen_mem) in active_scenario_notes.iter().enumerate() {
            combined.extend(buckets[idx].iter().copied());
            combined.push(*scen_mem);
        }
        combined.extend(buckets[active_scenario_notes.len()].iter().copied());
    }

    let mut out = format!(
        "# Project Engineering Notes ({scope})\n\n\
Append-only operational, architectural, and debugging notes accumulated across sessions.\n\n"
    );
    for mem in combined {
        out.push_str(&format_markdown_bullet(mem));
        out.push('\n');
    }
    out
}

fn populate_markdown_tree(
    tree_dir: &Path,
    scope: &str,
    scenario_memories: &[SeedMemory],
    noise_memories: &[SeedMemory],
) -> Result<()> {
    fs::create_dir_all(tree_dir)
        .with_context(|| format!("create markdown tree {}", tree_dir.display()))?;

    let mut by_topic: BTreeMap<String, Vec<&SeedMemory>> = BTreeMap::new();
    for mem in noise_memories
        .iter()
        .chain(scenario_memories.iter())
        .filter(|m| !m.statement.trim().is_empty())
    {
        let topic = classify_topic_slug(mem);
        by_topic.entry(topic).or_default().push(mem);
    }

    let mut index = format!(
        "# Project Memory Index ({scope})\n\n\
Search this directory with `grep` or inspect individual topic files below:\n\n"
    );

    if by_topic.is_empty() {
        index.push_str("- _(no prior notes recorded yet)_\n");
    } else {
        for (topic, items) in &by_topic {
            let file_name = format!("{topic}.md");
            index.push_str(&format!("- `{file_name}` ({} notes)\n", items.len()));

            let mut body = format!("# Notes: {topic}\n\n");
            for item in items {
                body.push_str(&format_markdown_bullet(item));
                body.push('\n');
            }
            fs::write(tree_dir.join(&file_name), body)?;
        }
    }

    fs::write(tree_dir.join("INDEX.md"), index)?;
    Ok(())
}

fn classify_topic_slug(mem: &SeedMemory) -> String {
    if let Some(first_about) = mem.about.first() {
        let cleaned: String = first_about
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        let trimmed = cleaned.trim_matches('-');
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let kind_clean: String = mem
        .kind
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if kind_clean.is_empty() {
        "general".into()
    } else {
        kind_clean.to_ascii_lowercase()
    }
}

fn format_markdown_bullet(mem: &SeedMemory) -> String {
    let mut meta = Vec::new();
    if !mem.kind.trim().is_empty() {
        meta.push(mem.kind.trim().to_string());
    }
    if !mem.when.is_empty() {
        meta.push(format!("when: {}", mem.when.join(", ")));
    }
    if !mem.cites.is_empty() {
        meta.push(format!("ref: {}", mem.cites.join(", ")));
    }
    if meta.is_empty() {
        format!("- {}", mem.statement.trim())
    } else {
        format!("- [{}] {}", meta.join(" | "), mem.statement.trim())
    }
}

/// Connect to `<store>/run/hivemindd.sock` on Unix and send a JSON shutdown request so
/// per-trial `hivemindd` daemons exit immediately instead of outliving `TempDir` removal.
pub fn shutdown_hivemind_daemon(store_dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::net::UnixStream;

        let sock_path = store_dir.join("run").join("hivemindd.sock");
        if !sock_path.exists() {
            return;
        }
        if let Ok(mut stream) = UnixStream::connect(&sock_path) {
            let io_timeout = Some(Duration::from_millis(500));
            let _ = stream.set_write_timeout(io_timeout);
            let _ = stream.set_read_timeout(io_timeout);
            let _ = stream.write_all(b"{\"id\":1,\"method\":\"shutdown\",\"params\":{}}\n");
            let _ = stream.flush();
            let mut reply = [0u8; 256];
            let _ = stream.read(&mut reply);
        }
        let deadline = Instant::now() + Duration::from_millis(250);
        while sock_path.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[cfg(not(unix))]
    {
        let _ = store_dir;
    }
}

#[derive(Clone, Debug)]
struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u32, salt: &str) -> Self {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325 ^ (seed as u64);
        for byte in salt.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        if hash == 0 {
            hash = 0x9e37_79b9_7f4a_7c15;
        }
        Self { state: hash }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crucible_core::{ScenarioCategory, VerifierSpec};

    use super::*;

    fn sample_scenario(root: &Path) -> ScenarioSpec {
        let mut ctx = BTreeMap::new();
        ctx.insert("os".into(), "linux".into());
        ctx.insert("env".into(), "prod".into());

        ScenarioSpec {
            root: root.to_path_buf(),
            id: "supersede-check".into(),
            title: "Supersede Check".into(),
            category: ScenarioCategory::Supersession,
            description: "Test lifecycle".into(),
            scope: "project:payments".into(),
            context: ctx,
            noise_distractor_count: 6,
            memories: vec![
                SeedMemory {
                    id: "m1".into(),
                    op: MemoryOp::Remember,
                    kind: "fact".into(),
                    scope: None,
                    statement: "Legacy retry limit is 2 attempts.".into(),
                    about: vec!["mod:retry".into()],
                    when: Vec::new(),
                    fields: Vec::new(),
                    origin: Some("agent".into()),
                    cites: vec!["src/retry.rs:10".into()],
                    quotes: Vec::new(),
                    is_gold: false,
                },
                SeedMemory {
                    id: "m2".into(),
                    op: MemoryOp::Supersede {
                        supersedes: vec!["m1".into()],
                    },
                    kind: "decision".into(),
                    scope: None,
                    statement: "Active retry limit is 5 attempts on linux prod.".into(),
                    about: vec!["mod:retry".into()],
                    when: vec!["os=linux".into()],
                    fields: vec!["max_retries=5".into()],
                    origin: Some("agent".into()),
                    cites: vec!["src/retry.rs:12".into()],
                    quotes: Vec::new(),
                    is_gold: true,
                },
            ],
            episodes: vec![EpisodeSpec {
                id: "target".into(),
                role: EpisodeRole::Target,
                workspace_subdir: "workspace".into(),
                prompt: "Update retry behavior.".into(),
                timeout_secs: 30,
            }],
            verifier: VerifierSpec {
                command: vec!["true".into()],
                overlay_dir: None,
                forbidden_tool_substrings: Vec::new(),
                forbidden_diff_substrings: Vec::new(),
                forbidden_paths: Vec::new(),
                timeout_secs: 10,
            },
            gold_patch_dir: "gold_patch".into(),
        }
    }

    #[test]
    fn noise_generator_is_deterministic_and_seed_sensitive() {
        let a1 = generate_noise_memories(10, 7, "project:alpha");
        let a2 = generate_noise_memories(10, 7, "project:alpha");
        let b = generate_noise_memories(10, 8, "project:alpha");
        assert_eq!(a1, a2);
        assert_ne!(a1, b);
        assert_eq!(a1.len(), 10);
        assert!(a1.iter().all(|m| !m.is_gold && !m.statement.is_empty()));
    }

    #[test]
    fn static_notes_and_oracle_arms_prepare_expected_artifacts() {
        let dir = TempDir::new().unwrap();
        let scenario = sample_scenario(dir.path());
        let repo_dir = dir.path().join("repo");
        fs::create_dir_all(&repo_dir).unwrap();

        let b1 = prepare_trial_memory(
            &scenario,
            &ArmSpec::b1_notes(),
            MemoryMode::Given,
            0,
            &dir.path().join("t-b1"),
        )
        .unwrap();
        let b1_ctx = b1
            .prepare_episode_workspace(&scenario.episodes[0], &repo_dir)
            .unwrap();
        assert!(b1_ctx.mcp_config_path.is_none());
        let agents_md = fs::read_to_string(repo_dir.join("AGENTS.md")).unwrap();
        let pos_old = agents_md.find("Legacy retry limit is 2 attempts.").unwrap();
        let pos_new = agents_md
            .find("Active retry limit is 5 attempts on linux prod.")
            .unwrap();
        assert!(
            pos_old < pos_new,
            "chronological order must be preserved in AGENTS.md"
        );

        let oracle = prepare_trial_memory(
            &scenario,
            &ArmSpec::oracle(),
            MemoryMode::Given,
            0,
            &dir.path().join("t-oracle"),
        )
        .unwrap();
        let oracle_ctx = oracle
            .prepare_episode_workspace(&scenario.episodes[0], &repo_dir)
            .unwrap();
        assert!(
            oracle_ctx
                .prompt
                .contains("Active retry limit is 5 attempts")
        );
        assert!(!oracle_ctx.prompt.contains("Legacy retry limit is 2"));
    }

    #[test]
    fn markdown_tree_writes_index_and_topic_files() {
        let dir = TempDir::new().unwrap();
        let scenario = sample_scenario(dir.path());
        let repo_dir = dir.path().join("repo");
        fs::create_dir_all(&repo_dir).unwrap();

        let grep_arm = prepare_trial_memory(
            &scenario,
            &ArmSpec::b1_grep(),
            MemoryMode::Given,
            1,
            &dir.path().join("t-grep"),
        )
        .unwrap();
        let ctx = grep_arm
            .prepare_episode_workspace(&scenario.episodes[0], &repo_dir)
            .unwrap();
        assert!(repo_dir.join(".memory/INDEX.md").is_file());
        assert!(ctx.prompt.contains(".memory/"));
    }

    #[cfg(unix)]
    #[test]
    fn hivemind_seeding_and_daemon_guard_shutdown_work_end_to_end() {
        use std::os::unix::fs::PermissionsExt as _;
        use std::os::unix::net::UnixListener;

        let dir = TempDir::new().unwrap();
        let mock_bin = dir.path().join("mock-hivemind.sh");
        let log_file = dir.path().join("invocations.log");
        let script = format!(
            "#!/bin/sh\necho \"$@\" >> \"{}\"\ncount=$(wc -l < \"{}\" | tr -d ' ')\nprintf '{{\"claim\":\"clm_mock_%s\"}}\\n' \"$count\"\n",
            log_file.display(),
            log_file.display()
        );
        fs::write(&mock_bin, script).unwrap();
        fs::set_permissions(&mock_bin, fs::Permissions::from_mode(0o755)).unwrap();

        let mut scenario = sample_scenario(dir.path());
        scenario.memories.push(SeedMemory {
            id: "m3".into(),
            op: MemoryOp::Retract {
                target: "m1".into(),
                reason: "wrong".into(),
                note: "obsolete".into(),
            },
            kind: "fact".into(),
            scope: None,
            statement: String::new(),
            about: Vec::new(),
            when: Vec::new(),
            fields: Vec::new(),
            origin: None,
            cites: Vec::new(),
            quotes: Vec::new(),
            is_gold: false,
        });

        let trial_dir = dir.path().join("trial");
        let arm = ArmSpec::hivemind(mock_bin, MemoryMode::Given);
        let prepared =
            prepare_trial_memory(&scenario, &arm, MemoryMode::Given, 0, &trial_dir).unwrap();

        let map = prepared.hivemind_claim_ids().unwrap();
        assert_eq!(map.get("m1").map(String::as_str), Some("clm_mock_7"));
        assert_eq!(map.get("m2").map(String::as_str), Some("clm_mock_8"));

        let logged = fs::read_to_string(&log_file).unwrap();
        assert!(logged.contains("--supersedes clm_mock_7"));
        assert!(logged.contains("retract clm_mock_7 --reason wrong --note obsolete"));

        let repo_dir = dir.path().join("repo");
        fs::create_dir_all(&repo_dir).unwrap();
        let ep_ctx = prepared
            .prepare_episode_workspace(&scenario.episodes[0], &repo_dir)
            .unwrap();
        assert!(ep_ctx.mcp_config_path.as_ref().unwrap().is_file());
        let settings = ep_ctx.copilot_settings_json.as_ref().unwrap();
        assert!(settings.contains("userPromptSubmitted"));
        assert!(settings.contains("--ctx 'env=prod'"));
        assert!(settings.contains("--ctx 'os=linux'"));
        assert!(settings.contains("--hook-json"));

        // Verify Unix domain socket shutdown payload on drop.
        let run_dir = trial_dir.join("hivemind-store/run");
        fs::create_dir_all(&run_dir).unwrap();
        let sock_path = run_dir.join("hivemindd.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();
        let server_sock = sock_path.clone();
        let handle = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut buf = String::new();
            conn.read_to_string(&mut buf).unwrap();
            let _ = fs::remove_file(server_sock);
            buf
        });

        drop(prepared);
        let received = handle.join().unwrap();
        assert_eq!(
            received,
            "{\"id\":1,\"method\":\"shutdown\",\"params\":{}}\n"
        );
    }
}
