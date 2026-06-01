# DUA0 Control Plane

status: in_progress
date: 2026-06-01
branch: codex/deno-rusty-v8-upstream-alignment
worktree: /Users/jack/src/github.com/nimbus/nimbus-worktrees/deno-rusty-v8-upstream-alignment
pr: pending first push and draft PR creation
verifier: scripts/verify-deno-rusty-v8-upstream-alignment.sh

## Proof Contract Checklist

1. **Row and status.** DUA0 is the active row for the Deno/rusty_v8 upstream
   alignment pause gate.
2. **Input baseline.** Baseline fork pins and NDS handoff are recorded in
   `dua0-baseline.md`.
3. **Disposition table.** Patch classification begins in DUA1; DUA0 records
   that no patch is replayed without a later disposition.
4. **Implementation evidence.** Worktree, branch, base branch, PR state,
   verifier, and resume protocol are recorded below.
5. **Focused verification.** DUA0 verifier and fork-state commands are recorded
   in the baseline proof.
6. **Broad verification.** DUA0 has no broad runtime rerun; DUA6 owns the
   broad pre/post compatibility rebaseline.
7. **Residual risks.** The control plane remains in progress until the draft PR
   URL is recorded.

## Row And Status

DUA0 is in progress. This control plane exists so a future context-compacted
agent can resume from the upstream-alignment pause gate without rediscovering
the branch topology or accidentally continuing NDS fixture greening on the
stale `v2.8.0-nimbus.15` fork stack.

## Input Baseline

| Field | Value |
| --- | --- |
| DUA worktree | `/Users/jack/src/github.com/nimbus/nimbus-worktrees/deno-rusty-v8-upstream-alignment` |
| DUA branch | `codex/deno-rusty-v8-upstream-alignment` |
| Base branch | `codex/node-default-runtime-support-hardening` |
| Base commit | `001d3c2dbe199d671184d2c9293c4d47d001c029` |
| NDS draft PR | `https://github.com/nimbus/nimbus/pull/10` |
| DUA draft PR | pending first push and draft PR creation |
| Upstream Deno target | `denoland/deno@v2.8.1` |
| Upstream rusty_v8 target | `denoland/rusty_v8@v149.2.0` |

DUA is intentionally stacked on the NDS checkpoint branch because the DUA plan,
verifier, and `.15` provenance updates are introduced by that checkpoint. The
DUA draft PR should therefore use base
`codex/node-default-runtime-support-hardening` until PR #10 lands, then retarget
or rebase as needed.

## Disposition Table

DUA0 does not classify source patches. It establishes this rule for DUA1-DUA4:
no Deno or `rusty_v8` patch may be replayed, dropped, or rewritten unless the
owning proof records exactly one allowed disposition and cites the source
location plus verification evidence.

## Implementation Evidence

Worktree creation command:

```console
git worktree add -b codex/deno-rusty-v8-upstream-alignment /Users/jack/src/github.com/nimbus/nimbus-worktrees/deno-rusty-v8-upstream-alignment codex/node-default-runtime-support-hardening
```

Result:

- Worktree created at
  `/Users/jack/src/github.com/nimbus/nimbus-worktrees/deno-rusty-v8-upstream-alignment`
- Branch created: `codex/deno-rusty-v8-upstream-alignment`
- HEAD: `001d3c2dbe199d671184d2c9293c4d47d001c029`

## Resume Protocol

1. Start in
   `/Users/jack/src/github.com/nimbus/nimbus-worktrees/deno-rusty-v8-upstream-alignment`.
2. Run `git status --short --branch` and confirm the branch is
   `codex/deno-rusty-v8-upstream-alignment`.
3. Read `docs/plans/deno-rusty-v8-upstream-alignment-plan.md`.
4. Resume the first DUA ledger row that is not `done`.
5. Keep each row's proof file current before switching rows.
6. Use `bash scripts/verify-deno-rusty-v8-upstream-alignment.sh` as the control
   gate. Expected output at DUA0 start is failing; closeout requires `0 failed`.
7. Do not continue NDS3 fixture promotion from the old `v2.8.0-nimbus.15` fork
   stack. Resume NDS only after DUA publishes upstream-aligned fork tags,
   repins Nimbus, and records the DUA6 rebaseline.

## Draft PR Bootstrap

The DUA branch should be pushed and opened as a draft PR with:

| Field | Value |
| --- | --- |
| Repository | `nimbus/nimbus` |
| Head | `codex/deno-rusty-v8-upstream-alignment` |
| Base | `codex/node-default-runtime-support-hardening` |
| Draft | yes |
| Title | `[codex] Align Deno and rusty_v8 upstream baselines` |

After the PR exists, update this file, `dua0-baseline.md`, and the DUA plan
ledger with the PR URL, then mark DUA0 `done`.

## Focused Verification

DUA0 focused verification is recorded in
`docs/plans/proof/deno-rusty-v8-upstream-alignment/dua0-baseline.md`.

## Broad Verification

No broad compatibility claim is made by DUA0. DUA6 must rerun broad Node
compatibility groups after the repin.

## Evidence Links

- `docs/plans/deno-rusty-v8-upstream-alignment-plan.md`
- `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua0-baseline.md`
- `scripts/verify-deno-rusty-v8-upstream-alignment.sh`
- `docs/plans/proof/node-default-runtime-support-hardening/nds3-official-fixture-promotion.md`

## Residual Risks

- DUA0 remains in progress until the DUA draft PR URL is recorded.
- The DUA branch is stacked on PR #10; if PR #10 is rebased or retargeted,
  retarget this branch before closeout.
