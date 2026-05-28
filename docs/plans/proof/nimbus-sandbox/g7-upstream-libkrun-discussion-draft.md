# G7 — Draft text for the upstream libkrun discussion issue

This file is the prepared text for the upstream-libkrun discussion
issue that the Fork-Health Guardrails §G7 (in
`docs/plans/nimbus-sandbox-plan.md`) requires opening before Band S
starts. The intent is friendly proactive engagement — we are not
asking the maintainers for anything, only signaling intent and
opening a channel for eventual upstreaming.

## Where to post

Open a discussion (not a pull request, not an issue) at:

> https://github.com/containers/libkrun/discussions

Category: **Ideas** (closest to the actual content) or **Q&A** if
Ideas isn't enabled.

Once posted, record the discussion URL in
[`docs/operating/fork-health.md`](../../../operating/fork-health.md)
in the "Upstream engagement (G7)" section, replacing the
_(to be opened at S0 start — URL recorded here)_ placeholder.

## Title

> Out-of-tree prototype: snapshot/restore + sub-ms `MAP_PRIVATE`
> session-fork for libkrun

## Body

```markdown
Hi libkrun maintainers — courtesy heads-up that we are starting an
out-of-tree prototype on a Nimbus fork of libkrun. The goal is to
make sure we are aligned with upstream's direction before the
prototype diverges in a way we'd later regret.

## What we are prototyping

Two related capabilities on top of the libkrun v1.18.x base:

1. **Snapshot/restore.** Save/Restore for vCPU + memory + the libkrun
   simple-device set (`virtio-block`, `virtio-net`, `virtio-vsock`,
   `virtio-rng`, `virtio-balloon`, `virtio-pmem`), with re-init on
   restore for the harder devices (`virtio-fs`, `virtio-gpu` with
   Venus/native-context, `virtio-input`, `virtio-snd`). Wire format
   modeled on Firecracker's `MicrovmState` envelope. UFFD lazy
   page-fault restore as a follow-on.
2. **Sub-ms `MAP_PRIVATE` session-fork.** A parent template VM is
   paused with its memory backed by `MAP_PRIVATE` on a sealed file;
   child VMs are spawned by mapping the same file `MAP_PRIVATE` in a
   new libkrun process, inheriting parent memory via copy-on-write
   at the kernel level. Pattern lifted from
   [zeroboot](https://github.com/zerobootdev/zeroboot) (Apache-2.0).

Linux KVM only for now; no macOS HVF code path is in scope.

## Why a heads-up here, not silence

We expect the bulk of the work (≥70% by LoC) to land in a sister
crate (`crates/nimbus-libkrun-snapshot` on our side), with the
upstream-touching delta confined to per-device `SaveState` /
`RestoreState` trait impls registered via a small sidecar module —
deliberately *not* methods added to upstream device structs. We're
designing the work so it rebases cleanly on `nimbus/v1.18.1` and
each upstream device change flows through without conflict.

If snapshot/restore is something upstream libkrun has interest in
adopting (we noticed issue #67 from 2022 was closed without an
implementation), we'd like to coordinate so our patches are
upstreamable rather than fork-only. If upstream's direction is
"snapshot/restore is out of scope for libkrun," that's a fine answer
too — we'll keep the prototype out-of-tree and just confirm we're
not stepping on a planned design.

## Specific questions

1. Is there current interest (or active work) on Save/Restore in
   libkrun upstream?
2. Are there device-internals refactors landing soon that we should
   plan around (so we don't carry per-device save logic against a
   struct shape that's about to change)?
3. Would you be open to PRs for small upstreamable patches we
   already have (TSI bind-address fix `15bcf49`, anticipated
   passt-mode bind-address) as a way to start collaborating before
   the larger snapshot work?

Happy to share the design doc privately if useful. Thanks for
maintaining libkrun — it's the right substrate for what we're
building.
```

## Once the discussion is open

1. Replace the placeholder URL in
   `docs/operating/fork-health.md` "Upstream engagement (G7)" with
   the live discussion URL.
2. Subscribe to notifications on the discussion.
3. If a maintainer reply suggests upstream interest, open a tracking
   issue against `docs/plans/nimbus-sandbox-plan.md` so the Band S
   patches are designed with upstream-ability in mind from the
   start.
4. If a maintainer reply suggests upstream is not interested, that's
   still useful — it confirms the out-of-tree posture is the right
   one and the sister-crate ratio target (G1, ≥70%) becomes more
   load-bearing as a rebase-friction insurance policy.

## Provenance

- Drafted: 2026-05-27, as part of the pre-band guardrail bootstrap
  for `docs/plans/nimbus-sandbox-plan.md` Band B.
- Authored by Nimbus engineering. No external review needed before
  posting — the text is intentionally low-stakes and reversible.
