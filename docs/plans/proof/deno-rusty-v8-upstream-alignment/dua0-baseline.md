# DUA0 Baseline

status: in_progress
date: 2026-06-01
branch: codex/deno-rusty-v8-upstream-alignment
worktree: /Users/jack/src/github.com/nimbus/nimbus-worktrees/deno-rusty-v8-upstream-alignment
pr: pending first push and draft PR creation
verifier: scripts/verify-deno-rusty-v8-upstream-alignment.sh

## Proof Contract Checklist

1. **Row and status.** DUA0 is in progress in the dedicated DUA worktree on
   `codex/deno-rusty-v8-upstream-alignment`.
2. **Input baseline.** Current fork SHAs, dirty files, Nimbus pins, upstream
   targets, and Node compatibility counts are recorded below.
3. **Disposition table.** DUA0 does not classify patches; DUA1 owns the full
   disposition table. The DUA0 placeholder is explicit below.
4. **Implementation evidence.** The verifier, proof directory, branch, and
   baseline handoff evidence are recorded below.
5. **Focused verification.** Scaffold verifier and fork provenance commands are
   recorded below.
6. **Broad verification.** DUA0 has no broad runtime rerun; broad pre/post
   reruns begin in DUA6 after fork alignment.
7. **Residual risks.** The branch must record a draft PR URL before DUA0 can
   move to done.

## Row And Status

DUA0 bootstraps the Deno/rusty_v8 upstream-alignment plan that NDS3 now treats
as the pause gate before further serious fixture promotion. This row is not
closed until the draft PR URL is recorded in both DUA0 proof files and the plan
ledger.

## Input Baseline

Nimbus control branch:

| Field | Value |
| --- | --- |
| DUA worktree | `/Users/jack/src/github.com/nimbus/nimbus-worktrees/deno-rusty-v8-upstream-alignment` |
| DUA branch | `codex/deno-rusty-v8-upstream-alignment` |
| Branch base | `codex/node-default-runtime-support-hardening` |
| Base commit | `001d3c2dbe199d671184d2c9293c4d47d001c029` |
| NDS PR | `https://github.com/nimbus/nimbus/pull/10` |
| DUA draft PR | pending first push and draft PR creation |

Current Nimbus runtime pins:

| Fork | Current Nimbus tag | Current SHA | Cargo source proof |
| --- | --- | --- | --- |
| `nimbus/deno` | `v2.8.0-nimbus.15` | `1f101bf0032a223463507f500ddd236afebd9fcc` | `Cargo.toml` and `Cargo.lock` resolve Deno-family patch-sensitive crates to this tag/SHA. |
| `nimbus/rusty_v8` | `v149.0.0-nimbus.1` | `9b77553883f1117ab3df62709b8673b803ed721b` | `Cargo.toml` and `Cargo.lock` resolve `v8` to this tag/SHA. |

Local fork state at DUA0 start:

| Fork | Local path | Branch | HEAD | Exact tag | Dirty files |
| --- | --- | --- | --- | --- | --- |
| `nimbus/deno` | `/Users/jack/src/github.com/nimbus/deno` | `nimbus/v2.8.0` | `1f101bf0032a223463507f500ddd236afebd9fcc` | `v2.8.0-nimbus.15` | none observed by `git status --short --branch` |
| `nimbus/rusty_v8` | `/Users/jack/src/github.com/nimbus/rusty_v8` | `nimbus/v149.0.0` | `9b77553883f1117ab3df62709b8673b803ed721b` | `v149.0.0-nimbus.1` | none observed by `git status --short --branch` |

Upstream alignment targets:

| Upstream repo | Target |
| --- | --- |
| `denoland/deno` | `v2.8.1` |
| `denoland/rusty_v8` | `v149.2.0` |

Current generated official fixture posture from the NDS checkpoint:

| Lane | Role | Upstream | Passed | Vendored | Pass rate |
| --- | --- | --- | ---: | ---: | ---: |
| `node20` | legacy | `v20.20.2` | `893` | `1308` | `68.3%` |
| `node22` | supported | `v22.22.3` | `1023` | `4773` | `21.4%` |
| `node24` | default | `v24.16.0` | `892` | `5198` | `17.2%` |
| `node26` | current | `v26.2.0` | `0` | `5578` | `0.0%` |

Current NDS3 loader/context checkpoint:

| Command | Result |
| --- | --- |
| `cargo test -p nimbus-runtime node24_default_lane_loader_context_watchpoint -- --ignored --nocapture --test-threads=1` | `173 passed, 0 skipped, 4 failed` |
| `cargo test -p nimbus-runtime node24_loader_context_global_paths_preserve_local_precedence_regression -- --nocapture --test-threads=1` | `1 passed` |
| `cargo test -p nimbus-runtime loader_context_followup_v8_green_batch_fixture -- --nocapture --test-threads=1` | `3 passed` |

Remaining loader/context failures handed to DUA/NDS continuation:

- `test-async-hooks-enable-recursive.js`
- `test-async-hooks-enable-before-promise-resolve.js`
- `test-async-hooks-enable-during-promise.js`
- `test-v8-serdes.js`

## Disposition Table

DUA0 does not classify fork patches. DUA1 must classify every carried Deno
patch from the first `v2.8.0-nimbus.*` carry through `v2.8.0-nimbus.15`, plus
any dirty fork work, with exactly one allowed disposition:
`upstream-replaced`, `upstream-adjacent`, `nimbus-embedding-specific`,
`still-needed-node-gap`, or `drop-no-longer-needed`.

| Scope | DUA0 disposition |
| --- | --- |
| Deno fork patch stack through `v2.8.0-nimbus.15` | deferred to DUA1 |
| Deno `.15` loader/crypto/V8 work | deferred to DUA3 reevaluation after upstream 2.8.1 baseline |
| `rusty_v8` `v149.0.0-nimbus.1` locker/safety stack | deferred to DUA4 |

## Implementation Evidence

DUA0 creates the execution surface for the alignment plan:

- Dedicated worktree:
  `/Users/jack/src/github.com/nimbus/nimbus-worktrees/deno-rusty-v8-upstream-alignment`
- Dedicated branch: `codex/deno-rusty-v8-upstream-alignment`
- Stacked base branch: `codex/node-default-runtime-support-hardening`
- Control gate: `scripts/verify-deno-rusty-v8-upstream-alignment.sh`
- Required proof directory:
  `docs/plans/proof/deno-rusty-v8-upstream-alignment/`

## Focused Verification

DUA0 input commands already run before this proof was written:

```console
git status --short --branch
git -C /Users/jack/src/github.com/nimbus/deno status --short --branch
git -C /Users/jack/src/github.com/nimbus/deno rev-parse HEAD
git -C /Users/jack/src/github.com/nimbus/deno describe --tags --exact-match HEAD
git -C /Users/jack/src/github.com/nimbus/rusty_v8 status --short --branch
git -C /Users/jack/src/github.com/nimbus/rusty_v8 rev-parse HEAD
git -C /Users/jack/src/github.com/nimbus/rusty_v8 describe --tags --exact-match HEAD
```

The DUA verifier scaffold was run after this DUA0 baseline/control-plane proof
was created:

```console
bash scripts/verify-deno-rusty-v8-upstream-alignment.sh
```

Observed: `3 passed, 20 failed`. The passes prove the scaffold can see the
DUA0 baseline/control-plane details, fork bump ledger, and DUA docs; the
failures are expected because DUA0 still needs the draft PR URL and DUA1-DUA8
are not complete yet.

## Broad Verification

DUA0 is a baseline/control-plane row and does not claim a broad runtime
compatibility improvement. DUA6 must run the broad Node compatibility rebaseline
after the upstream-aligned fork tags are published and Nimbus is repinned.

## Evidence Links

- `docs/plans/deno-rusty-v8-upstream-alignment-plan.md`
- `docs/plans/node-default-runtime-support-hardening-plan.md`
- `docs/plans/proof/node-default-runtime-support-hardening/nds3-official-fixture-promotion.md`
- `docs/architecture/runtime/deno-fork-bump-ledger.md`
- `docs/architecture/runtime/node-compat-evidence/latest/status-summary.md`
- `docs/architecture/runtime/node-default-support-posture.md`
- `scripts/verify-deno-rusty-v8-upstream-alignment.sh`
- `scripts/verify-deno-fork-provenance.sh`
- `scripts/verify-deno-fork-upstream-policy.sh`

## Residual Risks

- DUA0 still needs the DUA draft PR URL recorded before it can be marked done.
- DUA1 may find that some `.15` fixes overlap upstream 2.8.1 and should be
  dropped instead of replayed.
- DUA4 may decide to hold `rusty_v8` at `v149.0.0-nimbus.1` if upstream
  `v149.2.0` does not add consumed behavior beyond fixes Nimbus already carries
  or if rebasing would risk the `Locker` / `UnenteredIsolate` safety stack.
