# SBA6 Adapters Extraction Proof

Date: 2026-05-28
Status: completed

## Scope

Decide whether to extract an aggregate `nimbus-adapters` crate now.

## SBA5 Input

SBA5 rejected aggregate extraction because adapters still import broad
server-private composition surfaces:

- `AppState` / `DeploymentState`
- local-server route authorization and audit
- `_nimbus` system-evidence writers
- runtime invocation/provenance orchestration
- runtime service registry
- tenant-admission shims
- WebSocket handshake/protocol glue
- server auth transport/deployment wrappers

## Decision Under Review

The extraction is only allowed if it can move true adapter protocol code
without:

- importing `nimbus-server`,
- accepting server state objects,
- owning listener/router lifecycle,
- directly writing `_nimbus`,
- bypassing `nimbus-bridge`,
- bypassing `nimbus-auth`.

## Task Checklist

- [x] SBA6.1 Create `crates/nimbus-adapters`, or record why not.
- [x] SBA6.2 Move true adapter protocol code, or record why not.
- [x] SBA6.3 Enforce adapter dependency rules, or record why no crate exists.
- [x] SBA6.4 Preserve adapter behavior.

## Verification Log

- No `crates/nimbus-adapters` crate was created.
- The SBA5 server-private import audit is the blocking evidence for this keep
  decision.
- `cargo test -p nimbus-server auth -- --nocapture` passed after the auth and
  bridge extractions: 63 passed, 0 failed, 705 filtered; integration filters
  reported 0/23 and 0/32.
- `cargo test -p nimbus-server cloud_functions -- --nocapture` passed after
  the bridge extraction: 39 passed, 0 failed, 729 filtered; integration filters
  reported 0/23 and 0/32.
- `cargo check --workspace` passed after `nimbus-auth` extraction.
- `cargo fmt --all --check` passed after `nimbus-auth` extraction.

## Extraction Decision

Do not create `nimbus-adapters` in this plan.

This is a deliberate keep/reject decision, not a skipped implementation task.
The aggregate crate would currently need to own or import server composition
surfaces that are explicitly denied by the plan: `AppState`, route/listener
lifecycle, local admin route gates, `_nimbus` persistence wiring, WebSocket
handshake glue, runtime invocation/provenance orchestration, and deployment
auth wrappers.

The follow-on should be a per-adapter readiness plan, ordered by dependency
shape:

1. MongoDB adapter crate readiness.
2. Firebase/provider-family adapter crate readiness.
3. Cloud Functions adapter crate readiness.
4. Convex adapter crate readiness.

Each follow-on must first introduce only the composition traits that have real
consumers in that adapter. Creating an aggregate crate first would reduce
enterprise trust because reviewers could no longer tell where server authority
ends and adapter compatibility begins.

## Post-SBA Supersession

This proof records the original SBA6 decision. The later Server Crate
Extraction Completion plan changed the adapter-crate decision after extracting
the per-adapter crates first. Its FCE9 proof permits `crates/nimbus-adapters`
only as a feature-gated re-export facade with no server, effect, listener,
state, or implementation ownership. Current verification treats the FCE9/FCE10
proofs as the required proof-decision change for the facade.
