# Crucible Evaluation Report

- **Run ID:** `latent-memory-v1-1795052655`
- **Suite ID:** `latent-memory-v1`
- **Model:** `claude-sonnet-5.5`
- **Baseline Arm:** `b0`

## Arm Performance & Cost Ledger

| Arm | Pass@1 [95% CI] | Pass^k | Mean Wall (s) | Tool Calls (MCP) | In/Out Tok | Mean Cost ($) | Cost/Success ($) | Mean AIU | Pareto |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| `b0` | 54.2% [20.8%, 87.5%] | 50.0% | 24.72 | 7.3 (0.0) | 196445 / 3050 | $0.1569 | $0.2896 | 8.6455 | Yes |
| `b1-notes` | 95.8% [87.5%, 100.0%] | 87.5% | 28.06 | 8.9 (0.0) | 278705 / 3407 | $0.2306 | $0.2406 | 12.8650 | Yes |
| `hivemind` | 83.3% [58.3%, 100.0%] | 75.0% | 25.24 | 9.2 (1.5) | 228019 / 3146 | $0.1765 | $0.2118 | 9.6648 | Yes |

## Paired Rescue vs. Harm & Statistical Tests

| Treatment vs Baseline | Paired N | Rescued | Harmed | Rescue Rate | Harm Rate (Preservation) | Net Uplift [95% CI] | McNemar p (Holm) | Wilcoxon Cost p | Wilcoxon Time p | Power |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| `b1-notes` vs `b0` | 24 | 10 | 0 | 41.7% [12.5%, 70.8%] | 0.0% (100.0%) | +41.7% [+12.5%, +70.8%] | 0.0020 (0.0039) | 0.0002 | 0.4663 | Adequate |
| `hivemind` vs `b0` | 24 | 7 | 0 | 29.2% [8.3%, 54.2%] | 0.0% (100.0%) | +29.2% [+8.3%, +54.2%] | 0.0156 (0.0156) | 0.4663 | 0.5971 | Adequate |

## Pass Rate by Scenario Category

| Category | `b0` | `b1-notes` | `hivemind` |
| :--- | :---: | :---: | :---: |
| `applicability` | 100.0% | 100.0% | 100.0% |
| `convention` | 0.0% | 100.0% | 33.3% |
| `diagnosis` | 100.0% | 100.0% | 100.0% |
| `earned_chain` | 0.0% | 66.7% | 33.3% |
| `handoff` | 0.0% | 100.0% | 100.0% |
| `poisoning` | 100.0% | 100.0% | 100.0% |
| `retraction` | 33.3% | 100.0% | 100.0% |
| `supersession` | 100.0% | 100.0% | 100.0% |
