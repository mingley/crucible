//! Non-parametric paired hypothesis tests, multiple-testing corrections, and cluster bootstrap.

use crate::Ci95;
use crate::prng::SplitMix64;

const ZERO_TOL: f64 = 1e-12;

/// Exact two-sided McNemar test p-value on discordant counts `(b, c)`.
///
/// Under the null hypothesis, `b ~ Binomial(b + c, 0.5)`.
pub fn mcnemar_exact_two_sided(b: usize, c: usize) -> f64 {
    let n = b.saturating_add(c);
    if n == 0 || b == c || b.abs_diff(c) == 1 {
        return 1.0;
    }
    let k = b.min(c);

    if n <= 1000 {
        let mut term = 2.0_f64.powi(-(n as i32));
        let mut sum = term;
        for i in 1..=k {
            term *= (n - i + 1) as f64 / (i as f64);
            sum += term;
        }
        return (2.0 * sum).clamp(0.0, 1.0);
    }

    // For large n (> 1000), start at the largest tail probability P(X = k) in log-space
    // and walk downward so intermediate terms never overflow or prematurely underflow.
    let ln_pk = ln_factorial(n)
        - ln_factorial(k)
        - ln_factorial(n - k)
        - (n as f64) * std::f64::consts::LN_2;
    if ln_pk < -750.0 {
        return 0.0;
    }
    let pk = ln_pk.exp();
    let mut rel_term = 1.0_f64;
    let mut rel_sum = 1.0_f64;
    for i in (1..=k).rev() {
        rel_term *= (i as f64) / ((n - i + 1) as f64);
        rel_sum += rel_term;
        if rel_term < 1e-16 * rel_sum {
            break;
        }
    }
    (2.0 * pk * rel_sum).clamp(0.0, 1.0)
}

/// Paired two-sided Wilcoxon signed-rank test p-value.
///
/// Computes `d_i = y_i - x_i`, drops near-zeros (`|d_i| <= 1e-12`), assigns average
/// ranks to tied magnitudes, uses the exact `2^n` sign distribution when `n_nonzero <= 20`
/// without ties, and otherwise uses the continuity-corrected normal approximation with
/// exact tie variance adjustment.
pub fn wilcoxon_signed_rank_two_sided(pairs: &[(f64, f64)]) -> f64 {
    let mut diffs: Vec<(f64, bool)> = pairs
        .iter()
        .filter_map(|&(x, y)| {
            let d = y - x;
            if d.is_finite() && d.abs() > ZERO_TOL {
                Some((d.abs(), d > 0.0))
            } else {
                None
            }
        })
        .collect();

    let n = diffs.len();
    if n == 0 {
        return 1.0;
    }

    diffs.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut w_plus = 0.0_f64;
    let mut w_minus = 0.0_f64;
    let mut has_ties = false;
    let mut tie_sum = 0.0_f64;

    let mut i = 0;
    while i < n {
        let base_mag = diffs[i].0;
        let mut j = i + 1;
        while j < n {
            let next_mag = diffs[j].0;
            let tol = ZERO_TOL * 1.0_f64.max(next_mag);
            if next_mag - base_mag <= tol {
                j += 1;
            } else {
                break;
            }
        }
        let group_len = j - i;
        if group_len > 1 {
            has_ties = true;
            let t = group_len as f64;
            tie_sum += t * (t * t - 1.0);
        }
        let avg_rank = ((i + 1) + j) as f64 / 2.0;
        for &(_, is_pos) in &diffs[i..j] {
            if is_pos {
                w_plus += avg_rank;
            } else {
                w_minus += avg_rank;
            }
        }
        i = j;
    }

    if n <= 20 && !has_ties {
        return wilcoxon_exact_no_ties(n, w_plus.round() as usize, w_minus.round() as usize);
    }

    let w_obs = w_plus.min(w_minus);
    let nf = n as f64;
    let mean = nf * (nf + 1.0) / 4.0;
    let variance = nf * (nf + 1.0) * (2.0 * nf + 1.0) / 24.0 - tie_sum / 48.0;
    if variance <= 0.0 {
        return 1.0;
    }

    let delta_cc = ((mean - w_obs).abs() - 0.5).max(0.0);
    let z = delta_cc / variance.sqrt();
    erfc(z / std::f64::consts::SQRT_2).clamp(0.0, 1.0)
}

/// Exact two-sided Wilcoxon signed-rank p-value over all `2^n` sign assignments (`n <= 20`, no ties).
fn wilcoxon_exact_no_ties(n: usize, w_plus: usize, w_minus: usize) -> f64 {
    let total_rank_sum = n * (n + 1) / 2;
    let w_obs = w_plus.min(w_minus);
    if 2 * w_obs >= total_rank_sum {
        return 1.0;
    }

    // Exact subset-sum distribution of W+ across all 2^n sign assignments of ranks 1..=n.
    let mut dp = vec![0_u64; total_rank_sum + 1];
    dp[0] = 1;
    let mut current_max = 0;
    for rank in 1..=n {
        current_max += rank;
        for s in (rank..=current_max).rev() {
            dp[s] += dp[s - rank];
        }
    }

    let extreme_assignments: u64 = (0..=total_rank_sum)
        .filter(|&s| s.min(total_rank_sum - s) <= w_obs)
        .map(|s| dp[s])
        .sum();

    let total_assignments = 1_u64 << n;
    (extreme_assignments as f64 / total_assignments as f64).clamp(0.0, 1.0)
}

/// Holm-Bonferroni step-down family-wise error rate adjustment.
pub fn holm_bonferroni(raw_p: &[f64]) -> Vec<f64> {
    let m = raw_p.len();
    if m == 0 {
        return Vec::new();
    }

    let mut indexed: Vec<(usize, f64)> = raw_p.iter().copied().enumerate().collect();
    indexed.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));

    let mut adjusted = vec![0.0_f64; m];
    let mut running_max = 0.0_f64;
    for (k, &(orig_idx, p)) in indexed.iter().enumerate() {
        let p_clean = if p.is_nan() { 1.0 } else { p.clamp(0.0, 1.0) };
        let scaled = ((m - k) as f64) * p_clean;
        let adj = scaled.max(running_max).clamp(0.0, 1.0);
        running_max = adj;
        adjusted[orig_idx] = adj;
    }
    adjusted
}

/// Cluster bootstrap 95% confidence interval for a scenario-clustered ratio `sum(passed) / sum(total)`.
pub(crate) fn cluster_bootstrap_ratio(
    clusters: &[(usize, usize)],
    resamples: usize,
    rng: &mut SplitMix64,
) -> Ci95 {
    let total_num: usize = clusters.iter().map(|c| c.0).sum();
    let total_den: usize = clusters.iter().map(|c| c.1).sum();
    let point = if total_den == 0 {
        0.0
    } else {
        total_num as f64 / total_den as f64
    };

    let s = clusters.len();
    if s <= 1 || resamples == 0 || total_den == 0 {
        return Ci95 {
            point,
            low: point,
            high: point,
        };
    }

    let mut draws = Vec::with_capacity(resamples);
    for _ in 0..resamples {
        let mut num = 0_usize;
        let mut den = 0_usize;
        for _ in 0..s {
            let (p, n) = clusters[rng.next_index(s)];
            num += p;
            den += n;
        }
        draws.push(if den == 0 {
            0.0
        } else {
            num as f64 / den as f64
        });
    }

    percentile_ci95(point, &mut draws)
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PairedClusterCounts {
    pub(crate) total: usize,
    pub(crate) rescued: usize,
    pub(crate) harmed: usize,
}

/// Joint paired cluster bootstrap for `(rescue_rate, harm_rate, net_pass_delta)` across scenarios.
pub(crate) fn paired_cluster_bootstrap(
    clusters: &[PairedClusterCounts],
    resamples: usize,
    rng: &mut SplitMix64,
) -> (Ci95, Ci95, Ci95) {
    let total: usize = clusters.iter().map(|c| c.total).sum();
    let rescued: usize = clusters.iter().map(|c| c.rescued).sum();
    let harmed: usize = clusters.iter().map(|c| c.harmed).sum();

    let (rescue_pt, harm_pt, delta_pt) = if total == 0 {
        (0.0, 0.0, 0.0)
    } else {
        let den = total as f64;
        (
            rescued as f64 / den,
            harmed as f64 / den,
            (rescued as f64 - harmed as f64) / den,
        )
    };

    let s = clusters.len();
    if s <= 1 || resamples == 0 || total == 0 {
        return (
            Ci95 {
                point: rescue_pt,
                low: rescue_pt,
                high: rescue_pt,
            },
            Ci95 {
                point: harm_pt,
                low: harm_pt,
                high: harm_pt,
            },
            Ci95 {
                point: delta_pt,
                low: delta_pt,
                high: delta_pt,
            },
        );
    }

    let mut rescue_draws = Vec::with_capacity(resamples);
    let mut harm_draws = Vec::with_capacity(resamples);
    let mut delta_draws = Vec::with_capacity(resamples);

    for _ in 0..resamples {
        let mut tot = 0_usize;
        let mut resc = 0_usize;
        let mut harm = 0_usize;
        for _ in 0..s {
            let c = clusters[rng.next_index(s)];
            tot += c.total;
            resc += c.rescued;
            harm += c.harmed;
        }
        if tot == 0 {
            rescue_draws.push(0.0);
            harm_draws.push(0.0);
            delta_draws.push(0.0);
        } else {
            let den = tot as f64;
            rescue_draws.push(resc as f64 / den);
            harm_draws.push(harm as f64 / den);
            delta_draws.push((resc as f64 - harm as f64) / den);
        }
    }

    (
        percentile_ci95(rescue_pt, &mut rescue_draws),
        percentile_ci95(harm_pt, &mut harm_draws),
        percentile_ci95(delta_pt, &mut delta_draws),
    )
}

fn percentile_ci95(point: f64, samples: &mut [f64]) -> Ci95 {
    if samples.is_empty() {
        return Ci95 {
            point,
            low: point,
            high: point,
        };
    }
    samples.sort_by(|a, b| a.total_cmp(b));
    let low = sorted_quantile(samples, 0.025);
    let high = sorted_quantile(samples, 0.975);
    Ci95 { point, low, high }
}

fn sorted_quantile(sorted: &[f64], q: f64) -> f64 {
    debug_assert!(!sorted.is_empty());
    if sorted.len() == 1 {
        return sorted[0];
    }
    let pos = q.clamp(0.0, 1.0) * ((sorted.len() - 1) as f64);
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let frac = pos - (lo as f64);
    sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

/// Complementary error function `erfc(x) = (2 / sqrt(pi)) * int_x^inf exp(-t^2) dt`
/// accurate to ~15 significant digits across `(-inf, +inf)` in pure Rust.
pub(crate) fn erfc(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x < 0.0 {
        return 2.0 - erfc_nonneg(-x);
    }
    erfc_nonneg(x)
}

fn erfc_nonneg(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    if x > 27.0 {
        return 0.0;
    }
    if x < 1.5 {
        // Positive-term Kummer series: erf(x) = (2/sqrt(pi)) * exp(-x^2) * sum_{k>=0} (2^k x^(2k+1)) / (2k+1)!!
        let two_x2 = 2.0 * x * x;
        let mut term = x;
        let mut sum = term;
        for k in 1..200 {
            term *= two_x2 / ((2 * k + 1) as f64);
            sum += term;
            if term <= f64::EPSILON * sum {
                break;
            }
        }
        let erf_val = (2.0 / std::f64::consts::PI.sqrt()) * (-x * x).exp() * sum;
        return (1.0 - erf_val).clamp(0.0, 1.0);
    }

    // Stieltjes continued fraction via modified Lentz's method for x >= 1.5:
    // erfc(x) = (exp(-x^2) / sqrt(pi)) * 1 / (x + (1/2) / (x + (2/2) / (x + (3/2) / ...)))
    let tiny = 1e-30_f64;
    let mut f = x.max(tiny);
    let mut c = f;
    let mut d = 0.0_f64;

    for m in 1..600 {
        let a = (m as f64) * 0.5;
        d = x + a * d;
        if d.abs() < tiny {
            d = tiny;
        }
        c = x + a / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        let delta = c * d;
        f *= delta;
        if (delta - 1.0).abs() <= 1e-15 {
            break;
        }
    }

    ((-x * x).exp() / std::f64::consts::PI.sqrt() / f).clamp(0.0, 1.0)
}

fn ln_factorial(n: usize) -> f64 {
    const EXACT: [f64; 21] = [
        1.0,
        1.0,
        2.0,
        6.0,
        24.0,
        120.0,
        720.0,
        5_040.0,
        40_320.0,
        362_880.0,
        3_628_800.0,
        39_916_800.0,
        479_001_600.0,
        6_227_020_800.0,
        87_178_291_200.0,
        1_307_674_368_000.0,
        20_922_789_888_000.0,
        355_687_428_096_000.0,
        6_402_373_705_728_000.0,
        121_645_100_408_832_000.0,
        2_432_902_008_176_640_000.0,
    ];
    if n < EXACT.len() {
        return EXACT[n].ln();
    }
    ln_gamma((n as f64) + 1.0)
}

fn ln_gamma(z: f64) -> f64 {
    const COEFFS: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    let x = z - 1.0;
    let mut sum = COEFFS[0];
    for (i, &c) in COEFFS.iter().enumerate().skip(1) {
        sum += c / (x + (i as f64));
    }
    let t = x + 7.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + sum.ln()
}

#[cfg(test)]
mod internal_tests {
    use super::*;

    #[test]
    fn erfc_series_and_continued_fraction_agree_at_boundary() {
        for step in 12..=20 {
            let x = step as f64 * 0.1;
            let two_x2 = 2.0 * x * x;
            let mut term = x;
            let mut sum = term;
            for k in 1..200 {
                term *= two_x2 / ((2 * k + 1) as f64);
                sum += term;
                if term <= f64::EPSILON * sum {
                    break;
                }
            }
            let from_series = 1.0 - (2.0 / std::f64::consts::PI.sqrt()) * (-x * x).exp() * sum;

            let tiny = 1e-30_f64;
            let mut f = x;
            let mut c = f;
            let mut d = 0.0_f64;
            for m in 1..600 {
                let a = (m as f64) * 0.5;
                d = 1.0 / (x + a * d).max(tiny);
                c = (x + a / c).max(tiny);
                let delta = c * d;
                f *= delta;
                if (delta - 1.0).abs() <= 1e-15 {
                    break;
                }
            }
            let from_cf = (-x * x).exp() / std::f64::consts::PI.sqrt() / f;

            let rel_err = (from_series - from_cf).abs() / from_cf;
            assert!(
                rel_err < 1e-13,
                "x={x}: series={from_series} cf={from_cf} rel_err={rel_err}"
            );
        }
    }

    #[test]
    fn erfc_matches_reference_normal_tails() {
        assert!((erfc(0.0) - 1.0).abs() < 1e-15);
        assert!((erfc(1.0) + erfc(-1.0) - 2.0).abs() < 1e-15);
        assert!((erfc(1.959_963_984_540_054 / std::f64::consts::SQRT_2) - 0.05).abs() < 1e-14);
        assert!((erfc(2.575_829_303_548_900_4 / std::f64::consts::SQRT_2) - 0.01).abs() < 1e-14);
        let p_z6 = erfc(6.0 / std::f64::consts::SQRT_2);
        assert!((p_z6 - 1.973_175_290_075_396e-9).abs() / 1.973_175_290_075_396e-9 < 1e-12);
    }

    #[test]
    fn wilcoxon_dp_matches_bruteforce_sign_enumeration() {
        for n in 1..=12_usize {
            let total_sum = n * (n + 1) / 2;
            for w_plus in 0..=total_sum {
                let w_minus = total_sum - w_plus;
                let w_obs = w_plus.min(w_minus);
                let mut brute = 0_u64;
                for mask in 0..(1_u32 << n) {
                    let mut wp = 0_usize;
                    for bit in 0..n {
                        if (mask & (1_u32 << bit)) != 0 {
                            wp += bit + 1;
                        }
                    }
                    if wp.min(total_sum - wp) <= w_obs {
                        brute += 1;
                    }
                }
                let expected = brute as f64 / ((1_u64 << n) as f64);
                let got = wilcoxon_exact_no_ties(n, w_plus, w_minus);
                assert!(
                    (got - expected).abs() < 1e-15,
                    "n={n}, w_plus={w_plus}: got={got}, expected={expected}"
                );
            }
        }
    }
}
