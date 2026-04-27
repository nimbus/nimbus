# Plan: Adapter Runtime Trust Hardening

Canonical execution control plane for the next post-Firebase, post-Cloud
Functions architecture wave. This plan exists because the previous adapter
boundary work landed the right broad direction, but the review found several
remaining seams that are still less canonical, less idiomatic, or less
trustworthy than Neovex should accept pre-launch:

- server-owned application auth still depends on Convex-owned types and
  registries
- Cloud Functions callable auth still fails open in one important path
- the `firebase-admin/firestore` runtime shim still reaches into the Firebase
  adapter instead of meeting it on a shared provider-family seam
- runtime invocation bootstrap is duplicated between shared and Convex paths
- `runtime_host/*` still mixes primitive capability execution with runtime ABI
  payload dispatch
- one covered Firestore-admin response reports incorrect update metadata
- the touched stack is not yet at a clean `clippy -D warnings` baseline

Reviewed against:

- `README.md`
- `ARCHITECTURE.md`
- `docs/README.md`
- `docs/plans/README.md`
- `AGENTS.md`
- `docs/architecture/testing/reliability-posture.md`
- `docs/architecture/testing/ci-failure-investigation.md`
- `docs/architecture/runtime/adapter-boundary.md`
- `docs/adapters/firebase/auth-contract.md`
- `docs/adapters/firebase/compatibility.md`
- `docs/adapters/cloud-functions/compatibility.md`
- `docs/plans/runtime-capability-adapter-boundary-plan.md`
- `docs/plans/archive/multi-adapter-boundary-hardening-plan.md`
- `docs/plans/native-transport-evolution-plan.md`
- the current git worktree on `2026-04-26`

---

## Status

- **Status:** `done`
- **Primary owner:** next adapter/runtime/auth/trust cleanup wave
- **Activation gate:** immediate; the runtime-capability boundary plan is
  complete
- **Execution order:** this plan precedes any activation of
  `docs/plans/native-transport-evolution-plan.md`
- **Verification posture:** every implementation item must record focused
  verification; docs-only reasoning is not sufficient because this plan is
  correcting live correctness and trust seams

## Why This Exists

Neovex now has multiple adapter families and a much cleaner shared primitive
core than it did before the Firebase work. That progress is real. The current
review findings are narrower, but they matter more:

- auth ownership is still not fully server-owned
- some compatibility layers still depend on each other instead of on shared
  provider-neutral or provider-family seams
- one covered metadata contract is currently wrong
- the shared runtime-host layer is partly primitive and partly runtime ABI
- compiler-guided cleanup is not yet back to green

This is exactly the kind of cleanup we should do pre-launch while we can still
make direct breaking changes instead of supporting awkward historical seams.

## Relationship To Other Plans

- `docs/plans/archive/multi-adapter-boundary-hardening-plan.md`
  is the completed wave that fixed Firebase principal propagation,
  compatibility-truth alignment, WebSocket legacy cleanup, and the first big
  ownership-based decompositions.
- `docs/plans/runtime-capability-adapter-boundary-plan.md`
  is the completed wave that corrected the major extraction mistake: shared
  runtime-host code is now provider-neutral and `firebase-admin/firestore`
  moved back under adapter ownership.
- `docs/plans/native-transport-evolution-plan.md`
  remains deferred. Transport evolution should not move forward while auth,
  compatibility-family seams, runtime bootstrap ownership, and trusted
  metadata contracts are still being corrected.

## Scope

This plan covers:

- server-owned application auth and principal normalization
- Cloud Functions callable auth fail-closed behavior
- Firebase-family compatibility seams that should be shared without making
  adapters depend on each other
- truthful document lifecycle metadata on covered Firestore-admin surfaces
- shared runtime invocation bootstrap ownership
- clearer layering between runtime capabilities and runtime ABI dispatch
- idiomatic-Rust and trust hardening on the touched hot paths
- focused proofs and control-plane closeout

This plan does not cover:

- new Firebase surface breadth
- new Cloud Functions surface breadth
- MongoDB adapter work
- native transport evolution
- new storage provider topology work

## Control Plan Rules

1. Shared auth belongs to the server, not to one adapter.
   Firebase, Cloud Functions, and Convex may use different compatibility
   contracts, but bearer verification, principal normalization, and fail-open
   versus fail-closed server behavior are server-owned concerns.

2. Adapters may share provider-family helpers, but not through each other.
   If Cloud Functions needs Firestore-family path or locator logic, it should
   depend on a shared Firestore-family seam, not on `adapters/firebase/mod.rs`
   or another adapter composition root.

3. Truth beats compatibility theater.
   Do not emit metadata such as `update_time_ms` unless the value is actually
   correct. Pre-launch direct corrections are preferred over preserving a false
   contract.

4. Shared runtime bootstrap should have one authoritative implementation.
   Session creation, mutation execution-unit ownership, trigger origin
   attachment, and nested runtime budgeting should not drift across adapter
   bridges.

5. `runtime_host/*` should distinguish primitive execution from runtime ABI.
   Capability execution may be shared; `HostCallPayload` dispatch is still a
   runtime ABI layer and should be named and placed accordingly.

6. Hot paths should fail deliberately, not accidentally.
   Avoid silent auth downgrade, avoid `expect` in non-test request paths when a
   typed error is possible, and keep compiler-guided cleanup green.

## Current Assessed State

- Firebase application auth now reaches the server edge, but
  `application_auth.rs` still uses `state.convex_registry.current()` and
  Convex-owned principal normalization helpers.
- Cloud Functions callable auth returns `Ok(None)` when no Convex auth registry
  is loaded, then proceeds as anonymous instead of failing closed.
- The Cloud Functions `firebase-admin/firestore` runtime shim now lives under
  the correct adapter, but still imports Firebase adapter locator logic.
- The covered Firestore-admin read response currently emits
  `update_time_ms = creation_time`, which is not truthful for updated
  documents.
- `runtime_host/capabilities.rs` is a good provider-neutral primitive seam, but
  `runtime_host/abi/document_calls.rs` still mixes that primitive layer with
  runtime ABI payload dispatch.
- `RuntimeHostContext::build(...)` and `ConvexHostBridge::build(...)` still
  duplicate mutation-session bootstrap logic.
- `cargo clippy -p neovex-server --lib --tests -- -D warnings` currently fails
  immediately on `neovex-core/src/typed_scalar.rs` for `manual_div_ceil`,
  which is a useful signal that the touched stack is not back to a clean
  idiomatic baseline yet.

## Context Window Scale

Use these sizing bands before loading code:

| Band | Estimated context | Expected shape |
| --- | --- | --- |
| `S` | 8k-12k tokens, 6-10 files | one narrow docs or cleanup slice |
| `M` | 12k-18k tokens, 8-14 files | one ownership seam plus focused proofs |
| `L` | 18k-28k tokens, 12-20 files | one cross-layer extraction or contract fix |
| `XL` | 28k-40k tokens, 18-30 files | only for decomposition or end-state closeout; split if possible |

Rule:

- if execution needs more than the estimated band for an item, split the item
  in this plan before continuing

## Roadmap Summary

| Item | Focus | Depends on | Context estimate | Status |
| --- | --- | --- | --- | --- |
| `ARTH1` | Server-owned auth contract baseline and review findings ledger | — | `S` | `done` |
| `ARTH2` | Server-owned application auth and principal normalization extraction | `ARTH1` | `L` | `done` |
| `ARTH3` | Cloud Functions callable fail-closed auth behavior | `ARTH2` | `M` | `done` |
| `ARTH4` | Shared Firestore-family compatibility seam; remove adapter-to-adapter imports | `ARTH1` | `L` | `done` |
| `ARTH5` | Truthful Firestore-admin document lifecycle metadata | `ARTH4` | `L` | `done` |
| `ARTH6` | Shared runtime invocation bootstrap seam | `ARTH2`, `ARTH4` | `L` | `done` |
| `ARTH7` | Separate runtime capabilities from runtime ABI dispatch | `ARTH6` | `L` | `done` |
| `ARTH8` | Idiomatic Rust and hot-path trust hardening | `ARTH2`, `ARTH3`, `ARTH5`, `ARTH7` | `M` | `done` |
| `ARTH9` | Focused proofs, docs closeout, and native-transport gate refresh | `ARTH8` | `M` | `done` |

## Roadmap

### ARTH1 — Server-Owned Auth Contract Baseline And Review Findings Ledger

Goal: turn the review findings into one explicit execution baseline before more
code moves.

- Publish the current auth, runtime, provider-family, and metadata issues as
  one settled control-plane baseline
- Record which findings are correctness risks, which are ownership problems,
  and which are idiomatic-Rust follow-ons
- Refresh any reference docs needed so later implementation can point to one
  agreed contract

Completion gate:

- the plan and related docs clearly state the remaining auth/runtime/provider
  seams and later items can execute without reopening the review debate

### ARTH2 — Server-Owned Application Auth And Principal Normalization Extraction

Goal: remove shared auth ownership from Convex adapter namespaces.

- Move principal normalization out of `adapters/convex/auth/*` into a
  server-owned auth or application-auth module
- Introduce a server-owned auth-verification seam so Firebase and Cloud
  Functions no longer reach through `convex_registry` as their semantic owner
- Keep adapter-specific auth-provider config and Convex compatibility behavior
  under Convex where appropriate, but stop making shared auth consumers import
  Convex-owned helpers

Completion gate:

- Firebase, Cloud Functions, and Convex all consume a server-owned auth entry
  seam, and server-owned application auth no longer depends on Convex-named
  principal helpers

### ARTH3 — Cloud Functions Callable Fail-Closed Auth Behavior

Goal: make callable auth trustworthy under missing or misconfigured auth
providers.

- Decide the exact first-slice callable contract when an Authorization header
  is presented but no auth verifier is configured
- Prefer explicit failure over silent anonymous downgrade
- Reconcile the callable auth path with the settled Firebase application-auth
  contract

Completion gate:

- callable auth either verifies successfully or fails explicitly; it does not
  silently fall back to anonymous when a bearer token cannot be validated

### ARTH4 — Shared Firestore-Family Compatibility Seam

Goal: let Firebase and Cloud Functions share Firestore-family translation logic
without adapters depending on each other.

- Extract the shared Firestore-family path or locator mapping that Cloud
  Functions currently imports from the Firebase adapter
- Keep transport-specific resource-name parsing under adapters where it belongs
- Define one honest seam for provider-family logic that is not a core
  primitive and not another adapter’s composition root

Completion gate:

- Cloud Functions no longer imports Firebase adapter internals to resolve
  Firestore-family document targets

### ARTH5 — Truthful Firestore-Admin Document Lifecycle Metadata

Goal: stop emitting incorrect document lifecycle metadata.

- Decide the canonical source of document update time for covered
  Firestore-admin responses
- If Neovex needs a new shared `update_time` concept, add it end to end through
  the correct primitive layers
- If the value cannot be made truthful in this slice, remove the false field
  instead of preserving a lie

Completion gate:

- covered Firestore-admin read and write responses expose only truthful
  lifecycle metadata

### ARTH6 — Shared Runtime Invocation Bootstrap Seam

Goal: remove duplicated runtime-session bootstrap logic.

- Identify the common build path between `RuntimeHostContext::build(...)` and
  `ConvexHostBridge::build(...)`
- Extract one authoritative helper or builder for:
  - mutation execution-unit creation
  - trigger write origin attachment
  - runtime host state/session creation
  - nested runtime budgeting inputs
- Keep adapter-specific additions separate from the shared bootstrap

Completion gate:

- Convex and shared runtime-host bootstrap no longer duplicate the same
  mutation-session setup logic

### ARTH7 — Separate Runtime Capabilities From Runtime ABI Dispatch

Goal: make the shared runtime layer read clearly as primitive capability code,
not as half capability layer and half ABI layer.

- Keep capability execution in provider-neutral modules
- Move or rename `HostCallPayload` and `RuntimeAsync*Payload` dispatch so the
  runtime ABI layer is explicit
- Make module naming reflect the actual layering without historical baggage

Completion gate:

- `runtime_host/*` clearly separates typed capability execution from runtime
  ABI dispatch

### ARTH8 — Idiomatic Rust And Hot-Path Trust Hardening

Goal: restore a compiler- and linter-guided quality baseline on the touched
surfaces.

- Fix the current `clippy -D warnings` failures that block the touched stack
- Remove avoidable `expect`, `panic!`, or similar assumptions from non-test
  request and auth hot paths where a typed error is appropriate
- Prefer clearer builder or helper patterns where they reduce drift or hidden
  failure modes

Completion gate:

- the touched server/auth/runtime stack is back to a clean focused clippy
  baseline and the main non-test hot paths avoid accidental process aborts

### ARTH9 — Focused Proofs, Docs Closeout, And Native-Transport Gate Refresh

Goal: leave the repo pointing at the corrected canonical baseline.

- Add focused tests that prove:
  - server-owned auth still works for Firebase and Cloud Functions
  - callable auth fails closed correctly
  - Firestore-family compatibility sharing no longer depends on another adapter
  - runtime bootstrap still works across Convex and Cloud Functions
  - metadata truth is preserved
- Update `docs/plans/README.md`, `AGENTS.md`, and any affected reference docs
- Refresh `docs/plans/native-transport-evolution-plan.md` if its promotion
  guidance changes after this wave closes

Completion gate:

- focused verification and docs both point at the corrected auth/runtime trust
  baseline

## Verification Expectations

Each implementation item should record focused verification before it closes.
Expected lanes include the narrowest proofs that match the touched surface, for
example:

- `cargo test -p neovex-server cloud_functions --lib`
- focused Firebase lanes under `cargo test -p neovex-server ...`
- focused Convex auth/runtime lanes under `cargo test -p neovex-server ...`
- `cargo check -p neovex-server`
- `cargo fmt --all --check`
- `cargo clippy -p neovex-server --lib --tests -- -D warnings`
- focused `npm run typecheck --workspace @neovex/firebase` when adapter-facing
  JS surfaces change

Use broader workspace verification only after the focused proofs are green.

## Execution Log

| Date | Item | Status | Notes |
| --- | --- | --- | --- |
| 2026-04-26 | plan authoring | `done` | Created this control-plane plan from the full post-RCAB architecture review to close the remaining auth ownership, provider-family seam, runtime bootstrap, metadata truth, and idiomatic-Rust trust gaps before any native transport promotion. No code verification ran in the authoring pass because this step only established the control plane. |
| 2026-04-26 | `ARTH1`-`ARTH7` | `done` | Landed the server-owned auth seam, fail-closed callable auth, shared Firestore-family compatibility helpers, truthful Firestore-admin metadata, shared runtime bootstrap construction, and explicit runtime ABI layering under `runtime_host/abi/*`. |
| 2026-04-26 | `ARTH8` | `done` | Closed the hot-path idiomatic-Rust cleanup with shared auth/bootstrap argument shaping, Cloud Functions callable request shaping, `typed_scalar.rs` `div_ceil` cleanup, and a scoped Firebase gRPC lint posture that keeps `tonic::Status` explicit at the transport boundary while removing the real shape issues called out by clippy. |
| 2026-04-26 | `ARTH9` | `done` | Focused verification is green: `cargo fmt --all --check`, `cargo clippy -p neovex-server --lib --tests -- -D warnings`, `cargo test -p neovex-server cloud_functions --lib`, `cargo test -p neovex-server adapters::convex::tests::authorization --lib`, `cargo test -p neovex-server provider_family::firestore --lib`, and `npm run test --workspace @neovex/codegen`. Updated `docs/plans/README.md`, `AGENTS.md`, reference docs, and the deferred native transport plan to point at this completed trust baseline. |
