# Crucible Evaluation Report

- **Run ID:** `coding-memory-v1-1795051685`
- **Suite ID:** `coding-memory-v1`
- **Model:** `claude-sonnet-5.5`
- **Baseline Arm:** `b0`

## Arm Performance & Cost Ledger

| Arm | Pass@1 [95% CI] | Pass^k | Mean Wall (s) | Tool Calls (MCP) | In/Out Tok | Mean Cost ($) | Cost/Success ($) | Mean AIU | Pareto |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| `b0` | 95.8% [87.5%, 100.0%] | 87.5% | 22.94 | 7.0 (0.0) | 185590 / 2881 | $0.1482 | $0.1546 | 8.1640 | Yes |
| `b1-notes` | 100.0% [100.0%, 100.0%] | 100.0% | 21.88 | 6.6 (0.0) | 189705 / 2718 | $0.1564 | $0.1564 | 8.6965 | Yes |
| `hivemind` | 95.8% [87.5%, 100.0%] | 87.5% | 22.19 | 8.7 (1.4) | 198856 / 2824 | $0.1569 | $0.1637 | 8.6320 | No |

## Paired Rescue vs. Harm & Statistical Tests

| Treatment vs Baseline | Paired N | Rescued | Harmed | Rescue Rate | Harm Rate (Preservation) | Net Uplift [95% CI] | McNemar p (Holm) | Wilcoxon Cost p | Wilcoxon Time p | Power |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| `b1-notes` vs `b0` | 24 | 1 | 0 | 4.2% [0.0%, 12.5%] | 0.0% (100.0%) | +4.2% [+0.0%, +12.5%] | 1.0000 (1.0000) | 0.3104 | 0.1747 | Underpowered |
| `hivemind` vs `b0` | 24 | 1 | 1 | 4.2% [0.0%, 12.5%] | 4.2% (95.7%) | +0.0% [-12.5%, +12.5%] | 1.0000 (1.0000) | 0.1264 | 0.1264 | Underpowered |

## Pass Rate by Scenario Category

| Category | `b0` | `b1-notes` | `hivemind` |
| :--- | :---: | :---: | :---: |
| `applicability` | 100.0% | 100.0% | 100.0% |
| `convention` | 100.0% | 100.0% | 100.0% |
| `diagnosis` | 100.0% | 100.0% | 66.7% |
| `earned_chain` | 100.0% | 100.0% | 100.0% |
| `handoff` | 100.0% | 100.0% | 100.0% |
| `poisoning` | 100.0% | 100.0% | 100.0% |
| `retraction` | 66.7% | 100.0% | 100.0% |
| `supersession` | 100.0% | 100.0% | 100.0% |
