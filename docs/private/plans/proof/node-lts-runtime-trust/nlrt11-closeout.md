# NLRT11 Closeout

Date: 2026-05-28
Authoring agent: Codex
Status: done

## Scope

Close the Node LTS Runtime Trust plan by adding the final verifier, archiving
the completed execution plan, updating the proof index and plans index, and
running the verifier plus the focused runtime, tenant, bridge, and Convex
tests required by the completion gate.

## Git Status

- Baseline before NLRT11: commit `3268088f` (`Expand active LTS Node canary
  evidence`).
- Pre-existing unrelated dirty file: `docs/plans/dynamodb-adapter-plan.md`.
  NLRT11 does not stage or depend on that file.

## Files Changed

- `scripts/verify-node-lts-runtime-trust.sh`
- `docs/plans/archive/node-lts-runtime-trust-plan.md`
- `docs/plans/README.md`
- `docs/plans/proof/node-lts-runtime-trust/README.md`
- `docs/plans/proof/node-lts-runtime-trust/nlrt11-closeout.md`

## Decisions

- Archived the plan instead of leaving a completed control plane in the active
  plan list.
- Made the final verifier a composite gate over the focused checks produced by
  NLRT1 through NLRT10, plus closeout-only checks for the archived ledger,
  proof files, public docs, dashboard evidence, harness diagnostics, permission
  docs, formatting, and Markdown references.
- Kept Node22 as the product default while requiring Node22 and Node24
  lane-local support evidence. Node20 remains legacy-grace only after its
  2026-04-30 EOL.
- Kept ignored fatal/hanging fixture cases as explicit watchpoints. They are
  counted by the harness and classifications verifier and are not treated as
  green support evidence.

## Verification

```text
cargo fmt --all --check
pass
```

```text
bash scripts/verify-node-lts-runtime-trust.sh
Summary: 16 passed, 0 failed
```

The verifier composes:

- archived plan/proof/registry/docs/diagnostics invariants
- `bash scripts/verify-node-lts-lanes.sh`
- `bash scripts/verify-deno-fork-provenance.sh`
- `bash scripts/verify-deno-fork-upstream-policy.sh`
- `bash scripts/verify-node-fixture-provenance.sh`
- `bash scripts/verify-node-compat-harness-hardening.sh`
- `bash scripts/verify-node-lts-canaries-and-oracles.sh`
- `bash scripts/verify-node-lts-docs.sh`
- focused `nimbus-runtime`, `nimbus-tenant`, `nimbus-bridge`, and
  `nimbus-convex` tests
- `cargo fmt --all --check`
- `npm run docs:validate-refs:strict`

```text
cargo test -p nimbus-runtime node_compat_supplementary_process_shape -- --nocapture --test-threads=1
3 passed; 0 failed
```

```text
cargo test -p nimbus-tenant production_untrusted_runtime_admission -- --nocapture
8 passed; 0 failed
```

```text
cargo test -p nimbus-bridge runtime_execution_admission -- --nocapture
2 passed; 0 failed
```

```text
cargo test -p nimbus-convex runtime_access -- --nocapture
2 passed; 0 failed; 2 ignored
```

```text
npm run docs:validate-refs:strict
docs reference validation: pass
```

```text
git diff --check
pass
```

## Remaining Risks

- The Node22 oracle remains a recorded dashboard artifact from existing
  evidence because this shell has Node24 installed locally but not a Node22
  binary. The final verifier still requires published oracle coverage for
  Node22 and Node24 with matching major versions.
- Tooling canary packages still use the repo's host-node shim for package CLI
  subprocesses. The NLRT10 verifier proves lane-local Nimbus runtime execution;
  a future stricter lane can version-manage the host shim itself if needed.
