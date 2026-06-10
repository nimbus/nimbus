# Plan: Nimbus-libkrun Snapshot Port (Archived)

> **Superseded 2026-05-27 by [`docs/plans/nimbus-sandbox-plan.md`](../nimbus-sandbox-plan.md) Band S.**
> The unified sandbox plan rolls P0–P5 into Band S (S0–S5) on the same
> `nimbus-libkrun` fork. Phase content was lifted verbatim; this file is
> retained as a baseline for the unified plan's Band S section.

Port snapshot/restore capability into the `nimbus-libkrun` fork so a single
VMM family covers every Nimbus sandbox workload (Lambda-style invocation,
agentic desktop sessions, GPU inference) through capability profiles rather
than separate VMMs.

## Status

- **Status:** `proposed`
- **Activation precondition:** none beyond ratification of the unified-lift
  decisions in `docs/plans/research/vmm-landscape-2026.md` (D1-D10).
- **Primary goal:** add snapshot/restore plus a sub-millisecond
  fork-on-demand primitive to `nimbus-libkrun` without giving up libkrun's
  existing virtio-fs, virtio-gpu (Venus + native-context), virtio-input, and
  virtio-snd device wiring.
- **References:**
  - `docs/plans/research/vmm-landscape-2026.md` (decision baseline; D1-D10)
  - `docs/plans/research/libkrun-session-sandbox.md`
  - `docs/plans/research/gpu-sandbox-backends.md`
  - `docs/plans/research/computer-use-capabilities-audit.md`
  - `docs/plans/computer-use-sandbox-plan.md` (desktop profile consumer)
  - `docs/plans/gpu-accelerated-sandbox-plan.md` (GPU profile consumer)
  - `docs/architecture/sandbox/microvm-service-baseline.md`
  - Firecracker `snapshot-support.md` and `src/vmm/src/persist.rs`
    (`~/src/github.com/firecracker-microvm/firecracker`, head `eaa62396d`,
    Apache-2.0)
  - zeroboot prototype (`~/src/github.com/zerobootdev/zeroboot`, Apache-2.0)
  - libkrun v1.18.1 (`~/src/github.com/nimbus/nimbus-libkrun`, branch
    `nimbus/v1.18.1`)

## Decision Summary

This plan executes decision D4 from `vmm-landscape-2026.md`: a phased
snapshot-port lift, owned in `nimbus-libkrun`, that closes the gap between
libkrun's device/GPU breadth and Firecracker's snapshot/restore mechanism.
The Firecracker plan it replaces (`firecracker-snapshot-invocation-backend-plan.md`,
archived 2026-05-27) is subsumed here.

## Deployment Topology Scope (D11 + D12)

**All snapshot/fork mechanism work in P0–P5 targets Linux KVM only.**
The plan ships no macOS HVF code path. This is not "deferred" — the
deployed Nimbus topology has no HVF consumer for the snapshot path:

- **Linux hosts** (every production class): per-request workloads run as
  direct libkrun-on-KVM microVMs. P0–P5 land here. This is where the
  snapshot mechanism, UFFD lazy loading, and MAP_PRIVATE fork primitive
  actually execute.
- **macOS dev hosts:** krunkit (libkrun-on-HVF) boots **one** outer
  machine-os Linux VM
  (`ghcr.io/nimbus/machine-os:v0.1.30`, pinned 2026-05-14). That outer
  VM is single-instance and long-lived per developer environment; it
  does not need snapshot/restore or sub-ms fork. Per-service workloads
  inside the outer VM run as **standard Linux containers** managed by
  the guest machine API
  (`docs/architecture/sandbox/macos-machine-flow.md` §"Flow 6"), not as
  nested libkrun microVMs. macOS dev gets crun's ~10–50 ms container
  cold start; production parity for fork-semantics is enforced via
  Linux CI.

Implementation implication: CI matrix and microbenchmarks for P0–P5
target Linux KVM only. Existing macOS proof helpers
(`make collect-nimbus-machine-cli-proof`,
`make collect-nimbus-machine-guest-proof`,
`make collect-nimbus-machine-service-proof`,
`make collect-nimbus-homebrew-cask-proof`) continue to cover the
outer-VM lifecycle but do **not** exercise the snapshot/fork code
paths added by this plan.

## Why A Snapshot Port, Not A Re-platform

libkrun and Firecracker share part of the `rust-vmm` substrate —
specifically `kvm-ioctls` 0.22, `vm-memory` 0.17, and `linux-loader`
0.13.2 (verified in `~/src/github.com/nimbus/nimbus-libkrun/src/*/Cargo.toml`).
libkrun does **not** depend on the rust-vmm `virtio-queue` or `vm-superio`
crates; it carries its own forked virtio queue impl at
`src/devices/src/virtio/queue.rs`. libkrun's `snapshot-support.md`
equivalent does not exist yet (libkrun upstream issue #67, closed
2022 without implementation); Firecracker's does. The mechanism is
portable because the underlying KVM state surface (`KVM_GET_REGS`,
`KVM_GET_SREGS`, `KVM_GET_LAPIC`, `KVM_GET_MSRS`, `KVM_GET_XSAVE`) and
`MAP_PRIVATE` memory-backing pattern are not Firecracker-specific. The
device-side serializers must be ported against libkrun's forked queue
impl, not lifted verbatim. Porting the mechanism keeps libkrun's
superset of devices intact while gaining Lambda-grade restore latency.

## Sizing Convention

Each phase is sized as `~X LoC net new + ~Y LoC tests` against the
`nimbus-libkrun` fork. SWAG can swing 30-50%; figures are concrete enough
to falsify when the code lands. See
[[feedback_engineering_sizing_loc_swag]] for the framing.

## Phases

### P0 — Scoping spike: vCPU + memory snapshot prototype on libkrun

Stand up a minimum-viable snapshot/restore round-trip on a stripped libkrun
guest (no devices beyond `serial`, no virtio surface). Goal is to falsify
or confirm the port assumption before committing to the full lift.

- Wire `KVM_GET_REGS` / `KVM_GET_SREGS` / `KVM_GET_LAPIC` / `KVM_GET_MSRS` /
  `KVM_GET_XSAVE` capture on a paused libkrun vCPU thread.
- Write captured state + a copy of guest memory to disk; restore into a
  freshly-launched libkrun process and resume.
- One end-to-end test that asserts a guest register value (e.g., `rip`)
  survives the round-trip.
- **SWAG: ~500 LoC net new (test counts inline).**

**Exit criterion:** a paused minimal guest can be resumed in a new libkrun
process with `rip` and a memory marker intact.

### P1 — vCPU + memory snapshot/restore (no devices)

Productionize the P0 spike behind a stable wire format.

- Define `SnapshotV1` envelope (header + version + payload sections) modeled
  on Firecracker's `MicrovmState` (`src/vmm/src/persist.rs`).
- Implement Save/Restore for: VM state (TSC, CPUID), all vCPUs, GIC (arm64),
  IRQ chip (x86_64), MSRs.
- Memory: serialize via the existing `vm-memory` backing-file path; restore
  by re-mapping the file with `MAP_SHARED` or `MAP_PRIVATE` (choice deferred
  to P4).
- UFFD eager-load mode (no lazy faults yet — see P5).
- Round-trip tests for boot → workload → pause → snapshot → restore → resume,
  asserting workload progress.
- **SWAG: ~1,800 LoC net new + ~500 LoC tests.**

**Exit criterion:** a device-less guest running a CPU-bound workload
survives snapshot/restore with bit-exact register and memory state.

### P2 — Simple device Save/Restore

Cover libkrun's simple virtio devices using Firecracker's Save/Restore
trait pattern. These devices have small, well-shaped state (queue indices,
config-space scratch, in-flight descriptors flushed at pause).

- Devices in scope: `virtio-block`, `virtio-net`, `virtio-vsock`,
  `virtio-rng`, `virtio-balloon`, `virtio-pmem`.
- Add `Persist` trait, port Firecracker's `MmioTransport` save/restore for
  libkrun's transport.
- Per-device unit tests + an integration test that boots a guest with one
  of each device, snapshots, restores, and asserts queue continuity.
- **SWAG: ~1,400 LoC net new + ~500 LoC tests.**

**Exit criterion:** a guest with the full simple-device set survives
snapshot/restore without packet/block-IO duplication or loss on restore.

### P3 — virtio-fs snapshot + re-init-on-restore for GPU/input/snd

The hardest devices to serialize (virtio-gpu Venus/native-ctx,
virtio-input, virtio-snd, virtio-fs) hold large external state (GPU
command rings, input device registrations, audio backend handles, FUSE
inode-generation + open-fd tables). libkrun's virtio-fs is **in-process
passthrough** (`src/devices/src/virtio/fs/linux/passthrough.rs`,
~2,200 LoC on Linux; ~2,500 LoC on macOS) — not an out-of-process
virtiofsd. Decision D5/D6 (revised after verification 2026-05-27):
**re-init on restore** for all four devices. Payload data on virtio-fs
lives on the host filesystem and survives the round-trip; only the
in-process FUSE state is dropped and re-established.

- virtio-fs (in-process passthrough): on restore, re-mount each tag
  from a clean state; agent userspace re-opens any in-flight files.
  Idempotent on host data.
- virtio-gpu (Venus + native-context): on restore, re-create the Venus
  renderer and replay the last known mode-set. Guest userspace must
  handle `VK_ERROR_DEVICE_LOST` or equivalent context loss; this is
  the same recovery path as a real GPU hot-unplug.
- virtio-input: re-register input devices; drop in-flight events.
- virtio-snd: re-open the host audio backend; mute briefly during
  recovery.
- Guest-side ABI: a `nimbus-init` hook in the guest that drives the
  re-init dance idempotently.
- Tests cover restore from snapshot with each device active, asserting
  that the guest reaches a usable state within a bounded recovery
  window. Include one test that snapshots a session with an active
  GPU-accelerated browser render, to characterize Vulkan/GL context-
  loss handling in the wild.
- **SWAG: ~1,050 LoC net new + ~600 LoC tests.**

**Exit criterion:** a desktop-profile guest survives snapshot/restore
with working display, input, audio, and shared-filesystem mounts within
a bounded recovery window. Guest userspace that cannot tolerate GPU
context loss is documented as a known limitation.

### P4 — MAP_PRIVATE fork-on-demand primitive (zeroboot port)

Port the zeroboot mechanism: a parent template VM is paused with its
memory backed by `MAP_PRIVATE` on a sealed file; child VMs are forked by
spawning a new libkrun process that maps the same file `MAP_PRIVATE`,
inheriting the parent's memory via copy-on-write at the kernel level.

- Add a `forkable` snapshot mode that seals the memory backing file and
  the device-Save metadata.
- Add `libkrun_session_fork(template_handle) -> session_handle` API that
  spawns a child sharing the parent's `MAP_PRIVATE` backing file.
- vCPU state: clone from template, scribble a per-child nonce into a
  reserved guest page (entropy seed, hostname, etc.).
- Microbenchmark: p50 spawn time + per-child resident memory. The
  zeroboot baseline (0.79 ms p50 / 265 KB per sandbox) is on minimal
  serial-only guests. A full desktop-profile session (virtio-gpu /
  virtio-fs / virtio-input / virtio-snd active) will fork more slowly;
  target is "within one order of magnitude," not parity.
- **SWAG: ~1,150 LoC net new + ~400 LoC tests.**

**Exit criterion:** `libkrun_session_fork` returns a running child guest
in under ~10 ms p50 for the desktop profile and under ~3 ms p50 for the
lambda profile on a Linux dev box, with per-child RSS overhead in the
low MB / low hundreds of KB respectively.

### P5 — Diff snapshots + UFFD page-fault loading

Add the latency wins that depend on the snapshot wire format being stable.

- Diff snapshots: capture only dirty pages since a base snapshot
  (KVM dirty-page bitmap).
- UFFD lazy page-fault loading: serve pages on demand from the snapshot
  file instead of eager-loading at restore.
- Restore-latency microbench: full snapshot vs. diff vs. UFFD lazy on a
  representative agentic workload.
- **SWAG: ~1,150 LoC net new + ~400 LoC tests.**

**Exit criterion:** UFFD lazy restore brings cold-restore latency under
the Firecracker reference number (~125 ms for a 128 MB guest) on the same
hardware class.

## Total

- **~7,050 LoC net new + ~2,400 LoC tests = ~9,450 LoC across the fork.**
- Phases are individually shippable; P0-P2 are sufficient for the
  Lambda-style invocation profile; P3 is required for the desktop profile;
  P4 unlocks the sub-ms fork product story; P5 is pure latency optimization.

## Cross-Plan Boundaries

- `docs/plans/computer-use-sandbox-plan.md` consumes the **desktop profile**
  produced by P3.
- `docs/plans/gpu-accelerated-sandbox-plan.md` consumes the **GPU profile**
  produced by P3 (re-init on restore is the same primitive).
- Lambda-style invocation workloads consume P0-P2 + P4.
- This plan owns the fork-side surface; consumer plans own the Nimbus-side
  capability profiles, packaging, and product UX.

## License Composition

All ingredients are permissive and mutually compatible — verified
2026-05-27 against upstream and the local fork. No LGPL relinking
constraint applies.

- libkrun base: **Apache-2.0**
  (`~/src/github.com/nimbus/nimbus-libkrun/LICENSE` and upstream
  `containers/libkrun/LICENSE` both declare Apache-2.0;
  `krun-sys/Cargo.toml` declares `license = "Apache-2.0"`).
- Firecracker patterns lifted into the fork: Apache-2.0.
- zeroboot primitive: Apache-2.0
  (`zerobootdev/zeroboot/LICENSE` declares Apache-2.0).
- muvm-derived bits: MIT
  (`AsahiLinux/muvm/crates/muvm/Cargo.toml` declares
  `license = "MIT"`; the repo has no root LICENSE file, so the
  Cargo.toml SPDX is authoritative for now).

See [[feedback_apache_license_posture]] — Apache-2.0 upstream code is
freely incorporable.

## Risk Register

- **Device-state escape hatches break under live workloads.** Mitigation:
  P3 acceptance includes a stress test that snapshots a guest with active
  GPU/input/audio rather than idle ones.
- **MAP_PRIVATE primitive interacts badly with in-process virtio-fs FUSE
  handles.** Mitigation: P4 lands after P3; the fork primitive triggers
  a virtio-fs re-init on the child (host filesystem data persists; only
  the in-process FUSE state resets).
- **GPU device-lost handling in guest userspace is uneven.** Vulkan apps
  that don't handle `VK_ERROR_DEVICE_LOST` will crash on restore. Most
  CLI inference loops (`llama.cpp`, `whisper.cpp`) are fine; browser GL
  contexts and stateful render pipelines are not. Mitigation: document
  the limitation; offer `snapshot --quiesce-gpu` flag in P3 that pauses
  the agent before snapshotting so the guest can drain GPU work.
- **Sub-ms fork target may not hold for full desktop sessions.**
  zeroboot's 0.79 ms is on minimal serial-only guests. Mitigation: the
  P4 exit criterion has tier-specific thresholds (lambda profile
  ~3 ms p50; desktop profile ~10 ms p50); fall-back is still 10×
  better than cold boot.
- **Snapshot wire-format churn during P5.** Mitigation: freeze `SnapshotV1`
  at end of P2; diff/UFFD changes ride a `SnapshotV2` capability flag.
- **Upstream libkrun rebase friction.** Mitigation: keep each phase as a
  patch series that rebases cleanly on `nimbus/v1.18.1`; track upstream
  release cadence in `docs/plans/research/vmm-landscape-2026.md`.

## Open Follow-Ups

- KVM PV-clock save/restore semantics on Linux 6.x. Validate during P1.
- macOS HVF parity for snapshot/restore: **out of scope by construction**
  per D11. The macOS deployment topology runs a single long-lived outer
  VM via libkrun-on-HVF; there is no per-request HVF consumer for the
  snapshot mechanism. Re-open only if D12 is revisited (nested
  libkrun-on-libkrun on Apple Silicon for production fork-parity in dev).
- Snapshot signing / attestation: integrate with Nimbus artifact admission
  in a follow-on plan once P2 lands.
