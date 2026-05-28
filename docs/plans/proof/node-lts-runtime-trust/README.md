# Node LTS Runtime Trust Proofs

This directory stores proof artifacts for the completed
`docs/plans/archive/node-lts-runtime-trust-plan.md`.

Rules:

- Write one proof file per ledger row:
  `nlrt<N>-<slug>.md`.
- Include date, agent, git status summary, changed files, decisions,
  verification commands, and concrete results.
- Do not mark an NLRT row `done` until its proof file exists and the row's
  acceptance criteria in the plan pass.
- If context is compacted, resume from the plan ledger, this directory, and the
  execution log in the plan.

Current state: plan archived; NLRT0 through NLRT11 completed. The closeout
verifier is `scripts/verify-node-lts-runtime-trust.sh`.
