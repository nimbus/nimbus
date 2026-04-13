# Plan: VMM Infrastructure — Patched crun + System Dependencies

Canonical plan for the VMM infrastructure that enables neovex to run OCI/Docker
images in hardware-isolated microVMs. Follows the Podman distribution model:
neovex is a single binary with system package dependencies.

**Platform scope: Linux.** The entire stack (conmon, crun, buildah, libkrun/KVM)
requires Linux. On macOS, neovex runs inside a Linux machine VM
(krunkit/libkrun) but services use standard containers (crun, no krun
handler) — same as Podman on macOS. MicroVM isolation via the krun handler
is a Linux production feature. See `distribution-plan.md` Channel 4.

This plan produces the VMM foundation that `microvm-runtime-plan.md` builds on.

---

## Status

- **Status:** `done`
- **Primary owner:** this plan
- **Activation gate:** met on 2026-04-11 when the krun-backed microVM workstream
  moved from naming and seam cleanup into concrete VMM infrastructure
- **Completion gate:** met on 2026-04-13 when V1 through V3 all reached `done`
  with real Linux-host verification evidence recorded in this plan
- **Related plans:**
  - `docs/plans/archive/runtime-sandbox-architecture-plan.md` — completed
    baseline that owns the canonical `neovex-sandbox` crate naming and the
    server-facing sandbox seam this plan must consume for any Rust
    implementation work
  - `microvm-runtime-plan.md` — builds OCI management, lifecycle, engine
    integration on top of this plan's VMM layer
  - `distribution-plan.md` — packages neovex + dependencies for each channel
  - `distribution-plan.md` Channel 4 — macOS support via krunkit machine VM
    (runs neovex inside a Linux guest VM, with standard containers on macOS
    and the krun-backed microVM path remaining Linux-only)

## Current Assessed State

- This archived file is the completed control plane and baseline for the
  krun-backed VMM foundation workstream.
- The naming and crate-boundary prerequisite is complete in
  `docs/plans/archive/runtime-sandbox-architecture-plan.md`: `neovex-sandbox`
  is the canonical sandbox seam and `neovex-server` owns the first
  `SandboxCatalog` integration seam.
- The repo now owns the first concrete V1 artifacts:
  `patches/crun/0001-krun-add-tsi-port-mapping-via-oci-annotation.patch`,
  `scripts/verify-crun-patch.sh`, `scripts/check-vmm-host.sh`,
  `scripts/build-neovex-crun.sh`, `scripts/prepare-krun-bundle.sh`,
  `scripts/verify-krun-bundle-helper.sh`,
  `scripts/prepare-direct-krun-drill.sh`,
  `scripts/verify-direct-krun-drill-helper.sh`,
  `scripts/verify-runtime-separation.sh`,
  `scripts/verify-runtime-separation-helper.sh`,
  `scripts/fixtures/crun-spec-config.json`, `make verify-crun-patch`,
  `make check-vmm-host`, `make build-neovex-crun`,
  `make prepare-krun-bundle`, `make verify-krun-bundle-helper`,
  `make prepare-direct-krun-drill`,
  `make verify-direct-krun-drill-helper`,
  `make verify-runtime-separation`,
  `make verify-runtime-separation-helper`,
  `docs/reference/krun-vmm-host-validation.md`, and
  `.github/workflows/verify-neovex-crun-patch.yml`.
- The repo now also owns the first V2 reproducibility artifacts:
  `scripts/collect-vmm-package-versions.sh`,
  `scripts/prepare-conmon-krun-drill.sh`,
  `scripts/verify-conmon-krun-drill-helper.sh`,
  `scripts/prepare-linux-vmm-validation-bundle.sh`,
  `scripts/verify-linux-vmm-validation-bundle-helper.sh`,
  `make collect-vmm-package-versions`,
  `make prepare-conmon-krun-drill`, and
  `make verify-conmon-krun-drill-helper`,
  `make prepare-linux-vmm-validation-bundle`, and
  `make verify-linux-vmm-validation-bundle-helper`.
- The repo now also owns a deterministic macOS Podman-machine diagnostics lane:
  `scripts/collect-podman-machine-diagnostics.sh`,
  `scripts/verify-podman-machine-diagnostics-helper.sh`,
  `scripts/check-podman-machine-socket-paths.sh`,
  `scripts/verify-podman-machine-socket-paths-helper.sh`,
  `scripts/validate-podman-machine-readiness.sh`,
  `scripts/recreate-podman-machine.sh`,
  `scripts/verify-podman-machine-readiness-helper.sh`,
  `scripts/verify-podman-machine-recreate-helper.sh`,
  `make collect-podman-machine-diagnostics`,
  `make verify-podman-machine-diagnostics-helper`,
  `make check-podman-machine-socket-paths`, and
  `make verify-podman-machine-socket-paths-helper`,
  `make validate-podman-machine-readiness`, and
  `make recreate-podman-machine`,
  `make verify-podman-machine-readiness-helper`, and
  `make verify-podman-machine-recreate-helper`.
- The repo now also owns the first V3 code slice under
  `crates/neovex-sandbox/src/backends/krun/`: `bundle.rs` for OCI config
  generation, `buildah.rs` for backend-local buildah command assembly,
  `conmon.rs` for `conmon -> /usr/libexec/neovex/crun` launch planning,
  `command.rs` for backend-local command specs, and `vm.rs` for manifest-backed
  `start` / `inspect` / `stop` lowering behind the generic `SandboxBackend`
  trait. `SandboxSpec` now carries generic filesystem, process, and port
  binding inputs so the sandbox seam can describe a real launch without leaking
  krun nouns into the public API.
- The checked-in patch has been verified to apply cleanly against a real local
  checkout at `~/src/github.com/containers/crun`.
- The current local execution host is macOS 15.7.2 on Apple Silicon with no
  `/dev/kvm`, no `conmon`, no `buildah`, and no `crun`, so host-level V1/V2
  proof remains pending on a supported Linux system or Linux machine-VM guest
  even though the repo-owned validation artifacts now exist.
- Homebrew Podman `5.8.1` is installed on the current Mac. The original
  `applehv`/`vfkit` machine (`neovex-vmm-validation`) proved unusable: its
  serial log at
  `/var/folders/kw/d608x5pn4cq73rz78ztl92cw0000gn/T/podman/neovex-vmm-validation.log`
  showed Ignition failure followed by `systemd-fsck-root.service` failure and
  emergency mode, and its stale `Starting: true` metadata later blocked the
  libkrun lane until the machine was removed with `podman machine rm -f
  neovex-vmm-validation`.
- `krunkit` is now installed directly on the current Mac at
  `/opt/homebrew/bin/krunkit` via `brew tap slp/krunkit && brew install
  krunkit`; `krunkit --version` reports `krunkit 1.1.1`. This closes the
  host-side machine-provider dependency gap for future macOS guest attempts,
  but it does not by itself prove a working Linux guest or satisfy `LH1`
  through `LH6`.
- A fresh libkrun-backed Podman machine,
  `neovex-libkrun-validation`, can now be created on this host with
  `CONTAINERS_MACHINE_PROVIDER=libkrun`. The resulting machine config lives
  under `~/.config/containers/podman/machine/libkrun/`, its disk image lives
  under `~/.local/share/containers/podman/machine/libkrun/`, and a start
  attempt launches both `gvproxy` and `/opt/homebrew/bin/krunkit`.
- The current host's Docker-compatible system socket is not owned by Podman:
  `/var/run/docker.sock` currently resolves to `/Users/jack/.docker/run/docker.sock`.
  Podman and Podman Desktop document `podman-mac-helper` as the optional
  system helper that binds `/var/run/docker.sock` to a Podman-managed machine
  socket for Docker-compatible clients. That helper may matter for Compose,
  Testcontainers, or `docker` CLI workflows, but it does not change Linux guest
  boot semantics and is not a blocker for `V1` or `V2`.
- GUI menu-bar listings on this host are not a canonical machine-health source.
  Local CLI inspection shows `docker context ls` only knows `default` and
  `desktop-linux`, while `podman system connection list` contains
  `neovex-libkrun-users-only` and `neovex-libkrun-validation`. Treat the
  Podman CLI output and serial logs as authoritative for the macOS validation
  lane; a toolbar entry that happens to show the same names does not prove a
  healthy guest or a Docker-managed context.
- The current macOS guest lane is still blocked on guest readiness rather than
  provider installation. On this MacBook Pro (`Mac14,5`, Apple `M2 Max`,
  macOS `15.7.2`), `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine start
  neovex-libkrun-validation` reaches `State: running`, but both
  `CONTAINERS_MACHINE_PROVIDER=libkrun podman info` and
  `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine ssh ...` fail with SSH
  handshake resets, while the serial log at
  `/var/folders/kw/d608x5pn4cq73rz78ztl92cw0000gn/T/podman/neovex-libkrun-validation.log`
  shows repeated `watchdog: BUG: soft lockup` events in `(udev-worker)` while
  the Podman-generated `krunkit` command is running with `--nested`. After
  forced cleanup, the machine returns to `Running: false` / `Starting: false`,
  so this host is still not a usable Linux-guest validation lane.
- Source review now explains two macOS observations that were ambiguous before:
  Podman's Darwin provider code supports both `applehv` and `libkrun` on Apple
  Silicon, but falls back to `applehv` when no provider is configured; and the
  shared Apple VM launcher appends `--nested` whenever a libkrun machine is
  started, with a comment that `krunkit` itself ignores the argument on
  unsupported hardware. On this `M2 Max` host, seeing `--nested` in the
  `krunkit` command line therefore does not prove that nested virtualization
  actually turned on.
- The macOS architecture distinction is now explicit in the owning docs:
  rejected architecture is "machine VM plus nested microVM per service",
  accepted architecture is "machine VM plus standard containers per service".
  Treat the outer machine VM as the macOS isolation boundary and reserve the
  krun-backed per-service microVM path for Linux production.
- The latest unrestricted macOS diagnostics capture at
  `/tmp/neovex-libkrun-diagnostics` now preserves the current libkrun-machine
  state for this host. It shows `podman machine list` and
  `podman machine inspect` succeeding for `neovex-libkrun-validation`,
  `podman info --debug` still failing with `connection refused`, the machine
  currently stopped, the Podman API / ready / gvproxy socket paths still
  present under the Podman tmp root, and no live `krunkit` / `gvproxy`
  process surviving at capture time.
- A second unrestricted diagnostics capture at
  `/tmp/neovex-libkrun-users-only-diagnostics` now preserves the reduced-volume
  follow-up machine, `neovex-libkrun-users-only`. Its copied machine config
  proves Podman initialized only one `virtiofs` mount (`/Users -> /Users`) and
  left `LibKrunHypervisor.KRun.BinaryPath` unset. Even with that smaller mount
  shape, `podman machine list` still reports `Running: true` plus
  `Starting: true`, `podman info --debug` still fails with `dial tcp
  127.0.0.1:52251: connect: connection refused`, the expected Podman API socket
  file is missing, and the serial log tail shows repeated long-lived
  `rcu_preempt` stalls. Treat that as evidence that Podman's default multi-mount
  shape is not the only plausible cause of the current guest-readiness failure.
- Host-side VMM control is healthier than guest readiness, but still not
  sufficient to recover the wedged machine. On the reduced-volume machine,
  `curl -i -sS http://localhost:52273/` and `GET /vm/state` both return
  `{"state": "VirtualMachineStateRunning"}` from `krunkit`'s REST API, proving
  the VMM process and its control socket are alive even while Podman's SSH/API
  path stays unavailable. However, `POST /vm/state {"state":"Stop"}` returns
  `VirtualMachineStateStopping` without actually transitioning the VM out of the
  running state, and `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine stop
  neovex-libkrun-users-only` hangs as well.
- The reduced-volume machine also still lacks the expected host-side networking
  helpers. There is no live `gvproxy` process, no
  `neovex-libkrun-users-only-gvproxy.sock`, and no
  `neovex-libkrun-users-only-api.sock` under the Podman tmp root even while
  `krunkit` remains running. Treat the missing gvproxy/API socket pair as part
  of the current host-observable failure signature.
- A clean repro on 2026-04-12 moved the macOS diagnosis from "generic guest
  wedge" to a likely concrete host-path bug. After force-killing the wedged
  reduced-volume machine, `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine
  rm -f neovex-libkrun-users-only` succeeded, a fresh `podman machine init`
  reproduced the machine cleanly, and `podman --log-level=debug machine start`
  showed `gvproxy` launching plus the exact helper paths Podman passed into
  `krunkit`. The attached `krunkit-debug.sh` terminal output then failed during
  `virtio-net` activation with
  `Error activating virtio-net (eth0) backend: InvalidAddress(ENAMETOOLONG)`,
  followed by a `BadActivate` panic in `src/devices/src/virtio/mmio.rs`.
  Combined with the live socket paths
  (`.../neovex-libkrun-users-only-gvproxy.sock` length 94,
  `...-api.sock` length 90, `.../neovex-libkrun-users-only.sock` length 86),
  the current best macOS-specific hypothesis is that Podman's default tmp root
  under `/var/folders/.../T/podman` is too long for at least one `krunkit`
  unix-socket path on this host. The next focused experiment should shorten the
  tmp root, not keep varying guest mounts.
- A short-runtime-dir rerun now validates that path-budget diagnosis enough to
  treat it as the leading startup mitigation, but the latest live readiness
  bundle shows it is not sufficient on its own. Running the same
  reduced-volume machine under `TMPDIR=/tmp` drops the derived
  `...-gvproxy.sock-krun.sock` path from 104 characters to 60, and the
  resulting `/tmp/podman` artifact set includes the ready, API, gvproxy, and
  derived krun sockets. However, the captured readiness bundle at
  `/tmp/neovex-libkrun-users-only-readiness` still shows
  `podman --connection neovex-libkrun-users-only info --debug` failing with
  SSH handshake reset (`status=125`), `podman machine ssh` failing with
  `kex_exchange_identification: read: Connection reset by peer`
  (`status=255`), and the guest log entering emergency mode after Ignition and
  `systemd-fsck-root.service` failures. Treat "short runtime dir under `/tmp`"
  as a necessary fix for the `ENAMETOOLONG` startup blocker, plus a newly
  exposed second-stage guest/boot blocker rather than a complete macOS
  readiness fix.
- A fresh short-root machine now proves that second-stage failure is not a
  universal libkrun/macOS outcome on this host. A disposable machine,
  `neovex-libkrun-sr-fresh`, created from scratch with the same short tmp root
  and the same one-mount `/Users` layout, reached full readiness:
  `podman machine start` exited successfully, the readiness bundle at
  `/tmp/neovex-libkrun-sr-fresh-readiness` reports both connection-targeted
  `podman info --debug` and `podman machine ssh` as `ok`, and the guest log
  reaches `ready.service`, `sshd.service`, and the login prompt with Ignition
  applied successfully. That strongly suggests the current `users-only`
  machine's short-root failure is tied to stale/corrupted machine state
  (disk/EFI/boot metadata), not to the short-root libkrun path in general.
- The repo now owns a durable repair path for that stale-state case, and it is
  proven on this host. `scripts/recreate-podman-machine.sh` captures
  pre-recreate diagnostics, removes the named machine, recreates it with the
  proven short-root `/tmp/podman` recipe, starts it, and captures a fresh
  readiness bundle. A live run at `/tmp/neovex-libkrun-users-only-recreate`
  shows the pre-recreate failure signature preserved under
  `/tmp/neovex-libkrun-users-only-recreate/pre-diagnostics/summary.txt`
  (`podman info --debug` failed, the API socket was missing, and the gvproxy
  socket was missing), followed by a successful recreate where
  `/tmp/neovex-libkrun-users-only-recreate/readiness/summary.txt` reports
  `result ready info=ok ssh=ok`. On this Mac, short-root plus clean recreate is
  now a working Podman-aligned mitigation for stale libkrun machine state.
- Podman's published macOS contract is now explicit in this workstream:
  `podman machine` starts a Linux VM where containers are run, and Podman
  Desktop uses that same backend on non-Linux operating systems. Treat that as
  the architectural target for neovex on macOS: a thin host-side control shim
  plus a Linux guest that owns the real container toolchain. Do not turn the
  current macOS guest blocker into a reason to invent a host-side direct-OCI
  path on macOS.
- Podman's machine-os guest image source also supports that architecture. The
  `containers/podman-machine-os` build path layers `crun`, `crun-wasm`,
  `podman`, `containers-common`, `netavark`, and `aardvark-dns` into the guest
  and removes `runc`; it does not show a guest-side `krun` runtime lane. Treat
  that as source-backed evidence that Podman's macOS guest is a standard Linux
  container environment, not a nested per-service microVM environment.
- The source-reference hierarchy for this macOS lane is now explicit. Use
  Podman core source as the primary implementation reference for helper
  resolution, networking/socket wiring, and machine readiness:
  `pkg/machine/libkrun/stubber.go` owns the libkrun helper selection, and
  `pkg/machine/apple/apple.go` owns gvproxy waiting, unix-socket wiring,
  ready-socket setup, and the debug-mode `krunkit-debug.sh` flow. Treat Podman
  Desktop source as a secondary reference for installer checks, dependency UX,
  and operator flows rather than the canonical VMM launch contract.
- The intended CLI taxonomy for that future macOS path is now explicit too:
  keep `neovex serve` as the server-start verb, use `neovex machine ...` for
  machine-VM lifecycle, and do not overload `service` as the daemon-start
  command. If workload-management nouns appear later, prefer a plural
  `neovex services ...` namespace for listing or inspecting managed workloads
  instead of replacing `serve`. The current binary is still flag-driven, so
  these names are a target interface rather than a shipped subcommand surface.
- A supplementary macOS userspace proof lane now exists:
  `scripts/verify-neovex-crun-fedora-userspace.sh` succeeds through Docker
  Desktop against `fedora:43`, proving that the checked-in patch applies and
  the build helper can produce a Linux `aarch64` `crun` binary at
  `/tmp/neovex-crun-fedora-userspace-output/crun`. This is preflight evidence
  only; it does not prove `/dev/kvm`, `libkrun` VM boot, direct runtime
  execution, or conmon lifecycle behavior.
- The repo-owned CI workflow now also runs the Fedora userspace build helper
  against the pinned upstream `crun` source on `ubuntu-latest`, so helper drift
  can fail in automation before Linux host/KVM work resumes.
- Linux-host validation is now an explicit parallel lane for another agent or
  laptop to execute. It is required before marking `V1`/`V2` `done`, but it is
  not a blocker for continued repo-owned prep work, helper scripts, docs, or
  focused verification on this macOS machine.
- A patched `neovex-crun` binary (crun 1.27 with `+LIBKRUN`) is now built and
  installed at `/usr/libexec/neovex/crun` on the Debian 13 Linux host.
- The checked-in patch now targets upstream crun `1.27` (updated from `1.22`).
- libkrun `1.17.4` and libkrunfw `5.3.0` are built from source and installed
  at `/usr/local/lib64/` on the Linux host. They are not packaged for Debian 13.
- The first Debian system-integration drill (`LH1`-`LH6`) is complete on Debian
  13 x86_64 with kernel `6.12.74`.
- The direct private-runtime drill (`LH5`) has been executed: krun VM boots,
  TSI port mapping works (`18080:8080`), HTTP connectivity proven via BusyBox httpd.
- The conmon lifecycle drill (`LH6`) has been executed: `conmon → crun → libkrun VM`
  process tree verified, TSI port binding confirmed, exit file written.
- `scripts/prepare-krun-bundle.sh` now removes the `network` namespace and sets
  `terminal: false` for krun bundles — TSI requires host network, and
  non-interactive drills fail with terminal mode enabled.
- `crates/neovex-sandbox/` no longer stops at an empty seam. The first backend-
  owned `backends/krun/` slice is now checked in and verified with targeted
  unit tests, including bundle generation, buildah command assembly, conmon
  launch planning, and a plan-only backend mode that exercises the generic
  `SandboxBackend` trait without requiring Linux/KVM in this workspace.
- The repo now also owns the first Rust-backend Linux smoke path definition:
  `crates/neovex-sandbox/tests/krun_linux_smoke.rs` and
  `docs/reference/krun-sandbox-backend-smoke.md`. The ignored integration test
  is Linux-only and is designed to prove real VM boot, guest connectivity,
  manifest-backed restart recovery, and persisted logs through the Rust backend
  once it is run on a supported host.
- The remaining `V3` gap is host-level integration rather than crate shape:
  the Rust backend still needs a documented Linux smoke path that boots a real
  VM through the backend, reaches the guest service, proves persisted logs, and
  confirms the VM survives a neovex-process restart while conmon remains alive.
  The repo-owned test and runbook now exist; the missing step is executing that
  path on Linux and writing the resulting evidence back into this plan.
- `microvm-runtime-plan.md` was the follow-on consumer and should not be
  treated as the live progress ledger for the historical V1-V3 execution in
  this archived file.

## Current Review Findings

- The `krun.port_map` contract must be treated as `"host:guest"`, not
  `"guest:host"`.
- Current `libkrun` expects `krun_set_port_map()` to receive a
  null-terminated array of port-pair strings, so the crun patch must parse the
  OCI annotation rather than pass one raw comma-delimited string through.
- The current macOS blocker now has a source-backed host-level failure
  signature: `krunkit` can panic during `virtio-net` activation with
  `InvalidAddress(ENAMETOOLONG)` even on a freshly recreated one-mount machine.
  Treat overly long unix-socket paths under the default Podman tmp root as the
  primary hypothesis to test next.
- A short runtime directory under `/tmp` is now the first verified mitigation
  for that overflow. The repo-owned socket-path helper shows the current
  default Darwin tmp root yields a 104-character derived
  `...-gvproxy.sock-krun.sock` path, while `/tmp/podman` yields 60. The latest
  live evidence now sharpens the repair rule further: the old short-root
  `users-only` machine still failed in place, but a clean recreate under that
  same `/tmp/podman` root now succeeds. Treat the short runtime dir as
  necessary, and treat recreate/reset as the next repair step when a short-root
  machine still wedges.
- The failing short-root result is now scoped more narrowly: a fresh disposable
  machine with the same short tmp root and the same one-mount configuration
  boots successfully on this host, so the current `users-only` failure pattern
  is more likely stale machine state than an unavoidable libkrun/macOS
  limitation.
- That stale-state theory now has a working repo-owned mitigation. The
  pre-recreate bundle at `/tmp/neovex-libkrun-users-only-recreate/pre-diagnostics`
  still shows the old missing API/gvproxy socket pair and `podman info --debug`
  failure, while the post-recreate bundle at
  `/tmp/neovex-libkrun-users-only-recreate/readiness` reports both
  connection-targeted `podman info --debug` and `podman machine ssh` as `ok`.
  Prefer the checked-in recreate helper over more ad hoc restart/debug loops
  when a short-root machine looks stale on this host.
- Podman's current macOS `libkrun` launch path does **not** appear to honor the
  machine-config `LibKrunHypervisor.KRun.BinaryPath` field for startup. The
  `libkrun` stubber hardcodes `krunkit` and passes that constant into
  `apple.StartGenericAppleVM(...)`, which then resolves the helper with
  `config.Default().FindHelperBinary(cmdBinary, true)`. Treat config-only
  `BinaryPath` edits as insufficient for a wrapper experiment unless fresh
  source evidence proves otherwise.
- Use Podman core, not Podman Desktop, as the canonical implementation
  reference for neovex's macOS machine layer. Podman Desktop is still useful
  for dependency detection, install flows, and UX, but the actual helper
  invocation, socket layout, and readiness contract live in Podman's machine
  backend.
- In the crun+krun model, the crun process remains the VMM until guest exit.
  Any operator drill, process-tree expectation, or later automation must model
  `conmon -> crun` as a long-lived relationship.
- On the current macOS `15.7.2` / Apple `M2 Max` host, the Podman `5.8.1`
  libkrun provider still launches `krunkit` with `--nested`, but that flag
  alone is not the blocker or the fix. The current host now has a stable
  short-root recreate recipe for the long-lived `users-only` machine; treat the
  remaining macOS risk as provider/image drift beyond that recipe, not as proof
  that libkrun cannot reach readiness on this host.
- Podman source confirms that the observed `--nested` argument alone is not a
  reliable signal for active nested virtualization on this host. The Darwin VM
  launcher appends it for libkrun machines and relies on `krunkit` to ignore it
  when the hardware or OS does not qualify.
- Podman's machine-os package set is now a positive reference point for neovex.
  If we need a macOS guest image contract, start from `crun` + standard Linux
  container plumbing inside the guest, not from a guest-side `krun` assumption.
- Keep packaging facts and provider behavior separate. Homebrew `podman`
  `5.8.1` packages `podman-mac-helper`, `gvproxy`, and `vfkit` on macOS; it
  does not package `krunkit`. Podman Desktop ships an app bundle, not a
  Homebrew dependency contract for `krunkit`. If neovex standardizes on
  `krunkit` for macOS, that dependency must be owned directly by neovex. This
  evidence is scoped to the Homebrew path we plan to ship, not to Podman's
  separate upstream macOS `.pkg` installer.
- Podman's upstream macOS `.pkg` source is broader than the Homebrew contract:
  `contrib/pkginstaller/Makefile` downloads `gvproxy`, `vfkit`, and `krunkit`
  into the installer payload. Use that upstream source as architectural context,
  but keep the Homebrew and `.pkg` packaging stories distinct in neovex docs.
- Upstream Podman already has precedent for libkrun macOS instability.
  `containers/podman` issue `#24559` reported libkrun machine start failures on
  macOS 15.1, and issue `#23296` captured krunkit-related macOS test failures.
  Treat the current `M2 Max` guest-readiness failure as an upstream-style
  machine/provider problem until repo-local evidence proves otherwise.
- The issue-comment history matters too: `#24559` was tied to an older
  krunkit-era startup problem that upstream worked around via newer krunkit /
  Podman releases (`krunkit` `0.1.4`, `podman` `5.3.1`). Because the current
  host is already on Homebrew `podman` `5.8.1` and `krunkit` `1.1.1`, do not
  assume the present guest-readiness failure is solved by replaying that older
  workaround verbatim.
- `containers/krunkit` issue `#17` narrows that history further: it describes a
  failure mode when the guest `--memory` value exceeds `27647`. The current
  failing Podman machine was launched with `--memory 4096`, so that older
  high-memory threshold bug does not match this host's observed failure
  directly.
- `podman-mac-helper` is a Docker-socket compatibility helper, not a libkrun
  machine-readiness helper. Do not treat `/var/run/docker.sock` wiring as
  evidence that the Podman-managed guest boot path is fixed.
- The VMM control plane should treat the repo state plus this plan's ledger and
  execution log as authoritative progress state. Chat history is not durable
  state.
- The first VMM slice should stay focused on patch fidelity and host-level
  verification. Do not jump to `neovex-sandbox` backend implementation before
  the patched crun path and manual system integration are proven.
- **Linux-host validated (2026-04-12):** krun OCI bundles must omit the
  `network` namespace type and set `process.terminal: false` for service-mode
  containers. TSI port mapping works through vsock in the parent network
  namespace; a separate network namespace hides the TSI-bound ports.
- **Conmon attach lifecycle:** conmon with `--full-attach` does not call
  `crun start` automatically. The `neovex-sandbox` backend must either connect
  to the attach socket or call `crun start` after the container reaches the
  `created` OCI state. The generated `start-container.sh` script demonstrates
  the poll-then-start pattern.
- **Rootless krun requires a user namespace:** the krun handler writes
  `.krun_config.json` to the rootfs via `openat2` during `crun create`. In
  rootless mode, this fails without a pre-established user namespace. The
  `neovex-sandbox` backend must either run crun/conmon inside a user namespace
  (the Podman pattern) or run as root.
- **libkrun and libkrunfw are not packaged for Debian 13.** They must be built
  from source on Debian-family hosts. Fedora has them packaged. This affects
  the CI runner setup and the distribution plan.
- **SIGTERM does not cleanly stop a krun VM.** The crun process IS the VMM;
  `krun_start_enter()` blocks until the guest exits. SIGKILL is currently
  required for forced stop (exit code 137). The V3 backend's graceful-stop
  path should attempt SIGTERM then fall back to SIGKILL.

## Feature Preservation Matrix

| Area | Must stay true during VMM work | Notes |
| --- | --- | --- |
| Public crate boundaries | `neovex-runtime` remains execution-only and `neovex-sandbox` remains the isolation seam | do not re-couple execution and sandbox concerns |
| Public sandbox nouns | `SandboxBackend`, `SandboxSpec`, `SandboxHandle`, and server-owned `SandboxCatalog` stay generic | krun/buildah/conmon/crun vocabulary remains backend-internal |
| System Podman/crun | system Podman must keep using distro `crun` untouched | neovex uses `/usr/libexec/neovex/crun` only |
| Patch verification | the checked-in patch must be reproducibly validated against a pinned upstream crun source layout | local script plus CI lane are the minimum guardrail |
| Upstream drop path | if upstream gains equivalent support, neovex can delete the patch and stop shipping `neovex-crun` | keep the delta small and easy to remove |
| MicroVM follow-on work | `microvm-runtime-plan.md` and `distribution-plan.md` consume this plan's outputs instead of inventing their own VMM contract | this plan owns the VMM foundation first |

## Control Plane Rules

Source of truth:
1. the current git worktree
2. this plan's `Roadmap Status Ledger`, `Implementation Checkpoints`, and
   `Execution Log`
3. `docs/research/libkrun-evaluation.md`
4. `docs/research/vm-lifecycle-probes.md`
5. `docs/research/gvisor-isolation-tier.md`

### Status model

- `todo` / `in_progress` / `blocked` / `done` / `deferred`
- Resume from the earliest non-`done` item after reconciling the plan ledger,
  execution log, and current git worktree.
- Before stopping or handing off, update the relevant phase status and
  execution log entry in the same change set as the code or doc changes.
- Do not trust chat history as durable progress state. Fresh context should
  come from this plan, `docs/plans/README.md`, and the current worktree.
- A phase is not `done` until its required repo outputs, host-local outputs,
  and recorded verification evidence are all present.

### Non-deviation rules

- Do not skip an existing `in_progress` phase to start a later `todo` phase.
- Do not mark a phase `done` from narrative confidence alone; capture explicit
  commands, artifact paths, and observed outcomes in `Implementation
  Checkpoints` and `Execution Log`.
- Do not start `V3` Rust integration while `V1` or `V2` still lack host-level
  proof that the patched-crun stack works end to end.
- Treat the Linux-host validation queue below as a parallel execution lane.
  It blocks phase closeout, but it does not block continued repo-owned prep
  work on macOS while that host is unavailable.
- Do not rely on one-off shell history as evidence. For every materially
  important step, prefer a checked-in script, workflow, test, or runbook plus
  a recorded command invocation.
- If a host-local artifact cannot live in git (for example
  `/usr/libexec/neovex/crun`, a local OCI bundle directory, or generated log
  files), record its absolute path, the command that produced it, and the
  command that proved it worked before moving on.

### Required write-back after each work session

- update the relevant phase status in `Roadmap Status Ledger`
- update `Implementation Checkpoints` with the newest verifiable outputs and
  the remaining gap to completion
- append a row to `Execution Log` with date, phase, outcome, verification, and
  next step
- update architecture or operator docs in the same change set when the session
  lands a new persistent workflow, package contract, or runtime/sandbox seam

### Suggested autonomous prompt

```text
Use docs/plans/archive/vmm-infrastructure-plan.md as the control plane. Reread
Current Assessed State, Current Review Findings, Feature Preservation Matrix,
Control Plane Rules, Verification Contract, Roadmap Status Ledger,
Implementation Checkpoints, Dependency Graph, Recommended Delivery Order, and
Execution Log, then inspect the current git worktree. Resume the earliest
phase that is not done; if any phase is already in_progress, continue it
before starting new scope. For the active phase, produce the required repo
outputs, host-local outputs, and recorded verification evidence exactly as the
plan requires. Prefer checked-in scripts, workflows, tests, or runbooks over
unrecorded shell history. After each work burst, update the ledger,
implementation checkpoints, and execution log in the same change set, then
continue to the next remaining output. Do not rely on chat history as
progress state. Do not mark a phase done until the plan contains concrete
artifact paths, commands, and observed outcomes that prove completion.
```

---

## Architecture: The Podman Model

neovex follows the same process model and dependency pattern as Podman.
neovex is a single binary. The VMM stack is system packages.

### Process model

```
neovex serve
  │
  └── conmon -r /usr/libexec/neovex/crun (per VM, long-lived, survives neovex restart)
        │
        ├── stdout/stderr → log files (persistent)
        ├── exit status → exit file
        ├── attach socket for interactive access
        │
        └── /usr/libexec/neovex/crun run --bundle path id
              │
              ├── namespaces (PID, mount, user)
              ├── cgroups (memory, CPU limits)
              ├── seccomp (syscall filtering)
              │
              └── krun handler (with TSI port mapping patch)
                    ├── krun_set_root() — virtiofs rootfs
                    ├── krun_set_port_map() — TSI port mapping
                    └── krun_start_enter() → _exit()
                          └── Guest VM
                                ├── catatonit/tini (PID 1)
                                └── workload (postgres, etc.)
```

conmon's `-r` flag accepts an arbitrary path to any OCI runtime binary. neovex
passes `-r /usr/libexec/neovex/crun` so the forked crun is used without
replacing the system crun. System Podman continues to use the distro crun
undisturbed.

### Dependency comparison with Podman

```
Podman:                             neovex:
  conmon               ✓             conmon
  crun | runc          ✓             neovex-crun (patched crun, small one-file delta, at /usr/libexec/neovex/crun)
  containers-common    ✓             containers-common
  netavark             ✗ (TSI)       —
  catatonit|tini       ✓             catatonit | tini | dumb-init
  buildah              ✓             buildah
  passt                ✓             passt
  uidmap               ✓             uidmap
  fuse-overlayfs       ✓             fuse-overlayfs
  libkrun              ✓             libkrun
  libkrunfw            ✓             libkrunfw
```

neovex drops only netavark (TSI replaces container networking) and adds
libkrun/libkrunfw (not needed by Podman's default runc mode).

### Evaluated alternatives (see research docs)

- **conmon-rs** — Rust rewrite (v0.8.0). Deferred: not production-default, per-pod
  model doesn't fit per-VM use, Podman integration incomplete
  (containers/conmon-rs#1127 open since 2023). Revisit if it becomes the
  Podman/CRI-O default.
- **gVisor** — User-space kernel, no KVM needed. Deferred: syscall compat gaps,
  I/O overhead, no hardware isolation boundary. See
  `docs/research/gvisor-isolation-tier.md`.
- **CRIU for snapshot/restore** — Cannot checkpoint KVM-based VMM processes (no
  KVM fd support). Every VMM with snapshot/restore implements it natively. See
  `docs/research/libkrun-evaluation.md` § "CRIU Cannot Solve the
  Snapshot/Restore Gap".
- **Warm pool mitigation** — Pre-boot idle VMs, aggressive rootfs caching,
  optimized guest kernel. Documented in `docs/research/libkrun-evaluation.md`
  § "Warm pool as a practical mitigation".

### Why no vsock

vsock was evaluated and deferred. Reasons:

1. **Guest apps don't speak AF_VSOCK.** postgres, redis, nginx use TCP. vsock
   requires a bridge process inside the VM — more complexity, not less.
2. **TSI handles service traffic.** V8 connects to guest services via
   TCP through TSI-mapped ports. Standard, works with every application.
3. **No performance benefit for DB/API workloads.** Transport overhead
   (microseconds) is negligible vs query latency (milliseconds).
4. **Security proxying doesn't need vsock.** A TCP proxy in neovex (v2)
   provides tenant isolation, audit logging, and rate limiting over TSI.
5. **Graceful shutdown via conmon.** SIGTERM → grace → SIGKILL, same as
   Podman. No custom guest agent needed.

**Future:** vsock may be added for dedicated control channels (exec, live
debugging, filesystem access) as part of `wasi-agent-capabilities-plan.md`.
If added, neovex-init and vsock support in crun would be revisited then.

### Communication model

```
v1 (this plan):
  V8 → TCP localhost:mapped_port → TSI → guest service
  Shutdown: conmon → SIGTERM → grace → SIGKILL (same as Podman)
  Health: TCP connect to TSI-mapped port

v2 (future, microvm-runtime-plan Phase M4+):
  V8 → neovex proxy (policy, audit, rate limit) → TCP → TSI → guest
  Same transport, neovex is now in the data path for observability
```

### Architectural evolution path

The Podman subprocess model (conmon → crun → buildah) is correct for v1's
long-running service VMs. CRI-O, containerd shims, and Podman all use this
pattern in production. Every microVM-at-scale system (Fly.io 2M+ VMs,
AWS Lambda, CodeSandbox, Gitpod Flex) eventually moved to direct VMM API
integration — but they all started with subprocess orchestration and
graduated when density/latency demands required it.

If neovex evolves toward high-density ephemeral VMs, the migration path is:
subprocess model → helper binary calling libkrun API directly (see
`docs/research/libkrun-evaluation.md`). The helper binary pattern is
architecturally compatible with the conmon process model — conmon monitors
the helper the same way it monitors crun.

### Rust crate target for Phase V3

When this plan graduates from manual infrastructure work into Rust integration,
the canonical crate target is `neovex-sandbox`, not `neovex-vmm`.

The public seam should stay generic:

- `SandboxBackend`
- `SandboxSpec`
- `SandboxHandle`
- published-endpoint / port-projection types

The first backend-specific implementation path should live under an internal
module such as `crates/neovex-sandbox/src/backends/krun/`, which may own the
current OCI/buildah + conmon + patched-crun + libkrun stack without turning
those implementation details into public product nouns.

---

## crun Patch: TSI Port Mapping

### What and why

The upstream crun krun handler does NOT call `krun_set_port_map()`. Without
TSI port mapping, V8 isolates cannot connect to guest services. This is the
only change needed.

**Upstream:** `containers/crun` (latest release)
**License:** GPL-2.0 (binary), LGPL-2.1 (libcrun library)
**Patch size:** small single-file delta in `krun.c`
**File:** `src/libcrun/handlers/krun.c`

### The patch

```c
// Read TSI port map from OCI annotation.
// Format: "15432:5432,16379:6379" (host:guest)
const char *annotation = find_annotation(container, "krun.port_map");

if (annotation != NULL) {
    // libkrun expects {"15432:5432", "16379:6379", NULL}, not one raw string.
    ret = libkrun_configure_port_map(ctx_id, handle, container, err);
}
```

The important detail is the ABI shape, not the exact helper name: current
`libkrun` expects a null-terminated array of `"host:guest"` strings, so the
patch must parse the OCI annotation before calling `krun_set_port_map()`.

The annotation name `krun.port_map` follows the convention established by
crun PR #1950 (Jan 2026), which added `krun.cpus`, `krun.ram_mib`, and
`krun.variant`. Using the same `krun.*` namespace keeps the OCI annotations
consistent with upstream conventions.

### Build-time patch, not a fork

A full GitHub fork is overkill for a small one-file C patch. neovex uses the standard
distro pattern: store a patch file, apply it to the upstream source at build
time. This is how Debian, Fedora, Homebrew, and Alpine handle minimal
customizations to upstream packages. No separate fork repo exists.

### End-to-end: patch → build → distribute → install

**Step 1: Patch file lives in this repo**

```
agentstation/neovex (this repo):
  patches/
    crun/
      0001-krun-add-tsi-port-mapping-via-oci-annotation.patch
```

The patch file is a standard unified diff generated from the upstream PR or
via `git format-patch`. It is checked into the neovex repo alongside the
Rust source code.

**Step 2: CI builds the patched crun binary**

A GitHub Actions workflow (defined in `distribution-plan.md` Phase D1) runs
on each neovex release tag:

```bash
# .github/workflows/build-neovex-crun.yml (conceptual)
CRUN_VERSION=1.22
curl -L -o crun-$CRUN_VERSION.tar.gz \
  https://github.com/containers/crun/archive/refs/tags/$CRUN_VERSION.tar.gz
tar xzf crun-$CRUN_VERSION.tar.gz
cd crun-$CRUN_VERSION

# Apply the neovex patch
patch -p1 < $GITHUB_WORKSPACE/patches/crun/0001-krun-add-tsi-port-mapping-via-oci-annotation.patch

# Build (requires: autoconf, automake, libkrun-dev, libseccomp-dev, etc.)
./autogen.sh
./configure --with-libkrun
make

# Output: crun binary with krun handler + TSI port mapping
```

The CI runner needs C build tools and libkrun-dev headers. The workflow pins
`CRUN_VERSION` to a known-good upstream release.

**Step 3: CI packages the binary per distribution channel**

The same CI workflow (or downstream jobs) produces packages:

| Channel | What CI produces | How patch is applied |
|---------|-----------------|---------------------|
| Binary tarball | `neovex-linux-amd64.tar.gz` containing `neovex` + `crun` | CI applies patch, ships binary |
| Debian (deb) | `neovex-crun_1.22+neovex1_amd64.deb` | `debian/patches/series` + quilt (standard) |
| Fedora (rpm) | `neovex-crun-1.22-1.neovex1.x86_64.rpm` | `Patch0:` in spec, `%autosetup -p1` |
| Homebrew | Formula with `patch :DATA` block | Homebrew applies patch before `make` |
| Container image | `ghcr.io/agentstation/neovex:latest` | `RUN patch -p1 < ...` in Dockerfile |

Each format has a native mechanism for applying patches — the patch file is
the same, only the build harness differs.

**Step 4: User installs neovex, gets patched crun automatically**

```bash
# Debian/Ubuntu
curl -fsSL https://neovex.dev/install.sh | sh
# install.sh adds apt repo, then:
# apt install neovex  (depends on neovex-crun, conmon, buildah, ...)
# neovex-crun installs to /usr/libexec/neovex/crun

# Fedora
dnf copr enable agentstation/neovex
dnf install neovex
# neovex-crun installs to /usr/libexec/neovex/crun

# Manual tarball
tar xzf neovex-linux-amd64.tar.gz
sudo mv neovex /usr/local/bin/
sudo mkdir -p /usr/libexec/neovex
sudo mv crun /usr/libexec/neovex/crun
```

The user never interacts with the patch. They install `neovex`, which depends
on `neovex-crun`, which is a pre-built binary at `/usr/libexec/neovex/crun`.
neovex invokes it via `conmon -r /usr/libexec/neovex/crun`.

**If upstream independently adds port mapping support:** Delete the patch
file, stop building `neovex-crun`, change the `neovex` package to depend on
system `crun` (>= the version with port mapping). The
`/usr/libexec/neovex/` path is no longer needed — neovex uses system crun
directly.

### Why not a full GitHub fork

| | Full fork | Build-time patch |
|---|-----------|-----------------|
| Maintenance | Rebase entire repo on each upstream release | Verify a small single-file patch applies cleanly |
| Signal | "Major divergence" — misleading for a small handler fix | "Minimal delta" — accurate |
| Staleness | Fork drifts, looks abandoned if not synced | Patch is obviously temporary |
| GPL compliance | Source = fork repo | Source = upstream tarball + patch file |
| Drop when upstream merges | Delete fork repo, update packaging | Delete patch file, update packaging |

### Upstream exit path

We are not submitting an upstream PR — the change is too small to justify the
overhead of upstream engagement (review cycles, API discussions, maintenance
commitments). The build-time patch is the plan, not a fallback.

If upstream independently adds `krun_set_port_map()` support via OCI
annotations (likely, given PR #1950 established the pattern), we drop the
patch and depend on system crun directly.

### Patch update process

When upstream crun releases a new version:

1. Attempt `patch -p1 --dry-run` against the new release
2. If it applies cleanly → update `CRUN_VERSION`, rebuild, done
3. If it conflicts → manually resolve the small handler delta, regenerate
   patch file
4. If upstream added native port mapping support → delete the patch file,
   depend on system crun directly

### GPL-2.0 compliance

crun is GPL-2.0. Distributing a patched binary requires providing the
complete corresponding source. For each distribution channel:

- **deb/rpm packages:** The source package (`.dsc` + `.orig.tar.gz` +
  `.debian.tar.xz`, or SRPM) contains the upstream tarball + patch file +
  build instructions. Standard and sufficient.
- **Homebrew:** The formula references the upstream URL and includes the patch
  inline. Anyone who has the formula can rebuild from source.
- **Binary tarball:** Include a `SOURCE.md` pointing to the upstream release
  URL and the patch file location in the neovex repo.

### Package naming

The patched crun is packaged as `neovex-crun`:

- **Binary name:** `crun` (it IS crun, just patched)
- **Install path:** `/usr/libexec/neovex/crun` (private to neovex)
- **Package name:** `neovex-crun` (deb/rpm — makes the relationship clear)
- **No Conflicts/Replaces/Provides:** Does not touch the system `crun`.
  Podman, CRI-O, and any other container tools continue using the distro crun.

The `neovex-crun` name follows the convention of scoping a patched build to
the project that needs it, similar to how Fedora has `crun-krun` as a separate
build of crun with libkrun support.

---

## System Dependencies

### Required

| Package | What | Available on Debian 13 | Available on Fedora 40+ |
|---------|------|----------------------|------------------------|
| `neovex-crun` | Patched crun with TSI port mapping | We package it | We package it |
| `libkrun` | VMM library | **Not in repos** — we package it | `dnf install libkrun` ✓ |
| `libkrunfw` | Guest kernel | **Not in repos** — we package it | `dnf install libkrunfw` ✓ |
| `conmon` | Process monitor | `apt install conmon` ✓ | `dnf install conmon` ✓ |
| `buildah` | Image build/pull/mount | `apt install buildah` ✓ | `dnf install buildah` ✓ |
| `containers-common` | Registry auth/config | Comes with buildah ✓ | Comes with buildah ✓ |

### Recommended

| Package | What | Why |
|---------|------|-----|
| `catatonit` \| `tini` \| `dumb-init` | Guest PID 1 init | Signal forwarding, zombie reaping |
| `passt` | Rootless networking | Non-root neovex operation |
| `uidmap` | User namespace mapping | Non-root neovex operation |
| `fuse-overlayfs` | Rootless overlay storage | Layer dedup for buildah |

### Runtime requirements

| Requirement | How to enable |
|------------|---------------|
| `/dev/kvm` | Enable VT-x in BIOS (bare metal) or nested virt (cloud VM) |
| KVM group membership | `sudo usermod -aG kvm $USER` |

---

## How neovex uses buildah (instead of custom OCI code)

buildah replaces the entire OCI image management layer that was previously
planned as custom Rust code (oci-client, layer flattening, whiteout handling,
layer caching, overlay assembly).

### Image pull

```bash
# neovex shells out to buildah to pull and mount an image:
buildah from --name neovex-postgres docker://postgres:16
ROOTFS=$(buildah mount neovex-postgres)
# $ROOTFS is now the merged rootfs directory (all layers applied)
# Pass to crun: krun_set_root($ROOTFS) via virtiofs
```

### Dockerfile build

```bash
buildah bud -t neovex-myapp -f ./Dockerfile .
buildah from --name neovex-myapp localhost/neovex-myapp
ROOTFS=$(buildah mount neovex-myapp)
```

### Cleanup

```bash
buildah umount neovex-postgres
buildah rm neovex-postgres
```

### What this eliminates from neovex's Rust code

| Previously planned (custom Rust) | Now handled by buildah |
|----------------------------------|----------------------|
| `oci-client` crate for registry pull | `buildah from docker://...` |
| Layer flattening with whiteout handling | `containers-storage` (via buildah) |
| Content-addressable layer cache | `containers-storage` |
| Layer deduplication across images | `containers-storage` + overlayfs |
| Registry authentication | `containers-common` (registries.conf) |
| `.krun_config.json` generation | Still in neovex (reads OCI image config via `buildah inspect`) |
| OCI bundle `config.json` generation | Still in neovex |

---

## Phase Plan

## Parallel Linux Host Validation Queue

Use this queue when working from the Linux laptop, from the Linux machine guest
on macOS, or on any supported Linux host. These items are mandatory before
`V1`/`V2` can be marked `done`, but they may run in parallel with continued
repo-owned preparation on macOS.

Primary command reference:
- `docs/reference/krun-vmm-host-validation.md`
- `bash scripts/prepare-linux-vmm-validation-bundle.sh --crun-source ~/src/github.com/containers/crun`

| Item | Status | What the Linux host must verify | Evidence to record back into this plan |
| --- | --- | --- | --- |
| `LH1` Host preflight | `done` | Debian 13 x86_64, kernel 6.12.74, `/dev/kvm` present and accessible, build tools present, conmon 2.1.12, buildah 1.39.3, system crun 1.21, podman 5.4.2, catatonit 0.2.1. libkrun/libkrunfw not in Debian repos — built from source (libkrun 1.17.4, libkrunfw 5.3.0) | `/tmp/neovex-linux-vmm-validation/artifacts/lh1/check-vmm-host.txt`, `/tmp/neovex-linux-vmm-validation/artifacts/lh1/collect-vmm-package-versions.txt` |
| `LH2` Patch fidelity on the real Linux host | `done` | Patch applies cleanly (with minor offsets) against upstream crun `1.27` (tag `a718a92c`) at `~/src/github.com/containers/crun` | `/tmp/neovex-linux-vmm-validation/artifacts/lh2/verify-crun-patch.txt` |
| `LH3` Build and install the private runtime | `done` | Built patched crun 1.27 with `+LIBKRUN`, staged at `/tmp/neovex-linux-vmm-validation/stage/crun`, installed at `/usr/libexec/neovex/crun`. libkrun 1.17.4 built from source (`~/src/github.com/containers/libkrun` tag `v1.17.4`) and installed at `/usr/local/lib64/libkrun.so.1.17.4`. libkrunfw 5.3.0 built from source (`~/src/github.com/containers/libkrunfw` tag `v5.3.0`) and installed at `/usr/local/lib64/libkrunfw.so.5.3.0` | `/tmp/neovex-linux-vmm-validation/artifacts/lh3/build-stage-runtime.txt`, `/tmp/neovex-linux-vmm-validation/artifacts/lh3/stage-runtime-version.txt`, `/tmp/neovex-linux-vmm-validation/artifacts/lh3/install-private-runtime.txt`, `/tmp/neovex-linux-vmm-validation/artifacts/lh3/install-runtime-version.txt` |
| `LH4` Prove system runtime separation | `done` | System crun 1.21 at `/usr/bin/crun`, private neovex crun 1.27+LIBKRUN at `/usr/libexec/neovex/crun`, Podman runtime pointed at system `/usr/bin/crun`. Realpaths distinct, separation confirmed | `/tmp/neovex-linux-vmm-validation/artifacts/lh4/verify-runtime-separation.txt` |
| `LH5` Generate and boot the first real krun bundle | `done` | krun VM booted via `crun run` inside `buildah unshare` with busybox rootfs overlay mount. TSI port mapping `18080:8080` verified — `libkrun VM` bound port 18080 on the host. BusyBox httpd responded via TSI (HTTP/1.1 404 — expected, empty docroot proves connectivity). OCI config required removing `network` namespace and setting `terminal: false` for krun bundles — fix checked into `scripts/prepare-krun-bundle.sh`. SIGTERM does not cleanly stop the krun VMM (SIGKILL required) | `/tmp/neovex-linux-vmm-validation/artifacts/lh5/direct-drill-full.txt`, `/tmp/neovex-linux-vmm-validation/artifacts/lh5/direct-probe-http.txt` |
| `LH6` Conmon lifecycle drill | `done` | First real `conmon -> /usr/libexec/neovex/crun -> krun VM` flow proven inside `buildah unshare`. Process tree: `conmon(90649) → libkrun VM(90651)` with 8 worker threads. TSI port 18080 bound by `libkrun VM`. HTTP connectivity confirmed. Exit file written at `/tmp/neovex-linux-vmm-validation/conmon-drill/exits/neovex-http` with exit code 137 (SIGKILL). Attach socket at `/tmp/neovex-linux-vmm-validation/bundle/attach`. Key finding: conmon with `--full-attach` waits for attach connection before calling `crun start`; `crun start` must be called explicitly or via attach to transition the krun container from `created` to `running` | `/tmp/neovex-linux-vmm-validation/artifacts/lh6/conmon-drill-full.txt`, `/tmp/neovex-linux-vmm-validation/artifacts/lh6/conmon-probe-http.txt`, `/tmp/neovex-linux-vmm-validation/artifacts/lh6/process-tree.txt`, `/tmp/neovex-linux-vmm-validation/artifacts/lh6/conmon-exit-status.txt` |

Supplementary macOS-only preflight evidence that does **not** close `LH1`-`LH6`:
- `bash scripts/verify-neovex-crun-fedora-userspace.sh --crun-source ~/src/github.com/containers/crun --output-dir /tmp/neovex-crun-fedora-userspace-output --work-dir /tmp/neovex-crun-fedora-userspace-build`
- `file /tmp/neovex-crun-fedora-userspace-output/crun`
- `bash scripts/collect-podman-machine-diagnostics.sh --machine neovex-libkrun-validation --provider libkrun --output-dir /tmp/neovex-libkrun-diagnostics`

### Linux Feedback Loop

When the Linux host finds a repo issue instead of a pure environment issue:

1. update the repo with the smallest correct fix or docs/runbook clarification
2. rerun the narrow repo verification that covers that change
3. rerun the same Linux queue item that exposed the issue
4. write the new result and the remaining gap into `Execution Log`

When the Linux host hits a pure environment issue:

1. record the exact command and output
2. note whether it is a missing package, permission problem, or host capability gap
3. continue with any still-safe queue items that do not depend on the missing capability
4. do not mark `V1` or `V2` `done` until the required host evidence exists

### Suggested Linux Host Prompt

```text
Use docs/plans/archive/vmm-infrastructure-plan.md as the control plane on the Linux
host. Reread Current Assessed State, Current Review Findings, Control Plane
Rules, the Parallel Linux Host Validation Queue (`LH1`-`LH6`), Verification
Contract, Implementation Checkpoints, and Execution Log, then inspect the
current git worktree. Your job is to execute the Linux-host validation lane and
feed results back into the repo cleanly.

Start with docs/reference/krun-vmm-host-validation.md and follow the queue in
order. To minimize judgment on the Linux host, first generate the numbered
command bundle:

`bash scripts/prepare-linux-vmm-validation-bundle.sh --crun-source ~/src/github.com/containers/crun`

Then run the emitted `commands/00-run-through-lh6.sh` or the numbered
`commands/01...11...` scripts one by one and write the resulting fixed artifact
paths back into this plan.

Queue order:
1. `LH1` host preflight
2. `LH2` patch fidelity on the real Linux host
3. `LH3` build and install the private runtime
4. `LH4` prove system runtime separation
5. `LH5` generate and boot the first real krun bundle
6. `LH6` conmon lifecycle drill

Rules:
- Treat the current git worktree plus this plan as the only durable progress
  state.
- Do not trust chat history.
- Record exact commands, absolute paths, and observed outcomes for every Linux
  queue item.
- If a Linux queue item exposes a repo issue, make the smallest correct repo
  fix, rerun the narrow verification for that change, rerun the same queue
  item, then update the plan.
- If a Linux queue item fails for an environment reason, record the exact
  command, output, and root cause, then continue with any still-safe later item
  that does not depend on the missing capability.
- Do not mark `V1` or `V2` done until the required Linux evidence exists in the
  plan.

Minimum commands to run on the Linux host:
- `bash scripts/check-vmm-host.sh`
- `bash scripts/collect-vmm-package-versions.sh`
- `bash scripts/verify-crun-patch.sh ~/src/github.com/containers/crun`
- `bash scripts/build-neovex-crun.sh --source ~/src/github.com/containers/crun --output /tmp/neovex-crun-stage/crun`
- `bash scripts/build-neovex-crun.sh --source ~/src/github.com/containers/crun --output /tmp/neovex-crun-stage/crun --install-path /usr/libexec/neovex/crun --sudo-install`
- `bash scripts/verify-runtime-separation.sh --system-runtime /usr/bin/crun --private-runtime /usr/libexec/neovex/crun`
- `bash scripts/prepare-krun-bundle.sh ...`
- `bash scripts/prepare-direct-krun-drill.sh ...`
- `bash scripts/verify-direct-krun-drill-helper.sh`
- `bash scripts/prepare-conmon-krun-drill.sh ...`
- `bash scripts/verify-conmon-krun-drill-helper.sh`
- `bash <state_root>/containers/<id>/start-runtime.sh`
- `bash <state_root>/containers/<id>/run-conmon.sh`
- the first real `conmon -r /usr/libexec/neovex/crun ...` lifecycle command

Before stopping:
- update the Linux queue item status in this plan if it changed materially
- append an Execution Log row with date, phase/item, result, verification, and next step
- keep the plan and repo in sync with the actual Linux host outcome
```

### Phase V1: Patch crun

**Goal:** Add TSI port mapping to crun's krun handler via build-time patch.

**Scope:**
1. Create patch file at
   `patches/crun/0001-krun-add-tsi-port-mapping-via-oci-annotation.patch`
2. Add a repo-owned verification entrypoint that dry-runs the patch against the
   pinned upstream crun source layout before any packaging or release work
3. Build patched crun and install to `/usr/libexec/neovex/crun`
4. Test on Debian 13 and Fedora

**Required verifiable outputs:**
1. **Repo-owned artifacts**
   - `patches/crun/0001-krun-add-tsi-port-mapping-via-oci-annotation.patch`
   - `scripts/verify-crun-patch.sh`
   - `scripts/check-vmm-host.sh`
   - `scripts/build-neovex-crun.sh`
   - `scripts/prepare-krun-bundle.sh`
   - `scripts/verify-krun-bundle-helper.sh`
   - `scripts/prepare-direct-krun-drill.sh`
   - `scripts/verify-direct-krun-drill-helper.sh`
   - `scripts/verify-runtime-separation.sh`
   - `scripts/verify-runtime-separation-helper.sh`
   - `scripts/verify-neovex-crun-fedora-userspace.sh`
   - `scripts/collect-podman-machine-diagnostics.sh`
   - `scripts/verify-podman-machine-diagnostics-helper.sh`
   - `scripts/prepare-linux-vmm-validation-bundle.sh`
   - `scripts/verify-linux-vmm-validation-bundle-helper.sh`
   - `scripts/check-podman-machine-socket-paths.sh`
   - `scripts/verify-podman-machine-socket-paths-helper.sh`
   - `scripts/validate-podman-machine-readiness.sh`
   - `scripts/verify-podman-machine-readiness-helper.sh`
   - `scripts/recreate-podman-machine.sh`
   - `scripts/verify-podman-machine-recreate-helper.sh`
   - `scripts/fixtures/crun-spec-config.json`
   - `docs/reference/krun-vmm-host-validation.md`
   - `make verify-crun-patch`
   - `make check-vmm-host`
   - `make build-neovex-crun`
   - `make prepare-krun-bundle`
   - `make verify-krun-bundle-helper`
   - `make verify-neovex-crun-fedora-userspace`
   - `make prepare-direct-krun-drill`
   - `make verify-direct-krun-drill-helper`
   - `make verify-runtime-separation`
   - `make verify-runtime-separation-helper`
   - `make collect-podman-machine-diagnostics`
   - `make verify-podman-machine-diagnostics-helper`
   - `make prepare-linux-vmm-validation-bundle`
   - `make verify-linux-vmm-validation-bundle-helper`
   - `make check-podman-machine-socket-paths`
   - `make verify-podman-machine-socket-paths-helper`
   - `make validate-podman-machine-readiness`
   - `make recreate-podman-machine`
   - `make verify-podman-machine-readiness-helper`
   - `make verify-podman-machine-recreate-helper`
   - `.github/workflows/verify-neovex-crun-patch.yml`
2. **Host-local build outputs**
   - upstream source checkout path, expected to be
     `~/src/github.com/containers/crun` unless explicitly changed
   - a built patched binary at a recorded staging path and, when installed, at
     `/usr/libexec/neovex/crun`
   - recorded build and install commands, plus observed `crun --version` or
     equivalent identity output for the patched binary
3. **Runtime proof outputs**
   - an OCI bundle or bundle recipe that includes the `krun.port_map`
     annotation in `"host:guest"` form
   - a recorded host-to-guest connectivity probe over the mapped port
   - recorded proof that system `crun` / Podman still use the distro runtime
     path untouched

**Phase verification gates:**
- `bash -n scripts/verify-crun-patch.sh`
- `bash -n scripts/check-vmm-host.sh`
- `bash -n scripts/build-neovex-crun.sh`
- `bash -n scripts/prepare-krun-bundle.sh`
- `bash -n scripts/verify-krun-bundle-helper.sh`
- `bash -n scripts/prepare-direct-krun-drill.sh`
- `bash -n scripts/verify-direct-krun-drill-helper.sh`
- `bash -n scripts/verify-runtime-separation.sh`
- `bash -n scripts/verify-runtime-separation-helper.sh`
- `bash -n scripts/verify-neovex-crun-fedora-userspace.sh`
- `bash -n scripts/collect-podman-machine-diagnostics.sh`
- `bash -n scripts/verify-podman-machine-diagnostics-helper.sh`
- `bash -n scripts/prepare-linux-vmm-validation-bundle.sh`
- `bash -n scripts/verify-linux-vmm-validation-bundle-helper.sh`
- `bash -n scripts/check-podman-machine-socket-paths.sh`
- `bash -n scripts/verify-podman-machine-socket-paths-helper.sh`
- `bash -n scripts/validate-podman-machine-readiness.sh`
- `bash -n scripts/verify-podman-machine-readiness-helper.sh`
- `bash -n scripts/recreate-podman-machine.sh`
- `bash -n scripts/verify-podman-machine-recreate-helper.sh`
- `bash scripts/verify-crun-patch.sh ~/src/github.com/containers/crun`
- `bash scripts/verify-krun-bundle-helper.sh`
- `bash scripts/verify-direct-krun-drill-helper.sh`
- `bash scripts/verify-runtime-separation.sh --help`
- `bash scripts/verify-runtime-separation-helper.sh`
- `bash scripts/verify-neovex-crun-fedora-userspace.sh --help`
- `bash scripts/collect-podman-machine-diagnostics.sh --help`
- `bash scripts/verify-podman-machine-diagnostics-helper.sh`
- `bash scripts/prepare-linux-vmm-validation-bundle.sh --help`
- `bash scripts/verify-linux-vmm-validation-bundle-helper.sh`
- `bash scripts/check-podman-machine-socket-paths.sh --machine neovex-libkrun-users-only --tmp-root /var/folders/kw/d608x5pn4cq73rz78ztl92cw0000gn/T/podman`
- `bash scripts/check-podman-machine-socket-paths.sh --machine neovex-libkrun-users-only --tmp-root /tmp/podman`
- `bash scripts/verify-podman-machine-socket-paths-helper.sh`
- `bash scripts/validate-podman-machine-readiness.sh --help`
- `bash scripts/verify-podman-machine-readiness-helper.sh`
- `bash scripts/recreate-podman-machine.sh --help`
- `bash scripts/verify-podman-machine-recreate-helper.sh`
- `make verify-podman-machine-diagnostics-helper`
- `make verify-linux-vmm-validation-bundle-helper`
- `make verify-podman-machine-socket-paths-helper`
- `make verify-podman-machine-readiness-helper`
- `make verify-podman-machine-recreate-helper`
- run `bash scripts/check-vmm-host.sh` on the actual Linux validation host and
  record the output
- `cargo fmt --all --check`
- if working from macOS, record whether the Docker Desktop Fedora userspace
  lane succeeded and where it staged the Linux binary
- if working from macOS, record whether the Podman-machine diagnostics lane
  captured a real host artifact bundle and where it was staged
- record the exact build/install/probe commands and their observed outputs in
  `Execution Log`

**Acceptance criteria:**
- `/usr/libexec/neovex/crun run` with a krun-configured OCI bundle boots a VM with TSI port mapping
- Guest service (e.g., `nc -l -p 8080`) is accessible from host via mapped port
- System crun is unaffected (Podman still works with distro crun)

### Phase V2: System Integration

**Goal:** Verify neovex can spawn conmon → crun → VM using system packages.

**Scope:**
1. Install dependencies: conmon, buildah, libkrun, libkrunfw, catatonit
2. Write a test script that manually creates an OCI bundle, spawns conmon
   → crun, boots a VM, connects via TSI
3. Verify: log files, exit status, process tree, port mapping
4. Document the manual flow for implementation agents

**Required verifiable outputs:**
1. **Repo-owned reproducibility outputs**
   - `scripts/collect-vmm-package-versions.sh`
   - `scripts/prepare-conmon-krun-drill.sh`
   - `scripts/verify-conmon-krun-drill-helper.sh`
   - `make collect-vmm-package-versions`
   - `make prepare-conmon-krun-drill`
   - `make verify-conmon-krun-drill-helper`
   - a checked-in runbook for the conmon -> patched-crun -> krun manual drill
   - updated plan notes that name the supported distro(s), package contract,
     and expected operator-visible files
2. **Host-local system outputs**
   - package inventory commands and observed versions for `conmon`, `buildah`,
     `libkrun`, `libkrunfw`, and the init binary in use
   - a concrete OCI bundle path plus the exact conmon invocation used
   - absolute paths to the resulting log files, exit file, pid file, and
     attach socket
3. **Operational proof outputs**
   - a captured process tree showing `neovex/test shell -> conmon -> crun`
   - a successful TCP request or session against the guest service through TSI
   - graceful-stop evidence and final exit-status evidence

**Phase verification gates:**
- rerun the full V1 verification contract first
- `bash -n scripts/collect-vmm-package-versions.sh`
- `bash -n scripts/prepare-conmon-krun-drill.sh`
- `bash -n scripts/verify-conmon-krun-drill-helper.sh`
- `bash scripts/collect-vmm-package-versions.sh`
- `bash scripts/verify-conmon-krun-drill-helper.sh`
- execute the checked-in manual drill or runbook steps on at least one
  supported host
- capture process-tree, log-path, exit-file, and connectivity evidence in
  `Execution Log`

**Implementation reference:**
- Podman's container creation flow:
  `containers/podman/pkg/specgen/` → `containers/podman/libpod/`
- conmon invocation:
  `containers/podman/libpod/oci_conmon_common_linux.go`
- crun invocation by conmon:
  `containers/conmon/src/runtime_args.c`

**Acceptance criteria:**
- Manual end-to-end: boot alpine in a krun VM via conmon → crun, connect
  via TSI, stop via conmon signal, verify logs and exit status
- Process tree matches the krun model (neovex → conmon → crun, where the crun
  process remains alive as the VMM until the guest exits)

**Note:** In the crun+krun model, crun does NOT exit after starting the VM.
`krun_start_enter()` blocks, so the crun process IS the VMM. conmon monitors
the crun process (which is the VMM). When the VM exits, `_exit()` kills the
crun process, conmon detects it and writes the exit file.

### Phase V3: `neovex-sandbox` krun backend

**Goal:** neovex can spawn and manage VMs programmatically.

**Scope:**
1. `docs/plans/archive/runtime-sandbox-architecture-plan.md` `RS4` is complete so the Rust wrapper
   lands on the canonical sandbox seam rather than inventing a second public
   lifecycle surface.
2. `crates/neovex-sandbox/src/backends/krun/conmon.rs`: Spawn conmon with
   `-r /usr/libexec/neovex/crun` as subprocess, read sync pipe, manage
   PID files, read exit files, connect to attach socket
3. `crates/neovex-sandbox/src/backends/krun/bundle.rs`: Generate OCI bundle for crun
   (config.json with krun handler, `krun.port_map` annotation)
4. `crates/neovex-sandbox/src/backends/krun/buildah.rs`: Shell out to buildah for image
   pull/build/mount/inspect
5. `crates/neovex-sandbox/src/backends/krun/vm.rs`: backend-local VM handle
   wrapping conmon management
6. `crates/neovex-sandbox/src/lib.rs`: expose the first generic
   `SandboxBackend` / `SandboxHandle` seam needed by the server integration

**Required verifiable outputs:**
1. **Repo-owned code outputs**
   - concrete backend code under `crates/neovex-sandbox/src/backends/krun/`
   - targeted tests that exercise bundle generation, command assembly, and the
     backend lifecycle surface
   - any required architecture or runbook updates for new persistent operator
     or developer workflows
2. **Repo-owned verification outputs**
   - recorded `cargo check` / `cargo test` commands for `neovex-sandbox` and
     any touched integration crate
   - named tests or smoke paths proving the backend lowers through generic
     sandbox nouns rather than backend-specific public APIs
3. **Host-level integration outputs**
   - a documented smoke path that boots a VM, reaches the guest service, and
     stops the VM through the Rust backend
   - recorded proof that logs persist and the VM survives a neovex-process
     restart when conmon remains alive

**Phase verification gates:**
- preserve and reference the completed V1/V2 evidence in the same execution
  window or a clearly linked prior log row
- run focused `cargo check` and targeted tests for `neovex-sandbox` plus any
  touched server integration seams
- record the exact smoke path and observed lifecycle outcomes in
  `Execution Log`

**Acceptance criteria:**
- `neovex` can programmatically boot a postgres:16 VM, connect via TSI,
  run a query, stop the VM, verify exit status
- VM survives `neovex` process restart (conmon keeps it alive)
- Logs are persisted to disk via conmon
- System crun/Podman remain functional (neovex uses private crun path)

---

## Verification Contract

Minimum verification for any change in this workstream:

- update this plan's status ledger and execution log in the same change set as
  the code, workflow, or patch changes
- update `Implementation Checkpoints` whenever the current phase remains
  partial or gains new evidence
- `cargo fmt --all --check`

Additional verification by phase:

- V1 patch-artifact work:
  - `bash -n scripts/verify-crun-patch.sh`
  - `bash -n scripts/check-vmm-host.sh`
  - `bash -n scripts/build-neovex-crun.sh`
  - `bash -n scripts/prepare-krun-bundle.sh`
  - `bash -n scripts/verify-krun-bundle-helper.sh`
  - `bash -n scripts/prepare-direct-krun-drill.sh`
  - `bash -n scripts/verify-direct-krun-drill-helper.sh`
  - `bash -n scripts/collect-podman-machine-diagnostics.sh`
  - `bash -n scripts/verify-podman-machine-diagnostics-helper.sh`
  - `bash -n scripts/prepare-linux-vmm-validation-bundle.sh`
  - `bash -n scripts/verify-linux-vmm-validation-bundle-helper.sh`
  - `bash -n scripts/check-podman-machine-socket-paths.sh`
  - `bash -n scripts/verify-podman-machine-socket-paths-helper.sh`
  - `bash -n scripts/validate-podman-machine-readiness.sh`
  - `bash -n scripts/verify-podman-machine-readiness-helper.sh`
  - `bash -n scripts/recreate-podman-machine.sh`
  - `bash -n scripts/verify-podman-machine-recreate-helper.sh`
  - `bash scripts/verify-crun-patch.sh ~/src/github.com/containers/crun`
  - `bash scripts/build-neovex-crun.sh --help`
  - `bash scripts/prepare-krun-bundle.sh --help`
  - `bash scripts/verify-krun-bundle-helper.sh`
  - `bash scripts/prepare-direct-krun-drill.sh --help`
  - `bash scripts/verify-direct-krun-drill-helper.sh`
  - `bash scripts/collect-podman-machine-diagnostics.sh --help`
  - `bash scripts/verify-podman-machine-diagnostics-helper.sh`
  - `bash scripts/prepare-linux-vmm-validation-bundle.sh --help`
  - `bash scripts/verify-linux-vmm-validation-bundle-helper.sh`
  - `bash scripts/check-podman-machine-socket-paths.sh --machine neovex-libkrun-users-only --tmp-root /var/folders/kw/d608x5pn4cq73rz78ztl92cw0000gn/T/podman`
  - `bash scripts/check-podman-machine-socket-paths.sh --machine neovex-libkrun-users-only --tmp-root /tmp/podman`
  - `bash scripts/verify-podman-machine-socket-paths-helper.sh`
  - `bash scripts/validate-podman-machine-readiness.sh --help`
  - `bash scripts/verify-podman-machine-readiness-helper.sh`
  - `bash scripts/recreate-podman-machine.sh --help`
  - `bash scripts/verify-podman-machine-recreate-helper.sh`
  - `make verify-podman-machine-diagnostics-helper`
  - `make verify-linux-vmm-validation-bundle-helper`
  - `make verify-podman-machine-socket-paths-helper`
  - `make verify-podman-machine-readiness-helper`
  - `make verify-podman-machine-recreate-helper`
  - run `bash scripts/check-vmm-host.sh` on the actual Linux validation host
    and record the output, even if it fails
  - record the patch source path, patched-binary path, and exact build/install
    commands before marking V1 `done`
- V2 system-integration work:
  - rerun the V1 verification first
  - `bash -n scripts/prepare-conmon-krun-drill.sh`
  - `bash -n scripts/verify-conmon-krun-drill-helper.sh`
  - `bash scripts/verify-conmon-krun-drill-helper.sh`
  - manual end-to-end conmon -> patched-crun -> krun drill on a supported host
  - capture process-tree, logs, exit-file, and port-mapping evidence in the
    execution log
- V3 `neovex-sandbox` backend work:
  - focused `cargo check` and targeted tests for the sandbox crate plus touched
    server integration seams
  - record the specific test names or smoke paths that proved the backend
    lifecycle contract
  - preserve V1/V2 verification evidence because V3 depends on that foundation

### Final verification before closing this plan

- rerun the strongest available per-phase verification for V1, V2, and V3
- `make check`
- `make test`
- `make clippy`

If environmental or host limits block a command, record the limitation and the
best available substitute evidence in `Execution Log` before stopping.

## Roadmap Status Ledger

| Phase | Status | Hard deps | Notes |
|-------|--------|-----------|-------|
| V1: Patch crun | `done` | none | All repo-owned artifacts checked in. Patch updated to target upstream crun 1.27 (was 1.22). Linux host validation complete on Debian 13 x86_64: patched crun 1.27 with `+LIBKRUN` built and installed at `/usr/libexec/neovex/crun`, libkrun 1.17.4 and libkrunfw 5.3.0 built from source, system/private runtime separation verified, krun VM booted via direct `crun run` with TSI port mapping `18080:8080` proven via HTTP connectivity. The `prepare-krun-bundle.sh` script now removes the network namespace and sets `terminal: false` for krun bundles |
| V2: System integration | `done` | V1, system packages installed | Conmon lifecycle drill proven on Debian 13: `conmon → crun → libkrun VM` process tree verified, TSI port binding confirmed, exit file written (code 137 from SIGKILL), attach socket present. Key finding: conmon `--full-attach` requires an attach connection (or manual `crun start`) to transition krun containers from `created` to `running`. libkrun and libkrunfw not packaged for Debian — built from source (`~/src/github.com/containers/libkrun` v1.17.4, `~/src/github.com/containers/libkrunfw` v5.3.0) |
| V3: `neovex-sandbox` krun backend | `done` | V2, `docs/plans/archive/runtime-sandbox-architecture-plan.md` RS4 | Rust backend lowers `SandboxSpec` through generic `SandboxBackend` trait into conmon -> crun -> krun VM lifecycle. Linux host smoke test passes on Debian 13: VM boots, TSI port `18080:8080` binds, HTTP connectivity proven (BusyBox httpd 404), fresh backend instance recovers running sandbox from manifest, stop succeeds (exit code 137), logs persist on disk. Fixes applied: added standard OCI mounts to bundle generation (crun requires `mounts` block), fixed smoke test compile error (`guest_port.to_string()` type mismatch), fixed HTTP probe to use HTTP/1.0 with read timeout and no write-shutdown (TSI drops connections on half-close) |

---

## Dependency Graph

- `V1` is the foundation. It owns the patched crun artifact and patch-fidelity
  verification.
- `V2` depends on `V1` because system integration requires the patched crun
  binary and the verified patch source.
- `V3` depends on `V2` because the Rust backend should wrap a proven host-level
  subprocess/VMM model, not invent it in parallel.
- `microvm-runtime-plan.md` Phase `M1` depends on `V3`.
- `docs/plans/distribution-plan.md` stays deferred until the microVM runtime
  plan reaches the bundle/boot activation gate it documents.

## Recommended Delivery Order

1. Finish `V1` patch fidelity and patched-binary build/install drills
2. Run the `LH1`-`LH6` Linux host validation queue in parallel with any
   remaining repo-owned helper, runbook, and verification prep on macOS
3. Finish `V2` manual conmon -> patched-crun -> krun integration on supported hosts
4. Start `V3` `neovex-sandbox` backend implementation slices
5. Only after `V3`, promote the next relevant slice from
   `microvm-runtime-plan.md`

## Implementation Checkpoints

- `ICP1` Patch fidelity:
  - checked-in patch applies cleanly to the pinned upstream crun checkout
  - local and CI verification entrypoints stay green
  - Linux host-probe, build helper, bundle helper, direct-runtime drill
    helper, runtime-separation helper, focused verifiers, and operator runbook
    are checked in
  - completion evidence includes the patched-binary path, install path,
    identity output, bundle annotation proof, and a recorded connectivity probe
- `ICP2` Host-level validation:
  - `scripts/collect-vmm-package-versions.sh` exists so `LH1`/`V2` can record
    package-manager and command-level version evidence without ad hoc shell
    history
  - `scripts/prepare-conmon-krun-drill.sh`,
    `scripts/verify-conmon-krun-drill-helper.sh`, and the runbook exist for
    the manual conmon drill
  - `scripts/prepare-linux-vmm-validation-bundle.sh`,
    `scripts/verify-linux-vmm-validation-bundle-helper.sh`,
    `make prepare-linux-vmm-validation-bundle`, and
    `make verify-linux-vmm-validation-bundle-helper` exist so the Linux host
    can generate a numbered `LH1`-`LH6` command bundle with fixed artifact
    paths and a write-back checklist instead of reconstructing the queue from
    prose
  - `scripts/collect-podman-machine-diagnostics.sh`,
    `scripts/verify-podman-machine-diagnostics-helper.sh`,
    `make collect-podman-machine-diagnostics`, and
    `make verify-podman-machine-diagnostics-helper` exist so the macOS
    research lane can preserve Podman version/info, machine list/inspect,
    config/disk paths, log tails, socket paths, process matches, and host
    metadata before the next provider or guest-image experiment
  - `scripts/check-podman-machine-socket-paths.sh`,
    `scripts/verify-podman-machine-socket-paths-helper.sh`,
    `make check-podman-machine-socket-paths`, and
    `make verify-podman-machine-socket-paths-helper` exist so the repo can
    prove the Darwin 104-byte unix-socket budget, the current 104-character
    overflow under `/var/folders/.../T/podman`, and the `/tmp/podman`
    mitigation without relying on ad hoc arithmetic
  - `scripts/validate-podman-machine-readiness.sh`,
    `scripts/verify-podman-machine-readiness-helper.sh`,
    `make validate-podman-machine-readiness`, and
    `make verify-podman-machine-readiness-helper` exist so the repo can
    capture connection-targeted `podman info`, `podman machine ssh`,
    short-root diagnostics, socket-budget evidence, and the resulting
    readiness verdict without mutating the default Podman connection
  - `scripts/recreate-podman-machine.sh`,
    `scripts/verify-podman-machine-recreate-helper.sh`,
    `make recreate-podman-machine`, and
    `make verify-podman-machine-recreate-helper` exist so the repo can
    preserve the failing machine state, remove/reinitialize a stale machine
    with the proven `/tmp/podman` recipe, and immediately capture the
    post-recreate readiness verdict without relying on ad hoc shell history
  - the supplementary macOS Docker Desktop userspace lane can build the
    patched Linux binary, but it is recorded as preflight evidence only
  - macOS Docker-socket compatibility remains explicitly optional; a missing
    `podman-mac-helper` install does not block Linux guest or Linux host
    validation
  - macOS packaging claims remain explicit: Podman's Homebrew formula and
    Podman Desktop cask do not count as a durable `krunkit` dependency contract,
    even though Podman's separate upstream macOS `.pkg` installer source does
    bundle `krunkit`
  - macOS architecture docs now explicitly distinguish the rejected
    "machine VM plus nested per-service microVMs" layout from the accepted
    "machine VM plus standard containers" layout
  - future CLI naming is recorded explicitly: `neovex serve` is the server
    verb, `neovex machine ...` owns VM lifecycle, and `service` stays reserved
    away from daemon startup; if workload-management nouns arrive later,
    prefer a plural `neovex services ...` namespace for list/inspect/logs style
    commands
  - the latest unrestricted macOS diagnostics artifact at
    `/tmp/neovex-libkrun-diagnostics` shows the current libkrun machine still
    present but stopped, `podman info --debug` failing with
    `connection refused`, and the serial log plus socket paths preserved for
    later comparison
  - the reduced-volume comparison artifact at
    `/tmp/neovex-libkrun-users-only-diagnostics` proves a one-mount
    (`/Users` only) libkrun machine can still wedge in `Running: true` plus
    `Starting: true` with a missing API socket and repeated `rcu_preempt`
    stalls, so extra default Podman mounts are not yet a sufficient root-cause
    explanation
  - direct `krunkit` REST inspection now shows the VM can remain
    `VirtualMachineStateRunning` even when Podman's guest-facing SSH/API path is
    unavailable, and the current `POST /vm/state {"state":"Stop"}` shutdown
    request does not complete the transition on the wedged reduced-volume
    machine
  - the current reduced-volume failure signature includes a missing live
    `gvproxy` process, missing `-gvproxy.sock`, and missing Podman API socket
    while `krunkit` itself continues to report a running VM
  - the fresh clean repro now includes a concrete `krunkit` startup failure:
    `InvalidAddress(ENAMETOOLONG)` while activating `virtio-net`, with the
    current users-only socket paths measuring 94, 90, and 86 characters under
    the default `/var/folders/.../T/podman` tmp root
  - the short-TMPDIR rerun now shows the derived
    `...-gvproxy.sock-krun.sock` path shrinking to 60 characters under
    `/tmp/podman`, eliminating the `ENAMETOOLONG` startup blocker and causing
    the full socket set to appear under `/tmp/podman`
  - the latest live short-root readiness bundle at
    `/tmp/neovex-libkrun-users-only-readiness` still reports
    `podman --connection neovex-libkrun-users-only info --debug`
    `status=125`, `podman machine ssh` `status=255`, SSH handshake resets, and
    a guest log that enters emergency mode after Ignition and
    `systemd-fsck-root.service` failures, so the short runtime dir is
    necessary but not sufficient on this host
  - a fresh disposable machine,
    `neovex-libkrun-sr-fresh`, created with the same short tmp root and
    one-mount layout reaches full readiness on this host; the bundle at
    `/tmp/neovex-libkrun-sr-fresh-readiness` records both
    `podman --connection ... info --debug` and `podman machine ssh` as `ok`,
    so the current `users-only` failure is likely stale/corrupted machine state
    rather than a universal libkrun short-root failure
  - the checked-in recreate flow now closes that stale-state loop on this host:
    `/tmp/neovex-libkrun-users-only-recreate/pre-diagnostics/summary.txt`
    preserves the old failure signature (missing API/gvproxy sockets and
    `podman info --debug` failure), while
    `/tmp/neovex-libkrun-users-only-recreate/readiness/summary.txt` records
    `result ready info=ok ssh=ok` after `rm -f`, reinit, and restart under the
    same `/tmp/podman` short-root contract
  - patched `neovex-crun` builds and installs to `/usr/libexec/neovex/crun`
  - manual OCI-bundle boot proves TSI port mapping, logging, and shutdown
  - Linux queue items `LH1` through `LH6` are recorded with concrete outcomes
  - completion evidence includes package versions, process tree, log/exit
    paths, and observed connectivity plus graceful-stop output
- `ICP3` Rust handoff:
  - `neovex-sandbox` now owns `backends/krun/` code after `ICP1` and `ICP2`
    completed: `bundle.rs`, `buildah.rs`, `command.rs`, `conmon.rs`, and
    `vm.rs` are checked in under the canonical backend-owned module path
  - the generic sandbox seam now carries real launch intent via
    `SandboxFilesystemSpec`, `SandboxProcessSpec`, and `SandboxPortBinding`,
    while the public trait surface remains `SandboxBackend` /
    `SandboxHandle` / `SandboxSpec`
  - current repo-owned verification proves bundle generation, buildah command
    assembly, conmon launch planning, manifest persistence, and plan-only
    lifecycle lowering through the generic trait with:
    `backends::krun::bundle::tests::bundle_config_sets_krun_handler_and_port_map`,
    `backends::krun::bundle::tests::bundle_config_omits_network_namespace`,
    `backends::krun::bundle::tests::write_bundle_config_materializes_config_json`,
    `backends::krun::buildah::tests::wrap_unshare_prefixes_existing_command`,
    `backends::krun::buildah::tests::build_command_matches_expected_shape`,
    `backends::krun::conmon::tests::conmon_launch_plan_uses_private_runtime_and_buildah_unshare`,
    `backends::krun::vm::tests::plan_only_backend_lowers_through_generic_trait_surface`,
    and `backends::krun::vm::tests::plan_start_writes_bundle_and_manifest_under_backend_roots`
  - the repo also now owns a Linux-only ignored integration test,
    `crates/neovex-sandbox/tests/krun_linux_smoke.rs`, and an operator runbook
    at `docs/reference/krun-sandbox-backend-smoke.md` so the next supported
    host can exercise the Rust backend directly instead of reconstructing the
    smoke flow from prose
  - remaining completion evidence is host-level: a Linux smoke path through the
    Rust backend, persisted-log proof, and restart-survival proof captured in
    `Execution Log`

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| libkrun not packaged for Debian/Ubuntu | High | Medium | Package it ourselves (see distribution-plan.md) |
| crun krun.c churn causes patch conflicts | Medium | Low | krun.c has ~30 commits/year, but the neovex delta stays intentionally small and localized. Verify `patch --dry-run` on each upstream release. Manual resolution should stay straightforward while the patch remains one focused handler fix. |
| conmon API changes between versions | Low | Low | conmon has a stable interface (used by Podman, CRI-O) |
| buildah CLI output format changes | Medium | Medium | Pin buildah version, use --json output |
| Rootless operation issues with KVM | Medium | Medium | Document KVM permissions, test rootless flow |
| No snapshot/restore in libkrun | High | Low (for v1) | Long-running service VMs tolerate ~100ms cold boot. Warm pool and rootfs caching available if latency matters. See `docs/research/libkrun-evaluation.md` for full analysis. |
| Subprocess model limits future density | Low | Medium | Standard for v1 (CRI-O, Podman do the same). See "Architectural evolution path" above for the migration path to direct libkrun API. |

---

## Source Code References

| File | Repo | What to study |
|------|------|---------------|
| `src/libcrun/handlers/krun.c` | containers/crun | krun handler — the only file to patch |
| PR #1950 (Jan 2026) | containers/crun | Reference: `krun.cpus`, `krun.ram_mib` annotations use same `find_annotation()` pattern |
| `include/libkrun.h` | containers/libkrun | `krun_set_port_map()` signature |
| `src/runtime_args.c` | containers/conmon | How conmon invokes crun (via `-r` flag) |
| `libpod/oci_conmon_common_linux.go` | containers/podman | How Podman invokes conmon with `-r` runtime path |
| `src/tini.c` | krallin/tini | PID 1 reference (signal forwarding) |

---

## Execution Log

| Date | Phase | Status | Notes | Verification | Next |
|------|-------|--------|-------|--------------|------|
| 2026-04-11 | V1 | `in_progress` | Promoted this plan from deferred design to active execution. The first repo-owned slice pins the crun patch against the upstream `1.22` source layout, corrects the `krun.port_map` ABI details (`host:guest` pairs lowered to a null-terminated array for `krun_set_port_map()`), and adds local plus CI dry-run verification so upstream drift becomes observable before packaging work starts. The checked-in patch now applies cleanly to a real local clone at `~/src/github.com/containers/crun`. Manual Debian/Fedora build, install, and KVM-backed validation remain open. | `bash -n scripts/verify-crun-patch.sh`; `bash scripts/verify-crun-patch.sh ~/src/github.com/containers/crun`; `cargo fmt --all --check`; focused review against upstream `containers/crun` `1.22` `src/libcrun/handlers/krun.c` and upstream `containers/libkrun` `include/libkrun.h` | Continue V1 with build/install drills on supported hosts, then start V2 system integration once the patched binary path is validated |
| 2026-04-11 | meta | `documented` | Tightened the active VMM control plane so autonomous execution must produce explicit repo outputs, host-local outputs, and recorded evidence for each phase. Added stricter control-plane rules, a suggested autonomous prompt, and per-phase verification gates so fresh agents can resume from the plan and worktree without trusting chat history. | docs-only review against the current VMM plan structure; `cargo fmt --all --check` | Resume `V1` and do not mark it `done` until the patched binary path, install commands, bundle proof, and connectivity evidence are recorded |
| 2026-04-11 | V1 | `in_progress` | Added the next durable repo-owned V1 artifacts: a Linux host probe (`scripts/check-vmm-host.sh` and `make check-vmm-host`), a Linux-only patched-crun build/install helper (`scripts/build-neovex-crun.sh` and `make build-neovex-crun`), and an operator runbook at `docs/reference/krun-vmm-host-validation.md` that records the private install path, bundle recipe, and system-crun proof commands. The current local host remains an environment blocker for host-level proof: `bash scripts/check-vmm-host.sh` reports macOS 15.7.2 on arm64, no `/dev/kvm`, and missing `automake`, `conmon`, `buildah`, `crun`, and init/runtime packages. | `bash -n scripts/verify-crun-patch.sh`; `bash -n scripts/check-vmm-host.sh`; `bash -n scripts/build-neovex-crun.sh`; `bash scripts/verify-crun-patch.sh ~/src/github.com/containers/crun`; `bash scripts/check-vmm-host.sh`; `bash scripts/build-neovex-crun.sh --help`; `cargo fmt --all --check` | Continue `V1` on a supported Linux host by running the host probe there, staging `/usr/libexec/neovex/crun`, and recording the first bundle plus connectivity proof |
| 2026-04-11 | V1 | `in_progress` | Converted the documented bundle recipe into checked-in tooling: `scripts/prepare-krun-bundle.sh` now writes the `run.oci.handler=krun` plus `krun.port_map=host:guest` config shape, `scripts/verify-krun-bundle-helper.sh` proves that rewrite against a checked-in fixture, and the CI workflow now runs that verifier alongside the pinned patch-apply lane. This closes more of the remaining repo-owned V1 gap while keeping the real Linux/KVM runtime proof explicitly pending. | `bash -n scripts/prepare-krun-bundle.sh`; `bash -n scripts/verify-krun-bundle-helper.sh`; `bash scripts/prepare-krun-bundle.sh --help`; `bash scripts/verify-krun-bundle-helper.sh`; `cargo fmt --all --check` | Continue `V1` on a supported Linux host by using `scripts/prepare-krun-bundle.sh` to generate the first real krun bundle, then record staged-binary, runtime-path, and host-to-guest connectivity proof |
| 2026-04-11 | meta | `documented` | Added an explicit parallel Linux-host validation queue (`LH1`-`LH6`) so another agent or laptop can pick up the supported-host verification work cleanly while repo-owned prep continues on macOS. The plan now says this queue is required for `V1`/`V2` closeout but is not a blocker for continued helper/docs/test preparation on the Mac. | docs-only review of queue, evidence contract, and delivery-order updates; `cargo fmt --all --check` | Use the Linux host queue for the next supported-host run and feed any repo fixes back into this plan plus the current worktree |
| 2026-04-11 | meta | `documented` | Added a dedicated Linux-host execution prompt directly beside the `LH1`-`LH6` queue so the other laptop can resume from the control plane alone, execute the supported-host lane in order, and feed repo fixes plus host evidence back into the same plan. | docs-only review of the Linux-host prompt and queue handoff; `cargo fmt --all --check` | Hand the Linux laptop this plan plus `docs/reference/krun-vmm-host-validation.md`, then continue recording each Linux queue result back into `Execution Log` |
| 2026-04-11 | V2 prep | `documented` | Added a checked-in conmon lifecycle drill preparer and focused verifier so the Linux host no longer has to reconstruct the `conmon -> /usr/libexec/neovex/crun -> guest` command from prose. `scripts/prepare-conmon-krun-drill.sh` now lays out deterministic pid/log/exit/persist paths, emits a runnable `run-conmon.sh`, and generates helper scripts for attach-socket discovery, process-tree capture, wait-for-exit, and graceful/forced stop. The runbook, Makefile, CI lane, and plan now all point at the same reproducible drill path, while the actual Linux-host execution evidence remains pending. | `bash -n scripts/prepare-conmon-krun-drill.sh`; `bash -n scripts/verify-conmon-krun-drill-helper.sh`; `bash scripts/prepare-conmon-krun-drill.sh --help`; `bash scripts/verify-conmon-krun-drill-helper.sh`; `cargo fmt --all --check` | Run `LH6` on a supported Linux host using the generated `run-conmon.sh` flow, then record the concrete attach-socket path, process tree, connectivity probe, graceful stop, and exit-status evidence |
| 2026-04-11 | V2 prep | `documented` | Added a dedicated package/version inventory helper so Linux-host validation no longer depends on ad hoc `rpm -q`, `dpkg-query`, or `--version` shell history. `scripts/collect-vmm-package-versions.sh` now emits stable host, package-manager, package, command, and Podman-runtime evidence for `conmon`, `buildah`, `libkrun`, `libkrunfw`, the init binary in use, `crun`, and `podman`. The Makefile, runbook, and active VMM plan now all point at the same inventory entrypoint for `LH1` and `V2`. | `bash -n scripts/collect-vmm-package-versions.sh`; `bash scripts/collect-vmm-package-versions.sh`; `cargo fmt --all --check` | Use the inventory helper on the real Linux host before `LH2`, then record its exact output alongside the host preflight result |
| 2026-04-11 | V2 prep | `documented` | Normalized `podman.runtime` evidence in both `scripts/check-vmm-host.sh` and `scripts/collect-vmm-package-versions.sh` so Podman remote output is collapsed to a single stable line instead of leaking multiline host metadata into the plan log. This keeps `LH1` evidence readable on macOS while still preserving the full provider signal (`applehv` on the current host). | `bash -n scripts/check-vmm-host.sh`; `bash -n scripts/collect-vmm-package-versions.sh`; `bash scripts/check-vmm-host.sh`; `bash scripts/collect-vmm-package-versions.sh`; `cargo fmt --all --check` | Reuse the normalized helpers for future macOS and Linux host evidence, then continue the Linux-host queue |
| 2026-04-11 | V1 prep | `documented` | Added a checked-in direct private-runtime drill preparer and focused verifier so `LH5` no longer depends on a hand-written `/usr/libexec/neovex/crun run --bundle ...` command. `scripts/prepare-direct-krun-drill.sh` now derives the first probe port from `krun.port_map`, emits runnable `run-runtime.sh` and `start-runtime.sh` scripts, and generates deterministic stdout, stderr, pid, launcher-pid, exit-status, and HTTP-probe helpers. The runbook, Makefile, CI lane, and plan now point at the same reproducible direct-runtime path, while the actual Linux-host execution evidence remains pending. | `bash -n scripts/prepare-direct-krun-drill.sh`; `bash -n scripts/verify-direct-krun-drill-helper.sh`; `bash scripts/prepare-direct-krun-drill.sh --help`; `bash scripts/verify-direct-krun-drill-helper.sh`; `cargo fmt --all --check` | Run `LH5` on a supported Linux host or Linux machine-VM guest using the generated `start-runtime.sh` flow, then record the direct-runtime command, concrete log/pid/exit paths, connectivity probe, graceful stop, and exit-status evidence |
| 2026-04-11 | V1 prep | `documented` | Added a supplementary macOS userspace validation lane and recorded the current Mac host evidence. `scripts/verify-neovex-crun-fedora-userspace.sh` now succeeds through Docker Desktop against `fedora:43`, using `~/src/github.com/containers/crun` as the source checkout and staging a Linux `aarch64` binary at `/tmp/neovex-crun-fedora-userspace-output/crun`. The first reruns exposed two real helper gaps that are now fixed in `scripts/build-neovex-crun.sh`: explicit `libocispec` generation before `crun`, and explicit `git-version.h` generation before the final build. In parallel, the local Homebrew Podman machine remains unusable for Linux-guest validation: `podman machine list` reports `neovex-vmm-validation` as `Currently starting`, `podman machine inspect neovex-vmm-validation` reports `State: running` with `VM TYPE applehv`, and the serial log at `/var/folders/kw/d608x5pn4cq73rz78ztl92cw0000gn/T/podman/neovex-vmm-validation.log` shows Ignition failure, `systemd-fsck-root.service` failure, and emergency mode. | `bash -n scripts/build-neovex-crun.sh`; `bash -n scripts/verify-neovex-crun-fedora-userspace.sh`; `bash scripts/verify-neovex-crun-fedora-userspace.sh --help`; `bash scripts/verify-neovex-crun-fedora-userspace.sh --crun-source ~/src/github.com/containers/crun --output-dir /tmp/neovex-crun-fedora-userspace-output --work-dir /tmp/neovex-crun-fedora-userspace-build`; `file /tmp/neovex-crun-fedora-userspace-output/crun`; `podman machine list`; `podman machine inspect neovex-vmm-validation`; `tail -n 60 /var/folders/kw/d608x5pn4cq73rz78ztl92cw0000gn/T/podman/neovex-vmm-validation.log`; `cargo fmt --all --check` | Keep the Fedora userspace lane as supplementary preflight, but continue `LH1`-`LH6` on a usable Linux host or Linux guest before attempting V2/V3 closeout |
| 2026-04-11 | meta | `documented` | Tightened the VMM helper CI lane so GitHub Actions now syntax-checks all checked-in crun/VMM helper scripts and exercises the non-host help entrypoints before downloading the pinned upstream crun source. This keeps the repo-owned control-plane scripts honest even while the Linux `/dev/kvm` evidence remains a separate host lane. | `cargo fmt --all --check`; workflow review of `.github/workflows/verify-neovex-crun-patch.yml` | Use the CI lane to catch script drift early, then keep advancing the remaining Linux-host queue items |
| 2026-04-11 | meta | `documented` | Strengthened the GitHub Actions VMM lane further so it now runs the full Docker-based Fedora userspace build helper against the pinned upstream `crun` checkout on `ubuntu-latest`, not just script syntax and `--help` entrypoints. This gives the repo a real automated Linux userspace build proof while `/dev/kvm` runtime proof remains delegated to `LH1`-`LH6`. | `cargo fmt --all --check`; workflow review of `.github/workflows/verify-neovex-crun-patch.yml`; prior local confirmation via `bash scripts/verify-neovex-crun-fedora-userspace.sh --crun-source ~/src/github.com/containers/crun --output-dir /tmp/neovex-crun-fedora-userspace-output --work-dir /tmp/neovex-crun-fedora-userspace-build` | Keep the CI userspace lane green, then continue the remaining Linux-host queue items for real runtime and conmon proof |
| 2026-04-11 | V1 prep | `documented` | Added a checked-in runtime-separation verifier so `LH4` no longer depends on manually reassembling `command -v crun`, `crun --version`, and `podman info` probes. `scripts/verify-runtime-separation.sh` now records the system-runtime path and version, the private neovex runtime path and version, Podman runtime evidence, resolved realpaths, and a final separation verdict. `scripts/verify-runtime-separation-helper.sh`, the Makefile, the operator runbook, the GitHub Actions lane, and this plan now all point at the same reproducible `LH4` entrypoint while the actual supported-host proof remains pending. | `bash -n scripts/verify-runtime-separation.sh`; `bash -n scripts/verify-runtime-separation-helper.sh`; `bash scripts/verify-runtime-separation.sh --help`; `bash scripts/verify-runtime-separation-helper.sh`; `make verify-runtime-separation-helper`; `cargo fmt --all --check` | Run `LH4` on a supported Linux host after `LH3`, capture the full helper output, and confirm Podman still points at the distro runtime while neovex uses `/usr/libexec/neovex/crun` |
| 2026-04-11 | V1 prep | `documented` | Closed the macOS machine-provider prerequisite for future guest validation by installing `krunkit` directly from the dedicated Homebrew tap. This confirmed the concrete dependency path we should document for Channel 4: Homebrew `podman` `5.8.1` plus the `podman-desktop` cask did not themselves provide a shell-visible `krunkit` binary on this host, while `brew tap slp/krunkit && brew install krunkit` produced `/opt/homebrew/bin/krunkit` with version `1.1.1`. The existing Podman machine remains broken and unvalidated, so Linux guest proof is still pending. | `which krunkit`; `krunkit --version`; `brew list --versions krunkit`; `brew info krunkit`; prior local evidence via `brew info --cask podman-desktop`; `brew cat --cask podman-desktop`; `find /Applications/Podman\\ Desktop.app -iname '*krunkit*'`; `cargo fmt --all --check` | Create or repair a krunkit-backed Linux guest on this Mac, then resume `LH1` through `LH6` inside that guest or fall back to the separate Linux host |
| 2026-04-11 | V1 prep | `blocked` | Attempted to promote this Mac into the Linux-guest validation lane with a fresh libkrun-backed Podman machine. `CONTAINERS_MACHINE_PROVIDER=libkrun` correctly flips `podman info --debug` to `provider: libkrun`, and `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine init --cpus 4 --memory 4096 --disk-size 60 neovex-libkrun-validation` produced a machine under the `libkrun` config/data roots. The first start was blocked by stale `applehv` metadata from the broken `neovex-vmm-validation` machine, so that machine was stopped and removed. A second libkrun start reached `State: running` and launched both `gvproxy` and `/opt/homebrew/bin/krunkit`, but the guest never reached SSH or API readiness: `podman info` and `podman machine ssh` both failed with SSH handshake resets, and the serial log showed repeated `(udev-worker)` soft lockups while the generated `krunkit` command was running with `--nested`. Forced cleanup returned the libkrun machine to `Running: false` / `Starting: false`. | `system_profiler SPHardwareDataType`; `uname -a`; `CONTAINERS_MACHINE_PROVIDER=libkrun podman info --debug`; `podman machine list --all-providers --format json`; `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine init --cpus 4 --memory 4096 --disk-size 60 neovex-libkrun-validation`; `sed -n '1,260p' ~/.config/containers/podman/machine/libkrun/neovex-libkrun-validation.json`; `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine start neovex-libkrun-validation`; `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine inspect neovex-libkrun-validation`; `ps -ax | rg 'krunkit|gvproxy|neovex-libkrun-validation'`; `CONTAINERS_MACHINE_PROVIDER=libkrun podman info`; `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine ssh neovex-libkrun-validation 'uname -a ...'`; `tail -n 120 /var/folders/kw/d608x5pn4cq73rz78ztl92cw0000gn/T/podman/neovex-libkrun-validation.log`; `podman machine stop neovex-vmm-validation`; `podman machine rm -f neovex-vmm-validation`; `kill 94782`; `podman machine list --all-providers --format json`; `cargo fmt --all --check` | Keep the current Mac as host-prep evidence only, then continue `LH1`-`LH6` on the separate Linux host or debug the Podman-aligned guest/provider recipe on macOS without introducing a host-side direct-container path |
| 2026-04-11 | meta | `documented` | Tightened the macOS architectural and packaging invariants around Podman parity. Podman's own docs and README define the macOS model as one Linux VM where containers are run, with Podman Desktop acting as a frontend over the same `podman machine` backend. Local Homebrew inspection shows `podman` `5.8.1` packages `podman-mac-helper`, `gvproxy`, and `vfkit`, while `podman-desktop` is just an app bundle from the cask. This means neovex should mirror Podman's one-machine-VM architecture on macOS, but it must own the `krunkit` dependency directly if it wants that provider. The packaging evidence recorded here is intentionally scoped to the Homebrew path we plan to ship, not to Podman's separate upstream macOS `.pkg` installer. | Podman docs: `https://docs.podman.io/en/latest/markdown/podman-machine-start.1.html`; Podman README: `https://raw.githubusercontent.com/containers/podman/main/README.md`; Podman release notes: `https://github.com/containers/podman/releases`; `brew cat podman`; `brew info podman`; `ls -l /opt/homebrew/Cellar/podman/5.8.1/bin /opt/homebrew/Cellar/podman/5.8.1/libexec/podman`; `brew cat --cask podman-desktop`; `brew info --cask podman-desktop`; `find /Applications/Podman\\ Desktop.app -iname '*krunkit*' -o -iname '*vfkit*' -o -iname '*gvproxy*'`; `cargo fmt --all --check` | Keep the macOS lane Podman-aligned and dependency-explicit; debug the guest/provider blocker or shift proof to the Linux host, but do not invent a host-side direct-container path on macOS |
| 2026-04-11 | meta | `documented` | Tightened the macOS architectural and packaging invariants around Podman parity further using upstream source files. `containers/podman` `v5.8.1` `pkg/machine/provider/platform_darwin.go` confirms Podman supports both `applehv` and `libkrun` on Apple Silicon but defaults to `applehv` when no provider is set. `pkg/machine/apple/apple.go` confirms Podman appends `--nested` for libkrun machines and relies on `krunkit` to ignore it when unsupported, so the observed `--nested` flag on this `M2 Max` host is not proof of active nested virtualization. `contrib/pkginstaller/Makefile` confirms Podman's upstream macOS `.pkg` bundles `gvproxy`, `vfkit`, and `krunkit`, which is broader than the Homebrew formula/cask contract we plan to mirror. | upstream source: `https://github.com/containers/podman/blob/v5.8.1/pkg/machine/provider/platform_darwin.go`; `https://github.com/containers/podman/blob/v5.8.1/pkg/machine/apple/apple.go`; `https://github.com/containers/podman/blob/v5.8.1/contrib/pkginstaller/Makefile`; prior local evidence via `ps -ax | rg 'krunkit|gvproxy|neovex-libkrun-validation'`; `brew cat podman`; `brew info podman`; `cargo fmt --all --check` | Keep the docs source-aligned: default provider is `applehv`, `libkrun` remains a supported explicit provider, and the macOS blocker is guest readiness rather than proof that nested virt activated |
| 2026-04-11 | meta | `documented` | Added upstream issue context for the current macOS blocker so future agents do not misclassify it as neovex-specific by default. `containers/podman` issue `#24559` documents libkrun provider startup failures on macOS 15.1, and issue `#23296` records krunkit-related macOS test failures in Podman's own CI and localmachine flow. Together with the local `M2 Max` soft-lockup evidence, this strengthens the current control-plane stance: keep the macOS lane as a useful research and packaging lane, but do not require it to close `V1` or `V2`. | upstream issues: `https://github.com/containers/podman/issues/24559`; `https://github.com/containers/podman/issues/23296`; `cargo fmt --all --check` | Continue Linux-host closeout for `LH1`-`LH6`, and treat any future macOS libkrun success as additive evidence rather than the sole closeout path |
| 2026-04-11 | meta | `documented` | Read the upstream issue comments for the main macOS libkrun regressions so future agents have the actual workaround history, not just the issue titles. `containers/podman` `#24559` comments say the older startup failure was fixed by newer krunkit / Podman releases (`krunkit` `0.1.4`, `podman` `5.3.1`) and was associated with memory-map issues on some high-memory Macs. `#23296` comments say a krunkit/libkrun mount failure was debugged upstream and fixed via `containers/libkrun#209`. Because the current host is already on newer tooling (`podman` `5.8.1`, `krunkit` `1.1.1`), those older workarounds are useful historical context but should not be assumed to explain the current guest-readiness failure on their own. | upstream comments: `https://github.com/containers/podman/issues/24559#issuecomment-2476356509`; `https://github.com/containers/podman/issues/24559#issuecomment-2486269345`; `https://github.com/containers/podman/issues/23296#issuecomment-2238926237`; `cargo fmt --all --check` | Keep using the Linux host as the required closeout lane, and if macOS research continues, focus on current `5.8.1` / `1.1.1` behavior rather than replaying older fixed regressions |
| 2026-04-11 | meta | `documented` | Checked the specific krunkit bug linked from Podman's older libkrun startup regression. `containers/krunkit` issue `#17` says the failure triggers when the guest `--memory` value exceeds `27647`. Our failing libkrun machine on this host was started with `--memory 4096`, so that older high-memory threshold bug does not line up directly with the current Podman `5.8.1` / `krunkit` `1.1.1` guest-readiness failure. | upstream issue: `https://github.com/containers/krunkit/issues/17`; prior local evidence via `ps -ax | rg 'krunkit|gvproxy|neovex-libkrun-validation'`; `cargo fmt --all --check` | Keep looking for a current guest/provider cause if macOS research continues, but do not anchor on the older >27 GiB krunkit bug |
| 2026-04-11 | meta | `documented` | Added direct guest-image evidence from `containers/podman-machine-os` so the macOS control plane no longer relies only on architecture prose. `build.sh` builds the guest from `podman-image/Containerfile.COREOS`, and `podman-image/build_common.sh` installs `crun`, `crun-wasm`, `podman`, `containers-common`, `containers-common-extra`, `netavark`, and `aardvark-dns` while removing `runc`. That package set reinforces the current neovex direction: mirror Podman's macOS machine-VM model with standard containers inside the guest, not nested guest-side `krun` microVMs. | upstream source: `https://github.com/containers/podman-machine-os/blob/main/build.sh`; `https://github.com/containers/podman-machine-os/blob/main/podman-image/Containerfile.COREOS`; `https://github.com/containers/podman-machine-os/blob/main/podman-image/build_common.sh`; `cargo fmt --all --check` | Keep the macOS guest contract source-backed and continue using the Linux host as the required `V1`/`V2` closeout lane |
| 2026-04-11 | V1 prep | `documented` | Verified the separate Docker-compatibility boundary on this Mac so future agents do not mistake it for the libkrun boot blocker. `/var/run/docker.sock` currently points to `/Users/jack/.docker/run/docker.sock`, and Podman's own CLI and Desktop docs describe `podman-mac-helper` as the optional system helper that binds the default Docker socket path to a Podman-managed machine socket. That makes the helper relevant for Docker-compatible client workflows, but not for the failing `CONTAINERS_MACHINE_PROVIDER=libkrun` guest boot path or for `LH1`-`LH6` closeout. | `ls -l /var/run/docker.sock`; `ls -l /Users/jack/.docker/run/docker.sock`; Podman docs at `https://docs.podman.io/en/latest/markdown/podman-machine-start.1.html`; Podman Desktop docs at `https://podman-desktop.io/docs/migrating-from-docker/customizing-docker-compatibility`; `cargo fmt --all --check` | Leave Docker-socket compatibility optional and keep investigating the provider or guest-image failure behind the Podman-managed libkrun lane |
| 2026-04-12 | V1 prep | `documented` | Closed the bookkeeping gap around the new macOS Podman-machine diagnostics lane and exercised it end to end. The repo-owned helper pair now has a stable deterministic verifier, and `scripts/collect-podman-machine-diagnostics.sh` was fixed so failed commands record their real exit status and preserve stderr in the captured artifact files. An unrestricted host run at `/tmp/neovex-libkrun-diagnostics` now records the current libkrun-machine state on this Mac: `podman machine list` and `podman machine inspect` succeed for `neovex-libkrun-validation`, `podman info --debug` still fails with `connection refused`, the machine is currently stopped, the serial log tail and API / ready / gvproxy socket paths are preserved, and no live `krunkit` / `gvproxy` process survived at capture time. This strengthens the macOS research lane without changing the core gate: Linux-host `LH1` through `LH6` remain the required closeout path for `V1` and `V2`. | `bash -n scripts/collect-podman-machine-diagnostics.sh`; `bash -n scripts/verify-podman-machine-diagnostics-helper.sh`; `bash scripts/collect-podman-machine-diagnostics.sh --help`; `bash scripts/verify-podman-machine-diagnostics-helper.sh`; `make verify-podman-machine-diagnostics-helper`; `bash scripts/collect-podman-machine-diagnostics.sh --machine neovex-libkrun-validation --provider libkrun --output-dir /tmp/neovex-libkrun-diagnostics`; `cargo fmt --all --check` | Use the captured macOS artifact bundle as the comparison point for the next libkrun-machine experiment, but keep the separate Linux host or Linux guest as the required lane for patched-crun build/install and runtime proof |
| 2026-04-12 | meta | `documented` | Clarified the macOS architecture and CLI taxonomy so the control plane no longer conflates a capability flag with the target design. `distribution-plan.md` Channel 4 now shows both the rejected "machine VM plus nested microVM per service" layout and the accepted Podman-aligned "machine VM plus standard containers per service" layout. It also records the intended CLI split: `neovex serve` stays the server-start verb, `neovex machine ...` owns machine lifecycle, and `service` is intentionally not the daemon-start command because it is already overloaded in the codebase and product vocabulary. The current binary remains flag-driven, so that command surface is documented as target state rather than shipped behavior. | docs review of `docs/plans/distribution-plan.md` and `docs/plans/vmm-infrastructure-plan.md`; `cargo fmt --all --check` | Continue V1 macOS provider diagnostics and Linux-host closeout with the architecture boundary now recorded explicitly in the control plane |
| 2026-04-12 | V1 prep | `documented` | Captured a second unrestricted macOS diagnostics bundle for the reduced-volume machine experiment. `neovex-libkrun-users-only` is now the default Podman libkrun machine on this host, and its copied machine config at `/tmp/neovex-libkrun-users-only-diagnostics/machine-config.json` proves Podman initialized only one `virtiofs` mount (`/Users -> /Users`) with `LibKrunHypervisor.KRun.BinaryPath` still `null`. Even in that reduced-mount configuration, the machine remains wedged in `Running: true` plus `Starting: true`; `podman info --debug` still fails with `dial tcp 127.0.0.1:52251: connect: connection refused`; the expected API socket path is missing; and the serial log tail is dominated by repeated `rcu_preempt` stalls instead of reaching guest readiness. This narrows the macOS investigation: reducing Podman's default host mounts changed the failure shape but did not produce a healthy guest, so mount count alone is not yet a sufficient explanation. | `ps -ax | rg 'krunkit|gvproxy|podman machine start|neovex-libkrun'`; `podman machine list --all-providers --format json`; `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine inspect neovex-libkrun-users-only`; `bash scripts/collect-podman-machine-diagnostics.sh --machine neovex-libkrun-users-only --provider libkrun --output-dir /tmp/neovex-libkrun-users-only-diagnostics`; `sed -n '1,120p' /tmp/neovex-libkrun-users-only-diagnostics/podman-info-debug.txt`; `tail -n 120 /tmp/neovex-libkrun-users-only-diagnostics/machine-log-tail.txt`; `sed -n '1,220p' /tmp/neovex-libkrun-users-only-diagnostics/machine-config.json`; `cargo fmt --all --check` | Decide whether the next macOS experiment is a controlled stop/cleanup plus restart recipe or a source-backed `krunkit` binary-path wrapper experiment, while keeping Linux-host `LH1`-`LH6` as the required closeout lane |
| 2026-04-12 | V1 prep | `documented` | Closed one branch of the next-experiment search with upstream source review. `containers/podman` `pkg/machine/libkrun/stubber.go` hardcodes `krunkit` as the libkrun helper name and calls `apple.StartGenericAppleVM(mc, krunkitBinary, ...)`; `pkg/machine/apple/apple.go` then resolves that helper through `config.Default().FindHelperBinary(cmdBinary, true)`. Combined with the copied machine JSON showing `LibKrunHypervisor.KRun.BinaryPath: null`, this means a config-only `BinaryPath` edit is not a source-backed way to inject a wrapper into the current Podman libkrun startup path. Any wrapper experiment now needs a different mechanism, such as PATH control or a Podman-side patch, rather than only editing the machine config. | upstream source: `https://github.com/containers/podman/blob/main/pkg/machine/libkrun/stubber.go`; `https://github.com/containers/podman/blob/main/pkg/machine/apple/apple.go`; local evidence via `sed -n '1,220p' /tmp/neovex-libkrun-users-only-diagnostics/machine-config.json`; `cargo fmt --all --check` | Keep the next macOS experiment honest: prefer controlled cleanup/restart or a source-backed wrapper mechanism, and do not burn time on a config-only `BinaryPath` edit |
| 2026-04-12 | meta | `documented` | Tightened the future CLI contract so the plan answers the `serve` versus `service` question directly. `distribution-plan.md` now states that `neovex serve` starts neovex itself, `neovex machine ...` owns machine lifecycle, and a future workload-management surface should prefer plural `neovex services ...` commands such as `list`, `inspect`, or `logs`. This keeps `serve` and `services` semantically distinct: one is the daemon-start verb, the other is a possible resource namespace for managed workloads. | docs review of `docs/plans/distribution-plan.md` and `docs/plans/vmm-infrastructure-plan.md`; `cargo fmt --all --check` | Keep future CLI work aligned with the recorded split: `serve` for server startup, `machine` for VM lifecycle, `services` for any later workload inventory surface |
| 2026-04-12 | V1 prep | `documented` | Checked the toolbar-entry confusion against the real local CLIs so future macOS debugging does not treat UI state as authoritative. On this host, `docker context ls` only reports `default` and `desktop-linux`, while `podman system connection list` reports `neovex-libkrun-users-only`, `neovex-libkrun-users-only-root`, `neovex-libkrun-validation`, and `neovex-libkrun-validation-root`. That means the screenshot entries line up with Podman-managed machine connections, not with Docker CLI contexts. They may be surfaced by a GUI, but they do not change the active diagnosis: the guest still wedges before its Podman API socket comes up. | `docker context ls`; `docker context inspect neovex-libkrun-users-only neovex-libkrun-validation`; `podman system connection list`; `cargo fmt --all --check` | Keep CLI evidence and serial logs as the source of truth for macOS validation; treat toolbar state as secondary UI decoration only |
| 2026-04-12 | V1 prep | `documented` | Narrowed the current reduced-volume failure signature further with direct VMM inspection. The `krunkit` REST API at `http://localhost:52273` responds successfully and reports `VirtualMachineStateRunning` for both `/` and `/vm/state`, which proves the VMM process itself is alive even though Podman's SSH/API path never comes up. However, the expected host-side helper artifacts are still missing: there is no live `gvproxy` process, no `neovex-libkrun-users-only-gvproxy.sock`, and no `neovex-libkrun-users-only-api.sock` under the Podman tmp root. A direct `POST /vm/state {"state":"Stop"}` returns `VirtualMachineStateStopping`, but the VM remains running afterward and `CONTAINERS_MACHINE_PROVIDER=libkrun podman --log-level=debug machine stop neovex-libkrun-users-only` hangs. This suggests the current wedge is deeper than a bad Podman connection target: the VMM stays alive while guest readiness and normal lifecycle completion both fail. | `curl -i -sS http://localhost:52273/`; `curl -i -sS http://localhost:52273/vm/state`; `curl -i -sS -X POST http://localhost:52273/vm/state -H 'content-type: application/json' --data '{"state":"Stop"}'`; `ps -ax -o pid,ppid,state,etime,command | rg '[g]vproxy|[p]odman machine start|[k]runkit'`; `ls -la /var/folders/kw/d608x5pn4cq73rz78ztl92cw0000gn/T/podman`; `CONTAINERS_MACHINE_PROVIDER=libkrun podman --log-level=debug machine stop neovex-libkrun-users-only`; `cargo fmt --all --check` | Choose the next macOS reset path deliberately: either force-clean the wedged validation machine for a fresh debug start or stop spending time on this host and move required closeout work to the Linux lane |
| 2026-04-12 | V1 prep | `documented` | Captured the first clean fresh-state repro and the first concrete crash signature for the macOS libkrun lane. After force-killing the wedged `users-only` machine processes, `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine rm -f neovex-libkrun-users-only` finally succeeded, a fresh `podman machine init --cpus 2 --memory 2048 --disk-size 20 -v /Users:/Users neovex-libkrun-users-only` completed successfully, and `CONTAINERS_MACHINE_PROVIDER=libkrun podman --log-level=debug machine start neovex-libkrun-users-only` showed Podman launching both `gvproxy` and `krunkit` from a clean state. The attached `krunkit-debug.sh` terminal then revealed the real failure: Fedora CoreOS begins booting, but `krunkit` logs `Error activating virtio-net (eth0) backend: InvalidAddress(ENAMETOOLONG)` and panics with `BadActivate`. The live socket paths on this host measure 94 (`...-gvproxy.sock`), 90 (`...-api.sock`), and 86 (`...users-only.sock`) characters under `/var/folders/.../T/podman`. That makes a too-long unix-socket path in Podman's default tmp root the leading hypothesis, ahead of guest mount count or generic Fedora CoreOS failure. | `kill 33394 57099 57106`; `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine rm -f neovex-libkrun-users-only`; `CONTAINERS_MACHINE_PROVIDER=libkrun podman machine init --cpus 2 --memory 2048 --disk-size 20 -v /Users:/Users neovex-libkrun-users-only`; `CONTAINERS_MACHINE_PROVIDER=libkrun podman --log-level=debug machine start neovex-libkrun-users-only`; local `krunkit-debug.sh` terminal output showing `InvalidAddress(ENAMETOOLONG)` and `BadActivate`; path-length measurement of `...-gvproxy.sock`, `...-api.sock`, and `...users-only.sock`; `podman machine list --all-providers --format json`; `cargo fmt --all --check` | Run the same reduced-volume machine with a deliberately short tmp root such as `/tmp/podman` so the `gvproxy` and API socket paths are much shorter, then see whether `virtio-net` activation and guest readiness succeed |
| 2026-04-12 | V1 prep | `documented` | Turned the new macOS path-length diagnosis into durable repo evidence and recorded the first validated mitigation. The repo now owns `scripts/check-podman-machine-socket-paths.sh`, `scripts/verify-podman-machine-socket-paths-helper.sh`, and the matching Make targets so the current socket layout can be proven from source-controlled tooling instead of ad hoc arithmetic. Those helpers show the default Darwin tmp root produces a 104-character derived `...-gvproxy.sock-krun.sock` path for `neovex-libkrun-users-only`, which exceeds the 103-character practical budget implied by Darwin's 104-byte `sockaddr_un.sun_path`, while `/tmp/podman` reduces the same derived path to 60. The live rerun with `TMPDIR=/tmp` then reached a stronger guest-boot state: `podman machine start` reported success, `/tmp/podman` contained the ready/API/gvproxy/derived-krun sockets, and the guest log reached `podman.socket`, `sshd.service`, `ready.service`, and the login prompt. The remaining gap is durability, not the path-budget hypothesis itself: later CLI inspection from the current automation environment still did not preserve a stable API/SSH session, so the next host experiment should be a persistent-session/manual proof against the short-root machine. This pass also records the implementation-reference split explicitly: Podman core (`pkg/machine/libkrun/stubber.go` and `pkg/machine/apple/apple.go`) is the primary macOS machine-layer reference, while Podman Desktop stays secondary for install/UX flows. | `bash -n scripts/check-podman-machine-socket-paths.sh`; `bash -n scripts/verify-podman-machine-socket-paths-helper.sh`; `bash scripts/check-podman-machine-socket-paths.sh --machine neovex-libkrun-users-only --tmp-root /var/folders/kw/d608x5pn4cq73rz78ztl92cw0000gn/T/podman`; `bash scripts/check-podman-machine-socket-paths.sh --machine neovex-libkrun-users-only --tmp-root /tmp/podman`; `bash scripts/verify-podman-machine-socket-paths-helper.sh`; `make verify-podman-machine-socket-paths-helper`; `TMPDIR=/tmp CONTAINERS_MACHINE_PROVIDER=libkrun podman machine init --cpus 2 --memory 2048 --disk-size 20 -v /Users:/Users neovex-libkrun-users-only`; `TMPDIR=/tmp CONTAINERS_MACHINE_PROVIDER=libkrun podman machine start neovex-libkrun-users-only`; `tail -n 80 /tmp/podman/neovex-libkrun-users-only.log`; `cargo fmt --all --check` | Keep the short runtime dir as the leading macOS mitigation, add the same rule to Channel 4, and do the next guest-liveness proof from a persistent session or manual terminal rather than from a transient automation shell |
| 2026-04-12 | V1 prep | `documented` | Added a repo-owned readiness validator for the macOS short-root lane and used it to capture the next real host result. The new helper pair, `scripts/validate-podman-machine-readiness.sh` and `scripts/verify-podman-machine-readiness-helper.sh`, now combines the connection-targeted `podman info --debug`, `podman machine ssh`, the short-root socket-budget report, and the existing diagnostics bundle into one readiness artifact set without mutating Podman's default connection. A live run at `/tmp/neovex-libkrun-users-only-readiness` against `TMPDIR=/tmp` shows the short runtime dir is necessary but not sufficient on this host: the named connection still fails with `ssh: handshake failed: ... connection reset by peer` (`status=125`), `podman machine ssh` still fails with `kex_exchange_identification: read: Connection reset by peer` (`status=255`), and the captured guest log enters emergency mode after Ignition and `systemd-fsck-root.service` failures. The live machine again wedged in `Running: true` plus `Starting: true`; a direct libkrun REST `POST /vm/state {"state":"Stop"}` returned `VirtualMachineStateStopping` without clearing it, and the cleanest successful cleanup was interrupting the hanging `podman machine start` process, after which `podman machine list` returned the machine to `Running: false` / `Starting: false` and no `krunkit` / `gvproxy` processes remained. | `bash -n scripts/validate-podman-machine-readiness.sh`; `bash -n scripts/verify-podman-machine-readiness-helper.sh`; `bash scripts/verify-podman-machine-readiness-helper.sh`; `make verify-podman-machine-readiness-helper`; `TMPDIR=/tmp CONTAINERS_MACHINE_PROVIDER=libkrun podman machine list --all-providers --format json`; `TMPDIR=/tmp CONTAINERS_MACHINE_PROVIDER=libkrun podman machine start neovex-libkrun-users-only`; `TMPDIR=/tmp bash scripts/validate-podman-machine-readiness.sh --machine neovex-libkrun-users-only --connection neovex-libkrun-users-only --provider libkrun --tmp-root /tmp/podman --output-dir /tmp/neovex-libkrun-users-only-readiness`; `sed -n '1,220p' /tmp/neovex-libkrun-users-only-readiness/summary.txt`; `sed -n '1,220p' /tmp/neovex-libkrun-users-only-readiness/podman-info-connection.txt`; `sed -n '1,220p' /tmp/neovex-libkrun-users-only-readiness/podman-machine-ssh.txt`; `sed -n '1,220p' /tmp/neovex-libkrun-users-only-readiness/diagnostics/machine-log-tail.txt`; `curl -i -sS http://localhost:52368/vm/state`; `curl -i -sS -X POST http://localhost:52368/vm/state -H 'content-type: application/json' --data '{"state":"Stop"}'`; interrupt of the hanging `podman machine start` process; `TMPDIR=/tmp CONTAINERS_MACHINE_PROVIDER=libkrun podman machine list --all-providers --format json`; `ps -axww -o pid=,ppid=,stat=,command= | rg 'neovex-libkrun-users-only|krunkit|gvproxy'`; `cargo fmt --all --check` | Keep the short runtime dir as a required macOS machine-manager rule, but shift the next host experiment toward guest-image / Ignition / CoreOS boot diagnosis rather than more socket-length tuning |
| 2026-04-12 | V1 prep | `documented` | Ran the decisive comparison experiment against a brand-new disposable short-root machine. `neovex-libkrun-sr-fresh`, created from scratch with `TMPDIR=/tmp`, the same `libkrun` provider, and the same one-mount `/Users` layout, reached full readiness on this host: `podman machine start` exited successfully, the readiness bundle at `/tmp/neovex-libkrun-sr-fresh-readiness` reports both `podman --connection neovex-libkrun-sr-fresh info --debug` and `podman machine ssh neovex-libkrun-sr-fresh` as `ok`, and the guest log reaches `sshd.service`, `ready.service`, `Ignition: user-provided config was applied`, and the login prompt. The disposable machine was then stopped and removed cleanly, and `podman machine list` plus the process table returned to only the two long-lived libkrun machines with no stray `krunkit` / `gvproxy` processes. This changes the macOS diagnosis materially: short-root libkrun can succeed on this host, so the current failure in `neovex-libkrun-users-only` is much more likely stale/corrupted machine state than a universal provider or guest-image blocker. | `bash scripts/check-podman-machine-socket-paths.sh --machine neovex-libkrun-sr-fresh --tmp-root /tmp/podman`; `TMPDIR=/tmp CONTAINERS_MACHINE_PROVIDER=libkrun podman machine init --cpus 2 --memory 2048 --disk-size 20 -v /Users:/Users neovex-libkrun-sr-fresh`; `TMPDIR=/tmp CONTAINERS_MACHINE_PROVIDER=libkrun podman machine start neovex-libkrun-sr-fresh`; `TMPDIR=/tmp bash scripts/validate-podman-machine-readiness.sh --machine neovex-libkrun-sr-fresh --connection neovex-libkrun-sr-fresh --provider libkrun --tmp-root /tmp/podman --output-dir /tmp/neovex-libkrun-sr-fresh-readiness`; `sed -n '1,220p' /tmp/neovex-libkrun-sr-fresh-readiness/summary.txt`; `sed -n '1,220p' /tmp/neovex-libkrun-sr-fresh-readiness/podman-info-connection.txt`; `sed -n '1,220p' /tmp/neovex-libkrun-sr-fresh-readiness/podman-machine-ssh.txt`; `tail -n 60 /tmp/podman/neovex-libkrun-sr-fresh.log`; `TMPDIR=/tmp CONTAINERS_MACHINE_PROVIDER=libkrun podman machine stop neovex-libkrun-sr-fresh`; `TMPDIR=/tmp CONTAINERS_MACHINE_PROVIDER=libkrun podman machine rm -f neovex-libkrun-sr-fresh`; `TMPDIR=/tmp CONTAINERS_MACHINE_PROVIDER=libkrun podman machine list --all-providers --format json`; `ps -axww -o pid=,ppid=,stat=,command= | rg 'neovex-libkrun-sr-fresh|neovex-libkrun-users-only|krunkit|gvproxy'`; `cargo fmt --all --check` | Stop treating the short-root libkrun lane as generically broken on this Mac. The next macOS experiment should rebuild or replace the stale `neovex-libkrun-users-only` machine (and, if needed, its associated disk/EFI state) using the proven fresh-machine recipe, then re-run the readiness helper |
| 2026-04-12 | V1 prep | `documented` | Turned that fresh-machine recipe into a checked-in recreate/reset workflow and validated it against the real long-lived `users-only` machine. The repo now owns `scripts/recreate-podman-machine.sh`, `scripts/verify-podman-machine-recreate-helper.sh`, `make recreate-podman-machine`, and `make verify-podman-machine-recreate-helper`, so the short-root repair path no longer depends on manual shell history. A live run at `/tmp/neovex-libkrun-users-only-recreate` first preserved the stale-state signature under `/tmp/neovex-libkrun-users-only-recreate/pre-diagnostics/summary.txt` (`podman info --debug` failed with `status=125`, the API socket was missing, and the gvproxy socket was missing), then removed the machine, recreated it with `env TMPDIR=/tmp CONTAINERS_MACHINE_PROVIDER=libkrun podman machine init --cpus 2 --memory 2048 --disk-size 20 -v /Users:/Users neovex-libkrun-users-only`, restarted it with `env TMPDIR=/tmp CONTAINERS_MACHINE_PROVIDER=libkrun podman machine start neovex-libkrun-users-only`, and captured a post-recreate readiness bundle where both connection-targeted `podman info --debug` and `podman machine ssh` returned `ok`. This makes the macOS result materially stronger: on this host, short-root plus clean recreate is a working Podman-aligned recovery path for stale libkrun machine state. | `bash -n scripts/recreate-podman-machine.sh`; `bash -n scripts/verify-podman-machine-recreate-helper.sh`; `bash scripts/recreate-podman-machine.sh --help`; `bash scripts/verify-podman-machine-recreate-helper.sh`; `make verify-podman-machine-diagnostics-helper`; `make verify-podman-machine-socket-paths-helper`; `make verify-podman-machine-readiness-helper`; `make verify-podman-machine-recreate-helper`; `bash scripts/verify-crun-patch.sh ~/src/github.com/containers/crun`; `bash scripts/recreate-podman-machine.sh --machine neovex-libkrun-users-only --connection neovex-libkrun-users-only --provider libkrun --tmp-root /tmp/podman --output-dir /tmp/neovex-libkrun-users-only-recreate --cpus 2 --memory 2048 --disk-size 20 --volume /Users:/Users`; `sed -n '1,220p' /tmp/neovex-libkrun-users-only-recreate/summary.txt`; `sed -n '1,220p' /tmp/neovex-libkrun-users-only-recreate/pre-diagnostics/summary.txt`; `sed -n '1,220p' /tmp/neovex-libkrun-users-only-recreate/readiness/summary.txt`; `sed -n '1,120p' /tmp/neovex-libkrun-users-only-recreate/podman-machine-init-command.txt`; `sed -n '1,120p' /tmp/neovex-libkrun-users-only-recreate/podman-machine-start-command.txt`; `cargo fmt --all --check` | Keep the checked-in recreate helper as the first repair path for wedged short-root libkrun machines on macOS, and continue parallel Linux-host `LH1`-`LH6` work for the required patched-crun build/install and runtime proof |
| 2026-04-12 | V1 prep | `documented` | Added the missing Linux-host command bundle so the other laptop can run `LH1` through `LH6` with minimal judgment. The repo now owns `scripts/prepare-linux-vmm-validation-bundle.sh`, `scripts/verify-linux-vmm-validation-bundle-helper.sh`, `make prepare-linux-vmm-validation-bundle`, and `make verify-linux-vmm-validation-bundle-helper`. The new preparer emits a fixed `session.env`, numbered `commands/01...11...` scripts, a full `commands/00-run-through-lh6.sh` sequence, and `99-writeback-checklist.txt`, all rooted under one output directory. That closes a real operator gap in the control plane: the Linux host no longer has to translate the queue from prose into ad hoc shell history before producing the required `V1` and `V2` evidence. | `bash -n scripts/prepare-linux-vmm-validation-bundle.sh`; `bash -n scripts/verify-linux-vmm-validation-bundle-helper.sh`; `bash scripts/prepare-linux-vmm-validation-bundle.sh --help`; `bash scripts/verify-linux-vmm-validation-bundle-helper.sh`; `make verify-linux-vmm-validation-bundle-helper`; `cargo fmt --all --check` | Use the new bundle generator on the Linux host, run the emitted `commands/00-run-through-lh6.sh` or numbered scripts, and write the resulting artifact paths back into this plan as the next closeout step |
| 2026-04-12 | V1 prep | `documented` | Materialized the first real Linux-host execution bundle from the pinned upstream checkout on the current machine so the other laptop has an immediate handoff target, not just the generator source. Running `bash scripts/prepare-linux-vmm-validation-bundle.sh --crun-source ~/src/github.com/containers/crun --output-root /tmp/neovex-linux-vmm-validation` produced a stable bundle rooted at `/tmp/neovex-linux-vmm-validation`, including `session.env`, `README.md`, `commands/00-run-through-lh6.sh`, the numbered queue scripts under `commands/`, and `99-writeback-checklist.txt`. This host-local artifact is now the concrete Linux-laptop handoff path: the operator can copy or recreate that bundle and run the exact queue commands without reconstructing the workflow from the docs first. | `bash scripts/prepare-linux-vmm-validation-bundle.sh --crun-source ~/src/github.com/containers/crun --output-root /tmp/neovex-linux-vmm-validation`; observed outputs `bundle.session_env=/tmp/neovex-linux-vmm-validation/session.env`, `bundle.queue_runner=/tmp/neovex-linux-vmm-validation/commands/00-run-through-lh6.sh`, `bundle.checklist=/tmp/neovex-linux-vmm-validation/99-writeback-checklist.txt`; `cargo fmt --all --check` | Hand `/tmp/neovex-linux-vmm-validation` to the Linux host or rerun the same generator there, then execute `commands/00-run-through-lh6.sh` and write each resulting artifact path back into this plan |
| 2026-04-12 | V1+V2 | `done` | Completed the full Linux host validation queue (`LH1`-`LH6`) on Debian 13 x86_64. Updated the checked-in patch to target upstream crun `1.27` (was `1.22`). Built libkrun `1.17.4` from source (`~/src/github.com/containers/libkrun` tag `v1.17.4`) and libkrunfw `5.3.0` from source (`~/src/github.com/containers/libkrunfw` tag `v5.3.0`); both installed to `/usr/local/lib64/` with ldconfig entry at `/etc/ld.so.conf.d/libkrun.conf`. Built patched crun `1.27-dirty` with `+LIBKRUN` and installed at `/usr/libexec/neovex/crun`. Runtime separation verified: system crun `1.21` at `/usr/bin/crun`, Podman `5.4.2` using system runtime. Fixed `scripts/prepare-krun-bundle.sh` to remove the `network` namespace (TSI requires host network) and set `terminal: false` for non-interactive krun drills. Direct `crun run` inside `buildah unshare` proved: krun VM boot, TSI port binding (`18080:8080`), and HTTP connectivity to BusyBox httpd via TSI. Conmon lifecycle drill inside `buildah unshare` proved: `conmon → crun → libkrun VM` process tree with 8 worker threads, TSI port binding, exit file written (code `137` from SIGKILL), attach socket at bundle directory. Key findings: (1) krun containers require no network namespace in the OCI config because TSI handles networking via vsock; (2) conmon `--full-attach` holds `crun start` until an attach connection arrives — production Podman handles this, but the drill needed manual `crun start`; (3) the krun handler writes `.krun_config.json` to the rootfs during `crun create` via `openat2`, which requires the rootfs to be writable from the creating namespace — `buildah unshare` provides this; (4) libkrun and libkrunfw are not packaged for Debian 13 and must be built from source. | `bash scripts/prepare-linux-vmm-validation-bundle.sh --crun-source ~/src/github.com/containers/crun --output-root /tmp/neovex-linux-vmm-validation`; `bash scripts/verify-crun-patch.sh ~/src/github.com/containers/crun` (against `1.27`); `PKG_CONFIG_PATH=/usr/local/lib64/pkgconfig bash scripts/build-neovex-crun.sh --source ~/src/github.com/containers/crun --output /tmp/neovex-linux-vmm-validation/stage/crun --install-path /usr/libexec/neovex/crun --sudo-install`; `bash scripts/verify-runtime-separation.sh --system-runtime /usr/bin/crun --private-runtime /usr/libexec/neovex/crun`; `buildah unshare -- crun run --bundle /tmp/neovex-linux-vmm-validation/bundle neovex-http` (TSI port `18080` bound, HTTP `404` from BusyBox); `buildah unshare -- conmon ... + crun start neovex-http` (process tree `conmon→libkrun VM`, exit file `137`); `cargo fmt --all --check` | Close out the script gaps exposed during `LH1`-`LH6` execution, then start V3 |
| 2026-04-12 | V1+V2 closeout | `done` | Closed the five script gaps exposed during `LH1`-`LH6` execution. (1) `scripts/build-neovex-crun.sh` now auto-detects `libkrun.pc` in `/usr/local/lib64/pkgconfig` or `/usr/local/lib/pkgconfig` and sets `PKG_CONFIG_PATH` before `./configure --with-libkrun`, so the build no longer requires the caller to export the variable manually. (2) `scripts/check-vmm-host.sh` now checks for `libkrun.so` and `libkrunfw.so` via `ldconfig -p` and common non-standard paths, and verifies `libkrun.pc` via `pkg-config`. The distro-package checks for `libkrun`/`libkrunfw` are now informational (`optional`), so source-built installs no longer cause false failures. (3) `scripts/prepare-direct-krun-drill.sh` now emits a `buildah unshare` auto-re-exec preamble in the generated `run-runtime.sh`, so the krun handler's `openat2 .krun_config.json` succeeds in rootless mode without manual wrapping. (4) `scripts/prepare-conmon-krun-drill.sh` emits the same `buildah unshare` preamble in `run-conmon.sh`, plus a new `start-container.sh` script that polls `crun state` until the container reaches `created`, then calls `crun start` to boot the krun VM. This closes the conmon `--full-attach` lifecycle gap where the VM never started because nothing connected to the attach socket or called `crun start`. (5) Added `docs/research/krun-ci-build-and-distribution.md` capturing the full dependency map, source-build steps for libkrun/libkrunfw, GitHub runner requirements, key lifecycle learnings, and the gap between Debian (build from source) and Fedora (packaged) for CI targeting. All changed scripts pass `bash -n`, the three helper verifiers pass, and `cargo fmt --all --check` is clean. | `bash -n scripts/build-neovex-crun.sh`; `bash -n scripts/check-vmm-host.sh`; `bash -n scripts/prepare-krun-bundle.sh`; `bash -n scripts/prepare-direct-krun-drill.sh`; `bash -n scripts/prepare-conmon-krun-drill.sh`; `bash scripts/check-vmm-host.sh` (reports `supported` on this host with source-built libkrun); `bash scripts/verify-krun-bundle-helper.sh`; `bash scripts/verify-direct-krun-drill-helper.sh`; `bash scripts/verify-conmon-krun-drill-helper.sh`; `cargo fmt --all --check` | Start V3 `neovex-sandbox` krun backend implementation now that V1, V2, and the supporting script fixes have concrete Linux host evidence |
| 2026-04-12 | V3 | `in_progress` | Landed the first backend-owned `neovex-sandbox` krun implementation slice. `SandboxSpec` now carries generic filesystem, process, and published-port intent via `SandboxFilesystemSpec`, `SandboxProcessSpec`, and `SandboxPortBinding`. `crates/neovex-sandbox/src/backends/krun/` now contains `bundle.rs` for OCI config generation with `run.oci.handler=krun` and `krun.port_map`, `buildah.rs` for backend-local buildah command assembly and `buildah unshare` wrapping, `conmon.rs` for deterministic `conmon -> /usr/libexec/neovex/crun` launch planning, `command.rs` for reusable backend-local command specs, and `vm.rs` for manifest-backed `start` / `inspect` / `stop` lowering behind the generic `SandboxBackend` trait. The backend now also distinguishes deliberate shutdown from unexpected crash semantics, so a forced stop can still surface as `SandboxStatus::Stopped`. The repo-owned Linux smoke path is now checked in via `crates/neovex-sandbox/tests/krun_linux_smoke.rs` plus `docs/reference/krun-sandbox-backend-smoke.md`; the missing closeout step is running that path on a supported Linux host and writing the resulting connectivity, log-persistence, and restart-survival evidence back into this plan. | `cargo fmt --all --check`; `cargo check -p neovex-sandbox -p neovex`; `cargo test -p neovex-sandbox` | Continue V3 on a supported Linux host by running the smoke test and recording evidence |
| 2026-04-12 | V3 | `done` | Ran the real Linux-host Rust backend smoke path on Debian 13 x86_64 and closed three issues in the process. (1) `bundle.rs` was missing the `mounts` block from the generated OCI config — `crun` requires at least the standard Linux mount set (`/proc`, `/dev`, `/dev/pts`, `/dev/shm`, `/dev/mqueue`, `/sys`, `/sys/fs/cgroup`). Added `default_linux_mounts()`. (2) The smoke test had a compile error: `guest_port.to_string()` returned `String` inside a `[&str; 5]` array literal. Fixed with a `let` binding. (3) The smoke test's HTTP probe used `read_to_string` after `shutdown(Write)` — TSI drops the vsock connection on half-close, producing empty reads. Fixed to use `HTTP/1.0`, no write-shutdown, and `read` with a fixed buffer and read timeout. After these three fixes the smoke test passes reproducibly in ~6s. Observed evidence: sandbox id `http-smoke-01kp1w86xbcyw96ph6f932asx3`, rootfs `/tmp/neovex-sandbox-smoke-rootfs` (extracted BusyBox via `buildah unshare`), workdir `/tmp/neovex-sandbox-smoke`, bundle at `workdir/bundles/<id>/config.json` with 7 mounts and `krun.port_map: 18080:8080`, conmon pid written, TSI port `*:18080` bound by `libkrun VM`, HTTP probe returned BusyBox httpd `404 Not Found` response, fresh `KrunSandboxBackend` instance recovered the running sandbox from the persisted manifest, stop succeeded with exit code `137` (SIGKILL), `ctr.log` and `oci.log` present under `workdir/state/containers/<id>/`. Phase-close verification: `cargo fmt --all --check`, `cargo check -p neovex-sandbox -p neovex`, `cargo test -p neovex-sandbox` (9 pass), and the ignored smoke test all pass. V3 is complete: the Rust backend boots a real VM through the generic `SandboxBackend` trait, proves host-to-guest connectivity over TSI, survives backend restart via manifest recovery, and records stop/exit outcomes. The next promoted slice is `M1` from `docs/plans/microvm-runtime-plan.md`. | `cargo test -p neovex-sandbox krun_backend_smoke_boots_http_service_and_survives_backend_restart -- --ignored --nocapture` with `NEOVEX_KRUN_SMOKE_ROOTFS=/tmp/neovex-sandbox-smoke-rootfs NEOVEX_KRUN_SMOKE_WORKDIR=/tmp/neovex-sandbox-smoke NEOVEX_KRUN_SMOKE_RUNTIME=/usr/libexec/neovex/crun NEOVEX_KRUN_SMOKE_CONMON=/usr/bin/conmon NEOVEX_KRUN_SMOKE_BUILDAH=/usr/bin/buildah NEOVEX_KRUN_SMOKE_HOST_PORT=18080 NEOVEX_KRUN_SMOKE_GUEST_PORT=8080`; `cargo fmt --all --check`; `cargo check -p neovex-sandbox -p neovex`; `cargo test -p neovex-sandbox` | Promote `M1` from `docs/plans/microvm-runtime-plan.md` now that V1, V2, and V3 are complete |
