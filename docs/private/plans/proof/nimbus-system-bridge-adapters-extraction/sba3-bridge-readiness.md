# SBA3 Bridge Readiness Proof

Date: 2026-05-28
Status: completed

## Scope

Prepare `nimbus-bridge` by classifying runtime host and bridge-adjacent
execution helpers, then remove false server-private edges before extraction.

## Candidate Files

- `crates/nimbus-server/src/runtime_host/*`
- `crates/nimbus-server/src/runtime_host/abi/*`
- `crates/nimbus-server/src/runtime_host/state.rs`
- `crates/nimbus-server/src/runtime_host/read_tracking/*`
- `crates/nimbus-server/src/runtime_host/admission.rs`
- `crates/nimbus-server/src/runtime_host/cancellation.rs`
- `crates/nimbus-server/src/execution/errors.rs`

## Intended Moves

- Classify each candidate as bridge-owned, server-owned, runtime-owned, or
  adapter-owned.
- Replace server-local `crate::local_enforcement` shim usage in bridge
  candidates with direct `nimbus-node` imports where appropriate.
- Replace server-local `crate::tenant` imports in bridge candidates with
  direct `nimbus-tenant` imports where appropriate.
- Keep provider-specific Convex/Firebase/Cloud Functions/MongoDB protocol logic
  outside the bridge candidate.
- Preserve runtime host behavior with focused tests before moving files.

## Forbidden Imports For Bridge Candidates

SBA3 is not complete while bridge-candidate production files contain unproven
imports or references to:

- `crate::adapters`
- `crate::router`
- `crate::state`
- `crate::http`
- `crate::system_tenant`
- `crate::local_server`
- `crate::application_auth`
- `crate::local_enforcement`

`crate::execution` references are allowed during SBA3 only where the referenced
helper has been explicitly classified for bridge extraction or server retention.

## Classification

| File or module | Owner decision | Rationale |
| --- | --- | --- |
| `runtime_host/mod.rs` | bridge-owned | Builds runtime-host bootstrap state from an admitted `TenantIsolationDecision`, runtime policy, service handle, and principal. |
| `runtime_host/capabilities.rs` | bridge-owned | Implements provider-neutral capability access for document host calls through admitted storage projections and local enforcement binding. |
| `runtime_host/abi/document_calls.rs` | bridge-owned | Owns generic runtime document host-call dispatch. Provider-specific labels were removed from validation errors. |
| `runtime_host/responses.rs` | bridge-owned | Owns provider-neutral runtime-host response/error encoding. The server `error_envelope` dependency was replaced by a local runtime-host public error shape. |
| `runtime_host/state.rs` | bridge-owned | Moved from `execution/host_state.rs`; owns session validation, nested invocation budget, trigger write origin, and runtime read-set recording state. |
| `runtime_host/read_tracking/*` | bridge-owned | Moved from `execution/read_tracking/*`; owns canonical runtime read-set, intersection, and subscription base-query synthesis. |
| `runtime_host/admission.rs` | bridge-owned | Moved from `execution/runtime_admission.rs`; maps tenant runtime policy admission to in-process/fail-closed execution availability. |
| `runtime_host/cancellation.rs` | bridge-owned | Splits generic cancellation checks out of server execution error glue. |
| `execution/errors.rs` | server-owned compatibility glue | Keeps Convex/server runtime error-to-core translation in server execution. It only re-exports generic cancellation helpers during the transition. |
| `execution/invocations/*` | server/provenance-owned, not bridge | Runtime bundle invocation orchestration, worker dispatch, and provenance gate config remain outside `nimbus-bridge`. |

## Readiness Changes

- Moved bridge-owned helpers from `execution` into `runtime_host`:
  `state.rs`, `read_tracking/*`, and `admission.rs`.
- Added `runtime_host/cancellation.rs` for provider-neutral host cancellation
  checks.
- Updated Convex and Cloud Functions runtime callers to import
  `RuntimeExecutionAdmission` and read-tracking helpers from `runtime_host`
  instead of server-private `execution` modules.
- Replaced bridge-candidate `crate::tenant` imports with direct
  `nimbus-tenant` imports.
- Replaced bridge-candidate `crate::local_enforcement` imports with direct
  `nimbus-node` imports.
- Removed the bridge-candidate dependency on `crate::error_envelope` by giving
  `runtime_host/responses.rs` its own neutral runtime-host error envelope.
- Removed provider vocabulary from generic document host-call validation
  labels.

## Authority Inputs

The bridge candidate is ready for extraction because the authority input is an
admitted decision, not raw request metadata:

- `RuntimeHostScope::new` receives a `TenantIsolationDecision`.
- `RuntimeCapabilityHost` implementations consume
  `TenantStorageAccessDecision` from `LocalEnforcementBinding::from_decision`.
- Runtime document calls validate the runtime session and cancellation token
  before reaching storage helpers.
- `RuntimeExecutionAdmission::for_decision` consumes
  `TenantRuntimePolicyAdmission` and fails closed when only an unavailable
  fallback route was admitted.

Raw tenant strings still appear as resource identifiers inside runtime payloads
and session validation, but they are not treated as authority.

## Task Checklist

- [x] SBA3.1 Classify runtime host and execution helpers.
- [x] SBA3.2 Separate provider-neutral bridge from adapter shims.
- [x] SBA3.3 Define bridge context/request API.
- [x] SBA3.4 Remove server-local shim dependence.
- [x] SBA3.5 Preserve runtime enforcement semantics.

## Verification Log

- `rg -n "crate::error_envelope|ctx\\.db|convex|firebase|firestore|cloud_functions|mongodb|crate::(adapters|router|state|http|system_tenant|local_server|application_auth|local_enforcement|tenant|execution)" crates/nimbus-server/src/runtime_host -g '*.rs'`
  returned no matches.
- `rg -n "execution::(host_state|read_tracking|runtime_admission)|pub\\(in crate::execution::read_tracking\\)" crates/nimbus-server/src -g '*.rs'`
  returned no matches.
- `cargo check -p nimbus-server` passed.
- `cargo test -p nimbus-server runtime_host -- --nocapture` passed:
  12 passed, 0 failed, 763 filtered; integration filters reported 0/0 and
  0/23, 0/32.
- `cargo test -p nimbus-server cloud_functions -- --nocapture` initially
  surfaced a concurrent-write seed conflict in
  `cloud_functions_trigger_lifecycle_processes_concurrent_writes`; the test
  helper now retries expected transaction conflicts while preserving failure
  on non-conflict errors. Re-run passed: 39 passed, 0 failed, 736 filtered;
  integration filters reported 0/23 and 0/32.
- `cargo fmt --all --check` passed.

## Extraction Readiness Decision

Proceed to SBA4. The provider-neutral runtime bridge boundary is now earned
well enough to move into `crates/nimbus-bridge`. The extraction should move
only the classified bridge-owned modules above. Server execution invocation
orchestration, adapter-specific host bridges, HTTP/router/state composition,
and `_nimbus` persistence must stay out.
