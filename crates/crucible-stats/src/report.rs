//! Markdown and JSON report rendering for `EvaluationReport`.

use std::fmt::Write as _;

use crate::EvaluationReport;

/// Renders an [`EvaluationReport`] as pretty-printed JSON.
pub fn render_json_report(report: &EvaluationReport) -> String {
    let mut out =
        serde_json::to_string_pretty(report).expect("EvaluationReport must serialize to JSON");
    out.push('\n');
    out
}

/// Renders an [`EvaluationReport`] as publication-ready Markdown tables.
pub fn render_markdown_report(report: &EvaluationReport) -> String {
    let mut out = String::new();

    // 1. Run Summary header
    let _ = writeln!(out, "# Crucible Evaluation Report\n");
    let _ = writeln!(out, "- **Run ID:** `{}`", report.run_id);
    let _ = writeln!(out, "- **Suite ID:** `{}`", report.suite_id);
    let _ = writeln!(out, "- **Model:** `{}`", report.model);
    let _ = writeln!(out, "- **Baseline Arm:** `{}`\n", report.baseline_arm);

    // 2. Arm Performance & Cost Ledger table
    let _ = writeln!(out, "## Arm Performance & Cost Ledger\n");
    let _ = writeln!(
        out,
        "| Arm | Pass@1 [95% CI] | Pass^k | Mean Wall (s) | Tool Calls (MCP) | In/Out Tok | Mean Cost ($) | Cost/Success ($) | Mean AIU | Pareto |"
    );
    let _ = writeln!(
        out,
        "| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |"
    );

    for arm in &report.arms {
        let pass_ci = format!(
            "{:.1}% [{:.1}%, {:.1}%]",
            arm.pass_at_1.point * 100.0,
            arm.pass_at_1.low * 100.0,
            arm.pass_at_1.high * 100.0,
        );
        let pass_k = format!("{:.1}%", arm.pass_all_seeds * 100.0);
        let mean_wall_s = format!("{:.2}", arm.mean_wall_ms / 1000.0);
        let tools = format!(
            "{:.1} ({:.1})",
            arm.mean_tool_calls, arm.mean_mcp_tool_calls
        );
        let tokens = format!(
            "{:.0} / {:.0}",
            arm.mean_total_input_tokens, arm.mean_output_tokens
        );
        let mean_cost = format!("${:.4}", arm.mean_cache_adjusted_usd);
        let cost_per_success = match arm.cost_per_success_usd {
            Some(v) => format!("${v:.4}"),
            None => "N/A".to_string(),
        };
        let mean_aiu = format!("{:.4}", arm.mean_aiu);
        let pareto = if arm.pareto_optimal { "Yes" } else { "No" };

        let _ = writeln!(
            out,
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            arm.arm_id,
            pass_ci,
            pass_k,
            mean_wall_s,
            tools,
            tokens,
            mean_cost,
            cost_per_success,
            mean_aiu,
            pareto,
        );
    }
    let _ = writeln!(out);

    // 3. Paired Rescue vs. Harm & Statistical Tests table
    let _ = writeln!(out, "## Paired Rescue vs. Harm & Statistical Tests\n");
    let _ = writeln!(
        out,
        "| Treatment vs Baseline | Paired N | Rescued | Harmed | Rescue Rate | Harm Rate (Preservation) | Net Uplift [95% CI] | McNemar p (Holm) | Wilcoxon Cost p | Wilcoxon Time p | Power |"
    );
    let _ = writeln!(
        out,
        "| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |"
    );

    for comp in &report.comparisons {
        let pair_label = format!("`{}` vs `{}`", comp.treatment_arm, comp.baseline_arm);
        let rescue_rate = format!(
            "{:.1}% [{:.1}%, {:.1}%]",
            comp.rescue_rate.point * 100.0,
            comp.rescue_rate.low * 100.0,
            comp.rescue_rate.high * 100.0,
        );
        let pres_rate = match comp.preservation_harm_rate {
            Some(v) => format!("{:.1}%", (1.0 - v) * 100.0),
            None => "N/A".to_string(),
        };
        let harm_col = format!("{:.1}% ({pres_rate})", comp.harm_rate.point * 100.0);
        let net_uplift = format!(
            "{:+.1}% [{:+.1}%, {:+.1}%]",
            comp.net_pass_delta.point * 100.0,
            comp.net_pass_delta.low * 100.0,
            comp.net_pass_delta.high * 100.0,
        );
        let mcnemar_col = format!(
            "{} ({})",
            fmt_p(comp.mcnemar_p_two_sided),
            fmt_p(comp.mcnemar_p_holm),
        );
        let wilcoxon_cost = fmt_p(comp.wilcoxon_cost_p_two_sided);
        let wilcoxon_time = fmt_p(comp.wilcoxon_wall_ms_p_two_sided);
        let power = if comp.underpowered {
            "Underpowered"
        } else {
            "Adequate"
        };

        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            pair_label,
            comp.paired_trials,
            comp.rescued,
            comp.harmed,
            rescue_rate,
            harm_col,
            net_uplift,
            mcnemar_col,
            wilcoxon_cost,
            wilcoxon_time,
            power,
        );
    }
    let _ = writeln!(out);

    // 4. Pass Rate by Scenario Category table
    let _ = writeln!(out, "## Pass Rate by Scenario Category\n");
    let mut arm_columns: Vec<String> = report.arms.iter().map(|a| a.arm_id.clone()).collect();
    for rates in report.by_category.values() {
        for arm_id in rates.keys() {
            if !arm_columns.contains(arm_id) {
                arm_columns.push(arm_id.clone());
            }
        }
    }

    let mut header = String::from("| Category |");
    let mut divider = String::from("| :--- |");
    for arm_id in &arm_columns {
        let _ = write!(header, " `{arm_id}` |");
        divider.push_str(" :---: |");
    }
    let _ = writeln!(out, "{header}");
    let _ = writeln!(out, "{divider}");

    for (category, rates) in &report.by_category {
        let mut row = format!("| `{category}` |");
        for arm_id in &arm_columns {
            let cell = match rates.get(arm_id) {
                Some(rate) => format!("{:.1}%", rate * 100.0),
                None => "N/A".to_string(),
            };
            let _ = write!(row, " {cell} |");
        }
        let _ = writeln!(out, "{row}");
    }

    out
}

fn fmt_p(p: f64) -> String {
    if p < 0.0001 {
        "<0.0001".to_string()
    } else {
        format!("{p:.4}")
    }
}
