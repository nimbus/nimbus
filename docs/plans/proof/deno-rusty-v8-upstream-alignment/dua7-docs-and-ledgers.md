# DUA7 Docs And Ledgers

status: done
date: 2026-06-01
branch: codex/deno-rusty-v8-upstream-alignment
worktree: /Users/jack/src/github.com/nimbus/nimbus-worktrees/deno-rusty-v8-upstream-alignment
source worktree: /Users/jack/src/github.com/nimbus/deno
source branch: nimbus/v2.8.1
source tag: v2.8.1-nimbus.1
source commit: 18f76a9a19ab74d49d9a40037733cc4aec983d26
rusty_v8 tag: v149.2.0-nimbus.1
rusty_v8 commit: ce6663111a3ff8fde06bc04ba19bbbced60dbc8d
pr: https://github.com/nimbus/nimbus/pull/11
verifier: scripts/verify-deno-rusty-v8-upstream-alignment.sh

## Proof Contract Checklist

1. **Row and status.** DUA7 is done. Docs, ledgers, generated evidence
   handling, and the NDS handoff note are updated.
2. **Input baseline.** DUA7 starts from the DUA6 rebaseline: promoted Node24
   foundation groups are restored to pre-DUA counts and loader/context retains
   the known four watchpoints.
3. **Disposition table.** DUA7 has no new runtime patch dispositions; it
   records documentation disposition below.
4. **Implementation evidence.** Updated files and generated-evidence handling
   are recorded below.
5. **Focused verification.** Focused verification passed for docs/evidence:
   generated status output matches checked-in docs, and default support posture
   check passes.
6. **Broad verification.** DUA7 consumes the DUA6 broad rerun and records that
   no dashboard count moved after the repin regressions were fixed.
7. **Residual risks.** DUA8 still owns final validation commands, PR status,
   and handoff closeout.

## Input Baseline

| Field | Value |
| --- | --- |
| DUA6 proof | `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua6-node-compat-rebaseline.md` |
| Current Deno pin | `v2.8.1-nimbus.1#18f76a9a19ab74d49d9a40037733cc4aec983d26` |
| Current rusty_v8 pin | `v149.2.0-nimbus.1#ce6663111a3ff8fde06bc04ba19bbbced60dbc8d` |
| Checked-in status summary | `docs/architecture/runtime/node-compat-evidence/latest/status-summary.md` |
| Default support posture | `docs/architecture/runtime/node-default-support-posture.md` |

DUA6 final counts:

| Family | Result |
| --- | --- |
| `core-semantics` | `122 passed, 1 skipped, 0 failed` |
| `process-and-timing` | `48 passed, 0 skipped, 0 failed` |
| `streams-and-local-io` | `308 passed, 0 skipped, 0 failed` |
| `networking` | `268 passed, 0 skipped, 0 failed` |
| `loader-context` | `173 passed, 0 skipped, 4 failed` |

The generated full-corpus counts did not change because DUA6 restored the
promoted groups to their pre-DUA posture rather than promoting additional
fixtures.

## Disposition Table

| Artifact | Disposition | Evidence |
| --- | --- | --- |
| `docs/architecture/runtime/deno-fork-bump-ledger.md` | updated | Records the `v2.8.1-nimbus.1` / `v149.2.0-nimbus.1` DUA6 rebaseline, the two local Nimbus embedder fixes, and the remaining async_hooks / V8 wire-format watchpoints. |
| `docs/plans/node-default-runtime-support-hardening-plan.md` | updated | Records that the upstream-alignment pause gate is satisfied through DUA6 and that NDS3 should resume from the upstream-aligned fork baseline after DUA7/DUA8 handoff. |
| Generated Node compatibility `status-summary` | no count change | `make node-compat-status` generated a fresh target summary; `diff -u docs/architecture/runtime/node-compat-evidence/latest/status-summary.md target/node-compat/status/status-summary.md` returned no output. |
| Default support posture | no count change | `python3 scripts/runtime/node/default_support_posture.py --check` passed. |

## Implementation Evidence

Files updated in DUA7:

- `docs/architecture/runtime/deno-fork-bump-ledger.md`
- `docs/plans/node-default-runtime-support-hardening-plan.md`
- `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua7-docs-and-ledgers.md`

The fork bump ledger now records a dedicated DUA6 rebaseline section. The NDS
plan now states that the stale-fork pause gate is satisfied by the DUA
alignment through DUA6 and records the upstream-aligned baseline in the NDS3
execution log.

## Focused Verification

Commands:

```console
make node-compat-status
diff -u docs/architecture/runtime/node-compat-evidence/latest/status-summary.md target/node-compat/status/status-summary.md
python3 scripts/runtime/node/default_support_posture.py --check
bash scripts/verify-deno-rusty-v8-upstream-alignment.sh
```

Observed:

- First `make node-compat-status` attempt failed inside the sandbox because
  Python could not create `target/node-compat/status`.
- Rerun with sandbox escalation passed:
  `wrote node-compat status summary to .../target/node-compat/status/status-summary.json`
  and `wrote node-compat status markdown to .../target/node-compat/status/status-summary.md`.
- `diff -u docs/architecture/runtime/node-compat-evidence/latest/status-summary.md target/node-compat/status/status-summary.md` returned no output, proving the generated status-summary is unchanged.
- `python3 scripts/runtime/node/default_support_posture.py --check` returned
  `node default support posture: pass`.
- DUA verifier after DUA6/DUA7 updates: expected remaining failures are DUA8
  closeout and final all-rows-done gates.

## Broad Verification

DUA7 did not rerun the broad Node fixture groups because it is a docs/ledger
row. It consumes DUA6's broad reruns. The dashboard/status-summary handling is
explicitly a no-change proof: generated Node compatibility evidence still shows
Node24 `892 / 5198`, and the checked-in status summary is identical to the
freshly generated output.

## NDS Handoff

NDS should resume from the upstream-aligned baseline after DUA8 closeout:

- Deno fork: `nimbus/deno v2.8.1-nimbus.1`
- Deno commit: `18f76a9a19ab74d49d9a40037733cc4aec983d26`
- rusty_v8 fork: `nimbus/rusty_v8 v149.2.0-nimbus.1`
- rusty_v8 commit: `ce6663111a3ff8fde06bc04ba19bbbced60dbc8d`
- Current public Node24 count: `892 / 5198`
- Current promoted foundation posture: core, process/timing, streams/local-I/O,
  and networking green; loader/context remains the known four-watchpoint
  cluster.

Next NDS work should continue the wide-then-focused loop from the loader/context
residuals or the next broad full-corpus cluster. It must not restart from the
old `v2.8.0-nimbus.15` baseline.

## Residual Risks

- DUA8 still must run closeout validation: `cargo fmt --all --check`, strict
  docs refs, `git diff --check`, fork verifiers, DUA verifier, and PR status.
- The generated public counts did not move; NDS still owns raising Node24 to
  the well-supported default target.
- `make node-compat-status` requires filesystem write access to the DUA
  worktree's `target/` directory. In the Codex sandbox this needed escalation;
  the failure was not a generator defect.
