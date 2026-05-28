# SBA8 Verifier Closeout Proof

Date: 2026-05-28
Status: completed

## Scope

Add and run the final verifier for
`docs/plans/nimbus-system-bridge-adapters-extraction-plan.md`, then run the
focused behavior tests required to close the extraction control plane.

## Task Checklist

- [x] SBA8.1 Add final verifier script.
- [x] SBA8.2 Run final verifier.
- [x] SBA8.3 Run workspace verification.
- [x] SBA8.4 Update closeout.

## Verifier

Added:

- `scripts/verify-server-system-bridge-adapters-extraction.sh`

The verifier checks:

- completed SBA0-SBA7 proof artifacts,
- workspace membership for `nimbus-system`, `nimbus-bridge`, `nimbus-auth`,
  and `nimbus-license`,
- the deliberate absence of aggregate `nimbus-adapters`,
- forbidden dependency/import edges for extracted crates,
- adapter runtime-host bypass prevention,
- neutral auth/license contract routing,
- ordered follow-on decisions,
- `_nimbus` evidence routing through system-owned writers,
- focused test evidence in proof files,
- `cargo check --workspace`.

Final run:

```text
bash scripts/verify-server-system-bridge-adapters-extraction.sh
Summary: 12 passed, 0 failed
cargo check --workspace passed inside the verifier.
```

## Focused Tests

```text
cargo test -p nimbus-system -p nimbus-bridge -p nimbus-auth -p nimbus-license -- --nocapture
nimbus-auth: 5 passed, 0 failed
nimbus-bridge: 7 passed, 0 failed
nimbus-license: 2 passed, 0 failed
nimbus-system: 8 passed, 0 failed
```

```text
cargo test -p nimbus-server system_tenant -- --nocapture
7 passed, 0 failed, 759 filtered
integration filters: 0/23 and 0/32
```

```text
cargo test -p nimbus-server runtime_host -- --nocapture
5 passed, 0 failed, 761 filtered
integration filters: 0/23 and 0/32
```

```text
cargo test -p nimbus-server auth -- --nocapture
63 passed, 0 failed, 703 filtered
integration filters: 0/23 and 0/32
```

```text
cargo test -p nimbus-server cloud_functions -- --nocapture
39 passed, 0 failed, 727 filtered
integration filters: 0/23 and 0/32
```

```text
cargo test -p nimbus-server license -- --nocapture
22 passed, 0 failed, 744 filtered
integration filters: 0/23 and 0/32
```

## Formatting

```text
cargo fmt --all --check
passed
```

## Final Decisions

- `nimbus-system`: extracted.
- `nimbus-bridge`: extracted.
- `nimbus-auth`: extracted.
- `nimbus-adapters`: not extracted; aggregate crate rejected until
  per-adapter readiness removes server-shaped imports.
- `nimbus-artifacts`: not extracted; pure contracts remain in
  `nimbus-tenant`, process-backed verifier effects remain in server wiring.
- `nimbus-provenance`: not extracted; provenance is still split across tenant
  authority, runtime integrity, execution admission, and verifier effects.
- `nimbus-operator`: not extracted; middleware/routes still close over Axum,
  `AppState`, audit, shutdown, and system evidence.
- `nimbus-services`: not extracted; service manager still depends on
  server-owned sandbox/service traits, system evidence, and adapter runtime
  wiring.
- `nimbus-license`: extracted.

Post-SBA supersession: the later Server Crate Extraction Completion plan
completed that per-adapter readiness work and added `crates/nimbus-adapters`
only as a feature-gated re-export facade. See
`docs/plans/proof/server-crate-extraction-completion/fce9-adapters-facade.md`
and `docs/plans/proof/server-crate-extraction-completion/fce10-closeout.md`.
The SBA verifier now accepts the facade only when those later proofs are
completed.

## Extracted Crates

- `crates/nimbus-system`
- `crates/nimbus-bridge`
- `crates/nimbus-auth`
- `crates/nimbus-license`

## Intentionally Retained Server Modules

- `crates/nimbus-server/src/adapters`
- `crates/nimbus-server/src/application_auth.rs` as transport/deployment
  auth glue
- `crates/nimbus-server/src/artifact_verifier_effects.rs` and children
- `crates/nimbus-server/src/execution/invocations`
- `crates/nimbus-server/src/local_server`
- `crates/nimbus-server/src/http/local_admin.rs`
- `crates/nimbus-server/src/http/services.rs`
- `crates/nimbus-server/src/service_manager.rs` and children
- `crates/nimbus-server/src/service_registry.rs`
- `crates/nimbus-server/src/sandbox.rs`

## Follow-Up Plans

Recommended next plans, in order:

1. Per-adapter extraction readiness, starting with MongoDB, then
   Firebase/provider-family, Cloud Functions, and Convex.
2. Artifact-effects readiness to separate process-backed verifier execution
   from pure artifact contracts.
3. Services readiness to invert system evidence writes and remove the
   `local_enforcement` shim before considering `nimbus-services`.
4. Operator readiness to split token/session value logic from Axum middleware,
   audit, shutdown, and system-event effects.
