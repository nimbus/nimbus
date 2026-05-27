---
status: research
owners: sandbox-tier-roadmap
related:
  - docs/plans/research/libkrun-session-sandbox.md
  - docs/plans/research/gpu-sandbox-backends.md
  - docs/plans/computer-use-sandbox-plan.md
  - docs/plans/gpu-accelerated-sandbox-plan.md
  - docs/plans/firecracker-snapshot-invocation-backend-plan.md
  - docs/plans/archive/nimbus-libkrun-runtime-stack-plan.md
---

# nimbus-libkrun fork inventory + muvm consumption map

This research doc establishes the **load-bearing surface** that the three-tier
sandbox roadmap (Tier 1 Firecracker, Tier 2 computer use, Tier 3 GPU AI)
assumes already exists in our fork plus what we need to lift from
`AsahiLinux/muvm`. It is the prerequisite for the `CUS0` and `GAW0` phases
of the two new active plans.

The doc is observational, not prescriptive — it records what is in the
fork today and where the muvm assets live, so subsequent execution phases
can cite specific commits and paths instead of restating assumptions.

## 1. Scope and method

- Fork worktree: `~/src/github.com/nimbus/nimbus-libkrun`, branch
  `nimbus/v1.18.1`, base `upstream/stable-1.18.x` (containers/libkrun).
- Range surveyed: `git log upstream/stable-1.18.x..nimbus/v1.18.1`
  (10 commits at time of writing, head `a8daa86`).
- Upstream muvm: `AsahiLinux/muvm` at default branch, MIT-licensed. No
  local clone yet; layout walked via `gh api` reads of `crates/muvm/`.
- License posture: muvm MIT is freely incorporable into Nimbus per
  durable feedback memory `feedback_apache_license_posture`. MIT
  attribution must be preserved when files are lifted.

## 2. nimbus-libkrun fork patch table

Ordered oldest → newest. Subsystem column names the largest changed
directory in each commit's diffstat.

| # | Commit  | Subsystem                | Category     | Tier relevance   | Notes |
|---|---------|--------------------------|--------------|------------------|-------|
| 1 | 15bcf49 | `src/devices/src/virtio/vsock` + `src/libkrun` + `include/libkrun.h` | **functional** | Tier 1 (and any TSI-mode lane) | The only functional libkrun change in the fork. Adds public C entrypoint `krun_set_port_map_with_bind_address`. |
| 2 | 4eb3fc5 | `src/devices/src/virtio/vsock/tsi_stream.rs` (tests) | functional-test | Tier 1 | Covers #1's listen mapping. |
| 3 | 917674c | release tooling           | scaffolding  | All (release lane) | Nimbus-side build/release helpers. |
| 4 | c261a65 | pkgconfig layout          | scaffolding  | All (packaging)    | Relocatable pkgconfig for Homebrew/apt/copr install paths. |
| 5 | c37b534 | CI workflow               | scaffolding  | All (release lane) | Installs `libcap-ng` in release CI. |
| 6 | 922ca06 | CI workflow               | scaffolding  | All (release lane) | Installs `clang` in release CI. |
| 7 | 0fbf4d2 | TSI test imports          | hygiene      | Tier 1 | Cleanup tied to #1. |
| 8 | 9d66ad1 | IPv6 test helper imports  | hygiene      | Tier 1 | Cleanup tied to #1. |
| 9 | 3bdc5ec | release contract docs     | scaffolding  | All (release lane) | Nimbus release contract update. |
|10 | a8daa86 | build mechanics           | scaffolding  | All (build lane)   | Follows libkrun v1.18 init build path. |

**Headline:** the functional Nimbus delta against upstream is one feature
(TSI bind-address mapping with fail-closed semantics on unmapped guest
listen). Everything else is build, release, packaging, CI, and hygiene
fallout from owning the release artifact.

## 3. The TSI bind-address hook in detail (commit 15bcf49)

### Public C API delta (`include/libkrun.h`)

```c
int32_t krun_set_port_map_with_bind_address(uint32_t ctx_id,
                                            const char *const port_map[]);
```

Accepted entry shapes:

- `host_address:host_port:guest_port` (IPv4)
- `[host_address]:host_port:guest_port` (IPv6, bracketed)
- `host_port:guest_port` (wildcard, legacy)

Documented error: `-ENOTSUP` when **passt networking is used**.

### Two semantic deltas vs upstream `krun_set_port_map`

1. Each exposed host listener can be pinned to a specific host IP
   (the localhost-only fail-closed contract from
   `docs/plans/archive/localhost-server-security-plan.md`).
2. When an explicit port map is configured, **guest TCP listen requests
   for unmapped ports are denied** instead of being silently exposed on
   the host. This is the load-bearing fail-closed behavior that the
   landed Linux krun service baseline relies on.

### Tier mapping

- **Tier 1 (Firecracker Lambda):** does not use TSI at all. Firecracker
  uses its own tap-device networking; this hook is **not relevant** to
  Tier 1.
- **Tier 2 (computer use, libkrun_session) and Tier 3 (GPU AI,
  libkrun_session):** **probably not relevant** in their default shape.
  Both tiers want guest-initiated outbound (browser fetches, model
  weight downloads, telemetry), which means **passt networking**, and
  this hook is documented `-ENOTSUP` under passt. If a Tier 2/3 session
  needs to expose a guest port to the host (e.g., a debug VNC), it will
  need to either (a) use the TSI mode and lose passt-style outbound, or
  (b) add a passt-side equivalent of this hook, which would be a new
  upstream contribution.
- **Existing krun service baseline:** this is the production user of
  the hook. Continues to apply.

### Open question for the plans

Whether Tier 2 / Tier 3 need a **passt-mode equivalent** of the TSI
bind-address hook. If yes, this is a second functional libkrun patch
(or an upstream-first contribution to libkrun's passt path). Tracked
in §7 below.

## 4. Upstreaming opportunities

| Patch  | Upstreamable? | Why |
|--------|---------------|-----|
| 15bcf49 (TSI bind-address) | **Yes, eventually.** | Real feature with a clean C API. Localhost-only fail-closed is a reasonable upstream contract. Blocked behind landing in our release first; revisit after the FUS plan stabilizes. |
| 4eb3fc5 (TSI test)   | Yes, paired with 15bcf49 | Goes upstream with the feature. |
| 917674c, c261a65, 3bdc5ec | No | Nimbus-specific release/pkgconfig/contract surface. |
| c37b534, 922ca06     | No (lib has its own CI). | Patches our own release workflow, not the libkrun project CI. |
| 0fbf4d2, 9d66ad1     | Yes, with 15bcf49 | Test-side cleanup that goes with the feature. |
| a8daa86              | No | Nimbus build path follow-up. |

Net: the TSI bind-address feature plus its tests (4 commits) are
upstreamable as a single PR when the maintenance bandwidth exists.
The other 6 are Nimbus-permanent scaffolding.

## 5. muvm crate layout (AsahiLinux/muvm)

Two binaries in a single Cargo crate, plus a separate `krun-sys` crate.

```
crates/krun-sys/                       MIT  Rust FFI bindings to libkrun.so
crates/muvm/
  src/bin/muvm.rs                      MIT  Host launcher (binary #1)
  src/cli_options.rs                   MIT  bpaf CLI definitions
  src/config.rs                        MIT  KrunBaseConfig serde shape
  src/cpu.rs                           MIT  vCPU sizing
  src/env.rs                           MIT  env-var passthrough
  src/launch.rs                        MIT  Launch orchestration
  src/monitor.rs                       MIT  VM supervisor
  src/net.rs                           MIT  Host networking (passt invocation)
  src/tty.rs                           MIT  Terminal proxy
  src/hidpipe_common.rs                MIT  Shared HID types
  src/hidpipe_server.rs                MIT  udev → virtio-input pipe (host)
  src/types.rs                         MIT  Shared host/guest wire types
  src/lib.rs                           MIT  Module root
  src/utils/{env,fs,launch,stdio,tty,mod}.rs  MIT  Generic helpers

  src/guest/bin/muvm-guest.rs          MIT  Guest PID-1 binary (binary #2)
  src/guest/mod.rs                     MIT  Guest dispatcher
  src/guest/mount.rs                   MIT  virtiofs mount setup
  src/guest/net.rs                     MIT  Guest interface bring-up
  src/guest/server.rs                  MIT  vsock server
  src/guest/server_worker.rs           MIT  vsock worker
  src/guest/socket.rs                  MIT  vsock socket plumbing
  src/guest/user.rs                    MIT  uid/gid mapping
  src/guest/hidpipe.rs                 MIT  virtio-input consumer (guest)
  src/guest/mount.rs                   MIT  filesystem prep
  src/guest/box64.rs                   MIT  x86-on-arm64 (Box64 launcher)
  src/guest/fex.rs                     MIT  x86-on-arm64 (FEX launcher)
  src/guest/x11.rs                     MIT  X11 display bridge
  src/guest/bridge/pipewire.rs         MIT  Audio bridge
  src/guest/bridge/x11.rs              MIT  X11 bridge wire
  src/guest/bridge/common.rs           MIT  Bridge shared
  src/guest/bridge/mod.rs              MIT  Bridge dispatcher
```

`Cargo.toml` confirms `license = "MIT"` and the dependency surface:
`bpaf`, `krun-sys = { path = "../krun-sys" }`, `input-linux`,
`input-linux-sys`, `nix`, `neli`, `udev`, `tokio`, `serde`,
`procfs`, `rustix`. No GPL contamination.

## 6. Per-component disposition for `nimbus-init` + libkrun_session backend

Dispositions:

- **Lift** — copy the file with MIT attribution. Mechanism applies as-is.
- **Lift+mod** — copy as starting point, rework for Nimbus seams.
- **Reference** — read for design, do not lift; rewrite from scratch
  against `Sandbox` trait + `nimbus-engine` integration.
- **Skip** — not relevant to Nimbus scope.

| Component                              | Disposition | Tier  | Reasoning |
|----------------------------------------|-------------|-------|-----------|
| `crates/krun-sys`                      | Reference   | All   | We maintain `nimbus-libkrun`; build our own bindings against our `include/libkrun.h` (which now has the bind-address hook). |
| `crates/muvm/src/bin/muvm.rs`          | Reference   | All   | Launcher is Nimbus-domain (engine-owned, tenant-scoped, admission-gated). |
| `cli_options.rs`                       | Skip        | —     | Nimbus uses clap; CLI lives in `nimbus-bin`. |
| `config.rs`                            | Reference   | All   | Read for `KrunBaseConfig` field layout; our spec is `LibkrunSessionSandboxSpec`. |
| `cpu.rs`                               | Lift+mod    | All   | vCPU sizing logic is portable across host OSes. |
| `env.rs`                               | Reference   | All   | Nimbus tenant env policy differs (no host-env passthrough by default). |
| `launch.rs` (host)                     | Reference   | All   | Canonical example of `krun_set_*` call order — read it carefully, write our own. |
| `monitor.rs`                           | Reference   | All   | Nimbus has its own service supervisor (`nimbus-engine`). |
| `net.rs` (host)                        | Lift+mod    | T2/T3 | passt invocation patterns reusable; bind to Nimbus sandbox-egress policy. |
| `tty.rs`                               | Skip        | T2    | T2 uses screencast + virtio-input, not TTY proxy. |
| `hidpipe_common.rs`                    | Lift        | T2    | Shared HID type definitions — pure protocol. |
| `hidpipe_server.rs`                    | Lift+mod    | T2    | udev → virtio-input forwarding; clean mechanism. Rework for Nimbus-driven (not udev-driven) injection from operator commands. |
| `types.rs`                             | Reference   | All   | Look at wire format inspiration; we'll define our own. |
| `utils/env.rs,fs.rs,launch.rs,stdio.rs,tty.rs` | Lift | All | Generic process/fs helpers. |
| **guest** `muvm-guest.rs` (entry)      | Lift+mod    | T2/T3 | PID-1 entry; basis for `nimbus-init`. Strip Asahi-Linux defaults; add Nimbus control protocol. |
| `guest/mod.rs`                         | Lift+mod    | T2/T3 | Guest dispatcher pattern. |
| `guest/mount.rs`                       | Lift+mod    | T2/T3 | virtiofs mount logic — clean mechanism. |
| `guest/net.rs` (guest)                 | Lift+mod    | T2/T3 | Guest network bring-up. |
| `guest/server.rs`, `server_worker.rs`, `socket.rs` | Reference | T2/T3 | vsock control channel; Nimbus has its own wire protocol (`nimbus-engine` <→ guest). |
| `guest/user.rs`                        | Lift        | T2/T3 | uid/gid mapping is generic. |
| `guest/hidpipe.rs`                     | Lift        | T2    | virtio-input consumer in guest — pairs with host `hidpipe_server.rs`. |
| `guest/box64.rs`, `fex.rs`             | **Reference, follow-on** | T2 post-v0 | x86-on-aarch64 binary translation. v0 Tier 2 covers the common case where host arch and workload arch match. Becomes load-bearing when (a) Nimbus serves Tier 2 sessions from **aarch64 hosts** (Apple Silicon dev path; Graviton/Ampere cloud) and (b) the agent must drive **x86-only Linux apps** (legacy proprietary apps, MATLAB/CAD/Adobe, Wine/Proton for Windows apps). Both Box64 and FEX are MIT; reusable. See §6.1 below. |
| `guest/x11.rs`                         | Skip (wrong topology) | T2 | muvm forwards guest X11 traffic **out to a host X11 server**. Nimbus Tier 2 is headless-capture: X11 apps connect to **in-guest XWayland** (built into wlroots) and render to the headless Wayland session we screencast. AI-agent X11-app driving is solved by the wlroots path, **not by lifting muvm code**. |
| `guest/bridge/pipewire.rs`             | **Reference, follow-on (strategic)** | T2 post-v0 | PipeWire on modern Linux is the **unified media server** — audio AND camera/V4L2 integration. The guest PipeWire wiring is the foundation for four of the six Tier 2 media capabilities (audio capture, audio injection, webcam capture, webcam injection — see §6.2). Topology differs from muvm: Nimbus captures/injects media as **streams over vsock** (parallel to screencast and HID), not as host-PipeWire forwarding — so we lift the guest-side **virtual source/sink wiring**, not the host bridge endpoint. v0 reserves spec-type fields and vsock channel IDs for these streams but does not implement them. |
| `guest/bridge/x11.rs`                  | Skip (wrong topology) | — | Same reasoning as `guest/x11.rs`: forwards to host X11 we don't have; in-guest XWayland is the answer. |
| `guest/bridge/common.rs`, `mod.rs`     | **Reference, follow-on** | T2 post-v0 | Bridge dispatcher framework. Carries with the PipeWire follow-on. X11 bridge dispatch can be omitted at lift time. |

**Lift estimate for v0:** ~10 files lifted wholesale + ~6 lifted with
modifications. Of the remaining surface, **X11 host-bridge code stays
skipped permanently** (wrong topology — we use in-guest XWayland);
**Box64/FEX, PipeWire bridge, and the bridge dispatcher framework are
follow-on lift candidates** when their owning capability phases land
(see §6.1). Roughly half of `muvm-guest` is in v0 scope; the other
half is staged.

**Attribution mechanism:** add `LICENSE-MIT-muvm` to the `nimbus-init`
crate root and a per-file header on every lifted file naming
`AsahiLinux/muvm` and the commit SHA at lift time. Bookkept in a
single `THIRD_PARTY.md` for nimbus-init.

## 6.1 AI agent computer use capability map

Indexed by agent capability to make in-scope-vs-follow-on calls
explicit. Reading order: §6 says "what disposition"; §6.1 says
"because this is what AI agents need to do."

| Agent capability                                | Required surface                                 | v0?              | muvm components used                      |
|-------------------------------------------------|--------------------------------------------------|------------------|-------------------------------------------|
| See the desktop (live view to operator/agent platform) | wlroots-headless + `wlr_screencopy_v1` + vsock video stream + encoder (H.264/VP9) | yes | none (built into wlroots; encoder is Nimbus-original) |
| Click, type, scroll                             | virtio-input HID injection                       | yes              | `hidpipe_*` (host + guest)                |
| Browse web (Chrome, Firefox)                    | Headless browser in guest + see/click            | yes              | (above)                                   |
| Operate terminals, shells                       | Headless terminal in guest + see/click           | yes              | (above)                                   |
| Operate native Linux GUI apps (matched arch)    | Wayland app in guest + see/click                 | yes              | (above)                                   |
| Operate X11-only Linux apps                     | **In-guest XWayland** → wlroots → screencast     | yes              | none (XWayland is wlroots-builtin; muvm's X11 host-bridge is the wrong topology) |
| Operate **x86-only** Linux apps on aarch64 host | Box64 or FEX inside the guest                    | **follow-on**    | `guest/box64.rs`, `guest/fex.rs` (lift+mod) |
| Operate Windows apps via Wine/Proton (on aarch64 host) | Wine + Box64/FEX in guest                | **follow-on**    | (above)                                   |
| **Stream session live to remote viewer** (operator, agent platform) | Screencast + vsock video stream + low-latency transport (WebRTC/RTP) | yes (video); audio is **follow-on** | (screencast only)        |
| **Record full session for audit / training**    | Screencast → file (video v0) + audio capture → file (follow-on); A/V sync via timestamps | yes (video); audio is **follow-on** | (screencast only)        |
| **Capture audio output of apps inside guest** (Zoom remote audio, music, TTS, notifications) | Guest PipeWire virtual **sink** + vsock audio stream | **follow-on** | `guest/bridge/pipewire.rs`, `bridge/common.rs`, `bridge/mod.rs` (reference) |
| **Inject audio into guest as virtual mic** (agent speaks into apps, voice-first workflows) | Guest PipeWire virtual **source** ← vsock audio stream | **follow-on** | (above) |
| **Capture virtual webcam output from guest** (app inside guest produces video — meeting client camera, video editor preview, screen-cap app) | Guest V4L2 + PipeWire camera integration → vsock video stream | **follow-on** | (above; PipeWire handles V4L2) |
| **Inject virtual webcam into guest** (agent provides synthetic/recorded video as a camera for Zoom/Meet/Teams) | Guest `v4l2loopback`-class virtual camera ← vsock video stream | **follow-on** | (above) |
| Use GPU-accelerated apps inside the session     | Tier 3 GPU mediation policy (cross-tier)         | covered by GAW   | (none from muvm directly; see §7)         |
| Open multiple windows / desktop multitasking    | wlroots compositor manages it                    | yes              | none                                      |
| Copy/paste between sandbox and outside          | wl_data_device proxy over vsock                  | **follow-on**    | none (Nimbus-original wire format)        |
| Read files inside sandbox                       | virtiofs tenant share                            | yes              | `guest/mount.rs` (lift+mod)               |
| Reach the network from inside sandbox           | passt with `SandboxEgressPolicy`                 | yes              | `net.rs` host (lift+mod), `guest/net.rs` (lift+mod) |

The follow-on rows are **not Skip** — they are scheduled work with a
specific owning component already identified. The disposition table
in §6 records that ownership; this map records the agent-facing
justification.

## 6.2 Media streaming and recording surface

The capability map collapses several distinct media flows. This
section walks them explicitly so the v0 architectural decisions
don't accidentally close off the follow-on paths.

### Six media flows

| # | Direction | Carries          | v0?           | Mechanism                                            |
|---|-----------|------------------|---------------|------------------------------------------------------|
| 1 | guest → host (viewer) | screen video | **yes** | wlroots `wlr_screencopy_v1` → encoder → vsock stream |
| 2 | guest → host (recorder) | screen video + audio (synced) | video v0, audio follow-on | (1) plus audio path from (3) with PTS sync |
| 3 | guest → host          | app audio output | follow-on    | guest PipeWire virtual **sink** → vsock audio stream |
| 4 | host → guest          | virtual mic input | follow-on   | vsock audio stream → guest PipeWire virtual **source** |
| 5 | guest → host          | app webcam output | follow-on   | guest V4L2 + PipeWire camera → vsock video stream    |
| 6 | host → guest          | virtual webcam input | follow-on | vsock video stream → guest `v4l2loopback`-class device |

### v0 architectural slots that must be reserved now

To keep follow-on flows from forcing a wire-format rewrite, three
things land in v0 (CUS1):

1. **Spec type fields** on `LibkrunSessionSandboxSpec`:
   - `audio: Option<AudioStreamPolicy>` — None in v0; type exists.
   - `camera: Option<CameraStreamPolicy>` — None in v0; type exists.
   - `recording: Option<RecordingPolicy>` — supports video-only in v0;
     extended to A/V with a follow-on.
2. **Vsock channel ID reservations** in the control protocol:
   - Channel `MEDIA_SCREEN_OUT` (used in v0).
   - Channels `MEDIA_AUDIO_OUT`, `MEDIA_AUDIO_IN`, `MEDIA_CAM_OUT`,
     `MEDIA_CAM_IN` (reserved; not implemented in v0).
3. **Timestamp/PTS contract** on the screen stream so a future audio
   stream can be sync-recordable against it. Without this, follow-on
   recording is a forensics exercise rather than a feature.

### Encoding and transport (v0 vs follow-on)

| Concern              | v0 choice                              | Follow-on                                   |
|----------------------|----------------------------------------|---------------------------------------------|
| Screen video codec   | H.264 baseline (browser-native, hardware encoders ubiquitous); fallback MJPEG for diagnostic captures | Add VP9/AV1 for higher-quality recording; revisit for WebGPU/WebCodecs viewers |
| Screen capture path  | wlr_screencopy_v1 (compositor-direct)  | Re-evaluate Path B: PipeWire-screencast for unified abstraction once PipeWire follow-on lands |
| Live transport       | WebRTC over the operator API (low latency, browser viewer compatible) | RTP/SRT for non-browser viewers; HLS for asynchronous viewing |
| Recording sink       | Local MP4/MKV file in tenant virtiofs share; uploadable via Nimbus storage | Direct upload to object store; per-tenant retention policy |
| A/V sync             | Single video stream in v0 (no sync question); record PTS for future audio mux | When audio lands, mux via fragmented MP4 or MKV with shared PTS base |

### Why screencast stays compositor-direct in v0 (Path A vs Path B)

Two reasonable architectures:

- **Path A — compositor-direct screencast + PipeWire only for audio/camera.**
  v0 uses `wlr_screencopy_v1` straight from wlroots; audio/camera
  follow-ons go through guest PipeWire when those phases land.
  Lower v0 dependency footprint; two parallel media paths long-term.
- **Path B — PipeWire-for-everything.**
  Screen, audio, and camera all flow through guest PipeWire (the
  `xdg-desktop-portal` screencast portal can route a Wayland screen
  capture into PipeWire). One unified media abstraction; larger v0
  dependency.

**v0 takes Path A** because the screencast path needs to ship in v0
and pulling in PipeWire + portal infrastructure for one capability
inflates v0 scope. The decision is **reviewable** when the PipeWire
follow-on phase opens — if guest PipeWire is on the floor anyway and
the portal route would tighten encoder reuse, Path B becomes
attractive. The v0 spec/protocol slots (above) keep both doors open.

### Why X11 bridges are not on this list

None of the six media flows route through X11. Apps that happen to
be X11 clients connect to **in-guest XWayland**, which renders to
the wlroots compositor; the screencast and (future) PipeWire paths
capture them at the compositor layer, exactly like any Wayland app.
muvm's X11 host-bridge code forwards X11 traffic to a host X11
server, which is the wrong topology for headless capture. Operator
remote-viewing of the sandbox uses an in-guest VNC/RDP server
(wayvnc, xrdp) exposed via passt port mapping, again not muvm
bridge code.

## 7. Canonical GPU-mode selection pattern (from muvm)

The clean reference call shape for `krun_set_gpu_options2`, lifted
from `crates/muvm/src/bin/muvm.rs:188-205`:

```rust
let virgl_mode = match options.gpu_mode.unwrap_or_default() {
    GpuMode::Drm     => VIRGLRENDERER_DRM,
    GpuMode::Venus   => VIRGLRENDERER_VENUS | VIRGLRENDERER_RENDER_SERVER,
    GpuMode::Software => 0,
};
let virgl_flags = VIRGLRENDERER_USE_EGL
    | VIRGLRENDERER_NO_VIRGL    /* legacy method; interferes with software-only */
    | virgl_mode
    | VIRGLRENDERER_THREAD_SYNC
    | VIRGLRENDERER_USE_ASYNC_FENCE_CB;
unsafe {
    krun_set_gpu_options2(ctx_id, virgl_flags, vram_mib_as_u64 * MEGABYTE);
}
```

Plus the host-side guest-env injection that goes with Venus:

```rust
if options.gpu_mode == Some(GpuMode::Venus) {
    env.insert("MESA_LOADER_DRIVER_OVERRIDE".into(), "zink".into());
}
```

(zink routes Mesa GL through Vulkan, which is what carries to Venus.)

### Mapping to `GpuMediationPolicy`

The enum from `docs/plans/research/gpu-sandbox-backends.md` maps
cleanly:

- `GpuMediationPolicy::Venus`
  → `VIRGLRENDERER_VENUS | VIRGLRENDERER_RENDER_SERVER`
  + guest `MESA_LOADER_DRIVER_OVERRIDE=zink`
- `GpuMediationPolicy::NativeContext { driver: _, ioctl_filter: _ }`
  → `VIRGLRENDERER_DRM`
  + ioctl filter enforced at host-side virtio-gpu (out of scope of the
    `krun_set_gpu_options2` flag set; a separate ioctl-filter surface
    that **does not exist upstream yet** and is a candidate new fork
    patch — see §8)
- Software fallback for headless workloads → `0` (lavapipe/llvmpipe)

The always-on flags
(`USE_EGL | NO_VIRGL | THREAD_SYNC | USE_ASYNC_FENCE_CB`)
are the canonical default; Nimbus should match unless we discover a
reason to deviate.

## 8. Likely additional libkrun patches (open questions)

These are **anticipated** Nimbus-permanent fork patches that the new
Tier 2/3 plans imply. Listed as research questions, not commitments.

1. **passt-mode bind-address hook.** Equivalent of 15bcf49 for the
   passt path, so Tier 2/3 sessions can expose a guest port to a
   specific host bind address while keeping outbound through passt.
   Without this, Tier 2/3 cannot match the Tier 1 fail-closed
   semantics. Owner: CUS phase that wires session port exposure.
2. **Per-tenant ioctl filter at the virtio-gpu native-context boundary.**
   Native-context mode forwards driver ioctls (amdgpu/freedreno/asahi)
   straight to the host KMS device. Trusted tenants only. Need a
   per-tenant allowlist enforced at the host-side virtio-gpu device
   model. Upstream native-context support lives in
   `rutabaga_gfx/cross_domain`; Nimbus likely needs a thin filter
   shim. Owner: GAW phase that admits native-context tenants.
3. **virtio-gpu memfd path resilience.** Nimbus already has the
   cross_domain audio-glitches fix (`c4a0168`) and CMD_WRITE handling
   (`8fbeb48`) in its upstream pulls, but Tier 3 stress (large model
   weight streaming through shared memory) may surface more. Owner:
   GAW stress phase.
4. **vsock multi-descriptor TX chains** (`d1ed94c`, already in our
   upstream pulls). Important for the high-volume control channel
   Tier 2 (per-frame screencast frames) needs. **No new patch
   required**; cite this commit in CUS frame-capture phase.

## 9. Wire-up to active plans

- `docs/plans/computer-use-sandbox-plan.md` — CUS0 (libkrun fork patch
  inventory) is satisfied by §2-3 of this doc plus the muvm map in §5-6.
  CUS phases that wire screencast and input injection should cite this
  doc as the reference, not muvm directly.
- `docs/plans/gpu-accelerated-sandbox-plan.md` — GAW0 (libkrun GPU
  surface inventory) is satisfied by §3 and §7 of this doc. GAW phases
  that wire Venus default and native-context opt-in should cite §7's
  canonical call shape and §8's anticipated ioctl-filter patch.
- `docs/plans/firecracker-snapshot-invocation-backend-plan.md` — out of
  scope for libkrun fork (Firecracker is a separate VMM). The §2 patch
  list confirms there is no Firecracker code path shared with libkrun
  in our fork.

## 10. Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| muvm upstream drifts faster than we can rebase lifts | medium | low | Pin every lifted file to a recorded muvm commit SHA; rebase only when a feature gap demands it. |
| passt-mode bind-address gap forces a new fork patch | medium | medium | Treat as anticipated; add as a GAW0/CUS0 follow-up patch if a Tier 2/3 session needs guest-port host exposure. |
| Native-context ioctl filter has no upstream contract | high | medium | Build as a Nimbus-permanent fork patch; revisit upstreaming after design stabilizes. |
| CUDA/ROCm-on-libkrun research overturned later | medium | low | Memory `project_sandbox_three_tier_roadmap` already records "CUDA-only and ROCm-only need Linux fleet". Native-context for AMD covers ROCm aspirations later. |
| muvm's `hidpipe_*` is udev-driven, but Nimbus wants operator-driven injection | high | low | Anticipated as "Lift+mod" in §6 — operator commands replace udev events. |
| v0 cannot run **x86-only Linux apps** when host is aarch64 (Apple Silicon dev, Graviton cloud) | medium | medium for agent demos on dev hosts | Box64/FEX scheduled as a post-v0 phase per §6.1; both are MIT and in-guest. Operator-facing capability flag (`computer_use.x86_translation`) admits the workload only on aarch64 hosts where translation is wired. |
| v0 cannot drive **Zoom/Meet/Teams** style audio scenarios | medium | high for video-conferencing agent demos | Audio (PipeWire guest virtual source/sink + vsock audio stream) scheduled as a post-v0 phase per §6.1. The minimal viable computer-use loop intentionally excludes audio to keep v0 scope tight; agent demos that need audio are deferred to that follow-on phase. |
| Agent encounters X11-only Linux apps (GIMP, MATLAB classic, legacy CAD) | high | low | Solved by **in-guest XWayland** (wlroots-builtin), not by lifting muvm's host-X11-bridge. Documented in §6 and §6.1; no Nimbus code needed beyond the wlroots compositor choice in CUS3. |
| v0 wire protocol or spec type fails to reserve audio/camera/recording slots, forcing a breaking change when media follow-ons land | medium | high (operator API churn; client SDK breaks) | Reserved in CUS1: `LibkrunSessionSandboxSpec.audio/camera/recording` fields, vsock channel IDs `MEDIA_AUDIO_OUT/_IN/_CAM_OUT/_CAM_IN`, and PTS contract on the screen stream. See §6.2. |
| v0 screen recording lacks PTS so the future audio track can't be synced | medium | medium (audit replay quality) | PTS lands on the v0 screen stream even though audio is follow-on. See §6.2 "Encoding and transport." |
| Path A (compositor-direct screencast) and Path B (PipeWire-for-everything) diverge over time | low | low | Documented architecture review at the PipeWire follow-on phase. Path A is the v0 commitment; Path B is the candidate replacement. See §6.2. |

## 11. Follow-ups

- When CUS0 or GAW0 begins implementation, clone muvm to
  `~/src/github.com/AsahiLinux/muvm` (mirror layout convention with
  `~/src/github.com/containers/`) and pin every lift to a recorded
  commit SHA.
- Inventory `nimbus-crun` against `containers/crun` in a paired doc
  when the runtime-stack-plan reactivates; the two forks are tightly
  paired in our release lane.
- Reconfirm the GPU-mode flag set against upstream libkrun headers
  when a Tier 3 phase begins (in case `VIRGLRENDERER_*` constants
  drift).
- Promote four new Tier 2 follow-on phases when the v0 computer-use
  loop closes:
  - **CUS-Aud:** guest PipeWire virtual sink/source + vsock audio
    stream (channels `MEDIA_AUDIO_OUT`, `MEDIA_AUDIO_IN`) + operator
    surface for Zoom/Meet/Teams scenarios and voice-first workflows.
    Reference base: `guest/bridge/pipewire.rs`, `bridge/common.rs`,
    `bridge/mod.rs`. Unlocks media flows #3 and #4 (§6.2).
  - **CUS-Cam:** guest V4L2 + PipeWire camera integration +
    `v4l2loopback`-class virtual camera + vsock video streams
    (channels `MEDIA_CAM_OUT`, `MEDIA_CAM_IN`). Depends on CUS-Aud
    landing the PipeWire scaffold. Unlocks media flows #5 and #6.
  - **CUS-Rec:** synchronized A/V session recording. Depends on
    CUS-Aud for the audio track; reuses the v0 PTS contract on the
    screen stream. Encoder: fragmented MP4 or MKV with shared PTS
    base. Unlocks media flow #2 fully (currently video-only in v0).
  - **CUS-X86:** in-guest Box64/FEX wiring for x86-on-aarch64 host
    coverage (Apple Silicon dev, Graviton/Ampere cloud). Reference
    base: `guest/box64.rs`, `guest/fex.rs` per §6.1. Operator-facing
    capability flag (`computer_use.x86_translation`) gates admission
    by host arch.
- The in-guest **XWayland** decision (CUS3 should bring up wlroots in
  a config that includes XWayland) is the answer to "AI agents driving
  X11-only apps." Capture that in CUS3's compositor configuration when
  the phase opens.
