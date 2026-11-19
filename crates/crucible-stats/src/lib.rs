//! Paired non-parametric statistics, scenario-cluster bootstrap confidence intervals,
//! Rescue/Harm decomposition, Pareto frontier analysis, and report rendering for Crucible.

mod math;
mod prng;
mod report;

use std::collections::{BTreeMap, BTreeSet};

use crucible_core::{ModelPricing, RunManifest, TrialRecord};
use serde::{Deserialize, Serialize};

use math::{PairedClusterCounts, cluster_bootstrap_ratio, paired_cluster_bootstrap};
pub use math::{holm_bonferroni, mcnemar_exact_two_sided, wilcoxon_signed_rank_two_sided};
use prng::SplitMix64;
pub use report::{render_json_report, render_markdown_report};

const PARETO_TOL: f64 = 1e-12;
/// Minimum discordant count (`rescued + harmed`) required for exact two-sided McNemar
/// to ever reach `p <= 0.05` (`2^(1 - 5) = 0.0625 > 0.05`, `2^(1 - 6) = 0.03125 <= 0.05`).
const MIN_DISCORDANT_FOR_POWER: usize = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ci95 {
    pub point: f64,
    pub low: f64,
    pub high: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArmSummary {
    pub arm_id: String,
    pub trials: usize,
    pub scenarios: usize,
    pub seeds_per_scenario: usize,
    pub pass_at_1: Ci95,
    /// Fraction of scenarios where every evaluated seed for this arm passed (`pass^k`).
    pub pass_all_seeds: f64,
    pub mean_wall_ms: f64,
    pub median_wall_ms: f64,
    pub mean_tool_calls: f64,
    pub mean_mcp_tool_calls: f64,
    pub mean_total_input_tokens: f64,
    pub mean_output_tokens: f64,
    pub mean_cache_adjusted_usd: f64,
    pub mean_uncached_equiv_usd: f64,
    pub mean_aiu: f64,
    pub cost_per_success_usd: Option<f64>,
    pub uncached_cost_per_success_usd: Option<f64>,
    pub aiu_per_success: Option<f64>,
    pub pareto_optimal: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PairedComparison {
    pub baseline_arm: String,
    pub treatment_arm: String,
    pub paired_trials: usize,
    pub both_pass: usize,
    pub both_fail: usize,
    pub rescued: usize,
    pub harmed: usize,
    pub rescue_rate: Ci95,
    pub harm_rate: Ci95,
    /// Conditional harm among baseline passes: `harmed / (both_pass + harmed)` (MemDev-Bench Preservation Harm).
    pub preservation_harm_rate: Option<f64>,
    pub net_pass_delta: Ci95,
    pub mcnemar_p_two_sided: f64,
    pub mcnemar_p_holm: f64,
    pub wilcoxon_cost_p_two_sided: f64,
    pub wilcoxon_wall_ms_p_two_sided: f64,
    pub wilcoxon_tokens_p_two_sided: f64,
    pub underpowered: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EvaluationReport {
    pub run_id: String,
    pub suite_id: String,
    pub model: String,
    pub baseline_arm: String,
    pub arms: Vec<ArmSummary>,
    pub comparisons: Vec<PairedComparison>,
    pub by_category: BTreeMap<String, BTreeMap<String, f64>>,
}

/// Computes arm summaries, Pareto frontier flags, paired Rescue/Harm statistics,
/// cluster bootstrap 95% confidence intervals, and category pass-rate breakdowns.
pub fn analyze_manifest(
    manifest: &RunManifest,
    baseline_arm: Option<&str>,
    bootstrap_resamples: usize,
    rng_seed: u64,
) -> EvaluationReport {
    let arm_ids = collect_ordered_arms(manifest);
    let resolved_baseline = match baseline_arm {
        Some(b) => b.to_string(),
        None => {
            if arm_ids.iter().any(|a| a == "b0") {
                "b0".to_string()
            } else {
                arm_ids.first().cloned().unwrap_or_default()
            }
        }
    };

    let mut trials_by_arm: BTreeMap<String, Vec<&TrialRecord>> = BTreeMap::new();
    for arm_id in &arm_ids {
        trials_by_arm.insert(arm_id.clone(), Vec::new());
    }
    for trial in &manifest.trials {
        trials_by_arm
            .entry(trial.arm_id.clone())
            .or_default()
            .push(trial);
    }

    let mut rng = SplitMix64::new(rng_seed);

    let mut arms: Vec<ArmSummary> = arm_ids
        .iter()
        .map(|arm_id| {
            let arm_trials = trials_by_arm.get(arm_id).map_or(&[][..], Vec::as_slice);
            summarize_arm(
                arm_id,
                arm_trials,
                &manifest.pricing,
                bootstrap_resamples,
                &mut rng,
            )
        })
        .collect();

    mark_pareto_frontier(&mut arms);

    let base_trials = trials_by_arm
        .get(&resolved_baseline)
        .map_or(&[][..], Vec::as_slice);

    let mut comparisons: Vec<PairedComparison> = arm_ids
        .iter()
        .filter(|arm_id| arm_id.as_str() != resolved_baseline.as_str())
        .map(|treat_id| {
            let treat_trials = trials_by_arm.get(treat_id).map_or(&[][..], Vec::as_slice);
            compare_paired_arms(
                &resolved_baseline,
                base_trials,
                treat_id,
                treat_trials,
                &manifest.pricing,
                bootstrap_resamples,
                &mut rng,
            )
        })
        .collect();

    let raw_mcnemar: Vec<f64> = comparisons.iter().map(|c| c.mcnemar_p_two_sided).collect();
    let adjusted_mcnemar = holm_bonferroni(&raw_mcnemar);
    for (comp, adj_p) in comparisons.iter_mut().zip(adjusted_mcnemar) {
        comp.mcnemar_p_holm = adj_p;
    }

    let by_category = compute_by_category(&manifest.trials);

    EvaluationReport {
        run_id: manifest.run_id.clone(),
        suite_id: manifest.suite_id.clone(),
        model: manifest.model.clone(),
        baseline_arm: resolved_baseline,
        arms,
        comparisons,
        by_category,
    }
}

fn collect_ordered_arms(manifest: &RunManifest) -> Vec<String> {
    let mut ordered = Vec::new();
    for arm in &manifest.arms {
        if !ordered.contains(arm) {
            ordered.push(arm.clone());
        }
    }
    let mut extra = BTreeSet::new();
    for trial in &manifest.trials {
        if !ordered.contains(&trial.arm_id) {
            extra.insert(trial.arm_id.clone());
        }
    }
    ordered.extend(extra);
    ordered
}

fn summarize_arm(
    arm_id: &str,
    arm_trials: &[&TrialRecord],
    pricing: &ModelPricing,
    bootstrap_resamples: usize,
    rng: &mut SplitMix64,
) -> ArmSummary {
    let trials = arm_trials.len();
    if trials == 0 {
        return ArmSummary {
            arm_id: arm_id.to_string(),
            trials: 0,
            scenarios: 0,
            seeds_per_scenario: 0,
            pass_at_1: Ci95::default(),
            pass_all_seeds: 0.0,
            mean_wall_ms: 0.0,
            median_wall_ms: 0.0,
            mean_tool_calls: 0.0,
            mean_mcp_tool_calls: 0.0,
            mean_total_input_tokens: 0.0,
            mean_output_tokens: 0.0,
            mean_cache_adjusted_usd: 0.0,
            mean_uncached_equiv_usd: 0.0,
            mean_aiu: 0.0,
            cost_per_success_usd: None,
            uncached_cost_per_success_usd: None,
            aiu_per_success: None,
            pareto_optimal: false,
        };
    }

    let mut by_scenario: BTreeMap<&str, Vec<&TrialRecord>> = BTreeMap::new();
    for &t in arm_trials {
        by_scenario.entry(&t.scenario_id).or_default().push(t);
    }

    let scenarios = by_scenario.len();
    let seeds_per_scenario = by_scenario.values().map(Vec::len).max().unwrap_or(0);

    let all_passed_scenarios = by_scenario
        .values()
        .filter(|scen_trials| scen_trials.iter().all(|t| t.passed))
        .count();
    let pass_all_seeds = if scenarios == 0 {
        0.0
    } else {
        all_passed_scenarios as f64 / scenarios as f64
    };

    let clusters: Vec<(usize, usize)> = by_scenario
        .values()
        .map(|scen_trials| {
            let passed = scen_trials.iter().filter(|t| t.passed).count();
            (passed, scen_trials.len())
        })
        .collect();
    let pass_at_1 = cluster_bootstrap_ratio(&clusters, bootstrap_resamples, rng);

    let n = trials as f64;
    let passed_total: usize = arm_trials.iter().filter(|t| t.passed).count();

    let mut walls: Vec<u64> = arm_trials
        .iter()
        .map(|t| t.total_usage.wall_duration_ms)
        .collect();
    let sum_wall: u128 = walls.iter().copied().map(u128::from).sum();
    walls.sort_unstable();
    let median_wall_ms = if walls.len() % 2 == 1 {
        walls[walls.len() / 2] as f64
    } else {
        let mid = walls.len() / 2;
        (walls[mid - 1] as f64 + walls[mid] as f64) / 2.0
    };

    let sum_tool_calls: u128 = arm_trials
        .iter()
        .map(|t| u128::from(t.total_usage.tool_calls))
        .sum();
    let sum_mcp_calls: u128 = arm_trials
        .iter()
        .map(|t| u128::from(t.total_usage.mcp_tool_calls))
        .sum();
    let sum_in_tok: u128 = arm_trials
        .iter()
        .map(|t| u128::from(t.total_usage.total_input_tokens))
        .sum();
    let sum_out_tok: u128 = arm_trials
        .iter()
        .map(|t| u128::from(t.total_usage.output_tokens))
        .sum();
    let sum_nano_aiu: u128 = arm_trials
        .iter()
        .map(|t| u128::from(t.total_usage.nano_aiu))
        .sum();
    let sum_aiu = (sum_nano_aiu as f64) / 1_000_000_000.0;

    let sum_uncached_in: u128 = arm_trials
        .iter()
        .map(|t| u128::from(t.total_usage.uncached_input_tokens))
        .sum();
    let sum_cache_write: u128 = arm_trials
        .iter()
        .map(|t| u128::from(t.total_usage.cache_write_tokens))
        .sum();
    let sum_cache_read: u128 = arm_trials
        .iter()
        .map(|t| u128::from(t.total_usage.cache_read_tokens))
        .sum();
    let mtok = 1_000_000.0;
    let sum_cache_usd = (sum_uncached_in as f64 / mtok) * pricing.uncached_input_per_mtok_usd
        + (sum_cache_write as f64 / mtok) * pricing.cache_write_per_mtok_usd
        + (sum_cache_read as f64 / mtok) * pricing.cache_read_per_mtok_usd
        + (sum_out_tok as f64 / mtok) * pricing.output_per_mtok_usd;
    let sum_uncached_usd = (sum_in_tok as f64 / mtok) * pricing.uncached_input_per_mtok_usd
        + (sum_out_tok as f64 / mtok) * pricing.output_per_mtok_usd;

    let (cost_per_success_usd, uncached_cost_per_success_usd, aiu_per_success) =
        if passed_total == 0 {
            (None, None, None)
        } else {
            let p = passed_total as f64;
            (
                Some(sum_cache_usd / p),
                Some(sum_uncached_usd / p),
                Some(sum_aiu / p),
            )
        };

    ArmSummary {
        arm_id: arm_id.to_string(),
        trials,
        scenarios,
        seeds_per_scenario,
        pass_at_1,
        pass_all_seeds,
        mean_wall_ms: (sum_wall as f64) / n,
        median_wall_ms,
        mean_tool_calls: (sum_tool_calls as f64) / n,
        mean_mcp_tool_calls: (sum_mcp_calls as f64) / n,
        mean_total_input_tokens: (sum_in_tok as f64) / n,
        mean_output_tokens: (sum_out_tok as f64) / n,
        mean_cache_adjusted_usd: sum_cache_usd / n,
        mean_uncached_equiv_usd: sum_uncached_usd / n,
        mean_aiu: sum_aiu / n,
        cost_per_success_usd,
        uncached_cost_per_success_usd,
        aiu_per_success,
        pareto_optimal: false,
    }
}

fn mark_pareto_frontier(arms: &mut [ArmSummary]) {
    let snapshot: Vec<(usize, f64, f64)> = arms
        .iter()
        .map(|a| (a.trials, a.pass_at_1.point, a.mean_cache_adjusted_usd))
        .collect();

    for (i, arm) in arms.iter_mut().enumerate() {
        if snapshot[i].0 == 0 {
            arm.pareto_optimal = false;
            continue;
        }
        let (_, a_pass, a_cost) = snapshot[i];
        let dominated = snapshot
            .iter()
            .enumerate()
            .any(|(j, &(b_trials, b_pass, b_cost))| {
                if i == j || b_trials == 0 {
                    return false;
                }
                (b_pass >= a_pass && b_cost < a_cost - PARETO_TOL)
                    || (b_pass > a_pass + PARETO_TOL && b_cost <= a_cost + PARETO_TOL)
            });
        arm.pareto_optimal = !dominated;
    }
}

fn compare_paired_arms(
    baseline_id: &str,
    base_trials: &[&TrialRecord],
    treatment_id: &str,
    treat_trials: &[&TrialRecord],
    pricing: &ModelPricing,
    bootstrap_resamples: usize,
    rng: &mut SplitMix64,
) -> PairedComparison {
    let base_map: BTreeMap<(&str, u32), &TrialRecord> = base_trials
        .iter()
        .map(|&t| ((t.scenario_id.as_str(), t.seed), t))
        .collect();
    let treat_map: BTreeMap<(&str, u32), &TrialRecord> = treat_trials
        .iter()
        .map(|&t| ((t.scenario_id.as_str(), t.seed), t))
        .collect();

    let mut paired_trials = 0_usize;
    let mut both_pass = 0_usize;
    let mut both_fail = 0_usize;
    let mut rescued = 0_usize;
    let mut harmed = 0_usize;

    let mut cluster_map: BTreeMap<&str, PairedClusterCounts> = BTreeMap::new();
    let mut cost_pairs = Vec::new();
    let mut wall_pairs = Vec::new();
    let mut token_pairs = Vec::new();

    for (&(scen_id, seed), &b_trial) in &base_map {
        let Some(&t_trial) = treat_map.get(&(scen_id, seed)) else {
            continue;
        };
        paired_trials += 1;
        let cluster = cluster_map.entry(scen_id).or_default();
        cluster.total += 1;

        match (b_trial.passed, t_trial.passed) {
            (true, true) => both_pass += 1,
            (false, false) => both_fail += 1,
            (false, true) => {
                rescued += 1;
                cluster.rescued += 1;
            }
            (true, false) => {
                harmed += 1;
                cluster.harmed += 1;
            }
        }

        cost_pairs.push((
            b_trial.total_usage.cache_adjusted_usd(pricing),
            t_trial.total_usage.cache_adjusted_usd(pricing),
        ));
        wall_pairs.push((
            b_trial.total_usage.wall_duration_ms as f64,
            t_trial.total_usage.wall_duration_ms as f64,
        ));
        token_pairs.push((
            (b_trial.total_usage.total_input_tokens + b_trial.total_usage.output_tokens) as f64,
            (t_trial.total_usage.total_input_tokens + t_trial.total_usage.output_tokens) as f64,
        ));
    }

    let clusters: Vec<PairedClusterCounts> = cluster_map.into_values().collect();
    let (rescue_rate, harm_rate, net_pass_delta) =
        paired_cluster_bootstrap(&clusters, bootstrap_resamples, rng);

    let baseline_passes = both_pass + harmed;
    let preservation_harm_rate = if baseline_passes == 0 {
        None
    } else {
        Some(harmed as f64 / baseline_passes as f64)
    };

    let mcnemar_p_two_sided = mcnemar_exact_two_sided(rescued, harmed);
    let wilcoxon_cost_p_two_sided = wilcoxon_signed_rank_two_sided(&cost_pairs);
    let wilcoxon_wall_ms_p_two_sided = wilcoxon_signed_rank_two_sided(&wall_pairs);
    let wilcoxon_tokens_p_two_sided = wilcoxon_signed_rank_two_sided(&token_pairs);
    let underpowered = rescued.saturating_add(harmed) < MIN_DISCORDANT_FOR_POWER;

    PairedComparison {
        baseline_arm: baseline_id.to_string(),
        treatment_arm: treatment_id.to_string(),
        paired_trials,
        both_pass,
        both_fail,
        rescued,
        harmed,
        rescue_rate,
        harm_rate,
        preservation_harm_rate,
        net_pass_delta,
        mcnemar_p_two_sided,
        mcnemar_p_holm: mcnemar_p_two_sided,
        wilcoxon_cost_p_two_sided,
        wilcoxon_wall_ms_p_two_sided,
        wilcoxon_tokens_p_two_sided,
        underpowered,
    }
}

fn compute_by_category(trials: &[TrialRecord]) -> BTreeMap<String, BTreeMap<String, f64>> {
    let mut counts: BTreeMap<String, BTreeMap<String, (usize, usize)>> = BTreeMap::new();
    for trial in trials {
        let cat_entry = counts
            .entry(trial.category.as_str().to_string())
            .or_default();
        let arm_entry = cat_entry.entry(trial.arm_id.clone()).or_insert((0, 0));
        if trial.passed {
            arm_entry.0 += 1;
        }
        arm_entry.1 += 1;
    }

    counts
        .into_iter()
        .map(|(cat, arms)| {
            let rates = arms
                .into_iter()
                .filter_map(|(arm, (pass, total))| {
                    if total == 0 {
                        None
                    } else {
                        Some((arm, pass as f64 / total as f64))
                    }
                })
                .collect();
            (cat, rates)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crucible_core::{ScenarioCategory, UsageMetrics, VerificationOutcome};

    struct TrialSpec<'a> {
        scenario_id: &'a str,
        category: ScenarioCategory,
        arm_id: &'a str,
        seed: u32,
        passed: bool,
        wall_ms: u64,
        uncached_in: u64,
        cache_read: u64,
        out_tok: u64,
        tool_calls: u64,
        mcp_calls: u64,
        nano_aiu: u64,
    }

    fn make_trial(spec: TrialSpec<'_>) -> TrialRecord {
        let usage = UsageMetrics {
            api_requests: 1,
            uncached_input_tokens: spec.uncached_in,
            cache_read_tokens: spec.cache_read,
            cache_write_tokens: 0,
            total_input_tokens: spec.uncached_in + spec.cache_read,
            output_tokens: spec.out_tok,
            reasoning_tokens: 0,
            nano_aiu: spec.nano_aiu,
            api_duration_ms: spec.wall_ms,
            wall_duration_ms: spec.wall_ms,
            tool_calls: spec.tool_calls,
            mcp_tool_calls: spec.mcp_calls,
            lines_added: 5,
            lines_removed: 2,
            files_modified: 1,
        };
        TrialRecord {
            run_id: "run-001".to_string(),
            suite_id: "coding-memory-v1".to_string(),
            scenario_id: spec.scenario_id.to_string(),
            category: spec.category,
            arm_id: spec.arm_id.to_string(),
            seed: spec.seed,
            order_index: 0,
            passed: spec.passed,
            verification: VerificationOutcome {
                passed: spec.passed,
                exit_code: if spec.passed { 0 } else { 1 },
                stdout: String::new(),
                stderr: String::new(),
                violations: Vec::new(),
            },
            target_usage: usage.clone(),
            history_usage: UsageMetrics::default(),
            total_usage: usage,
            tool_trace: Vec::new(),
            agent_exit_code: 0,
        }
    }

    #[test]
    fn mcnemar_exact_values_and_large_n_stability() {
        assert_eq!(mcnemar_exact_two_sided(0, 0), 1.0);
        assert_eq!(mcnemar_exact_two_sided(4, 4), 1.0);
        assert_eq!(mcnemar_exact_two_sided(3, 4), 1.0);
        assert_eq!(mcnemar_exact_two_sided(0, 1), 1.0);

        // Binomial(6, 0.5): 2 * (1/64) = 0.03125
        assert!((mcnemar_exact_two_sided(6, 0) - 0.03125).abs() < 1e-15);
        assert!((mcnemar_exact_two_sided(0, 6) - 0.03125).abs() < 1e-15);

        // Binomial(9, 0.5), k = 1: 2 * (1 + 9) / 512 = 20 / 512 = 0.0390625
        assert!((mcnemar_exact_two_sided(1, 8) - 0.0390625).abs() < 1e-15);

        // Binomial(12, 0.5), k = 2: 2 * (1 + 12 + 66) / 4096 = 158 / 4096
        let expected_2_10 = 158.0 / 4096.0;
        assert!((mcnemar_exact_two_sided(2, 10) - expected_2_10).abs() < 1e-15);

        // Large n (> 1000) must neither overflow nor NaN
        assert_eq!(mcnemar_exact_two_sided(1500, 1500), 1.0);
        let p_large = mcnemar_exact_two_sided(900, 1100);
        assert!(p_large > 5e-6 && p_large < 1.2e-5, "p_large={p_large}");
        assert_eq!(mcnemar_exact_two_sided(0, 5000), 0.0);

        // Continuity across n = 1000 vs n = 1002 boundary
        let p_1000 = mcnemar_exact_two_sided(470, 530);
        let p_1002 = mcnemar_exact_two_sided(471, 531);
        assert!((p_1000 - p_1002).abs() < 0.005);
    }

    #[test]
    fn wilcoxon_exact_ties_and_normal_paths() {
        // Empty and near-zero differences
        assert_eq!(wilcoxon_signed_rank_two_sided(&[]), 1.0);
        assert_eq!(
            wilcoxon_signed_rank_two_sided(&[(1.0, 1.0), (2.0, 2.0 + 1e-13)]),
            1.0
        );

        // Exact small sample (n = 6, all positive distinct differences): p = 2 / 64 = 0.03125
        let six_pos: Vec<(f64, f64)> = (1..=6).map(|k| (0.0, k as f64)).collect();
        assert!((wilcoxon_signed_rank_two_sided(&six_pos) - 0.03125).abs() < 1e-15);

        // Exact n = 6 with smallest difference negative (W- = 1, W+ = 20): p = 4 / 64 = 0.0625
        let mut one_neg = six_pos.clone();
        one_neg[0] = (1.0, 0.0);
        assert!((wilcoxon_signed_rank_two_sided(&one_neg) - 0.0625).abs() < 1e-15);

        // Ties path: differences [1, 1, 3, 4, 5, 6] -> tie group t=2 at rank 1.5
        // mean = 10.5, var = 22.75 - 6/48 = 22.625, z = (10.5 - 0.5) / sqrt(22.625)
        let tied_pairs = vec![
            (0.0, 1.0),
            (2.0, 3.0),
            (0.0, 3.0),
            (0.0, 4.0),
            (0.0, 5.0),
            (0.0, 6.0),
        ];
        let p_tied = wilcoxon_signed_rank_two_sided(&tied_pairs);
        let z_expected = 10.0 / 22.625_f64.sqrt();
        let expected_tied = math::erfc(z_expected / std::f64::consts::SQRT_2);
        assert!((p_tied - expected_tied).abs() < 1e-14);

        // Large n > 20 normal approximation path: 25 strictly positive distinct differences
        let large_pos: Vec<(f64, f64)> = (1..=25).map(|k| (0.0, k as f64)).collect();
        let p_large = wilcoxon_signed_rank_two_sided(&large_pos);
        assert!(p_large > 0.0 && p_large < 1e-4, "p_large={p_large}");
    }

    #[test]
    fn holm_bonferroni_preserves_order_and_monotonicity() {
        assert!(holm_bonferroni(&[]).is_empty());
        assert_eq!(holm_bonferroni(&[0.04]), vec![0.04]);

        // Unsorted raw p-values: [0.04, 0.01, 0.012, 0.50]
        // Sorted: (idx 1, 0.01)*4 = 0.04
        //         (idx 2, 0.012)*3 = 0.036 -> max(0.04, 0.036) = 0.04 (step-down monotonicity!)
        //         (idx 0, 0.04)*2 = 0.08 -> max(0.04, 0.08) = 0.08
        //         (idx 3, 0.50)*1 = 0.50 -> 0.50
        let raw = [0.04, 0.01, 0.012, 0.50];
        let adj = holm_bonferroni(&raw);
        assert!((adj[1] - 0.04).abs() < 1e-15);
        assert!((adj[2] - 0.04).abs() < 1e-15);
        assert!((adj[0] - 0.08).abs() < 1e-15);
        assert!((adj[3] - 0.50).abs() < 1e-15);

        // Clamping at 1.0
        let big = holm_bonferroni(&[0.6, 0.8, 0.9]);
        assert_eq!(big, vec![1.0, 1.0, 1.0]);
    }

    #[test]
    fn cluster_bootstrap_respects_edge_cases_and_determinism() {
        let mut rng1 = SplitMix64::new(42);
        let single_cluster = cluster_bootstrap_ratio(&[(2, 3)], 5_000, &mut rng1);
        assert!((single_cluster.point - 2.0 / 3.0).abs() < 1e-15);
        assert_eq!(single_cluster.low, single_cluster.point);
        assert_eq!(single_cluster.high, single_cluster.point);

        let clusters = vec![(3, 3), (3, 3), (0, 3), (0, 3), (3, 3), (0, 3)];
        let zero_resamples = cluster_bootstrap_ratio(&clusters, 0, &mut rng1);
        assert_eq!(zero_resamples.point, 0.5);
        assert_eq!(zero_resamples.low, 0.5);
        assert_eq!(zero_resamples.high, 0.5);

        let mut rng_a = SplitMix64::new(99);
        let mut rng_b = SplitMix64::new(99);
        let ci_a = cluster_bootstrap_ratio(&clusters, 2_000, &mut rng_a);
        let ci_b = cluster_bootstrap_ratio(&clusters, 2_000, &mut rng_b);
        assert_eq!(ci_a, ci_b);
        assert!(ci_a.low < ci_a.point && ci_a.point < ci_a.high);
    }

    #[test]
    fn analyze_manifest_rescue_harm_pareto_and_rendering() {
        let mut trials = Vec::new();
        let categories = [
            ScenarioCategory::Handoff,
            ScenarioCategory::Supersession,
            ScenarioCategory::Applicability,
            ScenarioCategory::Diagnosis,
            ScenarioCategory::Retraction,
            ScenarioCategory::Poisoning,
            ScenarioCategory::Convention,
            ScenarioCategory::EarnedChain,
        ];

        // 8 scenarios x 2 seeds = 16 paired trials per arm:
        // - b0: passes scenarios 0, 1 (both seeds = 4 passes), fails scenarios 2..8 (12 fails)
        //   cheap cost (10k input, 500 output)
        // - b1-notes: passes scenario 0 (2 passes), harms scenario 1 (2 harmed!), rescues scenario 2 (2 rescued)
        //   higher cost (40k input, 1k output) -> same 4/16 pass rate as b0, strictly higher cost => DOMINATED!
        // - hivemind: passes scenarios 0, 1 (4 both_pass), rescues scenarios 2, 3, 4, 5 (8 rescued, 0 harmed!)
        //   moderate cost (15k input, 600 output) -> 12/16 pass rate, Pareto optimal!
        for (s_idx, &cat) in categories.iter().enumerate() {
            let scen = format!("scen-{s_idx:02}");
            for seed in 1..=2_u32 {
                let b0_pass = s_idx < 2;
                let b1_pass = s_idx == 0 || s_idx == 2;
                let hm_pass = s_idx < 6;

                trials.push(make_trial(TrialSpec {
                    scenario_id: &scen,
                    category: cat,
                    arm_id: "b0",
                    seed,
                    passed: b0_pass,
                    wall_ms: 1_000 * (s_idx as u64 + 1),
                    uncached_in: 5_000,
                    cache_read: 5_000,
                    out_tok: 500,
                    tool_calls: 4,
                    mcp_calls: 0,
                    nano_aiu: 10_000_000,
                }));
                trials.push(make_trial(TrialSpec {
                    scenario_id: &scen,
                    category: cat,
                    arm_id: "b1-notes",
                    seed,
                    passed: b1_pass,
                    wall_ms: 2_000 * (s_idx as u64 + 1),
                    uncached_in: 20_000,
                    cache_read: 20_000,
                    out_tok: 1_000,
                    tool_calls: 6,
                    mcp_calls: 0,
                    nano_aiu: 30_000_000,
                }));
                trials.push(make_trial(TrialSpec {
                    scenario_id: &scen,
                    category: cat,
                    arm_id: "hivemind",
                    seed,
                    passed: hm_pass,
                    wall_ms: 1_200 * (s_idx as u64 + 1),
                    uncached_in: 10_000,
                    cache_read: 5_000,
                    out_tok: 600,
                    tool_calls: 5,
                    mcp_calls: 2,
                    nano_aiu: 15_000_000,
                }));
            }
        }

        let manifest = RunManifest {
            run_id: "run-2026-test".to_string(),
            created_at_utc: "2026-11-18T00:00:00Z".to_string(),
            suite_id: "coding-memory-v1".to_string(),
            driver: "mock".to_string(),
            model: "claude-sonnet-4".to_string(),
            reasoning_effort: "medium".to_string(),
            seeds: vec![1, 2],
            arms: vec![
                "b0".to_string(),
                "b1-notes".to_string(),
                "hivemind".to_string(),
            ],
            pricing: ModelPricing::sonnet_default(),
            trials,
        };

        let report = analyze_manifest(&manifest, None, 2_000, 12345);
        assert_eq!(report.baseline_arm, "b0");
        assert_eq!(report.arms.len(), 3);
        assert_eq!(report.comparisons.len(), 2);

        let b0 = &report.arms[0];
        let b1 = &report.arms[1];
        let hm = &report.arms[2];

        assert_eq!(b0.trials, 16);
        assert_eq!(b0.scenarios, 8);
        assert_eq!(b0.seeds_per_scenario, 2);
        assert!((b0.pass_at_1.point - 0.25).abs() < 1e-15);
        assert!((b0.pass_all_seeds - 0.25).abs() < 1e-15);
        // Median of [1000,1000, 2000,2000, ..., 8000,8000] is (4000 + 5000)/2 = 4500
        assert!((b0.median_wall_ms - 4500.0).abs() < 1e-12);
        assert!(b0.pareto_optimal, "b0 is cheapest so Pareto optimal");
        assert!(
            !b1.pareto_optimal,
            "b1-notes has same pass rate as b0 at higher cost so dominated"
        );
        assert!(
            hm.pareto_optimal,
            "hivemind achieves 75% pass rate so Pareto optimal"
        );

        // Paired comparison: b1-notes vs b0
        let comp_b1 = &report.comparisons[0];
        assert_eq!(comp_b1.treatment_arm, "b1-notes");
        assert_eq!(comp_b1.paired_trials, 16);
        assert_eq!(comp_b1.both_pass, 2);
        assert_eq!(comp_b1.both_fail, 10);
        assert_eq!(comp_b1.rescued, 2);
        assert_eq!(comp_b1.harmed, 2);
        assert!((comp_b1.rescue_rate.point - 0.125).abs() < 1e-15);
        assert!((comp_b1.harm_rate.point - 0.125).abs() < 1e-15);
        // Preservation harm: 2 harmed out of (2 both_pass + 2 harmed) = 0.5
        assert_eq!(comp_b1.preservation_harm_rate, Some(0.5));
        assert!((comp_b1.net_pass_delta.point - 0.0).abs() < 1e-15);
        assert_eq!(comp_b1.mcnemar_p_two_sided, 1.0);
        assert!(comp_b1.underpowered, "2 + 2 = 4 < 6 discordant pairs");

        // Paired comparison: hivemind vs b0
        let comp_hm = &report.comparisons[1];
        assert_eq!(comp_hm.treatment_arm, "hivemind");
        assert_eq!(comp_hm.rescued, 8);
        assert_eq!(comp_hm.harmed, 0);
        assert_eq!(comp_hm.both_pass, 4);
        assert_eq!(comp_hm.both_fail, 4);
        assert_eq!(comp_hm.preservation_harm_rate, Some(0.0));
        assert!((comp_hm.net_pass_delta.point - 0.5).abs() < 1e-15);
        // McNemar(8, 0) = 2 / 256 = 0.0078125; Holm with m=2: 2 * 0.0078125 = 0.015625
        assert!((comp_hm.mcnemar_p_two_sided - 0.0078125).abs() < 1e-15);
        assert!((comp_hm.mcnemar_p_holm - 0.015625).abs() < 1e-15);
        assert!(!comp_hm.underpowered, "8 discordant pairs >= 6");

        // Category breakdown
        assert_eq!(report.by_category["handoff"]["b0"], 1.0);
        assert_eq!(report.by_category["applicability"]["b0"], 0.0);
        assert_eq!(report.by_category["applicability"]["hivemind"], 1.0);

        // Markdown and JSON rendering
        let md = render_markdown_report(&report);
        assert!(md.contains("run-2026-test"));
        assert!(md.contains("coding-memory-v1"));
        assert!(md.contains("claude-sonnet-4"));
        assert!(md.contains("| Arm | Pass@1 [95% CI] | Pass^k | Mean Wall (s) | Tool Calls (MCP) | In/Out Tok | Mean Cost ($) | Cost/Success ($) | Mean AIU | Pareto |"));
        assert!(md.contains("| Treatment vs Baseline | Paired N | Rescued | Harmed | Rescue Rate | Harm Rate (Preservation) | Net Uplift [95% CI] | McNemar p (Holm) | Wilcoxon Cost p | Wilcoxon Time p | Power |"));
        assert!(md.contains("## Pass Rate by Scenario Category"));

        let json = render_json_report(&report);
        let round_trip: EvaluationReport = serde_json::from_str(&json).expect("valid report JSON");
        assert_eq!(round_trip, report);
    }

    #[test]
    fn zero_passes_yields_none_for_conditional_denominators() {
        let trials = vec![
            make_trial(TrialSpec {
                scenario_id: "scen-1",
                category: ScenarioCategory::Diagnosis,
                arm_id: "b0",
                seed: 1,
                passed: false,
                wall_ms: 500,
                uncached_in: 5_000,
                cache_read: 0,
                out_tok: 200,
                tool_calls: 2,
                mcp_calls: 0,
                nano_aiu: 1_000_000,
            }),
            make_trial(TrialSpec {
                scenario_id: "scen-1",
                category: ScenarioCategory::Diagnosis,
                arm_id: "hivemind",
                seed: 1,
                passed: false,
                wall_ms: 600,
                uncached_in: 6_000,
                cache_read: 0,
                out_tok: 250,
                tool_calls: 3,
                mcp_calls: 1,
                nano_aiu: 1_500_000,
            }),
        ];
        let manifest = RunManifest {
            run_id: "run-zero".to_string(),
            created_at_utc: "2026-11-18T00:00:00Z".to_string(),
            suite_id: "suite-zero".to_string(),
            driver: "mock".to_string(),
            model: "claude-haiku-4".to_string(),
            reasoning_effort: "low".to_string(),
            seeds: vec![1],
            arms: vec!["b0".to_string(), "hivemind".to_string()],
            pricing: ModelPricing::haiku_default(),
            trials,
        };

        let report = analyze_manifest(&manifest, Some("b0"), 500, 7);
        assert_eq!(report.arms[0].cost_per_success_usd, None);
        assert_eq!(report.arms[0].uncached_cost_per_success_usd, None);
        assert_eq!(report.arms[0].aiu_per_success, None);
        assert_eq!(report.comparisons[0].preservation_harm_rate, None);

        let md = render_markdown_report(&report);
        assert!(md.contains("N/A"));
    }

    #[test]
    fn odd_median_pass_all_seeds_and_cost_per_success_math() {
        // 1 scenario x 3 seeds: 2 pass, 1 fail -> pass@1 = 2/3, pass^3 = 0.0
        // wall_ms = [900, 100, 400] -> sorted [100, 400, 900] -> median = 400
        // Sonnet pricing: $3/MTok uncached, $0.30/MTok cache_read, $15/MTok out
        // Per trial: 100_000 uncached ($0.30) + 100_000 cache_read ($0.03) + 10_000 out ($0.15)
        //   cache_adjusted_usd = $0.48 per trial, total over 3 trials = $1.44 -> cost_per_success (2 passes) = $0.72
        //   uncached_equiv_usd = 200_000 * $3/MTok ($0.60) + $0.15 = $0.75 per trial -> total $2.25 -> /2 = $1.125
        let trials = vec![
            make_trial(TrialSpec {
                scenario_id: "scen-a",
                category: ScenarioCategory::Handoff,
                arm_id: "arm-a",
                seed: 1,
                passed: true,
                wall_ms: 900,
                uncached_in: 100_000,
                cache_read: 100_000,
                out_tok: 10_000,
                tool_calls: 3,
                mcp_calls: 1,
                nano_aiu: 500_000_000,
            }),
            make_trial(TrialSpec {
                scenario_id: "scen-a",
                category: ScenarioCategory::Handoff,
                arm_id: "arm-a",
                seed: 2,
                passed: false,
                wall_ms: 100,
                uncached_in: 100_000,
                cache_read: 100_000,
                out_tok: 10_000,
                tool_calls: 3,
                mcp_calls: 1,
                nano_aiu: 500_000_000,
            }),
            make_trial(TrialSpec {
                scenario_id: "scen-a",
                category: ScenarioCategory::Handoff,
                arm_id: "arm-a",
                seed: 3,
                passed: true,
                wall_ms: 400,
                uncached_in: 100_000,
                cache_read: 100_000,
                out_tok: 10_000,
                tool_calls: 3,
                mcp_calls: 1,
                nano_aiu: 500_000_000,
            }),
        ];
        let manifest = RunManifest {
            run_id: "run-math".to_string(),
            created_at_utc: "2026-11-18T00:00:00Z".to_string(),
            suite_id: "suite-math".to_string(),
            driver: "mock".to_string(),
            model: "claude-sonnet-4".to_string(),
            reasoning_effort: "medium".to_string(),
            seeds: vec![1, 2, 3],
            arms: vec!["arm-a".to_string()],
            pricing: ModelPricing::sonnet_default(),
            trials,
        };

        let report = analyze_manifest(&manifest, None, 1_000, 1);
        assert_eq!(report.baseline_arm, "arm-a");
        assert!(report.comparisons.is_empty());

        let s = &report.arms[0];
        assert!((s.pass_at_1.point - 2.0 / 3.0).abs() < 1e-15);
        assert_eq!(s.pass_all_seeds, 0.0);
        assert_eq!(s.median_wall_ms, 400.0);
        assert!((s.mean_cache_adjusted_usd - 0.48).abs() < 1e-12);
        assert!((s.mean_uncached_equiv_usd - 0.75).abs() < 1e-12);
        assert!((s.cost_per_success_usd.unwrap() - 0.72).abs() < 1e-12);
        assert!((s.uncached_cost_per_success_usd.unwrap() - 1.125).abs() < 1e-12);
        assert!((s.aiu_per_success.unwrap() - 0.75).abs() < 1e-12);
        assert!(s.pareto_optimal);
    }

    #[test]
    fn pareto_handles_equal_cost_dominance_and_exact_ties() {
        // arm_tie_1 and arm_tie_2: identical 100% pass rate & identical cost -> both Pareto optimal
        // arm_worse: 0% pass rate & identical cost -> dominated by arm_tie_1/2 -> NOT Pareto optimal
        let trials = vec![
            make_trial(TrialSpec {
                scenario_id: "s1",
                category: ScenarioCategory::Convention,
                arm_id: "tie-1",
                seed: 1,
                passed: true,
                wall_ms: 100,
                uncached_in: 10_000,
                cache_read: 0,
                out_tok: 1_000,
                tool_calls: 1,
                mcp_calls: 0,
                nano_aiu: 1_000,
            }),
            make_trial(TrialSpec {
                scenario_id: "s1",
                category: ScenarioCategory::Convention,
                arm_id: "tie-2",
                seed: 1,
                passed: true,
                wall_ms: 100,
                uncached_in: 10_000,
                cache_read: 0,
                out_tok: 1_000,
                tool_calls: 1,
                mcp_calls: 0,
                nano_aiu: 1_000,
            }),
            make_trial(TrialSpec {
                scenario_id: "s1",
                category: ScenarioCategory::Convention,
                arm_id: "same-cost-worse-pass",
                seed: 1,
                passed: false,
                wall_ms: 100,
                uncached_in: 10_000,
                cache_read: 0,
                out_tok: 1_000,
                tool_calls: 1,
                mcp_calls: 0,
                nano_aiu: 1_000,
            }),
        ];
        let manifest = RunManifest {
            run_id: "run-pareto".to_string(),
            created_at_utc: "2026-11-18T00:00:00Z".to_string(),
            suite_id: "suite-pareto".to_string(),
            driver: "mock".to_string(),
            model: "claude-sonnet-4".to_string(),
            reasoning_effort: "medium".to_string(),
            seeds: vec![1],
            arms: vec![
                "tie-1".to_string(),
                "tie-2".to_string(),
                "same-cost-worse-pass".to_string(),
            ],
            pricing: ModelPricing::sonnet_default(),
            trials,
        };

        let report = analyze_manifest(&manifest, Some("same-cost-worse-pass"), 100, 9);
        assert!(report.arms[0].pareto_optimal);
        assert!(report.arms[1].pareto_optimal);
        assert!(!report.arms[2].pareto_optimal);
    }

    #[test]
    fn empty_manifest_is_nan_free_and_serializable() {
        let manifest = RunManifest {
            run_id: "run-empty".to_string(),
            created_at_utc: "2026-11-18T00:00:00Z".to_string(),
            suite_id: "suite-empty".to_string(),
            driver: "mock".to_string(),
            model: "claude-sonnet-4".to_string(),
            reasoning_effort: "medium".to_string(),
            seeds: Vec::new(),
            arms: Vec::new(),
            pricing: ModelPricing::sonnet_default(),
            trials: Vec::new(),
        };

        let report = analyze_manifest(&manifest, None, 1_000, 0);
        assert!(report.arms.is_empty());
        assert!(report.comparisons.is_empty());
        assert!(report.by_category.is_empty());

        let json = render_json_report(&report);
        let parsed: EvaluationReport = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, report);
        let md = render_markdown_report(&report);
        assert!(md.contains("run-empty"));
    }
}
