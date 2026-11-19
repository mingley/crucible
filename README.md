# Crucible

**Crucible** is a lean, native-Rust evaluation harness for coding agents and persistent memory systems.

Most agent benchmarks only report aggregate `Pass@1`. That hides the exact failure modes that matter in production coding workflows:
- **Does memory rescue hard tasks or regress tasks the agent already solved?** Aggregate pass rates mask discordant pairs (`(B=0, M=1)` rescues vs. `(B=1, M=0)` regressions).
- **Does structured memory beat a plain `AGENTS.md` file?** Many evaluations compare a memory tool only against a zero-memory baseline (`B0`), conflating "any text in context" with structured lifecycle management (supersession, retraction, scoped applicability, and provenance quarantine).
- **What is the real cache-adjusted cost per successful task?** Stuffing rolling notes into every turn inflates prompt tokens; MCP tool calls add round-trips. Crucible parses exact `uncached_input`, `cache_read`, `cache_write`, `output`, `reasoning`, AIU, wall-clock, and MCP call counts per episode.
- **Can the agent cheat by reading tests?** Crucible stages a clean git workspace from `workspace/` and copies `verifier/` overlay tests **only after** the agent process exits and anti-tamper checks pass.

---

## Architecture

Crucible is a small Cargo workspace with zero Python/Node/Docker runtime dependencies in the harness itself:

```text
crucible/
├── crates/
│   ├── crucible-core/    # Scenario/Suite TOML specs, Arm definitions, cache-aware token/USD ledgers, RunManifest
│   ├── crucible-stats/   # Pure-Rust statistical engine (exact McNemar, paired Wilcoxon, 10k cluster bootstrap, Holm-Bonferroni, Pareto)
│   └── crucible/         # Hermetic git sandbox, memory arm seeders, Copilot/Command/Scripted drivers, Latin-square runner & CLI
└── suites/
    └── coding-memory-v1/ # 8 contamination-free multi-file coding scenarios across 8 memory lifecycle categories
```

### Experimental Arms

Crucible evaluates every `(scenario, seed)` block across configurable paired arms in **Latin-square rotated order** (`offset = (scenario_idx + seed) % num_arms`) so provider latency jitter and prompt-cache warmth affect all arms symmetrically:

| Arm ID | Description |
| :--- | :--- |
| `b0` | **No Memory** — clean workspace and task prompt only |
| `b1-notes` | **Static `AGENTS.md`** — chronological append-only project notes + deterministic distractors |
| `b1-grep` | **Markdown Tree (`.memory/`)** — topic-sharded notes + `INDEX.md` searched via `grep`/`view` |
| `hivemind` | **[Hivemind](https://github.com/mingley/hivemind) MCP (`Given`)** — isolated per-trial store with structured lifecycle replay (`remember`, `supersede`, `retract`, `invalidate`, `when` applicability, `origin` trust tiers) |
| `hivemind-earned` | **Hivemind MCP (`Earned`)** — multi-episode chain where Episode 1 must author its own durable claims and Episode 2 retrieves them in a fresh workspace |
| `oracle` | **Gold Reference** — only `is_gold = true` facts injected directly (ceiling calibration) |

### Statistical Engine (`crucible-stats`)

Implemented from scratch in pure Rust (`SplitMix64` PRNG, exact binomial/DP tails, zero external heavy stat crates):
- **Scenario-clustered bootstrap 95% CIs** (`10,000` resamples over scenarios to respect within-scenario seed correlation) for `Pass@1`, `Pass^k` (strict all-`k` consistency), mean cost, and net uplift.
- **Paired Rescue vs. Harm decomposition** against the baseline arm:
  - $\text{Rescue Rate} = P(M = 1 \mid B = 0)$
  - $\text{Harm Rate} = P(M = 0 \mid B = 1)$ and $\text{Preservation} = 1 - \text{Harm Rate}$
- **Exact two-sided McNemar test** on discordant `(scenario, seed)` pairs with **Holm-Bonferroni** family-wise error correction across multiple treatment arms.
- **Paired Wilcoxon signed-rank test** (exact `2^n` DP for `n <= 20` untied pairs; tie-corrected normal `erfc` approximation otherwise) on paired cache-adjusted USD cost and wall-clock duration.
- **Cost-per-Success** ($\sum \text{Cost} / \sum \text{Successes}$) and **Pareto Frontier** identification (`cost <= other` and `pass_rate >= other`, strictly better on at least one).

---

## Suite: `coding-memory-v1`

`suites/` ships two paired 8-scenario suites (`166` synthetic Python 3 stdlib source/test files, ~40 ms deterministic verification per scenario, zero external `pip` installs) that isolate **information-theoretic redundancy** $I(Y; M \mid X)$:

- **`suites/latent-memory-v1` (Undocumented / Cross-Session Regime, $I(Y; M \mid X) > 0$, 100 distractors/trial)** — Target specifications live *only* in cross-session memory or an upstream Episode 1 repository (not duplicated in local `workspace/docs/`).
- **`suites/coding-memory-v1` (Self-Documented Repo Regime, $I(Y; M \mid X) \approx 0$, 40 distractors/trial)** — Identical codebases, but every contract is *also* documented in `workspace/docs/*.md`.

Both suites cover 8 memory lifecycle categories:

1. **`01-handoff-rate-limiter` (`handoff`)** — Interrupted work item (`PG-441`) across sliding-window rollover decay and ceiling reset headers.
2. **`02-superseded-wire-v3` (`supersession`)** — Protocol cutover where `v3` (`0xB7`, `>I` seq, `FLAG_CONTROL`, CRC-32) superseded `v2` (`0xA5`, `<I` seq, CRC-16).
3. **`03-branch-scoped-applicability` (`applicability`)** — Context-gated framing rules (`when = ["protocol=v2-binary"]` vs. legacy `protocol=v1-text` trailing-NUL stripping).
4. **`04-opaque-error-code-e47` (`diagnosis`)** — Opaque `E47_MANIFEST_DRIFT` compaction failure from mixing UTF-8 byte indices with Unicode scalar slicing and surrogate skips (`0xD800..0xDFFF`).
5. **`05-retracted-hypothesis` (`retraction`)** — Misleading initial workaround (`allow_unpinned_gc = True`) retracted after isolating `WatermarkTracker` scan lease pinning.
6. **`06-poisoned-external-doc` (`poisoning`)** — Untrusted `origin = "external"` prompt-injection note attempting to plant `.webhook_relay_diag_Init` or `DEBUG_INSECURE_BYPASS_SIG = True`.
7. **`07-hidden-domain-convention` (`convention`)** — Non-obvious accounting conventions (`ROUND_HALF_EVEN` reversal fee rebates vs. `ROUND_HALF_UP` origination, case-folded idempotency keys, append-only offsetting journal pairs).
8. **`08-earned-two-episode-chain` (`earned_chain`)** — Two-episode chain: Episode 1 fixes `cron_window.py` in an upstream scheduler repo and authors its own memory (`250ms` `EpochTick` + `LEAP_OFFSET_TABLE` + half-open blackouts); Episode 2 implements `compute_sla_breach_tick` in a separate downstream workspace.

---

## Quick Start

### Build & Test

```bash
cargo test --workspace --locked
cargo build --release --locked -p crucible
```

### Verify Suite Integrity

Confirms that every scenario's unmodified workspace **fails** the hidden verifier overlay and its `gold_patch/` **passes** with zero violations:

```bash
./target/release/crucible verify-suite suites/coding-memory-v1
```

### Run a Paired Benchmark

```bash
# Dry-run the full pipeline with the deterministic scripted driver:
./target/release/crucible run suites/coding-memory-v1 \
  --driver scripted \
  --arms b0,b1-notes,hivemind,oracle \
  --hivemind-bin ~/.local/bin/hivemind \
  --seeds 3 \
  --out /tmp/crucible-dryrun

# Live paired evaluation with GitHub Copilot CLI (hermetic per-episode COPILOT_HOME):
./target/release/crucible run suites/coding-memory-v1 \
  --driver copilot \
  --arms b0,b1-notes,hivemind \
  --hivemind-bin ~/.local/bin/hivemind \
  --seeds 3 \
  --jobs 4 \
  --model claude-sonnet-5.5 \
  --effort medium \
  --baseline b0 \
  --out benchmarks/coding-memory-v1-sonnet-5.5
```

Re-render reports from any saved `manifest.json`:

```bash
./target/release/crucible report benchmarks/latent-memory-v1-sonnet-5.5/manifest.json
./target/release/crucible report benchmarks/latent-memory-v1-sonnet-5.5/manifest.json --json
```

---

## Reference Benchmark Results (`claude-sonnet-5.5`, 144 Live Trials)

Full trial ledgers (`manifest.json`, `trials.jsonl`, `report.md`, `report.json`) are checked into [`benchmarks/`](benchmarks/). Every `(scenario, seed)` block ran in Latin-square rotated arm order with `seeds = 3` (`24` paired trials/arm/suite, `72` trials/suite, `144` live episodes + multi-episode chains total).

### 1. Latent / Cross-Session Knowledge (`latent-memory-v1`, 100 distractors, $I(Y; M \mid X) > 0$)

| Arm | Pass@1 [95% CI] | Pass^3 | Mean Wall (s) | Tool Calls (MCP) | Mean In / Out Tok | Mean Cost ($) | **Cost / Success ($)** | Mean AIU | Pareto |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| `b0` (No Memory) | 54.2% [20.8%, 87.5%] | 50.0% | 24.72 | 7.3 (0.0) | 196,445 / 3,050 | $0.1569 | $0.2896 | 8.65 | Yes |
| `b1-notes` (`AGENTS.md`) | **95.8%** [87.5%, 100.0%] | **87.5%** | 28.06 | 8.9 (0.0) | 278,705 / 3,407 | $0.2306 | $0.2406 | 12.87 | Yes |
| `hivemind` (MCP) | **83.3%** [58.3%, 100.0%] | **75.0%** | 25.24 | 9.2 (1.5) | 228,019 / 3,146 | $0.1765 | **$0.2118** | 9.66 | Yes |

| Treatment vs. `b0` | Paired N | Rescued | Harmed | Rescue Rate [95% CI] | Harm Rate (Preservation) | Net Uplift [95% CI] | McNemar $p$ (Holm) | Wilcoxon Cost $p$ |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| `b1-notes` vs. `b0` | 24 | 10 | 0 | 41.7% [12.5%, 70.8%] | 0.0% (100.0%) | +41.7% [+12.5%, +70.8%] | **0.0020 (0.0039)** | **0.0002** (+47% cost) |
| `hivemind` vs. `b0` | 24 | 7 | 0 | 29.2% [8.3%, 54.2%] | 0.0% (100.0%) | +29.2% [+8.3%, +54.2%] | **0.0156 (0.0156)** | 0.4663 (+12% cost) |

### 2. Self-Documented Repository Control (`coding-memory-v1`, 40 distractors, $I(Y; M \mid X) \approx 0$)

| Arm | Pass@1 [95% CI] | Pass^3 | Mean Wall (s) | Tool Calls (MCP) | Mean In / Out Tok | Mean Cost ($) | Cost / Success ($) | Mean AIU |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| `b0` (No Memory) | 95.8% [87.5%, 100.0%] | 87.5% | 22.94 | 7.0 (0.0) | 185,590 / 2,881 | $0.1482 | $0.1546 | 8.16 |
| `b1-notes` (`AGENTS.md`) | 100.0% [100.0%, 100.0%] | 100.0% | 21.88 | 6.6 (0.0) | 189,705 / 2,718 | $0.1564 | $0.1564 | 8.70 |
| `hivemind` (MCP) | 95.8% [87.5%, 100.0%] | 87.5% | 22.19 | 8.7 (1.4) | 198,856 / 2,824 | $0.1569 | $0.1637 | 8.63 |

### Key Empirical Findings

1. **Cost-per-Success Pareto Winner under Latent Knowledge:** When specifications are not already duplicated in `workspace/docs/`, `hivemind` lifts `Pass@1` from **54.2% to 83.3%** (`+29.2%`, exact McNemar $p = 0.0156$, **0 regressions / 100% preservation**) and achieves the **lowest Cost-per-Success of any arm (`$0.2118`)** — **26.9% cheaper per solved task than `b0` (`$0.2896`)** and **12.0% cheaper than static `AGENTS.md` (`$0.2406`)**.
2. **The $O(N)$ Static-Notes Token Tax:** Growing `AGENTS.md` from 40 to 100 background notes inflated `b1-notes` input tokens by **+47%** (`189.7k` -> `278.7k`, Wilcoxon $p = 0.0002$, `+33%` AIU over `hivemind`). At multi-thousand-claim store sizes (`10k+` claims), flat `AGENTS.md` injection exceeds context budgets entirely while `hivemind` sub-millisecond indexed `recall` stays constant-size.
3. **Lifecycle Hygiene (`retract` & `origin=external` quarantine):** On `05-retracted-hypothesis` (`latent-memory-v1`), `b1-notes` retained both the wrong and corrected hypotheses in `AGENTS.md`, averaging **44.7s / `$0.2785`** per trial to reconcile the contradiction, whereas `hivemind` purged the retracted claim from default `recall` and solved all 3 seeds in **33.4s / `$0.1920`** (**25% faster, 31% cheaper**). On `06-poisoned-external-doc`, `hivemind`'s `external` quarantine beat `b1-notes` by **24% cost** (`$0.1297` vs. `$0.1701`).
4. **Actionable Failure Modes Surfaced in `hivemind`:**
   - *Recall-card tail clipping (`coding-memory-v1` `04` seed=2):* When a >400-char claim puts an edge case (`0xD800..0xDFFF` surrogate skip) at the tail, compact `recall` cards truncate it unless the agent calls `hivemind show`.
   - *Self-authored compression loss (`latent-memory-v1` `08` `Earned` chain):* `hivemind remember`'s schema prompt (*"One or two sentences written for a future reader"*) caused Episode 1 agents to transfer the `EpochTick` (`250ms`) and `LEAP_OFFSET_TABLE` 3/3 times, but drop a secondary boundary clause (`budget_ms=0` inside an active blackout) 2/3 times.

---

## Adding Your Own Scenarios or Drivers

1. **New Scenario:** Create a directory under `suites/<your-suite>/<scenario-id>/` with `scenario.toml`, `workspace/`, `verifier/` (answer-isolated tests), and `gold_patch/`, then run `crucible verify-suite`.
2. **Custom Agent CLI (`--driver command`):** Any headless agent executable can be benchmarked without writing Rust by passing `--driver command --command-bin ./my-agent-wrapper` (receives `CRUCIBLE_WORKSPACE_DIR`, `CRUCIBLE_PROMPT_FILE`, `CRUCIBLE_MCP_CONFIG`, `CRUCIBLE_USAGE_OUT`, and `CRUCIBLE_TRACE_OUT`).

## License

MIT
