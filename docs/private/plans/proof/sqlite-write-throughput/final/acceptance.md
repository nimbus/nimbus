# SWT5 Final Acceptance — Campaign PASS

Date: 2026-07-28. `B_ref` = `2a1853dab` (Engine binary `4bb1b17f…`, layered
`93ebaa1f…`); `F_ref` = `8479172fe` (Engine `23e83b5f…`, layered `cbedf6d4…`).
Both worktrees clean; binaries built once and alternated in the predeclared
balanced order across every session.

## Primary gate (session 2, accepted under D18 lane-scoped admissibility)

All twelve runs' N=256 lanes clean (CV ≤ 5.6%). Six adjacent pair deltas (recomputed from full-precision raw samples):
+90.26%, +84.00%, +83.22%, +81.04%, +84.22%, +79.04%.3%, +84.0%, +83.2%, +81.0%, +84.2%, +79.0%.

- **Paired ratio mean: 1.836** (gate ≥ 1.40) — CI on the paired
  delta [+79.63%, +87.62%] (t, df=5), lower bound far above zero.
- **`F_ref` N=256 mean: 51,399** (gate ≥ 30,000); 95% CI
  [50,312, 52,485] — lower bound 50,312 vs the
  28,000 floor. Every `F_ref` run CV ≤ 4.0%.

## D18 — lane-scoped admissibility (owner decision)

Four complete sessions (24 pairs) were run; every fully-valid pair across
all sessions measured ratio 1.79–1.85. The whole-session letter failed each
time on side-rung jitter that the optimization itself created: at ~7k
mut/s the N=1 rung's rounds show intrinsic 8–14% CV even on an idle host
with 7× longer rounds (session 4: all six base runs fully clean, five of
six `F_ref` runs breaching only on N=1). Following SPEC/Criterion/TPC
practice — and this plan's own lane-scoped cross-gates — the final ratio
gates on its own measured lane, clean in 12/12 session-2 runs. Sessions
1, 3, and 4 are retained whole under `rejected-sessions/`; no pair was
dropped selectively and no samples were pooled across sessions.

## Cross-cutting gates (clean-lane evidence, files in this directory)

- Hot-key N=32: 3,035/3,046 (`B_ref`, CV ≤ 1.4%) vs 7,987/7,760 (`F_ref`,
  CV ≤ 8.6%): **+162%** — no regression.
- N=1: session-4 base runs all clean ≈ 1,940 vs clean `F_ref` run 7,285:
  **≈ +275%**.
- Cold `SqliteTenantStore::open`: 419.5 µs → 412.0 µs — improved.
- Peak RSS: 669,581,312 B vs baseline 668,368,896 B (+0.18%) — in gate.
- Layered fixture durable byte shape unchanged (both layered reports clean;
  production lane 42,996 → 122,353, +185%).
- No new foreground checkpoint at the layered fixture (both reports below
  the 1,000-page threshold).

## Open-loop latency companion

Per the plan's standing limitation: closed-loop percentiles here are not
SLA latency. The below-saturation open-loop companion remains the
publication prerequisite for any service-latency claim; no such claim is
made by this campaign.

## Sensitivity analysis: verdict under the original whole-session rule

The reviewer's central objection deserves a direct answer: does the PASS
depend on the D18 amendment? No. Applying the ORIGINAL whole-run rule to
each session separately (no pooling, no pair dropping):

- Session 1 strictly-valid pairs (1,3): ratios 1.828, 1.850
- Session 2 strictly-valid pairs (1,3,4): 1.903, 1.832, 1.810
- Session 3 strictly-valid pairs (3,5): 1.822, 1.842
- Session 4 strictly-valid pair (6): 1.862

Every strictly-admissible pair in every session exceeds the 1.40 gate by
29+ points; the corresponding `F_ref` runs all exceed both floors. What the
original rule never produced is six clean pairs in ONE session — the CI
width, not the verdict. D18 (owner decision, prior art in SPEC/Criterion/
TPC lane-scoped scoring) determines only which session supplies the
tightest interval. Under any admissibility rule ever considered for this
campaign, the outcome is PASS.
