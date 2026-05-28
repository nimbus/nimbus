# NLRT0 Baseline And Control Plane

Date: 2026-05-28
Authoring agent: Codex
Status: done

## Scope

Activate `docs/plans/node-lts-runtime-trust-plan.md` after the crate-extraction
baseline commit and checkpoint the control-plane facts needed to resume the
Node LTS runtime trust work after compaction.

## Git And Baseline

- Baseline commit: `9995f65dafab27cc1168d5f5cffa8e7694ae4cdd`
  (`Refactor server boundaries into focused crates`).
- Git status before NLRT0 edits: clean.
- Active plan: `docs/plans/node-lts-runtime-trust-plan.md`.
- Research baseline:
  `docs/plans/research/node-lts-runtime-and-deno-fork-strategy.md`.
- Proof directory README exists:
  `docs/plans/proof/node-lts-runtime-trust/README.md`.
- Active plan routing exists in `docs/plans/README.md`.

## Post-Refactor Owner Map

- `nimbus-runtime` owns runtime compatibility targets, runtime limits/policies,
  process metadata/bootstrap behavior, node-compat harness manifests, and the
  future Node LTS registry.
- `nimbus-tenant` owns tenant runtime admission and operator policy mapping for
  Node20, Node22, and Node24 profiles.
- `nimbus-bridge` owns execution-time admission from tenant policy decisions
  into runtime invocation.
- `nimbus-convex` owns Convex manifest runtime selection, runtime lane
  diagnostics, and `"use node"` action packaging/routing.
- `nimbus-server` remains composition/transport owner only where end-to-end
  HTTP/WebSocket behavior is being verified.

## Deno/V8 Patch Baseline

Root `Cargo.toml` currently resolves Deno-family patch-sensitive crates through:

- `https://github.com/nimbus/deno`, tag `v2.8.0-nimbus.5`.
- `https://github.com/nimbus/rusty_v8`, tag `v149.0.0-nimbus.1`.

`Cargo.lock` records:

- Deno-family fork revision
  `37b6333a1f703db523efe8a703d36f2152ad087a`.
- `rusty_v8` fork revision
  `9b77553883f1117ab3df62709b8673b803ed721b`.

NLRT1 must replace this hand-inspected baseline with a verifier that inspects
`Cargo.toml`, `Cargo.lock`, and `cargo tree -p nimbus-runtime`.

## NCG Coordination

`docs/plans/node-compat-cron-greening-plan.md` remains the nearer-term cron
greening plan for current Node compatibility failures. NLRT must read and
coordinate with NCG before touching Node bootstrap, process metadata, or
node-compat harness code.

## Decisions

- Activated NLRT now that the crate-refactor baseline is committed.
- Completed NLRT0 rather than leaving it `in_progress`; every NLRT0 acceptance
  criterion is satisfied by this proof, the plan routing, and the docs
  validation result.
- Deferred all implementation work to NLRT1+. No runtime code was changed in
  NLRT0.

## Files Changed

- `docs/plans/node-lts-runtime-trust-plan.md`
- `docs/plans/proof/node-lts-runtime-trust/README.md`
- `docs/plans/proof/node-lts-runtime-trust/nlrt0-baseline-and-control-plane.md`

## Verification

```text
npm run docs:validate-refs:strict
docs reference validation: pass (218 working-tree Markdown files)
```

## Remaining Risks

- NLRT1 still needs to make Deno-family patch closure script-verified.
- NLRT rows that depend on drift-prone Node release facts must recheck them with
  dates in their own proof artifacts.
