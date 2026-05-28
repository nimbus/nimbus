# Node LTS Runtime Trust Proofs

This directory stores proof artifacts for
`docs/plans/node-lts-runtime-trust-plan.md`.

Rules:

- Write one proof file per ledger row:
  `nlrt<N>-<slug>.md`.
- Include date, agent, git status summary, changed files, decisions,
  verification commands, and concrete results.
- Do not mark an NLRT row `done` until its proof file exists and the row's
  acceptance criteria in the plan pass.
- If context is compacted, resume from the plan ledger, this directory, and the
  execution log in the plan.

Current state: plan active; NLRT0 through NLRT10 completed; NLRT11 is the next
pending row.
