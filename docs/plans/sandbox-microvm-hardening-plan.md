# Plan: Sandbox MicroVM Hardening

Focused active plan for closing the sandbox isolation audit findings that
block production-safe microVM service exposure.

This plan starts from the completed execution-boundary baseline:
`docs/plans/archive/execution-isolation-and-runtime-backends-plan.md`.

---

## Status

- **Status:** `active`
- **Primary owner:** this plan
- **Autonomous goal prompt:**
  `docs/plans/prompts/sandbox-microvm-hardening-goal.md`
- **Source audit:**
  `docs/plans/security/sandbox-isolation-audit.md`
- **Architecture baseline:**
  `docs/architecture/sandbox/microvm-service-baseline.md`
- **Activation gate:** EIB7 closed with microVM service exposure marked no-go
  until EIB4 F1-F4/F7 are implemented or explicitly accepted with operator
  controls.

## Scope

This plan owns:

- F1 explicit krun OCI seccomp profile
- F2 explicit krun process capability set
- F3 `process.noNewPrivileges`
- F4 TSI bind-address carry-through through Nimbus, patched crun, and libkrun
- F7 patched crun `krun.port_map` parser robustness
- proof artifacts that show the generated `config.json` and Linux krun smoke
  behavior

This plan does not own:

- Bun/JSC, wasmtime, or runtime-engine routing
- distribution image provenance policy for F6
- long-term non-root VMM / `/dev/kvm` investigation for F5
- macOS machine VM implementation beyond making sure Linux service microVM
  semantics remain clear

## Phase Status Ledger

| Phase | Status | Goal | Verification |
| --- | --- | --- | --- |
| SMH0 | `done` | Promote this focused active plan and wire plan routing. | `git diff --check` passed for touched docs. |
| SMH1 | `done` | Land the local krun OCI bundle hardening baseline for F1-F3. | `cargo test -p nimbus-sandbox bundle_config --lib` passed 14 tests; `cargo fmt --all --check` passed; `git diff --check` passed. |
| SMH2 | `in_progress` | Fix F4 TSI bind-address carry-through across Nimbus, nimbus-crun, and libkrun. | Rust unit tests and nimbus-crun patch verification pass; libkrun bind-address hook and Linux localhost-only smoke remain. |
| SMH3 | `done` | Add F7 malformed-input coverage for patched crun `krun.port_map` parsing. | `bash scripts/verify-patch.sh /Users/jack/src/github.com/containers/crun` passed and now runs parser malformed-input coverage. |
| SMH4 | `todo` | Record Linux host proof for F1-F4/F7 and update production exposure gates. | Linux krun smoke logs and rendered `config.json` excerpt. |

## Phase Details

### SMH0: Active Plan Promotion

Status: `done`

Deliverables:

- add this plan to `docs/plans/`
- list it as active in `docs/plans/README.md`
- point future sandbox hardening work here from the security audit

Acceptance criteria:

- future microVM hardening work has a focused active owner
- active plan list remains small and non-overlapping

### SMH1: Local OCI Bundle Hardening

Status: `done`

Deliverables:

- add `process.noNewPrivileges: true` to krun bundle generation
- add explicit `process.capabilities` for the host-side krun VMM process
- add an explicit `linux.seccomp` allowlist profile
- add unit tests that assert the generated OCI JSON contains the security
  baseline
- update the security audit with the landed local status and remaining Linux
  smoke requirement

Acceptance criteria:

- generated krun `config.json` no longer omits F1-F3 fields
- tests assert the exact hardening shape
- Linux smoke is still recorded as required before claiming production closure

Completion notes:

- `build_bundle_config()` now emits `process.noNewPrivileges: true`
- `build_bundle_config()` now emits explicit krun VMM capabilities:
  `CAP_NET_ADMIN`, `CAP_NET_BIND_SERVICE`, and `CAP_SYS_ADMIN`
- `build_bundle_config()` now emits an explicit `linux.seccomp` allowlist with
  `SCMP_ACT_ERRNO` default denial
- unit tests assert the generated OCI JSON shape for F1-F3
- F1-F3 remain pending Linux krun smoke before they are marked production
  closed in the security audit

### SMH2: TSI Bind Address

Status: `in_progress`

Deliverables:

- extend Nimbus `krun.port_map` formatting to carry host address explicitly
- patch nimbus-crun to parse and validate address-bearing entries
- patch or adopt libkrun support for bind addresses
- reject unsupported host-address combinations before launch when the patched
  stack is unavailable

Acceptance criteria:

- localhost bindings do not expose services on `0.0.0.0`
- malformed or out-of-range port-map entries fail before `krun_set_port_map()`
- Linux smoke proves service reachability only on the intended host address

Current notes:

- Nimbus now formats krun TSI annotations as address-bearing entries:
  `ADDR:HOST_PORT:GUEST_PORT`, with IPv6 rendered as
  `[ADDR]:HOST_PORT:GUEST_PORT`
- patched `nimbus-crun` validates legacy and address-bearing entries before
  calling libkrun
- address-bearing entries fail closed unless libkrun exports
  `krun_set_port_map_with_bind_address`
- required libkrun hook shape:
  `int32_t krun_set_port_map_with_bind_address(uint32_t ctx_id, const char *const port_map[])`
- the hook must preserve the address-bearing entry format and bind each TSI
  listener to the requested host address
- no local `containers/libkrun` worktree is available, so the libkrun hook and
  Linux localhost-only smoke remain the active SMH2 blocker

### SMH3: crun Parser Robustness

Status: `done`

Deliverables:

- add parser coverage in `~/src/github.com/nimbus/nimbus-crun`
- include empty, malformed, out-of-range, duplicate, and long annotation input
- keep the verification script green against the pinned crun source

Acceptance criteria:

- parser rejects invalid input without partial side effects
- verification can run from the nimbus-crun worktree without ad hoc commands

Completion notes:

- `nimbus-crun` now has `tests/port_map_parser_test.c` covering legacy,
  IPv4, bracketed IPv6, empty, malformed, out-of-range, duplicate, and long
  `krun.port_map` annotations
- `scripts/verify-port-map-parser.sh` compiles and runs the parser harness
- `scripts/verify-patch.sh` now runs both the crun patch dry-run and parser
  malformed-input coverage
- `nimbus-crun` CI path filters include `tests/**`, and the verify job checks
  the new parser script syntax
- `nimbus-crun` checkpoint commit: `576e1f9` (`Harden krun port map parsing`)

### SMH4: Linux Host Proof And Gate Update

Status: `todo`

Deliverables:

- run the focused nimbus-sandbox unit tests
- run the Linux krun smoke that boots a service with the hardened bundle
- capture rendered `config.json` proof for F1-F3
- update `docs/plans/security/sandbox-isolation-audit.md` statuses and
  production exposure gates

Acceptance criteria:

- F1-F3 are either closed or explicitly marked pending with the exact blocker
- F4/F7 status reflects the patched-stack proof result
- remaining production exposure blockers are clear

Current notes:

- local host preflight is `Darwin arm64`, with no `buildah` or `crun` on PATH,
  so Linux krun smoke cannot run from this machine
- SMH4 remains gated on SMH2's libkrun bind-address hook plus a capable Linux
  host
- next Linux proof command should start from the existing krun smoke path after
  the patched `nimbus-crun` and libkrun hook are installed together

## Execution Log

| Date | Phase | Status | Notes | Verification |
| --- | --- | --- | --- | --- |
| 2026-05-21 | SMH0 | `done` | Promoted this focused active plan from the archived execution-boundary baseline and sandbox isolation audit. | Documentation-only change; `git diff --check -- crates/nimbus-sandbox/src/backends/krun/bundle.rs docs/plans/sandbox-microvm-hardening-plan.md docs/plans/README.md docs/plans/security/sandbox-isolation-audit.md` passed. |
| 2026-05-21 | SMH1 | `done` | Landed the local krun OCI bundle hardening baseline for F1-F3 in `crates/nimbus-sandbox/src/backends/krun/bundle.rs`. The generated bundle now includes `process.noNewPrivileges`, an explicit krun capability set, and a seccomp allowlist. Linux host smoke remains required before production exposure gates can close. | `cargo fmt --all` passed; `cargo fmt --all --check` passed; `cargo test -p nimbus-sandbox bundle_config --lib` passed 14 tests, 0 failed, 77 filtered out; `git diff --check -- crates/nimbus-sandbox/src/backends/krun/bundle.rs docs/plans/sandbox-microvm-hardening-plan.md docs/plans/README.md docs/plans/security/sandbox-isolation-audit.md` passed. |
| 2026-05-21 | SMH2 | `in_progress` | Nimbus now emits address-bearing `krun.port_map` entries from `SandboxPortBinding::host_address`, validates zero ports locally, and keeps IPv6 unambiguous with `[ADDR]:HOST_PORT:GUEST_PORT`. `nimbus-crun` now validates address-bearing annotations and fails closed unless libkrun exports `krun_set_port_map_with_bind_address`. The required libkrun hook and Linux localhost-only smoke are still pending. | `cargo fmt --all` passed; `cargo fmt --all --check` passed; `cargo test -p nimbus-sandbox bundle_config --lib` passed 16 tests, 0 failed, 79 filtered out; `cargo test -p nimbus-sandbox format_port_map --lib` passed 2 tests, 0 failed, 93 filtered out; `cargo test -p nimbus-sandbox krun::vm --lib` passed 27 tests, 0 failed, 68 filtered out; `bash scripts/verify-patch.sh /Users/jack/src/github.com/containers/crun` in `nimbus-crun` passed; `git diff --check` passed in both worktrees for touched files. |
| 2026-05-21 | SMH3 | `done` | Added nimbus-crun parser malformed-input coverage and wired it into the patch verifier and CI path filters. Coverage includes empty, malformed, out-of-range, duplicate, and long annotations, plus valid legacy, IPv4, and bracketed IPv6 forms. Checkpointed in `nimbus-crun` commit `576e1f9` (`Harden krun port map parsing`). | `bash -n scripts/verify-patch.sh` passed; `bash -n scripts/verify-port-map-parser.sh` passed; `bash scripts/verify-port-map-parser.sh` passed; `bash scripts/verify-patch.sh /Users/jack/src/github.com/containers/crun` passed; `git diff --check -- .github/workflows/build.yml README.md patches/0001-krun-add-tsi-port-mapping-via-oci-annotation.patch scripts/verify-patch.sh scripts/verify-port-map-parser.sh tests/port_map_parser_test.c` passed. |
| 2026-05-21 | SMH4 | `todo` | Linux host proof cannot run locally yet: this host is `Darwin arm64`, `buildah` is not on PATH, and `crun` is not on PATH. SMH4 remains gated on the SMH2 libkrun bind-address hook and a capable Linux host. | `uname -s` returned `Darwin`; `uname -m` returned `arm64`; `command -v buildah` exited 1; `command -v crun` exited 1. |
