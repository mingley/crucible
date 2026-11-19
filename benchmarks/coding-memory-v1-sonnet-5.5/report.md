# Crucible Evaluation Report

- **Run ID:** `coding-memory-v1-1795057808`
- **Suite ID:** `coding-memory-v1`
- **Model:** `claude-sonnet-5.5`
- **Baseline Arm:** `b0`

## Arm Performance & Cost Ledger

| Arm | Pass@1 [95% CI] | Pass^k | Mean Wall (s) | Tool Calls (MCP) | In/Out Tok | Mean Cost ($) | Cost/Success ($) | Mean AIU | Pareto |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| `b0` | 95.8% [87.5%, 100.0%] | 87.5% | 24.38 | 7.7 (0.0) | 188962 / 3009 | $0.1523 | $0.1589 | 8.4074 | No |
| `b1-notes` | 100.0% [100.0%, 100.0%] | 100.0% | 21.44 | 6.4 (0.0) | 177710 / 2535 | $0.1482 | $0.1482 | 8.2649 | No |
| `hivemind` | 100.0% [100.0%, 100.0%] | 100.0% | 19.85 | 5.9 (0.0) | 173667 / 2374 | $0.1329 | $0.1329 | 7.2575 | Yes |

## Paired Rescue vs. Harm & Statistical Tests

| Treatment vs Baseline | Paired N | Rescued | Harmed | Rescue Rate | Harm Rate (Preservation) | Net Uplift [95% CI] | McNemar p (Holm) | Wilcoxon Cost p | Wilcoxon Time p | Power |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| `b1-notes` vs `b0` | 24 | 1 | 0 | 4.2% [0.0%, 12.5%] | 0.0% (100.0%) | +4.2% [+0.0%, +12.5%] | 1.0000 (1.0000) | 0.7210 | 0.0146 | Underpowered |
| `hivemind` vs `b0` | 24 | 1 | 0 | 4.2% [0.0%, 12.5%] | 0.0% (100.0%) | +4.2% [+0.0%, +12.5%] | 1.0000 (1.0000) | 0.0031 | 0.0011 | Underpowered |

## Pass Rate by Scenario Category

| Category | `b0` | `b1-notes` | `hivemind` |
| :--- | :---: | :---: | :---: |
| `applicability` | 100.0% | 100.0% | 100.0% |
| `convention` | 100.0% | 100.0% | 100.0% |
| `diagnosis` | 100.0% | 100.0% | 100.0% |
| `earned_chain` | 100.0% | 100.0% | 100.0% |
| `handoff` | 100.0% | 100.0% | 100.0% |
| `poisoning` | 100.0% | 100.0% | 100.0% |
| `retraction` | 66.7% | 100.0% | 100.0% |
| `supersession` | 100.0% | 100.0% | 100.0% |
