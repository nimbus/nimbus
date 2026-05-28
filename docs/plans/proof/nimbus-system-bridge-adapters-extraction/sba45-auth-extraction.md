# SBA4.5 Auth Extraction Proof

Date: 2026-05-28
Status: completed

## Scope

Extract neutral application-auth contracts into `crates/nimbus-auth` before
adapter extraction, while leaving deployment resolution, transport extraction,
router wiring, and local admin/operator authority in `nimbus-server`.

## Candidate Files

- `crates/nimbus-server/src/application_auth.rs`
- Adapter imports of `crate::application_auth`
- Server transport wrappers that extract HTTP headers or gRPC metadata

## Intended Moves

- Create `crates/nimbus-auth` and add it to the workspace.
- Move neutral auth contracts:
  `ApplicationAuthVerifier`, `ResolvedApplicationAuth`, principal
  normalization, neutral bearer-value parsing, subject alias normalization, and
  classified auth errors.
- Keep `AppState`, `DeploymentState`, axum header extraction, tonic metadata
  extraction, local admin token authority, route-family authorization, and
  adapter registry lookup in `nimbus-server`.
- Update server and adapter code to import neutral contracts from
  `nimbus_auth` instead of `crate::application_auth`.

## Forbidden Imports For Extracted Crate

SBA4.5 is not complete while `crates/nimbus-auth` contains production imports
or references to:

- `nimbus_server`
- `AppState`
- `DeploymentState`
- `axum`
- `tonic`
- `router`
- `LocalAdmin`
- `local_admin`
- adapter registries

## Task Checklist

- [x] SBA45.1 Create `crates/nimbus-auth`.
- [x] SBA45.2 Move neutral auth contracts.
- [x] SBA45.3 Keep deployment and transport wrappers in server.
- [x] SBA45.4 Update consumers.
- [x] SBA45.5 Test auth behavior.

## Verification Log

- `cargo metadata --no-deps --format-version 1` showed `crates/nimbus-auth`
  in `workspace_members`.
- `cargo tree -p nimbus-auth --edges normal --depth 1`: `nimbus-auth`
  depends on `futures`, `nimbus-core`, `nimbus-runtime`, `serde`, and
  `serde_json`; no `nimbus-server` or adapter edge.
- `rg -n "nimbus_server|AppState|DeploymentState|axum|tonic|router|LocalAdmin|local_admin|crate::adapters|ConvexRegistry|FirebaseConfig|CloudFunctionsRegistry" crates/nimbus-auth -g '*.rs' -g 'Cargo.toml'`
  returned no matches.
- `rg -n "crate::application_auth::(ApplicationAuthVerifier|normalize_principal_context|ResolvedApplicationAuth)|use crate::application_auth::\\{[^\\n]*(ApplicationAuthVerifier|normalize_principal_context|ResolvedApplicationAuth)" crates/nimbus-server/src -g '*.rs'`
  returned no matches.
- `cargo check -p nimbus-auth` passed.
- `cargo check -p nimbus-server` passed.
- `cargo test -p nimbus-auth -- --nocapture` passed: 5 passed, 0 failed.
- `cargo test -p nimbus-server auth -- --nocapture` passed: 63 passed,
  0 failed, 705 filtered; integration filters reported 0/23 and 0/32.
- `cargo check --workspace` passed.
- `cargo fmt --all --check` passed.

## Extraction Decision

`nimbus-auth` is extracted. The server `application_auth` module remains only
as transport/deployment glue: it extracts axum headers and tonic metadata,
consults `DeploymentState`, applies Firebase emulator opt-in policy, and maps
neutral `ApplicationAuthError` values onto `AppError`/gRPC status. Shared auth
contracts and principal normalization now come from `nimbus_auth`.
