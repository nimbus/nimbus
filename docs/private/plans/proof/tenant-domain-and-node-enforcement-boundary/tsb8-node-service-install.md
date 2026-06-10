# TSB8 Node Service Install

Date: 2026-05-27

## Status

Status: `done`

## Git Base

- Branch: `main`
- Base revision: `b570b02c`

## Files Touched

- `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
- `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb8-node-service-install.md`
- `crates/nimbus-bin/src/cli_ux.rs`
- `crates/nimbus-bin/src/main.rs`
- `crates/nimbus-bin/src/node_service.rs`
- `crates/nimbus-bin/src/start/boot.rs`
- `crates/nimbus-bin/src/start/mod.rs`

## Requirement IDs Touched

- `REQ-ARTIFACT`: `nimbus node install/status/logs/doctor/uninstall` is now an
  explicit operator surface. Native systemd and containerized Quadlet artifacts
  are generated from typed Nimbus-owned renderers with deterministic provenance
  hashes, no raw unit/Quadlet text input, overwrite protection, dry-run review,
  user/system locations, and fixed `systemctl`/`journalctl` invocations without
  a shell.
- `REQ-RAW`: native unit rendering only accepts safe absolute binary/state
  paths and generates `ExecStart`; Quadlet rendering accepts only explicit
  `ghcr.io/nimbus/nimbus:<tag>` image references and exposes no raw
  `PodmanArgs`, host networking, privileged mode, arbitrary sections, or host
  mount pass-through.
- `REQ-LIFECYCLE`: native optional socket activation now has a real
  `nimbus start --systemd-socket-activation` listener path. The inherited
  listener is validated against the non-loopback admin-token gate using the
  actual socket address before the service/scheduler are started.
- `REQ-DOCS`: plan state and this proof note record exact files, commands,
  result counts, risks, and the next phase.

## Current Slice

Behavior changed intentionally:

- Added `nimbus node install`, `status`, `logs`, `doctor`, and `uninstall`.
- Added `NativeSystemdNodeService` rendering for `nimbus.service` plus optional
  `nimbus.socket`. The service uses a trusted Nimbus binary path, generated
  `ExecStart`, fixed restart/hardening/state settings, user/system install
  locations, dry-run rendering, provenance comments, overwrite protection, and
  `systemctl` execution without `shell`.
- Added `QuadletNodeService` rendering for `nimbus.container` when Nimbus is run
  as a host-managed Podman OCI image. It preserves the container image contract:
  `ghcr.io/nimbus/nimbus:<version>` image references, foreground Nimbus
  entrypoint, loopback `PublishPort=127.0.0.1:8080:8080`, state volume
  `/var/lib/nimbus`, health command for `/health`, no systemd-in-container, and
  no raw Podman/systemd escape hatches.
- Added node doctor diagnostics for Linux/systemctl/journalctl/Podman presence
  and the resolved artifact directory.
- Added `nimbus start --systemd-socket-activation` so rendered native socket
  units are backed by an actual inherited listener path instead of inert text.
  The path reuses the existing non-loopback opt-in and local-admin-token
  freshness checks against the inherited listener address.

Tests added under `crates/nimbus-bin/src/node_service.rs`:

- native systemd dry-run renders a service with provenance and hardening
- native socket activation renders matching `nimbus.service` and `nimbus.socket`
- Quadlet dry-run preserves the OCI image contract without escape hatches
- Quadlet rejects non-Nimbus and `latest` image references
- native paths reject whitespace and systemd specifiers
- artifact writes refuse overwrite without `--overwrite`
- doctor reports container mode and no systemd-in-container
- root CLI parses native and containerized `nimbus node install`

## Verification Commands

Commands run:

```sh
cargo fmt --all --check
cargo test -p nimbus-bin node_service -- --nocapture
cargo test -p nimbus-bin cli_surface -- --nocapture
cargo check -p nimbus-bin
cargo clippy -p nimbus-bin --all-targets --no-deps
git diff --check -- crates/nimbus-bin/src/main.rs crates/nimbus-bin/src/cli_ux.rs crates/nimbus-bin/src/node_service.rs crates/nimbus-bin/src/start/mod.rs crates/nimbus-bin/src/start/boot.rs docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb8-node-service-install.md
npm run docs:validate-refs:strict
```

Results:

- `cargo fmt --all --check`: passed with no output.
- `cargo test -p nimbus-bin node_service -- --nocapture`: 8 passed, 0 failed,
  541 filtered out. `tests/server_discovery_serde.rs` had 0 matching tests.
- `cargo test -p nimbus-bin cli_surface -- --nocapture`: 26 passed, 0 failed,
  523 filtered out. `tests/server_discovery_serde.rs` had 0 matching tests.
- `cargo check -p nimbus-bin`: passed, `Finished dev profile`.
- `cargo clippy -p nimbus-bin --all-targets --no-deps`: passed,
  `Finished dev profile`.
- `git diff --check -- ...`: passed with no output.
- `npm run docs:validate-refs:strict`: `docs reference validation: pass (210
  working-tree Markdown files)`.

## Remaining Risks

- The current host is not a Linux systemd/Podman node, so actual host install,
  enable, start, journal, and uninstall were not live-executed. The product path
  fails closed for non-Linux mutation and supports `--dry-run` review.
- Socket activation is render-tested and type-checked; a future Linux
  integration lane should exercise the inherited fd path under real systemd.
- Quadlet support depends on the host Podman/systemd generator. Nimbus renders a
  strict `.container` artifact and doctor diagnostics, but does not probe the
  generator's exact version in this phase.

## Next Resumable Action

Commit the TSB8 node-service install checkpoint, then start TSB9 by adding
explicit `nimbus compose export quadlet` support for operator-reviewed static
exports.
