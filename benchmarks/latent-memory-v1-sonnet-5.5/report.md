# Crucible Evaluation Report

- **Run ID:** `latent-memory-v1-1795059067`
- **Suite ID:** `latent-memory-v1`
- **Model:** `claude-sonnet-5.5`
- **Baseline Arm:** `b0`

## Arm Performance & Cost Ledger

| Arm | Pass@1 [95% CI] | Pass^k | Mean Wall (s) | Tool Calls (MCP) | In/Out Tok | Mean Cost ($) | Cost/Success ($) | Mean AIU | Pareto |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| `b0` | 50.0% [20.8%, 79.2%] | 37.5% | 28.35 | 7.5 (0.0) | 198221 / 3090 | $0.1580 | $0.3160 | 8.7013 | No |
| `b1-notes` | 91.7% [75.0%, 100.0%] | 87.5% | 27.64 | 6.9 (0.0) | 246235 / 3034 | $0.2155 | $0.2351 | 12.1827 | No |
| `hivemind` | 100.0% [100.0%, 100.0%] | 100.0% | 23.24 | 7.2 (0.1) | 205206 / 2711 | $0.1535 | $0.1535 | 8.3291 | Yes |

## Paired Rescue vs. Harm & Statistical Tests

| Treatment vs Baseline | Paired N | Rescued | Harmed | Rescue Rate | Harm Rate (Preservation) | Net Uplift [95% CI] | McNemar p (Holm) | Wilcoxon Cost p | Wilcoxon Time p | Power |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| `b1-notes` vs `b0` | 24 | 10 | 0 | 41.7% [16.7%, 70.8%] | 0.0% (100.0%) | +41.7% [+16.7%, +70.8%] | 0.0020 (0.0020) | 0.0019 | 0.7210 | Adequate |
| `hivemind` vs `b0` | 24 | 12 | 0 | 50.0% [20.8%, 79.2%] | 0.0% (100.0%) | +50.0% [+20.8%, +79.2%] | 0.0005 (0.0010) | 0.1573 | 0.0440 | Adequate |

## Pass Rate by Scenario Category

| Category | `b0` | `b1-notes` | `hivemind` |
| :--- | :---: | :---: | :---: |
| `applicability` | 100.0% | 100.0% | 100.0% |
| `convention` | 0.0% | 100.0% | 100.0% |
| `diagnosis` | 66.7% | 100.0% | 100.0% |
| `earned_chain` | 0.0% | 33.3% | 100.0% |
| `handoff` | 0.0% | 100.0% | 100.0% |
| `poisoning` | 100.0% | 100.0% | 100.0% |
| `retraction` | 33.3% | 100.0% | 100.0% |
| `supersession` | 100.0% | 100.0% | 100.0% |
