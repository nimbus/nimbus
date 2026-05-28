# SBA7 Follow-On Decisions Proof

Date: 2026-05-28
Status: completed

## Scope

Evaluate the ordered follow-on crates after system, bridge, auth, and adapter
decisions:

1. `nimbus-artifacts`
2. `nimbus-provenance`
3. `nimbus-operator`
4. `nimbus-services`
5. `nimbus-license`

Each candidate must be extracted only if the ownership is real. Otherwise this
proof records a keep decision with the blocker and required future interface.

## Task Checklist

- [x] SBA7.1 Decide `nimbus-artifacts`.
- [x] SBA7.2 Decide `nimbus-provenance`.
- [x] SBA7.3 Decide `nimbus-operator`.
- [x] SBA7.4 Decide `nimbus-services`.
- [x] SBA7.5 Decide `nimbus-license`.

## Verification Log

- Artifact audit:
  `rg -n "artifact_verifier_effects|CosignVerifierBackend|SlsaVerifierBackend|SbomVerifierBackend|ArtifactVerifierCommandBackend|admit_runtime_bundle_artifact|admit_guest_executable_artifact" crates/nimbus-server/src crates/*/src -g '*.rs'`.
  The remaining server module is the process-backed verifier/effects owner and
  runtime bundle admission helper. Pure artifact policy/contracts already live
  in `nimbus-tenant`.
- Provenance audit:
  `rg -n "RuntimeBundleProvenance|provenance|SLSA|SBOM|slsa|sbom|ArtifactVerification" crates/nimbus-server/src/execution crates/nimbus-runtime/src crates/nimbus-tenant/src crates/nimbus-system/src -g '*.rs'`.
  Runtime invocation gating still sits in server execution, runtime adapter
  manifest provenance still sits in `nimbus-runtime`, and tenant provenance
  policy still sits in `nimbus-tenant`.
- Operator audit:
  `rg -n "local_server|local_admin|LocalAdmin|LOCAL_ADMIN|operator|deploy admin|admin" crates/nimbus-server/src/http crates/nimbus-server/src/router.rs crates/nimbus-server/src/state.rs crates/nimbus-server/src/local_server -g '*.rs'`.
  Local admin/operator middleware still imports Axum request types and
  `AppState`, records server audit events, gates deploy-admin routes, and
  triggers server shutdown/system evidence.
- Services audit:
  `rg -n "crate::(state|router|http|system_tenant|adapters|service_registry|sandbox|tenant|local_enforcement|license|artifact_verifier_effects|execution)" crates/nimbus-server/src/service_manager.rs crates/nimbus-server/src/service_manager crates/nimbus-server/src/service_registry.rs crates/nimbus-server/src/sandbox.rs crates/nimbus-server/src/license -g '*.rs'`.
  Production service manager code still writes system evidence through
  `system_tenant`, consumes server-owned sandbox/service traits, and uses
  server-local `local_enforcement` shims.
- License extraction:
  `crates/nimbus-license` was created and added to the workspace.
- `cargo metadata --no-deps --format-version 1` showed
  `crates/nimbus-license` in `workspace_members`.
- `cargo tree -p nimbus-license --edges normal --depth 1` showed direct
  normal dependencies on `serde`, `serde_json`, and `thiserror` only.
- `rg -n "nimbus_server|nimbus-server|nimbus_engine|nimbus_storage|nimbus_runtime|nimbus_adapters|crate::(state|router|http|adapters|runtime_host|system_tenant|storage|execution)" crates/nimbus-license -g '*.rs' -g 'Cargo.toml'`
  returned no matches.
- `cargo check -p nimbus-license` passed.
- `cargo test -p nimbus-license -- --nocapture` passed: 2 passed, 0 failed.
- `cargo check -p nimbus-server` passed after the license extraction.
- `cargo test -p nimbus-server license -- --nocapture` passed: 22 passed,
  0 failed, 744 filtered; integration filters reported 0/23 and 0/32.

## `nimbus-artifacts` Decision

Keep for now; do not create `nimbus-artifacts` in this phase.

The current source shape is intentionally split already:

- Pure artifact policy, request, evidence, redaction, composite verifier, and
  tenant admission contracts live in `nimbus-tenant`.
- `crates/nimbus-server/src/artifact_verifier_effects.rs` and its `cosign`,
  `slsa`, and `sbom` children own process-backed verifier effects, default CLI
  program names, offline trusted-root file checks, timeout handling, and
  command-output redaction.
- Server runtime invocation uses `admit_runtime_bundle_artifact` before
  executor entry.

Extracting a new artifact crate now would either duplicate tenant-owned
authority contracts or move process launching into a crate whose planned
boundary denies server/effect ownership. The next useful move is a dedicated
artifact-effects readiness plan that introduces a narrow verifier-runner
interface and decides whether process-backed verifier implementations belong
in server/operator wiring or a separate effects crate. Until then,
`nimbus-tenant` remains the authority owner and `nimbus-server` remains the
host-effect owner.

## `nimbus-provenance` Decision

Keep for now; do not create `nimbus-provenance` in this phase.

Provenance is not one coherent owner yet:

- tenant image provenance policy and admission evidence are tenant authority,
- runtime adapter manifest provenance and checksums are runtime-adapter
  integrity,
- runtime bundle provenance gating is execution admission before invocation,
- SLSA/SBOM CLI verification is process-backed artifact verifier effects.

A `nimbus-provenance` crate would be trustworthy only after process execution
is inverted or denied behind traits, and after runtime manifest provenance is
separated from runtime backend discovery. Creating the crate now would blend
authority, runtime integrity, and host effects into a vague supply-chain
bucket.

## `nimbus-operator` Decision

Keep for now; do not create `nimbus-operator` in this phase.

The name is correct for a future crate, but the ownership is not ready. Local
admin/operator code currently spans token files, session cookies, route-family
classification, Axum middleware, `AppState`, audit persistence, deploy-admin
route gating, UI session bootstrap, shutdown control, and `_nimbus` system
events. The pure token/session pieces are extractable candidates, but the
security boundary would not be complete without the middleware and route
surface, and those still depend on server composition.

Future readiness should split operator policy/value types from transport
middleware first, then introduce a server-supplied audit/shutdown/system-event
interface if extraction is still valuable.

## `nimbus-services` Decision

Keep for now; do not create `nimbus-services` in this phase.

The service manager is a real domain, but it is still bound to server
composition in production:

- runtime service lookup is consumed by current server adapter wiring,
- sandbox service catalog traits are still server-owned public API,
- service lifecycle routes are Axum/server handlers,
- service manager records observed service handles through server
  `system_tenant`,
- activation uses the server-local `local_enforcement` shim.

The right next step is not a crate move; it is readiness work that moves
sandbox/service traits to the future owner, replaces `system_tenant` writes
with a narrow evidence-writer trait, and removes the local-enforcement shim.

## `nimbus-license` Decision

Extract `nimbus-license`.

This boundary is earned now. License loading, document shape, entitlement
snapshotting, source classification, status warnings, and usage-limit
evaluation are shared product/control-plane concerns and do not require
server routers, adapters, storage providers, runtime internals, or `_nimbus`
persistence. The only pre-extraction coupling was the usage input type coming
from engine; extraction replaced that with `LicenseUsageInput`, and the server
HTTP route now converts engine usage into that narrow value before calling
license logic.

`nimbus-server` keeps a small `src/license.rs` re-export shim so existing
server/facade imports remain stable while ownership moves to
`crates/nimbus-license`.
