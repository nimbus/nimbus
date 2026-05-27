---
status: research
owners: sandbox-tier-roadmap
related:
  - docs/plans/firecracker-snapshot-invocation-backend-plan.md
  - docs/plans/computer-use-sandbox-plan.md
  - docs/plans/gpu-accelerated-sandbox-plan.md
  - docs/plans/research/libkrun-session-sandbox.md
  - docs/plans/research/gpu-sandbox-backends.md
  - docs/plans/research/nimbus-libkrun-fork-inventory.md
  - docs/plans/research/computer-use-capabilities-audit.md
  - docs/plans/agent-browser-service-plan.md
---

# VMM Landscape 2026 — Evidence Base for the Three-Tier Sandbox Roadmap

This doc resolves the recurring "could we just use one VMM for everything"
question by recording the May 2026 state of the rust-vmm VMM family
(Firecracker, Cloud Hypervisor, libkrun) and the Firecracker-derivative
AI-sandbox projects (zeroboot, Fly.io Sprites, Northflank's Kata
layering). Cite this doc from FSI / CUS / GAW plans rather than restating
the evidence.

## 1. Scope

- Per-VMM capability state (devices, snapshot, host platforms, license).
- Lineage between VMMs — they are not three independent siblings.
- Production-platform mapping — who actually ships what in 2026.
- Decision implications for the three-tier roadmap (Tier 1 / 2 / 3).

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
        └──→ libkrun (Red Hat / containers org, LGPL-2.1)
                ↑
                └── also absorbs code from Cloud Hypervisor
```

libkrun's upstream README states verbatim: libkrun "incorporates code
from Firecracker, rust-vmm and Cloud-Hypervisor." This matters for two
reasons:

1. Porting Firecracker's snapshot machinery into libkrun is lineage-
   consistent, not a clean-room exercise.
2. Bug fixes and security patches sometimes flow across all three; we
   should track Firecracker security advisories even if our Tier 2/3
   lane is libkrun.

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

- v1.18.1 (2026-05-20). LGPL-2.1.
- **Devices** (per upstream README):
  `virtio-console, virtio-block, virtio-fs, virtio-gpu (venus +
  native-context), virtio-net, virtio-vsock, virtio-balloon (free-page
  reporting only), virtio-rng`. **No virtio-snd or virtio-input in
  upstream README** — both have been discussed in issues; mainline state
  to confirm before relying on either. Audio in particular is a
  follow-on per `nimbus-libkrun-fork-inventory.md` §6.2.
- **Snapshot/restore — not in mainline, not on any branch, not on the
  roadmap.** Major asymmetry vs Firecracker.
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
| Nimbus (planned) | Firecracker (Tier 1) + libkrun (Tier 2/3) | Three-tier roadmap | This roadmap |

The industry split-by-workload pattern is the dominant shape in 2026:
**fast-cold-start serverless ⇒ Firecracker; full desktop / GPU /
macOS-native ⇒ libkrun.** Nobody ships serverless on libkrun. Nobody
ships desktop / GPU on Firecracker. Nimbus's three-tier roadmap is
aligned with this pattern, not against it.

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

Implication: if Tier 1 ever needs warm bundle delivery via virtiofs
(e.g., to share a multi-GiB Python wheel cache across forks), the path
is "run virtiofsd alongside Firecracker over vhost-user-fs," not "fork
Firecracker." This is a real escape hatch the original two-VMM
analysis under-credited.

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

## 9. Decision implications

### 9.1 Tier 1 VMM

**Decided:** Firecracker. Every 2026 production fast-cold-start
serverless platform converges on Firecracker (or gVisor — different
trust model). libkrun's snapshot gap is load-bearing and not closing.

### 9.2 Tier 1 mechanism (open — see §9 below in conversation)

Three options on the same VMM:

1. **Cold boot** (~125 ms): Firecracker fresh-boots every invocation.
2. **Snapshot-restore** (~10 ms warm path): boot once, snapshot once,
   restore on each invocation. UFFD + MAP_PRIVATE.
3. **Template-fork** (~0.8 ms, zeroboot model): boot+snapshot once,
   then mmap(MAP_PRIVATE) + new KVM VM per invocation.

The right answer depends on Tier 1's actual product surface — see
open question §2 in the surfacing-decisions conversation.

### 9.3 Tier 2/3 VMM

**Decided:** libkrun. The only mainline VMM with virtio-gpu +
virtio-input + production HVF on Apple Silicon. Cloud Hypervisor is
not a viable substitute today (no GPU/input; weaker macOS posture).

### 9.4 Tier 2/3 snapshot/restore gap

**Open.** Three responses:

1. **Accept the gap for v0.** Reserve `Sandbox::snapshot/branch/restore`
   trait methods as `unimplemented!()`. CUS-Snap implements them
   later. (Current plan choice.)
2. **Port Firecracker's snapshot machinery into libkrun.** Multi-
   quarter project; lineage-consistent (libkrun already absorbs
   Firecracker code); high competitive leverage ("branch a desktop
   session in 0.8 ms" would be unique in 2026).
3. **Wait for upstream libkrun.** No evidence this is coming. Not a
   viable path.

Recommendation: **(1) for v0** plus **a scoping spike for (2)** before
committing to a follow-on plan.

### 9.5 The "one VMM" fantasy

There is no rust-vmm-family VMM that simultaneously has Firecracker's
snapshot maturity + libkrun's GPU/HVF/input + Cloud Hypervisor's
hotplug. The two-VMM split is a forced choice today.

Cloud Hypervisor is the natural future unification candidate if it
ever closes the GPU/input gap. We should re-evaluate every 6–12 months
or whenever either fork ships material new device support.

## 10. Risk register

- **Charter lock on Firecracker features.** AWS will not accept
  virtio-gpu / virtio-snd in mainline. If Tier 1 ever needs them, we
  must go vhost-user delegated devices or accept a Firecracker fork.
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
  path under multi-tenant load.
- Sizing spike for porting Firecracker snapshot machinery into
  libkrun (1–2 person-weeks).
- Tier 1 mechanism decision (cold boot vs snapshot vs template-fork) —
  driven by Tier 1 product-surface answer.
- Sandbox trait split (`InvocationSandbox` vs `SessionSandbox`) — design
  spike, see open question §5.
- Cloud Hypervisor unification re-evaluation cadence (6–12 months).
