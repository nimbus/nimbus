# Node FaaS Runtime Compatibility Proof

This directory will hold proof artifacts for
[`../../archive/node-faas-runtime-compatibility-plan.md`](../../archive/node-faas-runtime-compatibility-plan.md).

Each `nfrc<N>-<slug>.md` file must include:

- Date, authoring agent, git status summary, and relevant Node tags/SHAs.
- Files changed.
- Decisions made and alternatives rejected.
- Exact verification commands with concrete pass/fail counts or output
  summaries.
- Remaining risks tied to a later NFRC row or explicitly resolved.

The research baseline for the plan is
[`../../research/node-faas-runtime-compatibility-2026.md`](../../research/node-faas-runtime-compatibility-2026.md).

## Current Artifacts

- `nfrc0-baseline-and-control-plane.md`
- `nfrc1-faas-compat-profile.md`
- `nfrc2-latest-node-suite-tags.md`
- `nfrc3-node26-current-target.md`
- `nfrc4-latest-fixture-corpora.md`
- `nfrc5-node26-and-refresh-classification.md`
- `nfrc6-node24-default.md`
- `nfrc7-convex-app-canaries.md`
- `nfrc8-realistic-sdk-canaries.md`
- `nfrc9-host-heavy-diagnostics.md`
- `nfrc10-deno-style-docs.md`
- `nfrc11-release-train-automation.md`
- `nfrc12-ci-nightly-lanes.md`
- `nfrc13-closeout.md`
