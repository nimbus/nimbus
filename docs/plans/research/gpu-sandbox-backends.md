# Research: GPU Mediation Backends for Multi-Tenant MicroVM Sandboxes

Decision rationale for which GPU mediation backend Nimbus defaults to in
its libkrun-backed Tier-3 sandbox, plus the exception policy for trusted
workloads that need higher performance.

This is a research / decision document, not an execution plan. The active
execution plan that consumes this decision is:

- [`docs/plans/gpu-accelerated-sandbox-plan.md`](../gpu-accelerated-sandbox-plan.md) (GAW)

The shared libkrun-session backend design is covered by:

- [`docs/plans/research/libkrun-session-sandbox.md`](./libkrun-session-sandbox.md)

## Purpose

Pick the GPU mediation backend Nimbus exposes by default for Tier-3 (GPU
accelerated AI workload) sandboxes, and the policy for when other
backends are allowed.

The decision is per-host-vendor and per-trust-level. There is no single
right answer; the policy must enumerate.

## The Three Mediation Models

### 1. Venus (Vulkan command serialization)

Guest Mesa runs the `venus` ICD. Vulkan calls are serialized over
virtio-gpu into a host-side **isolated render-server process** that
translates them back into Vulkan calls on the host driver. The render
server runs in an OS-level sandbox separate from the VMM
([Collabora 2025-01](https://www.collabora.com/news-and-blog/blog/2025/01/15/the-state-of-gfx-virtualization-using-virglrenderer/)).

- Guest never issues DMA into host memory directly.
- Host kernel driver only sees Vulkan-validated commands from a sandboxed
  process.
- Vulkan-only surface — no OpenCL, no CUDA, no ROCm.
- Multi-vendor: works on every Mesa-supported Vulkan driver plus the
  NVIDIA blob and Mali blob
  ([Mesa Venus docs](https://docs.mesa3d.org/drivers/venus.html)).

### 2. Native-context (per-vendor command-stream passthrough)

Guest userspace runs a per-vendor Mesa driver that forwards the host
kernel driver's ioctl stream verbatim over virtio-gpu. There is no
command translation layer — the guest issues real driver ioctls;
virglrenderer forwards them.

Drivers that exist in Mesa upstream as of late 2025:

- `freedreno` (Adreno) — fully upstream, production on ChromeOS.
- `amdgpu` — fully upstream (Mesa 25.0,
  [Phoronix](https://www.phoronix.com/news/AMDGPU-VirtIO-Native-Mesa-25.0)).
- `asahi` (Apple AGX on Linux host) — partial upstream.
- `intel (i915)` — MRs open, not merged
  ([Mesa MR 29870](https://gitlab.freedesktop.org/mesa/mesa/-/merge_requests/29870)).

No native-context driver exists for nouveau/nova (NVIDIA), Mali, or any
other vendor.

- Approaching native GPU speeds for most applications ([Collabora]).
- Larger attack surface: every kernel-driver vulnerability in the
  underlying driver is reachable from the guest.

### 3. Full PCIe passthrough (VFIO)

Dedicates a physical GPU to one guest. Works on Linux with KVM and an
IOMMU group that isolates the GPU.

- Strongest per-guest performance.
- Single tenant per GPU — bad for multi-tenancy.
- Hard to verify isolation (DMA-from-device, MSI routing, IOMMU group
  composition).

Not a primary path for Nimbus's multi-tenant sandbox; included for
completeness.

## Vendor × Backend × Workload Support Matrix

Rows are host GPU vendor; columns are mediation backends; cells are
workload classes the combination can serve in production today.

| Host vendor | Venus | Native-context | Other |
| --- | --- | --- | --- |
| AMD (GFX9+, Linux host) | Vulkan, VK-compute | Vulkan, VK-compute, ROCm (draft) | — |
| Intel (Gen8+, Linux host) | Vulkan, VK-compute | Dev only (MRs open) | — |
| NVIDIA (Linux host) | Vulkan, VK-compute (sharp edges on blob ≥570.86) | None | vGPU (licensed), scuda RPC |
| Apple Silicon (Asahi Linux host) | Vulkan | Partial (asahi native-context) | — |
| Apple Silicon (macOS host via libkrun + krunkit) | **Vulkan, VK-compute (~75–80% native Metal)** | None (no host DRM driver) | None |
| Mali (Linux host) | Vulkan (PanVK / blob) | None | — |
| Adreno (Linux host) | Vulkan (Turnip) | Vulkan, VK-compute (production) | — |

**Bold cells** are non-obvious paths worth calling out. Apple Silicon on
macOS via Venus is the surprise of this matrix — the path is Venus →
virglrenderer → MoltenVK → Metal, and llama.cpp benchmarks land at
~75–80 % of native Metal performance ([Red Hat Developer 2025-09]).

## Multi-Tenancy and Security Properties

| Property | Venus | Native-context | VFIO |
| --- | --- | --- | --- |
| Guest can issue DMA into host memory | No | No (host driver still mediates) | Yes (until IOMMU stops it) |
| Host kernel-driver ioctls exposed to guest | No | Yes (full surface) | Yes (full surface) |
| Vulkan protocol-level validation on host | Yes (render server) | No | No |
| Host process isolation for guest GPU work | Yes (render server is a separate sandboxed process) | Partial (virglrenderer process) | None (direct device) |
| Per-tenant render-server pinning possible | Yes | Yes (per-tenant virglrenderer) | n/a |
| Cross-tenant info leak via GPU memory residue | Mitigated by Vulkan API surface | Vendor-driver-dependent | High |
| Useful for untrusted code | Yes | Risky (kernel-driver attack surface) | No |

For untrusted code under Nimbus's threat model (computer-use sandboxes
that load arbitrary websites; GPU AI sandboxes that take untrusted
prompts or load tenant-supplied model weights), Venus is the only
backend that keeps the host kernel driver out of the guest's reach.

For trusted code (tenant-verified model image, no arbitrary input
execution), native-context's perf advantage may justify the larger
attack surface.

## Performance

Concrete numbers where benchmarks exist:

- Venus on macOS (M-series Apple Silicon): ~75–80 % of native Metal for
  llama.cpp Vulkan compute
  ([Red Hat Developer 2025-09](https://developers.redhat.com/articles/2025/09/18/reach-native-speed-macos-llamacpp-container-inference)).
- Venus on Linux + AMD: typically within 10–20 % of native for Vulkan
  workloads; varies sharply by workload
  ([Collabora 2025-01](https://www.collabora.com/news-and-blog/blog/2025/01/15/the-state-of-gfx-virtualization-using-virglrenderer/)).
- Native-context (amdgpu / freedreno) on Linux: "approaching native GPU
  speeds for most applications" ([Collabora 2025-01]).
- API remoting ([libkrun PR #508](https://github.com/containers/libkrun/pull/508),
  draft) on macOS for llama.cpp: 95 %+ of native Metal — but only for
  llama.cpp's GGML tensor calls, not a general Vulkan surface
  ([llama.cpp PR #18718](https://github.com/ggml-org/llama.cpp/pull/18718)).

Treat published numbers as workload-specific. Nimbus must run its own
inference benchmark suite as part of the GAW plan (benchmark gate
before promoting any backend).

## Workload Class Fit

| Workload | Venus | Native-context | Other |
| --- | --- | --- | --- |
| Vulkan compute (llama.cpp, whisper.cpp, stable-diffusion.cpp via ggml-vulkan) | Yes | Yes (amdgpu, freedreno) | n/a |
| Vulkan graphics (browsers, headless rendering) | Yes | Yes | n/a |
| OpenCL | Limited (via clvk on Vulkan) | Yes on amdgpu | n/a |
| CUDA | No | No | vGPU only |
| ROCm / HIP | No | Draft only on amdgpu HSAKMT ([virglrenderer MR 1370](https://gitlab.freedesktop.org/virgl/virglrenderer/-/merge_requests/1370)) | — |
| Apple Metal-only frameworks (Core ML, MPS) | n/a (different surface) | n/a | Out-of-VM only |

Vulkan compute is the broadest "yes" cell. Most modern open-source ML
inference stacks have Vulkan backends:

- llama.cpp: ggml-vulkan
- whisper.cpp: ggml-vulkan
- stable-diffusion.cpp: ggml-vulkan
- vLLM: PyTorch with Vulkan (immature)
- diffusers / transformers: PyTorch with Vulkan via vulkano-rs or
  candle-vulkan (immature)

Tenants who *must* have CUDA cannot share a Linux microVM with a Venus
or native-context backend. They need a different fleet:

- NVIDIA vGPU (licensed)
- scuda / rCUDA RPC forwarding (research-grade)
- bare-metal or PCI-passthrough hosts (single-tenant)

This is a real product split. Document it; do not pretend GPU AI is
vendor-neutral.

## The macOS Surprise

Prior assumption: macOS host + libkrun guest meant CPU-only Tier 3.
That was wrong. Venus on macOS via krunkit works today:

- krunkit sets `VIRGLRENDERER_VENUS | VIRGLRENDERER_NO_VIRGL` by default
  in [src/context.rs](https://github.com/containers/krunkit/blob/main/src/context.rs).
- libkrun's `src/devices/src/virtio/gpu/virtio_gpu.rs` has macOS-specific
  paths using `RUTABAGA_MEM_HANDLE_TYPE_APPLE`.
- Pipeline: guest Mesa Venus → virtio-gpu shmem → host virglrenderer →
  MoltenVK → Metal.
- Real-workload evidence: llama.cpp Vulkan backend identifies the device
  as `Virtio-GPU Venus (Apple M4 Pro) (venus)` and runs at ~75–80 % of
  native Metal
  ([libkrun #353](https://github.com/containers/libkrun/issues/353),
  [libkrun #377](https://github.com/containers/libkrun/issues/377),
  [Red Hat Developer 2025-09](https://developers.redhat.com/articles/2025/09/18/reach-native-speed-macos-llamacpp-container-inference)).

This changes the macOS Tier-3 product story: Vulkan-compatible ML
workloads run on the macOS dev machine with acceptable perf. CUDA-only
and ROCm-only workloads still require a remote Linux fleet.

The pipeline is brittle — see
[libkrun #377](https://github.com/containers/libkrun/issues/377) for
`vn_ring_submit` aborts under heavy load — but it is good enough for
dev-loop workloads and is actively maintained.

What does not work on macOS:

- Native-context: no Linux DRM kernel driver on macOS to forward.
- Apple PVG via libkrun: libkrun uses Hypervisor.framework directly, not
  Virtualization.framework, which is where
  `VZVirtioGraphicsDeviceConfiguration` lives.
- CUDA: no production path.

## Recommended Selection Logic

Default: **Venus for all sandboxes that need GPU mediation.**

Exception path (per-tenant, per-spec, explicit opt-in only):

| Condition | Allowed alternative |
| --- | --- |
| Host GPU is AMD (Linux), workload is trusted, tenant opted in | `native-context-amdgpu` |
| Host GPU is Adreno (Linux), workload is trusted, tenant opted in | `native-context-freedreno` |
| Host GPU is Asahi (Linux host), workload is trusted, tenant opted in | `native-context-asahi` |
| Tenant requires CUDA | Out of scope for libkrun-session: route to NVIDIA fleet (vGPU or bare-metal). Reject at admission. |
| Tenant requires ROCm | Defer until amdgpu HSAKMT MR lands; until then, route to bare-metal. Reject at admission. |

Untrusted code (default Nimbus threat model: computer-use sandboxes
loading arbitrary websites, AI sandboxes taking untrusted prompts or
loading tenant-supplied weights) **must always** use Venus. No exception
path.

## Operator Surface

GPU policy is expressed in the sandbox spec:

```rust
enum GpuMediationPolicy {
    /// Default for all multi-tenant workloads. Vulkan command
    /// serialization through a sandboxed host render-server process.
    Venus,

    /// Per-vendor command-stream passthrough. Higher performance,
    /// larger attack surface. Requires trusted tenant opt-in and
    /// matching host vendor.
    NativeContext {
        driver: NativeContextDriver,
        ioctl_filter: TenantIoctlFilter,
    },
}

enum NativeContextDriver { Amdgpu, Freedreno, Asahi }

enum TenantIoctlFilter {
    /// Allowlist derived from observed-safe ioctls. Default for tenants
    /// on native-context.
    Strict,
    /// Tenant-opted-out filter. Operator policy must record explicit
    /// approval.
    Permissive,
}
```

Admission rejects:

- `NativeContext` for untrusted-workload classes.
- `NativeContext { driver: X }` when the host vendor does not match.
- `TenantIoctlFilter::Permissive` without an operator-policy record.
- `Venus` on a host where the render server is not installed (with an
  actionable error).
- CUDA or ROCm requests on libkrun-session (with an actionable error
  pointing at the NVIDIA fleet / ROCm deferral).

## Risks and Open Questions

1. **amdgpu native-context kernel attack surface is unbounded.** Every
   amdgpu CVE is guest-reachable. No published ioctl-allowlist comparable
   to Venus's render-server sandbox. Treat as a non-starter for
   untrusted workloads; for trusted workloads, document residual risk.

2. **ROCm-in-guest is a draft MR with cross-process host-libhsakmt
   sharing.** See [virglrenderer MR 1370]. That "one libhsakmt across
   guests" design is a tenancy red flag. Do not ship on it; track and
   revisit when the MR lands and the cross-tenant story is clarified.

3. **NVIDIA path stays bifurcated.** No nova native-context is in
   flight; CUDA requires vGPU licensing or out-of-VM RPC. Plan for two
   hardware fleets (AMD/Intel + NVIDIA-bare-metal), not a unified
   mediation story.

4. **Venus on macOS has known stability issues under load.**
   [libkrun #377] tracks `vn_ring_submit` aborts in llama.cpp Vulkan;
   under active investigation by Red Hat's CI but not closed. Real-host
   stability gate is part of the GAW plan before declaring macOS Tier-3
   ready.

5. **API remoting ([libkrun PR #508]) is a credible long-term path**
   for near-native macOS performance, but it is workload-specific (one
   per-framework shim) and currently in draft. Track but do not depend
   on.

6. **virglrenderer process model.** Today there is one virglrenderer per
   VMM. Confirm whether per-tenant render-server pinning is
   operationally feasible (a render server per VM) or whether tenants
   share a single host-side render server. This affects the isolation
   argument and is a GAW-plan investigation.

## References

- [Mesa Venus driver docs](https://docs.mesa3d.org/drivers/venus.html)
- [Mesa 26.0.0 release notes](https://docs.mesa3d.org/relnotes/26.0.0.html)
- [Collabora 2025-01: state of GFX virtualization with virglrenderer](https://www.collabora.com/news-and-blog/blog/2025/01/15/the-state-of-gfx-virtualization-using-virglrenderer/)
- [QEMU v14 virtio-gpu DRM native context patchset](http://www.mail-archive.com/qemu-devel@nongnu.org/msg1153159.html)
- [Mesa MR 29870: virtio-intel native context](https://gitlab.freedesktop.org/mesa/mesa/-/merge_requests/29870)
- [virglrenderer MR 1370: amdgpu HSAKMT/ROCm native context (draft)](https://gitlab.freedesktop.org/virgl/virglrenderer/-/merge_requests/1370)
- [Phoronix: AMDGPU VirtIO native context merged into Mesa 25.0](https://www.phoronix.com/news/AMDGPU-VirtIO-Native-Mesa-25.0)
- [Red Hat Developer 2025-06: AI inference on macOS Podman containers](https://developers.redhat.com/articles/2025/06/05/how-we-improved-ai-inference-macos-podman-containers)
- [Red Hat Developer 2025-09: Venus llama.cpp ~75–80% native on macOS](https://developers.redhat.com/articles/2025/09/18/reach-native-speed-macos-llamacpp-container-inference)
- [libkrun PR #508: API remoting capset](https://github.com/containers/libkrun/pull/508)
- [libkrun #353: Venus on macOS llama.cpp](https://github.com/containers/libkrun/issues/353)
- [libkrun #377: Venus stability under heavy load](https://github.com/containers/libkrun/issues/377)
- [libkrun #565: capset enumeration bug](https://github.com/containers/libkrun/issues/565)
- [krunkit src/context.rs](https://github.com/containers/krunkit/blob/main/src/context.rs)
- [llama.cpp PR #18718: virglrenderer API remoting ggml backend](https://github.com/ggml-org/llama.cpp/pull/18718)
- [NVIDIA devforum: virtio native context support request](https://forums.developer.nvidia.com/t/virtio-native-context-support/320346)
- [muvm cli_options.rs GpuMode enum](https://github.com/AsahiLinux/muvm/blob/main/crates/muvm/src/cli_options.rs)
