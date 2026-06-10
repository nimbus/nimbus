# Nimbus-libkrun fork health

This document records the quarterly fork-health metric for the
`nimbus-libkrun` fork at `~/src/github.com/nimbus/nimbus-libkrun`.
The unified sandbox plan
([`docs/plans/nimbus-sandbox-plan.md`](../plans/nimbus-sandbox-plan.md))
concentrates roughly ten thousand LoC of Nimbus-permanent delta into
this fork. The Fork-Health Guardrails (§G5/§G6/§G7) require quarterly
measurement, a named maintenance budget, and proactive upstream
engagement so the fork stays maintainable.

## Maintenance budget (G6)

Steady-state maintenance budget is **0.5–1 engineer-week per quarter**.
This covers:

- Upstream libkrun rebase work (track upstream releases monthly).
- Security-patch propagation from upstream into our pin.
- New per-device `SaveState` / `RestoreState` sidecar impls when
  upstream adds a virtio device.
- The G5 quarterly update of this document.

Major libkrun version bumps (e.g. v1.19, v2.0) may consume one
sprint. If per-quarter actuals exceed **1.5 engineer-weeks for two
consecutive quarters**, that is a stop signal — open an architecture
review of the sister-crate ratio and the upstream-touch surface
before scheduling the next quarter's work.

## Quarterly metric (G5)

For each quarter, record:

- **LoC delta vs upstream:**
  `cd ~/src/github.com/nimbus/nimbus-libkrun && git diff --shortstat <upstream-tag>..HEAD`.
- **Time since last upstream pull:** clock between newest libkrun
  upstream tag and the current Nimbus pin commit (use upstream tag
  date vs. our pin commit date).
- **Rebase pain:** subjective 1–5 from the most recent rebase
  attempt (5 = "we considered abandoning"). Default 1 if no rebase
  occurred this quarter.
- **Upstreamable patches awaiting submission:** count of identified
  patches in our fork that have not yet been sent upstream.

Early-warning signal: a trend of LoC delta ↑↑, time since pull ↑,
*or* rebase pain ≥4 two quarters in a row means we are losing the
race — schedule a sprint to upstream patches and pull current
upstream before the next quarterly slot.

| Quarter | Upstream tag (date) | Nimbus pin (date) | Time since pull | LoC delta vs upstream | Rebase pain (1-5) | Upstreamable patches | Sister-crate share | Notes |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-Q2 | `v1.18.1` (2026-05-18) | `v1.18.1-nimbus.1` @ `a8daa865` (2026-05-25) | 7 days | 15 files, +939 / -81 | 1 (no rebase this quarter — clean cherry-pick onto v1.18.1) | 2 (TSI bind-address `15bcf49` already-landed; anticipated passt-mode bind-address per `nimbus-libkrun-fork-inventory.md` §8 item #1) | n/a (sister crate `crates/nimbus-libkrun-snapshot` lands in Band S0+) | First quarterly entry. Pre-band guardrail bootstrap seeded against `v1.18.1`. |

## Sister-crate ratio (G1)

Once Band S phases begin, each S-phase closeout reports the
sister-crate vs in-fork LoC ratio. Target: **≥70% sister-crate share
by net-new LoC** in `crates/nimbus-libkrun-snapshot` (and any other
`crates/nimbus-libkrun-*` sister crate). Falling below 70% on a
single phase is a re-design signal for that phase, not a
continue-anyway signal.

| Phase | Sister LoC (new) | In-fork LoC (new) | Sister share | Status |
| --- | --- | --- | --- | --- |
| S0 | — | — | — | not started |
| S1 | — | — | — | not started |
| S2 | — | — | — | not started |
| S3 | — | — | — | not started |
| S4 | — | — | — | not started |
| S5 | — | — | — | not started |

## Upstream engagement (G7)

Engage libkrun maintainers proactively, not reactively.

- **Snapshot/restore discussion issue:** _(to be opened at S0 start —
  URL recorded here)_. Draft text ready at
  [`docs/plans/proof/nimbus-sandbox/g7-upstream-libkrun-discussion-draft.md`](../plans/proof/nimbus-sandbox/g7-upstream-libkrun-discussion-draft.md).
- **TSI bind-address (`15bcf49`):** local-only patch from the fork
  inventory; upstream candidacy pending S0 closeout.
- **Anticipated passt-mode bind-address:** likely future fork patch
  per `docs/plans/research/nimbus-libkrun-fork-inventory.md` §8 item
  #1; upstream candidacy decided when the patch stabilizes.

Track upstream releases monthly. Patch releases merge within one
quarter. Minor releases merge within two quarters.

## How to update this file

At the close of each quarter, append a new row to the quarterly
metric table. Compute the numbers with:

```sh
cd ~/src/github.com/nimbus/nimbus-libkrun
upstream_tag="$(git tag -l 'v*' | grep -v -- '-nimbus\.' | sort -V | tail -1)"
nimbus_pin="$(git describe --tags --abbrev=0)"
echo "Upstream: ${upstream_tag} ($(git log -1 --format=%ai "${upstream_tag}"))"
echo "Nimbus pin: ${nimbus_pin} ($(git log -1 --format=%ai "${nimbus_pin}"))"
git diff --shortstat "${upstream_tag}..HEAD"
```

If you closed an S-phase this quarter, also fill the sister-crate
ratio row from the phase closeout report.

If a libkrun rebase happened this quarter, set rebase pain
honestly — even a `1` is useful baseline data for noticing the
trend over time. If you considered abandoning the rebase, it's a
`5`.
