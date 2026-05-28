---
status: research
owners: sandbox-tier-roadmap
related:
  - docs/plans/nimbus-sandbox-plan.md
  - docs/plans/archive/nimbus-libkrun-snapshot-port-plan.md
  - docs/plans/archive/firecracker-snapshot-invocation-backend-plan.md
  - docs/plans/archive/computer-use-sandbox-plan.md
  - docs/plans/archive/gpu-accelerated-sandbox-plan.md
  - docs/plans/research/libkrun-session-sandbox.md
  - docs/plans/research/gpu-sandbox-backends.md
  - docs/plans/research/nimbus-libkrun-fork-inventory.md
  - docs/plans/research/computer-use-capabilities-audit.md
  - docs/plans/agent-browser-service-plan.md
---

# VMM Landscape 2026 — Evidence Base for the Unified-Lift Sandbox Roadmap

This doc resolves the recurring "could we just use one VMM for everything"
question by recording the May 2026 state of the rust-vmm VMM family
(Firecracker, Cloud Hypervisor, libkrun) and the Firecracker-derivative
AI-sandbox projects (zeroboot, Fly.io Sprites, Northflank's Kata
layering). Decisions D1–D12 in §9 are the durable record; cite them from
consumer plans rather than restating the evidence.

## 1. Scope

- Per-VMM capability state (devices, snapshot, host platforms, license).
- Lineage between VMMs — they are not three independent siblings.
- Production-platform mapping — who actually ships what in 2026.
- Decision implications for the unified-lift roadmap (D1–D12 below).
- Per-host deployment topology (Linux KVM vs macOS HVF) and what each
  host actually runs in production (D11, D12).

Out of scope: gVisor (not a VMM), QEMU microvm (not rust-vmm family),
CRIU-only checkpointing (orthogonal).

## 2. Method

- Local checkouts read at session start:
  - `~/src/github.com/firecracker-microvm/firecracker` head
    `eaa62396d` dated 2026-05-26.
  - `~/src/github.com/nimbus/nimbus-libkrun` branch
    `nimbus/v1.18.1` over upstream `stable-1.18.x`.
  - `~/src/github.com/containers/crun` tag `1.27.1` (2026-04-20).
- Upstream READMEs and changelogs fetched: libkrun v1.18.1, Cloud
  Hypervisor v52.0, zeroboot main, Firecracker mainline charter +
  CHANGELOG.
- Industry production posture cross-checked against Northflank's 2026
  sandbox comparison blog series, Fly.io Sprites architecture notes,
  Modal docs, AWS Lambda public guidance.

## 3. Lineage — these are not three siblings

```
rust-vmm crates
  (kvm-ioctls, kvm-bindings, vm-memory, virtio-queue, linux-loader,
   vm-superio, vmm-sys-util, vhost, virtio-gen, ...)
        |
        ↓
   Firecracker (AWS, Apache-2.0)
        |
        ├──→ Cloud Hypervisor (Intel/IBM + community, Apache-2.0)
        |
        └──→ libkrun (Red Hat / containers org, Apache-2.0)
                ↑
                └── also absorbs code from Cloud Hypervisor
```

libkrun depends on a **subset** of the rust-vmm crate family at the
Cargo.toml level: `kvm-ioctls` 0.22, `vm-memory` 0.17, `linux-loader`
0.13.2 (verified across `src/{libkrun,cpuid,smbios,vmm,arch,kernel,devices}/Cargo.toml`
on the `nimbus/v1.18.1` branch). It carries its own forked virtio queue
implementation at `src/devices/src/virtio/queue.rs` rather than
depending on the rust-vmm `virtio-queue` crate, and it does not use
`vm-superio`. This matters for the snapshot port plan: device-side
serializers must be designed against libkrun's queue impl, not against
upstream rust-vmm types.

libkrun's upstream README states verbatim: libkrun "incorporates code
from Firecracker, rust-vmm and Cloud-Hypervisor." This matters for two
reasons:

1. Porting Firecracker's snapshot machinery into libkrun is lineage-
   consistent, not a clean-room exercise.
2. Bug fixes and security patches sometimes flow across all three; we
   should track Firecracker security advisories even though Nimbus's
   unified backend is libkrun-based (lifting Firecracker snapshot
   patterns per D4).

## 4. Per-VMM evidence (May 2026)

### 4.1 Firecracker

- Head `eaa62396d` (2026-05-26). Apache-2.0. Release cadence stable
  since v1.0; current line is 1.15.
- **Charter is binding.** `docs/CHARTER.md`: *"Minimalist in Features:
  If it's not clearly required for our mission, we won't build it."*
  No GPU, no sound, no in-tree filesystem. This will not change.
- **Device set** (`src/vmm/src/devices/virtio/`):
  `balloon, block, net, pmem, rng, vsock`.
- **vhost-user wiring exists** (`vhost_user.rs`, `block/vhost_user/`).
  Out-of-process device emulation is mainline. A future `virtiofsd`
  daemon could be attached without forking Firecracker. This is the
  release valve for the charter's no-fs-in-tree rule.
- **Snapshot/restore** — production-quality since v0.23 (2020). Stable
  wire format. Diff snapshots in "developer preview" being unified with
  Linux `guest_memfd`. UFFD page-fault on-demand loading supported.
  `MAP_PRIVATE` copy-on-write of the memory snapshot file is the
  default loading mode.
- **Spec guarantees** (`SPECIFICATION.md`): VMM start to guest
  `/sbin/init` ≤ 125 ms; per-VMM memory overhead ≤ 5 MiB; supports 5
  microVM creations / host-core / sec.
- **Recent direction (2025–2026 CHANGELOG)**: PCI virtio + device
  hotplug (dev preview), virtio-mem (memory hotplug), virtio-pmem,
  virtio-balloon free-page reporting + hinting, VMClock device for
  snapshot-aware guest clocks, FIPS-mode guest kernels, `rng-seed` FDT
  node on aarch64, vsock UDS path renaming across snapshot restore,
  per-callsite log rate limiting.
- **Platforms**: Linux KVM only; x86_64 + aarch64. No macOS. No
  Windows.
- **Jailer**: first-class seccomp + chroot + cgroup wrapper; production
  hardening profile.
- **Production users**: AWS Lambda + Fargate (origin); Fly.io Sprites;
  Koyeb; Northflank (via Kata Containers); appfleet; firecracker-
  containerd; Kata; Flintlock; Qovery; webapp.io; UniK; microvm.nix.

### 4.2 libkrun

- v1.18.1 (2026-05-20). **Apache-2.0** (corrected 2026-05-27 against
  upstream `containers/libkrun/LICENSE` and the local fork's `LICENSE`
  + `krun-sys/Cargo.toml`; earlier internal notes calling it LGPL-2.1
  were wrong).
- **Devices** (per upstream README):
  `virtio-console, virtio-block, virtio-fs, virtio-gpu (venus +
  native-context), virtio-net, virtio-vsock, virtio-balloon (free-page
  reporting only), virtio-rng`. **No virtio-snd or virtio-input in
  upstream README** — both have been discussed in issues; mainline state
  to confirm before relying on either. Audio in particular is a
  follow-on per `nimbus-libkrun-fork-inventory.md` §6.2.
- **Snapshot/restore — not in mainline, not on any branch, not on the
  upstream roadmap.** Major asymmetry vs Firecracker. Nimbus closes
  this in-fork via D4 (`docs/plans/nimbus-sandbox-plan.md` Band S).
- **Networking**: TSI (Transparent Socket Impersonation, userspace
  network stack) for unprivileged use, plus passt support. Nimbus's
  only functional fork patch (`15bcf49`) is a passt-mode bind-address
  hook.
- **macOS host**: HVF backend works on Apple Silicon via krunkit;
  requires macOS 14+.
- **Form factor**: `libkrun.so` linked into the host process — not a
  standalone binary. Reduces context-switch overhead and simplifies
  unprivileged invocation but means host process IS the VMM trust
  surface.
- **Production users**: Red Hat krun OCI runtime; Asahi muvm; Nimbus
  (planned via `nimbus-libkrun` fork).

### 4.3 Cloud Hypervisor

- v52.0 (2026-05-14). Apache-2.0.
- **Devices**: `virtio-{net, block, pmem, fs, vsock}` + VFIO
  passthrough + CPU hotplug. **No virtio-gpu, virtio-snd, or
  virtio-input in mainline.**
- **Snapshot/restore**: exists but **"not supported across different
  versions"** — wire format is version-locked. Less mature than
  Firecracker's stable wire format. Live migration similarly version-
  locked.
- **macOS host**: separate `cloud-hypervisor/hypervisor-framework` Rust
  binding crate exists for Apple Silicon HVF, but integration with the
  main VMM is not advertised as production. MSHV (Microsoft Hypervisor)
  is the main non-KVM target.
- **Production users**: Northflank (mixed with Firecracker and gVisor
  under Kata Containers).

### 4.4 zeroboot (Firecracker-derivative AI sandbox)

- Apache-2.0. Working prototype, ~3 months mature in 2026.
- **Not a Firecracker fork — uses Firecracker stock.**
- **Mechanism**: one-time Firecracker boot → memory + CPU snapshot;
  each fork = new KVM VM + `mmap(MAP_PRIVATE)` snapshot pages + restore
  CPU state. The fork primitive is the load-bearing IP.
- **Spawn latency p50: 0.79 ms.** Python fork+exec ~8 ms.
- **Memory per sandbox: ~265 KB.**
- **Device set inside fork: serial I/O only. No network.**
- **API**: HTTP REST, Bearer auth; Python + TypeScript SDKs.
- **Honest about maturity**: own README labels it a "working prototype,
  not production-hardened yet."
- **License posture**: Apache-2.0 → incorporable into Nimbus per
  repo memory note `feedback_apache_license_posture.md`.

## 5. Production landscape (2026)

| Platform | VMM | Workload shape | Notes |
| --- | --- | --- | --- |
| AWS Lambda | Firecracker | Function invocation | Origin; trillions of invocations |
| AWS Fargate | Firecracker | Container | Long-lived microVM per task |
| Fly.io Sprites | Firecracker | Pause/resume IDE/dev | NVMe-backed persistent FS, pause/resume model |
| Koyeb | Firecracker | App/service hosting | GPU still preview |
| Northflank | Kata + {Firecracker, Cloud Hypervisor, gVisor} | Workload-dependent | "Strongest isolation lineup" per their pitch |
| Modal | gVisor | Function execution | Not a VMM — userspace syscall filter; 20k concurrent containers, sub-second cold starts |
| zeroboot | Firecracker | AI agent code execution | OSS sub-ms fork-on-demand |
| Asahi muvm | libkrun | Desktop apps on Asahi Linux | Unprivileged; GPU |
| Red Hat krun | libkrun | OCI runtime | Containers org |
| Nimbus (planned) | nimbus-libkrun (unified fork) | Capability-profile sandbox backend | This roadmap (D1–D12) |

The industry split-by-workload pattern is the dominant shape in 2026:
**fast-cold-start serverless ⇒ Firecracker; full desktop / GPU /
macOS-native ⇒ libkrun.** Nobody mainline-ships serverless on libkrun;
nobody mainline-ships desktop / GPU on Firecracker. Nimbus's unified-lift
roadmap (D1) closes that split inside a single fork by porting
Firecracker's snapshot mechanism into libkrun — the first rust-vmm-family
path that targets both shapes from one codebase.

Production-host topology footnote: Nimbus's per-request fork-fast
workloads run on **Linux KVM** (every production host class). macOS dev
hosts run an outer machine-os Linux VM via libkrun-on-HVF (krunkit) and
schedule per-service workloads as standard Linux containers **inside**
that outer VM, not as nested microVMs. See D11 + D12.

## 6. Capability matrix (mainline, 2026-05)

| Capability | Firecracker | libkrun | Cloud Hypervisor |
| --- | --- | --- | --- |
| KVM (Linux host) | ✓ | ✓ | ✓ |
| MSHV (Windows host) | ✗ | ✗ | ✓ |
| HVF (macOS host) | ✗ | ✓ (production via krunkit) | ◑ (separate crate, experimental integration) |
| Snapshot/restore | ✓ stable wire format, prod-grade | ✗ | ◑ version-locked, weaker |
| UFFD page-fault on-demand loading | ✓ | ✗ | ◑ |
| Jailer (seccomp+chroot+cgroup) | ✓ first-class | ◑ userns+seccomp inline | ◑ |
| virtio-net | ✓ TAP | ✓ TSI or passt | ✓ |
| virtio-block | ✓ + vhost-user | ✓ | ✓ |
| virtio-vsock | ✓ | ✓ | ✓ |
| virtio-fs | ✗ in-tree, ◑ via vhost-user | ✓ first-class | ✓ |
| virtio-gpu | ✗ (charter-blocked) | ✓ Venus + native-context | ✗ |
| virtio-snd | ✗ (charter-blocked) | ◑ in-progress / not in README | ✗ |
| virtio-input | ✗ (charter-blocked) | ✓ | ✗ |
| PCI virtio + hotplug | ✓ (dev preview) | ✗ | ✓ |
| Memory hotplug (virtio-mem) | ✓ | ✗ | ✓ |
| Cold boot to `/sbin/init` | ≤ 125 ms | multi-second | ~100 ms+ |
| Snapshot restore | ~10 ms warm path | n/a | ~100 ms |
| Template-fork (mmap MAP_PRIVATE) | ✓ (via snapshot loading); zeroboot proves sub-ms | n/a | ◑ untested |
| Production users on critical-path | AWS, Fly.io, Koyeb | Asahi, Red Hat | Northflank (mixed) |

Legend: ✓ supported and mature, ◑ experimental/conditional, ✗ unsupported or charter-blocked.

## 7. The Firecracker vhost-user observation

Mainline Firecracker carries vhost-user support
(`src/vmm/src/devices/virtio/vhost_user.rs`,
`src/vmm/src/devices/virtio/block/vhost_user/`). Out-of-process device
emulation IS wired. A future `virtiofsd` daemon could be attached
without modifying Firecracker. The charter's "no fs/gpu/snd in
Firecracker proper" rule applies to in-tree devices only and doesn't
block vhost-user delegated devices.

Implication (historical, pre-D2): under the dropped "Firecracker for
Lambda" framing, warm bundle delivery via virtiofs (e.g., a multi-GiB
Python wheel cache shared across forks) would have used "run
virtiofsd alongside Firecracker over vhost-user-fs," not a Firecracker
fork. The unified-lift decision (D1/D2) made this moot: the lambda
profile rides the same libkrun-based backend with native in-process
virtio-fs (D6).

## 8. zeroboot mechanism deep-dive

The zeroboot fork primitive is small enough to describe exactly:

```
Setup (once per template):
  1. Firecracker boots a base VM.
  2. Pre-load the runtime (Python interpreter, Node binary, etc.).
  3. Pause + snapshot memory + CPU state to disk.
  4. Memory file becomes the read-only template page set.

Fork (per invocation, ~0.8 ms):
  1. Create a new KVM VM (vCPU, memory slot).
  2. mmap(MAP_PRIVATE, template_memory_file) → backing pages.
       └─ Reads come from template; writes diverge per-fork (CoW).
  3. KVM_SET_REGS / KVM_SET_SREGS / restore CPU state from snapshot.
  4. KVM_RUN — guest resumes mid-execution at the snapshot point.

Isolation per fork:
  - Hardware-enforced memory boundary (KVM VM, not shared kernel).
  - No network device in the fork.
  - Serial I/O only for stdin/stdout/stderr to host orchestrator.
  - ~265 KB per-fork memory overhead (almost entirely CoW divergent
    pages from the small initial activity).
```

The primitive is incorporable into Nimbus as a Rust crate under
`nimbus-sandbox/backends/fsi/`. Attribution is Apache-2.0; the API
shell would be Nimbus-native (admission, policy, audit, observability),
not zeroboot's HTTP surface.

## 9. Decisions (D1–D12)

The "could we use one VMM for everything" question reopened in late May
2026 and resolved with a unified-lift decision: produce a single VMM
family (`nimbus-libkrun`) that carries libkrun's device/GPU breadth +
Firecracker's snapshot mechanism + zeroboot's MAP_PRIVATE fork
primitive. Decisions D1–D12 below are the durable record; cite this
section from consumer plans.

### 9.1 The "one VMM" fantasy is closeable in-fork

Mainline, the answer is still no: no rust-vmm-family VMM
simultaneously has Firecracker's snapshot maturity + libkrun's
GPU/HVF/input + Cloud Hypervisor's hotplug. The two-VMM split is the
forced choice if you only consume upstream releases.

D1 changes the equation: **we will produce that one VMM** by porting
Firecracker's snapshot machinery (Apache-2.0, lineage-consistent —
libkrun's README states it "incorporates code from Firecracker,
rust-vmm and Cloud-Hypervisor") and zeroboot's MAP_PRIVATE fork
primitive (Apache-2.0) into `nimbus-libkrun`. The lift is sized at
~9,450 LoC across the fork (see D4), not a multi-quarter clean-room.

### 9.2 D1 — Unified lift

One VMM family carries every Nimbus sandbox workload (Lambda-style
invocation, agentic desktop, GPU inference). Lift libkrun base + muvm
GPU/media wiring + Firecracker snapshot patterns + zeroboot
MAP_PRIVATE fork primitive into `nimbus-libkrun`.

### 9.3 D2 — nimbus-libkrun only; Firecracker-as-separate-VMM dropped

Firecracker is not run alongside nimbus-libkrun. The Firecracker
Snapshot Invocation (FSI) plan was archived 2026-05-27 without
execution. Firecracker code and patterns flow **into** nimbus-libkrun
under Apache-2.0; the two-VMM CI/device-set/test surface is collapsed
to one.

### 9.4 D3 — Tier collapse; capability profiles on a single backend

No Tier 1 / Tier 2 / Tier 3 split. Single sandbox backend with
capability profiles (`lambda`, `desktop`, `gpu`, `snapshot`); profiles
tune device set + memory + boot path on the same VMM binary. See
`docs/plans/research/libkrun-session-sandbox.md` §profiles for the
device-set mapping.

### 9.5 D4 — Phased snapshot port

S0–S5 phases in `docs/plans/nimbus-sandbox-plan.md` Band S (Linux-KVM
snapshot/fork). Total SWAG ~7,050 LoC net new + ~2,400 LoC tests =
~9,450 LoC across the fork. Phases are individually shippable: S0–S2
cover the Lambda-style invocation profile; S3 covers the desktop and
GPU profiles; S4 unlocks the sub-ms session fork; S5 is latency
optimization. See [[feedback_engineering_sizing_loc_swag]] for sizing
convention.

### 9.6 D5 — GPU re-init on restore

Do not serialize Venus / native-context state. Re-init virtio-gpu /
virtio-input / virtio-snd on restore. Data lives in guest memory +
virtio-fs payload, not in the device-private state machine. Guest
userspace must handle `VK_ERROR_DEVICE_LOST` (same recovery path as a
real GPU hot-unplug); workloads that cannot are documented as known
limitation.

### 9.7 D6 — virtio-fs over virtiofsd

libkrun's virtio-fs is in-process passthrough
(`src/devices/src/virtio/fs/linux/passthrough.rs`, ~2,200 LoC Linux /
~2,500 LoC macOS) — **not** an out-of-process virtiofsd. Snapshot
serializes mount metadata only; payload data lives on the host
filesystem and survives the round-trip via re-mount on restore.

### 9.8 D7 — License composition (all Apache-2.0 / MIT, no LGPL)

- libkrun base: **Apache-2.0** (verified 2026-05-27 against
  `~/src/github.com/nimbus/nimbus-libkrun/LICENSE` and upstream
  `containers/libkrun/LICENSE`; earlier internal notes calling it
  LGPL-2.1 were wrong).
- Firecracker patterns lifted into the fork: Apache-2.0.
- zeroboot primitive: Apache-2.0.
- muvm-derived bits: MIT (via `AsahiLinux/muvm/crates/muvm/Cargo.toml`;
  no root LICENSE, so Cargo.toml SPDX is authoritative).

All permissive; no LGPL relinking constraint. See
[[feedback_apache_license_posture]].

### 9.9 D8 — Sub-ms session fork is the 2026 product differentiator

MAP_PRIVATE fork (zeroboot baseline: 0.79 ms p50, 265 KB / sandbox on
minimal serial guests) is the load-bearing capability for per-request
invocation workloads. Profile-specific targets (lambda ~3 ms p50,
desktop ~10 ms p50) since full device sets fork more slowly than the
zeroboot minimal-serial baseline.

### 9.10 D9 — Port primitive, not project

zeroboot stays a credited reference. The mechanism (MAP_PRIVATE +
KVM_SET_REGS resume) is ported into nimbus-libkrun as a Rust crate;
the zeroboot codebase is not vendored. Attribution is Apache-2.0.

### 9.11 D10 — Stop tracking Cloud Hypervisor as a unification candidate

Cloud Hypervisor's GPU/input gap remains, snapshot wire format is
version-locked, and tracking it as a dual path adds CI cost without
competitive leverage. libkrun already inherits the relevant rust-vmm
lineage. Re-evaluate only if Cloud Hypervisor ships material GPU /
input / HVF parity in a future release.

### 9.12 D11 — Snapshot/fork is Linux-KVM-only by construction

P0–P5 of the snapshot port ship **no macOS HVF code path**. macOS HVF's
role in the deployed Nimbus topology is limited to running the
existing outer machine-os Linux VM via krunkit (libkrun-on-HVF —
today's shipped contract on `ghcr.io/nimbus/machine-os` v0.1.30,
pinned 2026-05-14). That outer VM is **single-instance, long-lived
per developer environment**; it does not need snapshot/restore or
sub-ms fork.

Reasons the scoping is sound:

- every per-request fork-fast workload runs on Linux KVM in production
  (lambda / serverless invocation profile);
- macOS dev per-service workloads are containers inside the outer VM
  (D12), not nested microVMs, so there is no HVF consumer for the
  snapshot path;
- `docs/architecture/sandbox/macos-machine-flow.md` §"Flow 6: Linux
  Production Contrast" documents the topology that makes this scoping
  valid.

Implementation implication: CI matrix and benchmarks for P0–P5 target
Linux KVM only; macOS proof helpers
(`make collect-nimbus-machine-cli-proof` etc.) cover the outer-VM
lifecycle but do not exercise snapshot/fork code paths. macOS HVF
parity for the snapshot mechanism is **not "track separately" —
it is out of scope by construction** because the topology has no
consumer for it.

### 9.13 D12 — macOS dev parity is "container-in-outer-VM," not nested microVM

Per-service workloads on macOS dev hosts run as standard Linux
containers (conmon → crun → standard container) **inside** the
machine-os outer VM, managed by the guest machine API
(`/run/nimbus/nimbus.sock`) and orchestrated by the host `nimbus`
server via the forwarded `<machine>-api.sock`. They do **not** run as
nested libkrun-microVMs via nested KVM on Apple Silicon.

Production fork-semantics parity is enforced through Linux CI rather
than nested-virt-on-macOS. This preserves the Option-A hybrid
control-plane decision
(`docs/plans/research/macos-host-vs-guest-control-plane-rationale.md`)
and explicitly defers:

- nested-virt performance overhead (typically 10–30% per layer);
- Apple Silicon nested HVF maturity (added in macOS 15 Sequoia, 2024 —
  recent);
- nested GPU passthrough complexity through HVF → outer libkrun →
  inner KVM → inner libkrun.

The trade is acceptable: macOS dev gets crun's ~10–50 ms container
cold start; production gets libkrun-on-KVM with snapshot fork (P4
target ~3 ms p50 lambda / ~10 ms p50 desktop). Re-evaluate D12 if
container/microVM divergence (memory model, signal handling, /proc
visibility, IPC semantics) becomes a recurring DX problem.

### 9.14 What unified lift changes vs the prior framing

Prior framing (pre-2026-05-27): "two VMMs, three tiers" —
Firecracker for Tier 1 invocation, libkrun for Tier 2/3 session/GPU,
duplicate snapshot/device/test surfaces, cross-fork bug propagation
manual.

Current framing (D1–D12): one VMM family, four capability profiles,
one device set, one snapshot mechanism, one CI matrix on Linux KVM
(D11), macOS dev parity via containers-in-outer-VM (D12), Cloud
Hypervisor dropped from active tracking (D10).

## 10. Risk register

- **Charter lock on Firecracker features.** AWS will not accept
  virtio-gpu / virtio-snd in mainline. Moot after D2 (Nimbus does not
  depend on Firecracker mainline for device coverage); listed for
  reviewers comparing back to the dropped framing.
- **libkrun snapshot gap is structural.** No upstream momentum. If our
  product surface depends on session-fork latency, we own the porting
  effort.
- **zeroboot is prototype-grade.** Their security model has not been
  audited at production scale. If we lift the primitive, we own the
  audit responsibility.
- **rust-vmm crate skew across VMMs.** Each VMM pins different versions
  of the shared crates; cross-VMM bug fix propagation is not
  automatic.

## 11. Open follow-ups (deferred to other docs / spikes)

- Security audit of zeroboot's `mmap(MAP_PRIVATE)` + KVM VM creation
  path under multi-tenant load — Nimbus owns the audit once the
  primitive lands in nimbus-libkrun (D9).
- Sandbox trait split — `InvocationSandbox` vs `SessionSandbox`, or a
  unified `Sandbox` with profile-tagged capability methods. Design
  spike to match the capability-profile contract (D3).
- macOS dev container/microVM divergence watch (D12) — re-evaluate if
  guest-container behavior on macOS drifts from production microVM
  behavior enough to break DX (memory model, signal handling, /proc
  visibility, IPC semantics).
- Snapshot signing / attestation integration with Nimbus artifact
  admission — separate plan once P2 lands.

The items previously listed here that D1–D12 settled:

- ~~Sizing spike for porting Firecracker snapshot machinery into
  libkrun~~ — settled by D4 and the snapshot-port plan
  (~9,450 LoC SWAG).
- ~~Tier 1 mechanism decision (cold boot vs snapshot vs
  template-fork)~~ — settled by D8 + D9 (snapshot + MAP_PRIVATE fork on
  the unified backend).
- ~~Cloud Hypervisor unification re-evaluation cadence (6–12 months)~~
  — settled by D10 (stop tracking).
