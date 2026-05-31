# DynamoDB Adapter — Startup Prompt

Recoverable copy of the autonomous objective for
`docs/plans/archive/dynamodb-adapter-plan.md`. The canonical definition lives in that
plan's **Goal Control Plane** section; this file mirrors it so the prompt is
recoverable from a file as well as from the plan.

## Objective

Complete `docs/plans/archive/dynamodb-adapter-plan.md` autonomously end to end. Ship a
`nimbus-dynamodb` concrete adapter crate that serves the DynamoDB HTTP/JSON wire
protocol on its own port, covering compatibility tiers T0–T7 (control plane;
single-item ops + expressions; Query/Scan; batch + transactions; secondary
indexes; Streams; TTL + tagging; SigV4 strict mode) plus the `@nimbus/dynamodb`
SDK package (T8). Prove every supported operation through at least one official
SDK client (AWS CLI, JS v3, Rust, Python) against an endpoint override; classify
every divergence from DynamoDB Local / ExtendDB in
`docs/adapters/dynamodb/divergences.md` with a regression test; hold tenant
isolation across at least two access keys; commit failure-injection, soak, and
performance-baseline evidence; ship the enterprise-readiness closeout doc; keep
DynamoDB protocol dependencies out of `nimbus-server`; and pass
`bash scripts/verify-dynamodb-adapter.sh` with `N passed, 0 failed`.

## Branch, CI, and PR workflow

- All work runs on the dedicated `dynamodb-adapter` worktree branch
  (`git worktree add ../nimbus-dynamodb-adapter -b dynamodb-adapter`). Never push
  to `main` directly.
- Commit per roadmap item, checkpointing plan state (status, phase ledger,
  execution log) in the same commit.
- CI is the real evidence: dual-target parity (DynamoDB Local + ExtendDB), the
  official-SDK matrix, and nightly external suites run in CI, not on the dev
  host. A green local `make check`/`clippy` is necessary but not sufficient.
- The terminal action is opening a PR `dynamodb-adapter → main` once the verifier
  is `N passed, 0 failed` and full branch CI is green. Do not self-merge.
- A `v0.1.32` release cut may be in flight (the
  `final-nimbus-release-readiness-plan` worktree). If so, hold the PR *merge*
  (not the branch work) until that release window closes.

## How to work

1. Read `AGENTS.md`, `README.md`, `ARCHITECTURE.md`, `docs/README.md`,
   `docs/plans/README.md`, and the plan before starting an item.
2. Resume any `in_progress` item; otherwise pick the first `pending` item in
   roadmap order whose hard deps are `done`.
3. Mark exactly one item `in_progress`. Satisfy its completion gate. Record the
   commands run plus observed counts in the execution log. Commit. Then advance.
4. Treat any failing completion-gate condition as a stop condition, not a TODO.

## `/goal` prompt

```text
/goal Complete docs/plans/archive/dynamodb-adapter-plan.md autonomously end to end on a dedicated worktree branch — never push to main directly. First (D0.0a) create the worktree branch: `git worktree add ../nimbus-dynamodb-adapter -b dynamodb-adapter`, and do all work from there. Work one roadmap item at a time in dependency order (D0.0a control-plane scaffold first, then D0.0..D9.7), mark exactly one item in_progress, satisfy its completion gate, commit per item with the plan checkpoint, and record the commands run plus observed counts in the execution log before closing it. Ship the nimbus-dynamodb concrete adapter crate for DynamoDB tiers T0-T7 plus the @nimbus/dynamodb package, prove every supported operation through an official SDK client (AWS CLI / JS v3 / Rust / Python) against an endpoint override, classify every DynamoDB-Local/ExtendDB divergence in docs/adapters/dynamodb/divergences.md with a regression test, hold two-tenant isolation, and commit failure-injection, soak, performance-baseline, and enterprise-readiness evidence. Keep DynamoDB protocol dependencies out of nimbus-server. Done when every roadmap item is done, `bash scripts/verify-dynamodb-adapter.sh` exits 0 with "N passed, 0 failed", `cargo fmt --all --check` + `make clippy` + `make deny` + `make verify-third-party-attribution` + strict docs-reference validation + `git diff --check` all pass, the `dynamodb-adapter` branch is pushed and full CI is green on it, and a PR `dynamodb-adapter → main` is open (the final closeout action — do not merge it yourself).
```
