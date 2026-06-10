# TSB9 Compose Quadlet Export

Date: 2026-05-27

## Status

Status: `done`

## Git Base

- Branch: `main`
- Base revision: `7e542a93`

## Files Touched

- `docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md`
- `docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb9-compose-quadlet-export.md`
- `crates/nimbus-bin/src/cli_ux.rs`
- `crates/nimbus-bin/src/compose/commands.rs`
- `crates/nimbus-bin/src/compose/mod.rs`
- `crates/nimbus-bin/src/compose/quadlet_export.rs`

## Requirement IDs Touched

- `REQ-ADMIT`: export loads the existing `ComposeProjectPlan`, so artifacts are
  rendered from the admitted Nimbus Compose plan rather than raw Compose text.
- `REQ-ARTIFACT`: `nimbus compose export quadlet` now supports stdout by
  default, `--output-dir`, `--service`, `--mode containers|pod|kube`,
  `--podman-version`, `--overwrite`, and `--strict`. Artifacts carry
  deterministic Nimbus provenance comments.
- `REQ-RAW`: the renderer owns a fixed allowlist of Quadlet/Kubernetes fields.
  It does not accept raw unit text, arbitrary Quadlet fields, `PodmanArgs`,
  host networking, privileged mode, or arbitrary systemd sections. Unsupported
  or lossy Compose material becomes a review warning and fails in `--strict`.
- `REQ-DOCS`: plan state and this proof note record exact files, commands,
  result counts, risks, and the next phase.

## Current Slice

Behavior changed intentionally:

- Added nested CLI shape `nimbus compose export quadlet`.
- Added a static Quadlet exporter under `compose::quadlet_export`; it is not
  wired into runtime start/stop paths.
- Containers mode renders one `.container` artifact per selected image service.
- Pod mode renders a `.pod` plus `.container` artifacts joined with `Pod=` /
  `StartWithPod=true`.
- Kube mode renders a `.kube` artifact plus a review YAML artifact for Quadlet
  `.kube` use.
- Export defaults to stdout with `### <filename>` separators and writes to
  `--output-dir` only when explicitly requested. Existing files are refused
  unless `--overwrite` is passed.
- Export warnings are emitted for known ignored Compose fields and for features
  that are unsupported or intentionally not lowered. `--strict` turns every
  warning into an error.
- Output uses fixed fields modeled after Podman's documented Quadlet keys such
  as `[Container]`, `Image=`, `PublishPort=`, `Volume=`, `[Pod]`, and `[Kube]`;
  it deliberately omits `PodmanArgs`, `Network=host`, and privileged
  pass-through.

Tests added under `crates/nimbus-bin/src/compose/quadlet_export.rs`:

- containers mode renders a reviewable `.container` artifact to stdout shape
- strict mode fails when export would drop/rewrite review material
- output-dir writes refuse overwrite without `--overwrite`
- pod and kube modes render expected review artifacts
- `nimbus compose export quadlet` CLI parsing works

## Verification Commands

Commands run:

```sh
cargo fmt --all --check
cargo test -p nimbus-bin quadlet_export -- --nocapture
cargo test -p nimbus-bin compose::tests::parse_help -- --nocapture
cargo check -p nimbus-bin
cargo clippy -p nimbus-bin --all-targets --no-deps
git diff --check -- crates/nimbus-bin/src/compose/commands.rs crates/nimbus-bin/src/compose/mod.rs crates/nimbus-bin/src/compose/quadlet_export.rs crates/nimbus-bin/src/cli_ux.rs docs/plans/tenant-domain-and-node-enforcement-boundary-plan.md docs/plans/proof/tenant-domain-and-node-enforcement-boundary/tsb9-compose-quadlet-export.md
npm run docs:validate-refs:strict
```

Results:

- `cargo fmt --all --check`: passed with no output.
- `cargo test -p nimbus-bin quadlet_export -- --nocapture`: 5 passed, 0
  failed, 549 filtered out. `tests/server_discovery_serde.rs` had 0 matching
  tests.
- `cargo test -p nimbus-bin compose::tests::parse_help -- --nocapture`: 18
  passed, 0 failed, 536 filtered out. `tests/server_discovery_serde.rs` had 0
  matching tests.
- `cargo check -p nimbus-bin`: passed, `Finished dev profile`.
- `cargo clippy -p nimbus-bin --all-targets --no-deps`: passed,
  `Finished dev profile`.
- `git diff --check -- ...`: passed with no output.
- `npm run docs:validate-refs:strict`: `docs reference validation: pass (210
  working-tree Markdown files)`.

## Remaining Risks

- The exporter uses local Podman Quadlet documentation as the compatibility
  source and golden tests assert representative Podlet/Quadlet-shaped fields,
  but it does not shell out to Podlet or Podman in the control path.
- Kube mode is intentionally review-oriented: volumes, probes, mixed restart
  semantics, and user/working-dir fields produce warnings instead of silent
  lowering. `--strict` is required for no-warning exports.
- Build services are not exported directly; operators must build/tag images
  first or use non-strict output as a warning-bearing review artifact.

## Next Resumable Action

Commit the TSB9 Quadlet export checkpoint, then start TSB10 by updating
operator docs, install docs, and machine-os references to distinguish native
node service units, containerized Quadlet node installs, dynamic transient
units, explicit Quadlet export, and direct-process fallback.
