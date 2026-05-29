# NDB4 Proof — error taxonomy

Replaces NDB3's temporary `map_zbus` with the full taxonomy in
`zbus_client/error.rs`, after extending `nimbus_core::Error` with the two
variants the D-Bus surface needs honest homes for.

## Core enum extension

`crates/nimbus-core/src/error.rs` gains:

```rust
#[error("not found: {0}")]
NotFound(String),
#[error("transport error: {0}")]
Transport(String),
```

`nimbus_core::Error` is **not** `#[non_exhaustive]`, so every exhaustive match
on it had to be updated. A workspace check found the only exhaustive matches
in `nimbus-server` (all other crates use `_` arms or don't match `Error`
exhaustively):

- `error_envelope.rs::from_core_error` — added `NotFound` (`op.not_found`) and
  `Transport` (`service.transport`, retryable) envelope arms.
- `error_envelope.rs` status mapping — `NotFound` → `404`, `Transport` → `503
  SERVICE_UNAVAILABLE`.
- `adapters/cloud_functions/http/callable.rs` — had a `_` arm (compiled
  regardless) but mapped explicitly anyway: `NotFound` → `404 NOT_FOUND`,
  `Transport` → `503 UNAVAILABLE`.

## Taxonomy (`error.rs`)

D-Bus method-error *replies* arrive as `zbus::Error::MethodError(name, ..)`
keyed by the error-name string; only zbus-internal failures use
`zbus::Error::FDO`. Both shapes plus the transport variants are mapped:

| zbus error / D-Bus name | `nimbus_core::Error` |
|---|---|
| `InputOutput`/`Address`/`Handshake`/`InvalidReply`/`MissingField` | `Transport` |
| `…Disconnected/NoServer/NoNetwork/NoReply/Timeout/TimedOut` | `Transport` |
| `…AccessDenied/AuthFailed/InteractiveAuthorizationRequired` | `PermissionDenied` |
| `…NoSuchUnit/NoSuchUnitProcess/UnknownObject/UnknownInterface/UnknownMethod/ServiceUnknown/FileNotFound`, `InterfaceNotFound` | `NotFound` |
| `…InvalidArgs/InvalidSignature/NotSupported/Failed` | `InvalidInput` |
| `…NoMemory/LimitsExceeded` | `ResourceExhausted` |
| (any other) | `Internal` |

Every D-Bus call in the client routes through `map_zbus` (NDB3 left the seam;
NDB4 swaps the body — `signals.rs` and `mod.rs` are unchanged because they
already `use error::map_zbus`).

## Tests

`error.rs` unit tests assert the mapping by error-name string (the realistic
systemd reply path, testable without a live `Message`):

- 12 standard + systemd error names → expected variant
  (`NoSuchUnit`→`NotFound`, `AccessDenied`→`PermissionDenied`,
  `InvalidArgs`→`InvalidInput`, `Disconnected`→`Transport`,
  `LimitsExceeded`→`ResourceExhausted`, unknown systemd name→`Internal`).
- `InputOutput`→`Transport`, `InterfaceNotFound`→`NotFound`,
  `InvalidReply`→`Transport` via `map_zbus`.
- internal `fdo::Error` (`AccessDenied`, `LimitsExceeded`) via `map_fdo`.

## Verification

- `cargo check --workspace --all-targets` — **0 E0004** (every exhaustive
  `Error` match across the workspace now covers `NotFound`/`Transport`).
  Match sites updated: `nimbus-server/error_envelope.rs` (×2),
  `nimbus-server/.../callable.rs`, `nimbus-bridge/responses.rs`,
  `nimbus-firebase/errors.rs` (×2), `nimbus-convex/host_bridge/responses.rs`
  (enum mirror + both directions), `nimbus-bin/machine/backend.rs`.
- `cargo test -p nimbus-node --features systemd-dbus,systemd-dbus-test-bus`
  — `36 passed; 0 failed` (incl. the 3 new `error.rs` taxonomy tests).
- `cargo clippy … --all-targets` — clean.

(Local full-workspace compile required stubbing the gitignored UI dist +
convex codegen artifacts, whose generation is broken in this worktree's
toolchain — unrelated to NDB; CI regenerates them properly.)

## Verifier

Condition 7 (core `Transport`/`NotFound` variants + `error.rs` taxonomy with
documented source-error names) flips to PASS. Verifier after NDB4:
`7 passed, 3 failed`.
