---
status: research
owners: sandbox-tier-roadmap
related:
  - docs/plans/nimbus-sandbox-plan.md
  - docs/plans/archive/computer-use-sandbox-plan.md
  - docs/plans/research/libkrun-session-sandbox.md
  - docs/plans/research/nimbus-libkrun-fork-inventory.md
  - docs/plans/research/gpu-sandbox-backends.md
  - docs/plans/agent-browser-service-plan.md
---

# Desktop-profile computer-use capabilities audit

This doc audits the desktop-profile computer-use plan
(`docs/plans/nimbus-sandbox-plan.md` Band D, superseding the archived
`docs/plans/archive/computer-use-sandbox-plan.md`) against what real
AI agent platforms exercise, identifies missing capabilities, and
records which gaps must be reserved in v0 architecture versus
deferred to named follow-on phases.

It is the companion to
`docs/plans/research/nimbus-libkrun-fork-inventory.md`: the
inventory doc covers what we lift from muvm and patch in our fork;
this doc covers what AI agents need beyond muvm.

## 1. Method and target platforms

Surveyed targets:

- **Anthropic Computer Use** (Claude with screenshot + click/type)
- **OpenAI Operator** (browser agent with structured page graph)
- **browser-use** (open-source agent framework, CDP-based)
- **OSWorld** (research benchmark; 369 real-world desktop tasks)
- **AgentBench** (LLM-as-agent benchmark suite)
- **Manus / Devin / Genspark** (commercial agent platforms,
  observed surface only)

Six dimensions audited per platform:

1. **Perception** — what the agent observes (pixels, text, structure)
2. **Input** — what the agent does (HID, drag/drop, clipboard write)
3. **State and lifecycle** — what persists, snapshot/branch/resume
4. **Outbound content** — streams/files leaving the VM
5. **Inbound content** — streams/files entering the VM
6. **Environment** — time, locale, identity, observability, security

## 2. Outbound content streams (the user's first question)

A complete enumeration of "stream content out from the VM" — beyond
the six media flows already in
`docs/plans/research/nimbus-libkrun-fork-inventory.md` §6.2.

### 2.1 Video

| Stream                             | Mechanism                                          | v0?         |
|------------------------------------|----------------------------------------------------|-------------|
| Live full-desktop screencast       | wlroots `wlr_screencopy_v1` → encoder → vsock      | yes         |
| Recorded full-desktop video        | Same + sink to file in tenant virtiofs             | yes         |
| Single-frame screenshot            | `wlr_screencopy_v1` one-shot → encoder → API       | yes (small spec extension on CUS3) |
| Per-window capture                 | `wlr_foreign_toplevel_management_v1` + screencopy targeted at toplevel | yes (small spec extension on CUS3) |
| Per-region (rectangle) capture     | screencopy with src rect                           | yes (CUS3 spec extension) |
| Multi-monitor / multi-display      | Headless wlroots can advertise N outputs           | follow-on (CUS-Multi) — defer; single virtual display in v0 |
| Virtual webcam **output** from guest (app produces video) | guest V4L2 + PipeWire camera → vsock | follow-on (CUS-Cam) |

### 2.2 Audio

| Stream                             | Mechanism                                          | v0?         |
|------------------------------------|----------------------------------------------------|-------------|
| Live audio (app output)            | guest PipeWire virtual sink → vsock                | follow-on (CUS-Aud) |
| Recorded audio                     | Same + sink to file                                | follow-on (CUS-Aud + CUS-Rec) |

### 2.3 Files and binary blobs

| Stream                             | Mechanism                                          | v0?         |
|------------------------------------|----------------------------------------------------|-------------|
| File pull (small, ad hoc)          | virtiofs tenant share                              | yes         |
| File pull (large, streaming)       | virtiofs share → host pipe to object store         | yes (operator CLI helper) |
| **Tenant output area** (auto-upload on write) | virtiofs-watched directory with object-store sync | follow-on (CUS-Out) |
| File generated in-guest (e.g., agent saves a PDF) | tenant output area                       | yes (via virtiofs); follow-on for auto-upload |
| Streaming/append-only logs         | guest writes to virtiofs path; host tails           | yes         |

### 2.4 Clipboard

| Stream                             | Mechanism                                          | v0?         |
|------------------------------------|----------------------------------------------------|-------------|
| Read text clipboard (out)          | wl_data_device + protocol proxy                    | follow-on (CUS-Clip) |
| Read image clipboard               | Same with image/png MIME                           | follow-on (CUS-Clip) |
| Read file clipboard                | Same with text/uri-list MIME                       | follow-on (CUS-Clip) |

### 2.5 Structured/observability streams (the big one — currently missing)

| Stream                             | Mechanism                                          | v0?         |
|------------------------------------|----------------------------------------------------|-------------|
| **Action log / trajectory**: structured `(timestamp, action, params, result)` events for every input + frame + system event | nimbus-guest structured logger → vsock | **yes baseline (CUS-Trace v0)**; richer schema follow-on |
| **Accessibility tree** for any app (AT-SPI for GTK/Qt; AX for browsers via CDP) | guest AT-SPI bridge → vsock; per-app CDP for browsers | follow-on (CUS-Acc) |
| Browser DOM access (Chrome DevTools Protocol direct)                  | CDP server in browser → passt port map        | follow-on (covered by `agent-browser-service-plan.md`) |
| Window / dialog / notification events | wlroots + libnotify bridge → vsock control      | follow-on (CUS-Evt) |
| Console/PTY logs                   | nimbus-guest log multiplexer → vsock                | yes (baseline; structured schema is CUS-Trace) |
| Network traffic inspection / pcap  | passt egress mirror → host pcap                    | follow-on (CUS-Net-Obs) |
| Performance counters (CPU/GPU/mem) | libkrun + host metrics → vsock                     | yes (Nimbus-engine reuses existing path) |

### 2.6 Why action log / trajectory is load-bearing for v0

Action logs unblock:

- **Audit:** what did the agent do, exactly?
- **Training data:** record real sessions to fine-tune models
- **RLHF / reward modeling:** trajectories are the unit of learning
- **Debugging:** replay step N with parameter X
- **Compliance:** SOX/HIPAA workflows need attributable agent actions

Every AI agent platform either has this (OpenAI Operator's "task
graph", Anthropic Computer Use's screenshot+action history) or
fakes it with screencast post-hoc OCR (low-fidelity).

**v0 baseline (CUS-Trace v0):** every HID event, every emitted frame
(with PTS matching §6.2 of inventory doc), every nimbus-guest
control-channel event gets a structured JSON line in a tenant-
accessible trajectory log. Schema is small (10 event types) and
versioned.

**Follow-on (CUS-Trace v1):** richer schema — accessibility-tree
snapshots at action boundaries, browser DOM diffs, semantic action
labels.

## 3. Inbound content streams

| Stream                             | Mechanism                                          | v0?         |
|------------------------------------|----------------------------------------------------|-------------|
| HID events                         | virtio-input via vsock                             | yes         |
| Files (upload to guest)            | virtiofs tenant share                              | yes         |
| Clipboard write (text)             | wl_data_device proxy                               | follow-on (CUS-Clip) |
| Clipboard write (image/file)       | Same with MIME types                               | follow-on (CUS-Clip) |
| Virtual mic input                  | vsock → guest PipeWire virtual source              | follow-on (CUS-Aud) |
| Virtual webcam input               | vsock → guest v4l2loopback-class device            | follow-on (CUS-Cam) |
| Drag-and-drop from host to guest   | wl_data_device drag protocol over vsock            | follow-on (CUS-DnD) |
| URL / intent injection             | `xdg-open` invoked via nimbus-guest control channel | yes (small CUS2 extension) |
| Bootstrap state                    | virtiofs preloaded with browser profile, cookies, credentials | yes (covered by spec; see §4) |

## 4. State and lifecycle gaps

### 4.1 Identity and state persistence (must decide in v0)

AI agents need both modes:

- **Ephemeral:** every session is fresh; nothing carries over.
  Default for untrusted agent runs.
- **Persistent (named profile):** browser cookies, IndexedDB, app
  logins persist across sessions of the same tenant + profile name.
  Required for "agent uses my Google account" workflows.

**v0 must reserve this in the spec.** `LibkrunSessionSandboxSpec`
gains a `state: StatePolicy` field:

```rust
enum StatePolicy {
    Ephemeral,
    Persistent { profile: ProfileId, mount: PersistentMountPolicy },
}
```

Implementation in v0 implements `Ephemeral` only; `Persistent` is a
named follow-on (CUS-Persist) that wires a per-profile virtiofs
mount backed by tenant-scoped storage.

### 4.2 Snapshot, branch, resume

libkrun gains snapshot/restore via Band S in
`docs/plans/nimbus-sandbox-plan.md` (the `lambda` profile relies on
it for cold-start). For the desktop profile (Band D):

- **Snapshot:** freeze the session at a decision point
- **Branch:** spawn N copies from the same snapshot (agents
  exploring multiple action sequences in parallel)
- **Resume:** restore a snapshot to continue later

OSWorld and academic benchmarks use this heavily. Manus and Devin
both expose "checkpoint" UX. **Not in v0** but should be reserved
architecturally:

- `Sandbox` trait gains `snapshot()` / `branch()` / `restore()`
  methods that return `unimplemented!()` in v0 for libkrun_session.
- Follow-on phase: **CUS-Snap**.

### 4.3 Pause / hibernate

Lighter than snapshot; freeze without writing state out. Defer
entirely to CUS-Snap.

## 5. Environment control (must address in v0)

| Concern              | v0 must do                              | Why                                         |
|----------------------|-----------------------------------------|---------------------------------------------|
| Time / timezone      | Spec-pinnable virtual clock + TZ; default real-time UTC | Testing reproducibility; deterministic agent replays |
| Locale (LANG/LC_*)   | Spec-pinnable; default `en_US.UTF-8`    | i18n testing; non-Latin keyboards depend on it |
| Display resolution + DPI | Spec-pinnable (default 1920×1080 @ 1.0×) | Pixel-accurate clicks; OCR consistency |
| Tenant identity injection | Workload identity via existing `TenantIsolationContext` | Audit + provider-auth chains |
| Secrets injection    | Spec-pinned secret refs; gated by `agent.secrets` grant | Agent runs apps that need credentials |
| Egress IP / DNS      | passt + `SandboxEgressPolicy`           | Already in plan; reuse                      |

The clock and locale items are missed in the current plan. They're
small in code but load-bearing for reproducibility — agent
trajectories that run "today at 2pm" can't replay if the clock
drifts.

## 6. Perception gaps (must address in v0)

| Gap                                | v0 disposition                                     | Why                                         |
|------------------------------------|----------------------------------------------------|---------------------------------------------|
| Active-window-only capture         | CUS3 small extension: `wlr_foreign_toplevel_management_v1` + targeted screencopy | Reduces frame size; agents want noise-free input |
| Per-region capture                 | CUS3 small extension: src rect                     | OCR/VLM efficiency                          |
| DPI/scaling control                | Spec field on `HeadlessCompositorPolicy`           | Pixel-accurate clicks                       |
| Multi-monitor                      | **Defer** (CUS-Multi follow-on)                    | Not blocking for v0; wlroots supports advertising N outputs |
| Accessibility tree (AT-SPI)        | **Follow-on (CUS-Acc)**                            | Structured perception; major modern-agent capability |
| Browser DOM (CDP)                  | **Follow-on** — owned by `agent-browser-service-plan.md` | Existing plan covers this surface           |
| OCR helpers                        | **Out of scope** for nimbus-sandbox; runs in operator API layer | Not a sandbox capability                    |

## 7. Input gaps

| Gap                                | v0 disposition                                     |
|------------------------------------|----------------------------------------------------|
| Keyboard with full modifier matrix | yes; virtio-input handles it                        |
| Mouse click/hold/drag/scroll       | yes; virtio-input handles it                        |
| IME for CJK / complex scripts      | Spec field; v0 enables ibus/fcitx via guest config; agents can `xdg-open` text or paste via clipboard |
| Touch / multi-touch / pen / stylus | **Defer** — no AI agent use case yet                |
| Drag-and-drop                      | **Follow-on (CUS-DnD)**                            |
| Clipboard write                    | **Follow-on (CUS-Clip)**                           |
| URL / intent injection             | yes; nimbus-guest exposes `xdg-open` over vsock      |

## 8. Security gaps

| Gap                                | v0 disposition                                     |
|------------------------------------|----------------------------------------------------|
| Egress policy                      | yes; `SandboxEgressPolicy`                          |
| Filesystem isolation               | yes; virtiofs per-tenant root                       |
| GPU mediation                      | `gpu` profile (Band G); cross-band                  |
| Tenant isolation                   | yes; existing `TenantIsolationContext`              |
| **Screen-content redaction**       | **Reserve in v0** — `RedactionPolicy` field on spec; default `None`. Pipeline: regex over OCR output, rect masking by app id, secret-token deny-list. Implementation **follow-on (CUS-Red)**. |
| **Action-log redaction**           | Same `RedactionPolicy` applies to trajectory log entries that contain typed input (esp. password fields); v0 baseline must support a `redact_typed_input_in_password_focus: bool` flag. |
| File-egress controls               | Covered by virtiofs share scoping                   |
| Capture pause / privacy mode       | **Follow-on (CUS-Red)** — operator can freeze the screencast stream temporarily |

## 9. Lifecycle gaps

| Gap                                | v0 disposition                                     |
|------------------------------------|----------------------------------------------------|
| Session start / stop               | yes; existing                                      |
| Snapshot                           | **Follow-on (CUS-Snap)** — `Sandbox::snapshot()` reserved in v0 trait, `unimplemented!()` body |
| Branch from snapshot               | **Follow-on (CUS-Snap)**                           |
| Resume from snapshot               | **Follow-on (CUS-Snap)**                           |
| Pause / hibernate                  | **Folded into CUS-Snap**                           |
| Idle timeout / auto-stop           | yes; `SessionLifetime` covers it                    |
| Forced kill                        | yes; existing libkrun lifecycle                     |

## 10. Gap summary table — v0 additions to existing CUS phases

These are **not new phases** — they are call-outs to the existing
phase owners that v0 must include something that's currently missing
in the plan stub.

| Existing phase | v0 addition required | Why |
|---|---|---|
| CUS1 (types) | `state: StatePolicy` (Ephemeral / Persistent), `time: TimePolicy`, `locale: LocalePolicy`, `display.dpi`, `audio: Option<AudioStreamPolicy>` (reserved), `camera: Option<CameraStreamPolicy>` (reserved), `recording: Option<RecordingPolicy>`, `trajectory: TrajectoryPolicy`, `redaction: RedactionPolicy`, `events: EventStreamPolicy` | Don't break the wire format when follow-ons land |
| CUS2 (nimbus-guest) | `xdg-open` URL/intent injection over vsock; structured trajectory log emitter; password-focus redaction flag handling | Inbound URL surface; baseline action log |
| CUS3 (compositor + screencast) | Per-window and per-region capture; DPI/scaling; PTS on every frame; single-frame screenshot API | Agent perception efficiency + recording sync |
| CUS4 (virtio-input) | IME enablement option in guest config; input event echoed into trajectory log | CJK input; audit baseline |
| CUS5 (passt + egress) | Reserve channel IDs `MEDIA_AUDIO_OUT/_IN/_CAM_OUT/_CAM_IN`, `TRACE`, `EVENTS`, `CLIP`, `DND` | Don't force protocol rewrite |
| CUS6 (virtiofs) | Tenant output area pattern (watched dir); large-file pull helper | Outbound file streaming |
| CUS7 (lifecycle) | `Sandbox::snapshot()` / `branch()` / `restore()` trait methods (return `unimplemented!()` in v0) | Reserve lifecycle API |
| CUS8 (operator CLI) | `nimbus sandbox session trajectory`, `screenshot`, `record` (video-only in v0), `file pull` subcommands | Operator UX                          |

## 11. New follow-on phases

These are above and beyond the four already named in
`docs/plans/research/nimbus-libkrun-fork-inventory.md` §11
(CUS-Aud, CUS-Cam, CUS-Rec, CUS-X86).

| Phase | Scope | Depends on |
|---|---|---|
| **CUS-Acc** | Accessibility tree export (AT-SPI for GTK/Qt; AX for browsers via CDP). Vsock channel `EVENTS` + structured tree snapshot at action boundaries. Folds DOM access for non-browser apps. | CUS-Trace v0 schema |
| **CUS-Clip** | Clipboard read/write, MIME-aware (text, image, file URIs). Vsock channel `CLIP` + wl_data_device proxy. | none |
| **CUS-Snap** | Session snapshot, branch, resume via libkrun snapshot/restore. `Sandbox` trait body implementations. | none |
| **CUS-Trace v1** | Richer trajectory schema: accessibility-tree snapshots, browser DOM diffs, semantic action labels. | CUS-Acc |
| **CUS-Red** | Screen-content redaction pipeline (OCR-based regex masking, rect masking by app id, secret-token deny-list) + capture pause / privacy mode. | CUS-Acc useful but not required |
| **CUS-Persist** | Persistent named profiles (StatePolicy::Persistent): per-profile virtiofs mount backed by tenant storage. | CUS6 tenant output area |
| **CUS-DnD** | Drag-and-drop host↔guest. Vsock channel `DND` + wl_data_device drag protocol. | CUS-Clip |
| **CUS-Evt** | Window / dialog / notification events. Vsock channel `EVENTS` + wlroots foreign-toplevel + libnotify bridge. | CUS-Trace v0 |
| **CUS-Out** | Tenant output area auto-upload (watched virtiofs dir → object store). | CUS6 |
| **CUS-Net-Obs** | passt egress mirror + host pcap export for compliance/inspection. | CUS5 |
| **CUS-Multi** | Multi-monitor / multi-output (advertise N wlroots outputs). | CUS3 |

## 12. Concrete plan stub edits

The unified `docs/plans/nimbus-sandbox-plan.md` Band D phases (which
supersede the archived CUS1–CUS8 phases in
`docs/plans/archive/computer-use-sandbox-plan.md`) each gain a v0
addition per §10 above. The plan's "Future phases" section gains the
new follow-on phases from §11.

Wording for the new follow-on phases is left to the plan author at
the time CUS-* opens; this doc records scope and dependency only.

## 13. Risk register additions

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| v0 ships without trajectory log baseline; agent audit story is just "we have video" | high | high (enterprise audit, training data, compliance) | CUS-Trace v0 baseline lands inside CUS2, not as a separate phase. Schema documented at CUS1. |
| v0 ships without identity/state persistence policy; first persistent-profile demand forces spec break | high | medium | `StatePolicy::{Ephemeral, Persistent}` reserved in CUS1; v0 implements Ephemeral only. |
| v0 ships without time/locale control; agent replays are non-reproducible | medium | medium | `TimePolicy`/`LocalePolicy` reserved in CUS1; v0 supports defaults plus tenant override. |
| Accessibility-tree perception lands late; agents stuck on pixel-only + OCR | medium | medium for non-browser apps; low for browser-dominant agents | CUS-Acc named as the first follow-on after the v0 media trio (CUS-Aud, CUS-Cam, CUS-Rec). |
| Screen redaction landing as a follow-on means v0 cannot serve credential-handling tenants | medium | high for those tenants | Reserve `RedactionPolicy` in CUS1; v0 tenants that need redaction are gated at admission until CUS-Red lands. |
| Snapshot/branch lifecycle reserved as trait methods but never implemented; trait surface rots | low | low | CUS-Snap is named and scheduled; trait methods documented as `unimplemented!()` until then. |

## 14. Open questions for v0

1. Is "video-only recording" in v0 actually useful, or does the
   absence of audio break the use case enough that recording should
   wait for CUS-Aud? **Tentative answer:** ship video-only recording
   in v0; it's useful for debugging the input path and for
   trajectory replay sanity-checking, even without audio.
2. Should the trajectory log schema be Nimbus-original or align
   with an existing format (OpenAI Operator JSON, Anthropic Computer
   Use action format, OSWorld trajectory format)? **Tentative
   answer:** Nimbus-original superset, with a v0 exporter to at
   least one of those formats for benchmark compatibility. Decide
   at CUS1.
3. What's the cap on session duration in v0? Snapshot lands in
   CUS-Snap, so long sessions before then risk losing work.
   **Tentative answer:** advisory cap of 4 hours in v0; clearly
   documented as a limit lifted by CUS-Snap.

## 15. Bottom line

**Outbound streaming coverage (the user's first question):** the
six media flows in §6.2 of the inventory doc cover video and audio.
Files are covered by virtiofs + a tenant output area pattern.
**Action log / trajectory + accessibility tree + clipboard read**
are three additional outbound streams that the plan was missing;
they are wired in as a v0 baseline (trajectory) and three named
follow-ons (CUS-Acc, CUS-Clip, CUS-Out).

**Missing core capabilities (the user's second question):** seven
significant gaps were found — trajectory log (now v0), state
persistence policy (now v0 spec), time/locale control (now v0
spec), per-window/region capture (now v0), DPI/scaling control
(now v0), redaction policy (now v0 spec, follow-on body),
snapshot/branch/resume (follow-on, reserved trait). Plus six
smaller follow-ons (CUS-Acc, CUS-Clip, CUS-Evt, CUS-DnD, CUS-Out,
CUS-Net-Obs, CUS-Multi).

Net effect on v0: ten small additions to existing CUS1-CUS8
phases. No new v0 numbered phases. Eleven named follow-on phases.
