# DUA8 Closeout

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

1. **Row and status.** DUA8 is done locally. The DUA control plane is closed
   and handoff back to NDS is recorded.
2. **Input baseline.** DUA8 starts from DUA7 with Deno/rusty_v8 repinned,
   DUA6 broad rebaseline complete, generated status evidence unchanged, and
   NDS handoff notes updated.
3. **Disposition table.** DUA8 has no new runtime patch dispositions; it
   records validation and remote-status disposition below.
4. **Implementation evidence.** Final validation commands, PR status, and
   handoff are recorded below.
5. **Focused verification.** Closeout-focused validation passed: format, docs
   refs, diff check, fork provenance, fork upstream policy, generated status
   no-change check, and default support posture check.
6. **Broad verification.** Broad runtime verification is inherited from DUA6.
7. **Residual risks.** Hosted PR checks must rerun after the local DUA closeout
   commit is pushed. The currently observed hosted failures are from the older
   pushed PR revision and are recorded below.

## Input Baseline

DUA8 consumes:

- DUA5 release and repin proof for `nimbus/deno v2.8.1-nimbus.1`.
- DUA6 broad rebaseline after the repin.
- DUA7 docs, ledger, generated-evidence, and NDS handoff updates.

Current handoff baseline:

| Field | Value |
| --- | --- |
| Deno fork | `nimbus/deno v2.8.1-nimbus.1` |
| Deno commit | `18f76a9a19ab74d49d9a40037733cc4aec983d26` |
| rusty_v8 fork | `nimbus/rusty_v8 v149.2.0-nimbus.1` |
| rusty_v8 commit | `ce6663111a3ff8fde06bc04ba19bbbced60dbc8d` |
| Node24 generated public count | `892 / 5198` |
| Promoted Node24 foundation posture | Core, process/timing, streams/local-I/O, and networking green; loader/context has the known four watchpoints. |

## Disposition Table

| Closeout item | Disposition | Evidence |
| --- | --- | --- |
| Local validation | passed | `cargo fmt --all --check`, strict docs refs, `git diff --check`, fork provenance, and fork upstream policy passed. |
| Generated evidence | no count change | `make node-compat-status` output matched checked-in `status-summary.md`; default support posture check passed. |
| Node FaaS generated docs | repaired after CI feedback | PR #11 exposed stale generated Node evidence docs. `publish_docs.py` updated `docs/runtimes/nodejs/evidence/latest.md` to the checked-in status timestamp. `release_train.py publish` updated `node-release-train.{json,md}` to the checked-in dashboard digests and `canary_check_count: 0`, matching `dashboard-summary.json` rather than preserving stale `101` metadata. |
| PR status | hosted rerun required | PR #11 is open, draft, mergeable, and currently shows three failing checks from the older pushed revision. The updated closeout commit must be pushed so CI can rerun on the DUA6 fixes. |
| NDS handoff | ready after DUA branch push | NDS plan now records the upstream-aligned baseline and must resume from `v2.8.1-nimbus.1` / `v149.2.0-nimbus.1`. |

## Implementation Evidence

Files changed across DUA5-DUA8:

- `Cargo.toml`
- `Cargo.lock`
- `crates/nimbus-runtime/src/runtime/bootstrap/js/node22_runtime_bootstrap.js`
- `docs/architecture/runtime/deno-fork-bump-ledger.md`
- `docs/architecture/runtime/node-lts-compat/node-release-train.json`
- `docs/architecture/runtime/node-lts-compat/node-release-train.md`
- `docs/plans/deno-rusty-v8-upstream-alignment-plan.md`
- `docs/plans/node-default-runtime-support-hardening-plan.md`
- `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua5-nimbus-repin.md`
- `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua6-node-compat-rebaseline.md`
- `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua7-docs-and-ledgers.md`
- `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua8-closeout.md`
- `docs/runtimes/nodejs/evidence/latest.md`
- `scripts/verify-deno-fork-provenance.sh`
- `scripts/verify-deno-fork-upstream-policy.sh`

Runtime changes made during DUA6:

- `process.loadEnvFile()` handles the Deno 2.8.1 read-permission check by
  falling back to Nimbus host-policy reads only when Deno denies the read.
- `fs.watch()` restores Node's synchronous missing-entry error when
  `throwIfNoEntry !== false`.

## Focused Verification

Commands and observed results:

```console
cargo fmt --all --check
git diff --check
npm run docs:validate-refs:strict
bash scripts/verify-deno-fork-provenance.sh
bash scripts/verify-deno-fork-upstream-policy.sh
make node-compat-status
diff -u docs/architecture/runtime/node-compat-evidence/latest/status-summary.md target/node-compat/status/status-summary.md
python3 scripts/runtime/node/default_support_posture.py --check
bash scripts/verify-node-lts-docs.sh
bash scripts/verify-node-release-train.sh
bash scripts/verify-node-latest-suite-tags.sh
```

Observed:

- `cargo fmt --all --check`: passed with no output.
- `git diff --check`: passed with no output.
- `npm run docs:validate-refs:strict`: `docs reference validation: pass (243 working-tree Markdown files)`.
- `bash scripts/verify-deno-fork-provenance.sh`: `5 passed, 0 failed`.
- `bash scripts/verify-deno-fork-upstream-policy.sh`: `27 passed, 0 failed`.
- `make node-compat-status`: passed after sandbox escalation allowed writing
  `target/node-compat/status`.
- `diff -u .../status-summary.md target/node-compat/status/status-summary.md`:
  passed with no output.
- `python3 scripts/runtime/node/default_support_posture.py --check`:
  `node default support posture: pass`.
- `bash scripts/verify-node-lts-docs.sh`: Node.js runtime evidence docs are
  current; Node LTS docs guard passed.
- `bash scripts/verify-node-release-train.sh`: `Node release-train summary is
  current: 4 lanes, 0 drift entries`; negative self-tests passed.
- `bash scripts/verify-node-latest-suite-tags.sh`: `validated Node latest suite
  tags: 4 lanes, 0 needing fixture sync`; negative self-tests passed.

## Broad Verification

Broad Node compatibility reruns were recorded in DUA6:

- `core-semantics`: `122 passed, 1 skipped, 0 failed`
- `process-and-timing`: `48 passed, 0 skipped, 0 failed`
- `streams-and-local-io`: `308 passed, 0 skipped, 0 failed`
- `networking`: `268 passed, 0 skipped, 0 failed`
- `loader-context`: `173 passed, 0 skipped, 4 failed`

The final DUA verifier was run after marking DUA8 done:

```console
bash scripts/verify-deno-rusty-v8-upstream-alignment.sh
```

Observed final local verifier output: `23 passed, 0 failed`.

## PR Status

Commands:

```console
gh pr view 11 --json number,title,state,isDraft,headRefName,baseRefName,mergeable,url,reviewDecision,statusCheckRollup
gh pr checks 11
```

Observed before pushing the DUA8 closeout commit:

- PR: `https://github.com/nimbus/nimbus/pull/11`
- State: `OPEN`
- Draft: `true`
- Base: `codex/node-default-runtime-support-hardening`
- Head: `codex/deno-rusty-v8-upstream-alignment`
- Mergeable: `MERGEABLE`
- Current failing hosted checks on the older pushed revision:
  - `Rust Runtime Tests`
  - `Node FaaS Compatibility`
  - `Rust Gate Summary`
- Passing hosted checks include Rust Format, Rust Clippy, Rust Dependency
  Audit, workspace test shards, provider integration tests, JS build/test,
  verification harness shards, and proof helper checks.

The hosted check state is not treated as green. It is the remote control-surface
status to revisit after the DUA8 commit is pushed.

After pushing DUA8, PR #11 exposed a real Node FaaS docs drift. The local
repair regenerated generated Node docs and release-train evidence and reran the
same three commands from the Node FaaS job successfully. A separate hosted
Postgres provider failure observed on the same run failed while fetching
`strsim` from crates.io with `curl [16] Error in the HTTP2 framing layer`, after
Docker/Postgres startup and before provider tests ran; that is treated as a
hosted registry/network retry, not as a DUA code change.

## NDS Handoff

NDS resumes from the upstream-aligned baseline:

- Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
- PR: `https://github.com/nimbus/nimbus/pull/10`
- Current row: `NDS3`
- Resume rule: continue the wide-then-focused loop from the current full-corpus
  inventory and do not rebuild claims on `v2.8.0-nimbus.15`.
- Baseline: `nimbus/deno v2.8.1-nimbus.1` and `nimbus/rusty_v8 v149.2.0-nimbus.1`.

The remaining NDS3 work is still substantial: Node24 remains `892 / 5198`, and
the plan's `>= 2000` full-corpus closeout gate is still unsatisfied.

## Residual Risks

- Hosted CI must rerun after this branch is pushed. Existing PR failures are
  recorded, not hidden.
- The loader/context async_hooks cluster remains a real runtime gap.
- `test-v8-serdes.js` remains a V8 wire-format boundary and must not be counted
  as a positive Node24 support claim without a versioned engine strategy.
