# Deno And rusty_v8 Upstream Alignment Plan (DUA)

Status: `active`
Owner: `node-compat`
Verifier: `scripts/verify-deno-rusty-v8-upstream-alignment.sh` (scaffolded in DUA0)
Baseline proof: `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua0-baseline.md`

## Active Execution Pointer

Current actionable row: `DUA3`
Current blocker that does not stop source work: `rusty_v8`
`v149.2.0-nimbus.1` was public before all required prebuilt assets were
available and is missing the hardened release workflow source. This blocks
local Deno `cargo check` on Apple Silicon until a superseding
`v149.2.0-nimbus.*` tag from the corrected branch publishes a complete,
atomically verified asset set.
Current DUA draft PR: `https://github.com/nimbus/nimbus/pull/11`
Current source worktree: `/Users/jack/src/github.com/nimbus/deno`
Current `rusty_v8` candidate branch: `nimbus/v149.2.0`
Current `rusty_v8` candidate branch head:
`83451cd2967ed8467dece09e9c847c2d6d882901`
Current `rusty_v8` candidate release tag: superseding
`v149.2.0-nimbus.2` is pending corrected branch CI; do not consume
`v149.2.0-nimbus.1` for DUA closeout.
Current DUA branch: `codex/deno-rusty-v8-upstream-alignment`
Next action: wait for corrected `rusty_v8` branch CI run `26769536048`; if it
passes, create and push `v149.2.0-nimbus.2`, update the Deno fork pin and
lockfile to that tag, then rerun the focused Deno fork `cargo check`.

## Why this plan exists

Nimbus carries Deno-family forks so we can prove Node compatibility fixes,
runtime embedding behavior, and `rusty_v8` locker safety against the product
runtime before release. That fork freedom is useful, but it also creates a
trust risk: if upstream Deno or `rusty_v8` now implements the same logical
behavior, Nimbus should consume the upstream implementation instead of carrying
parallel local logic.

This matters right now because upstream Deno 2.8 is a major Node compatibility
release. Deno's published 2.8 notes report Node's own test-suite pass rate
moving from roughly `42%` in Deno 2.7 to `76.4%` in Deno 2.8 (`3,405 / 4,457`
tests), with about 500 commits touching nearly every `node:` module. Upstream
Deno 2.8.1 then added targeted fixes in areas that overlap Nimbus' current
Node work: host-object deserialization, lazy ESM loading, TLS certificate
handling, `node:util`, `Module.register`, `fs.exists`, `fs.watch`,
`fs.promises`, `process.loadEnvFile`, `node:sqlite`, async-wrap tolerance,
`util.inspect`, and `node:http` runtime wakeups.

The same review found one especially important anti-duplication case:
upstream Deno 2.8.1 reverted the `module.enableCompileCache` polyfill because
Deno already enables V8 code caching and the polyfill added unwanted
environment-permission reads. If Nimbus still carries that API surface without
a product-specific reason, it should be dropped rather than maintained as
local compatibility code.

This plan aligns the forks to the newest relevant upstream baselines,
classifies every carried patch, removes duplicate logic, and repins Nimbus only
after the fork tags and Node compatibility evidence prove the result.

## Decision

Nimbus will use upstream Deno and `rusty_v8` logic whenever upstream now
implements the same observable behavior.

For Deno, the target upstream base is `denoland/deno@v2.8.1`. The Deno fork
must be rebuilt from that base and replay only patches that are still justified
after the overlap audit. The default disposition for matching behavior is
`upstream-replaced`, not "keep both".

Deno and `rusty_v8` are a lockstep runtime stack. A Deno base expects a matching
`rusty_v8`/V8 line, and Nimbus should keep those sources synchronized instead
of treating the V8 fork as an optional fixture-specific optimization. For
`denoland/deno@v2.8.1`, the matching upstream review point is
`denoland/rusty_v8@v149.2.0`. The expected closeout path is a
`v149.2.0-nimbus.*` fork tag that replays Nimbus' required `Locker` /
`UnenteredIsolate` safety stack on top of the Deno-compatible upstream V8 line.

Holding `rusty_v8` at `v149.0.0-nimbus.1` is not allowed merely because
`v149.2.0` does not immediately improve Node compatibility fixture counts.
Upstream currency is itself a trust requirement. A hold is allowed only as a
documented blocked decision when the latest Deno-compatible `rusty_v8` line
cannot preserve Nimbus' locker safety, build, or runtime verification after a
real rebase attempt.

The Node default-runtime support work should consume this alignment before
spending more loops on fixture-specific fixes that upstream may already have
solved. DUA is a prerequisite rebaseline for the next serious NDS3 fixture
promotion wave, not a replacement for NDS.

## Guiding Strategy

Use upstream first, then prove the remaining fork delta.

1. Compare before changing code. Every Deno and `rusty_v8` patch must be
   classified before it is replayed, dropped, or rewritten.
2. Prefer upstream semantics when they match. Avoid local patches that shadow
   upstream behavior or preserve older Deno behavior by accident.
3. Rebase broadly, test narrowly, then rerun broadly. Do not spend long loops
   fixing local fixture failures on a stale fork baseline.
4. Keep temporary local Cargo path overrides diagnostic-only. Closeout requires
   immutable fork tags and Nimbus repins in `Cargo.toml` plus `Cargo.lock`.
5. Do not claim a compatibility improvement from a fork bump until the Node
   compatibility evidence is regenerated or the proof explains why no public
   metric moved.
6. Keep Nimbus-specific embedding code small and explicit. If a patch exists
   only because Nimbus runs Deno inside its FaaS-style V8 isolate, mark it
   `nimbus-embedding-specific` with source locations and tests.
7. Never preserve `module.enableCompileCache` or similar reverted upstream API
   surface unless the proof names the Nimbus product requirement, permission
   behavior, and tests that justify diverging from upstream.
8. Keep Deno and `rusty_v8` in lockstep. Do not hold the older V8 fork because
   the latest compatible V8 line has no immediate fixture gain; hold only for a
   proven build, safety, or runtime verification blocker.

## Patch Disposition Vocabulary

Every carried or dirty fork change must have exactly one disposition:

| Disposition | Meaning | Required proof |
| --- | --- | --- |
| `upstream-replaced` | Upstream now implements the same logical behavior. Drop the Nimbus patch and cite the upstream release note, PR, or commit. | Upstream reference, removed local files/hunks, focused or broad test proving behavior still works. |
| `upstream-adjacent` | Upstream fixes a neighboring path, but the Nimbus patch still covers a distinct behavior. | Boundary explanation, source diff, test naming both behaviors when practical. |
| `nimbus-embedding-specific` | Needed only for Nimbus' embedding, runtime profile, fork pinning, or release contract. | Source location, reason it does not belong upstream, Nimbus-side focused test. |
| `still-needed-node-gap` | General Node compatibility behavior not yet implemented upstream. | Failing upstream evidence or absent upstream implementation, Node fixture/package evidence, upstream issue/PR trigger when practical. |
| `drop-no-longer-needed` | Local patch is obsolete, unsafe, or unsupported after the bump. | Removal reason and proof no positive Nimbus claim depends on it. |

## Proof Contract

Every DUA row has a required proof file. The proof files are the resume state
for autonomous execution and the audit trail for reviewers.

| Row | Required proof file |
| --- | --- |
| DUA0 | `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua0-baseline.md` |
| DUA0 | `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua0-control-plane.md` |
| DUA1 | `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua1-deno-overlap-audit.md` |
| DUA2 | `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua2-rusty-v8-alignment.md` |
| DUA3 | `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua3-deno-rebase.md` |
| DUA4 | `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua4-dirty-work-reevaluation.md` |
| DUA5 | `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua5-nimbus-repin.md` |
| DUA6 | `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua6-node-compat-rebaseline.md` |
| DUA7 | `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua7-docs-and-ledgers.md` |
| DUA8 | `docs/plans/proof/deno-rusty-v8-upstream-alignment/dua8-closeout.md` |

Each proof file must record:

1. **Row and status.** Row id, date, branch, worktree, PR URL, upstream tags,
   Nimbus fork tags, and verifier version.
2. **Input baseline.** Exact fork SHAs, dirty files, Nimbus pins, relevant
   upstream release notes or commits, and current Node compatibility counts.
3. **Disposition table.** Every local patch and dirty change with exactly one
   disposition from the vocabulary above.
4. **Implementation evidence.** Files changed, conflicts resolved, patches
   dropped, patches replayed, and why upstream or Nimbus owns each behavior.
5. **Focused verification.** Fork-side checks and Nimbus-side focused tests for
   changed behavior.
6. **Broad verification.** The wide Node compatibility or workspace command
   rerun after focused fixes, with before/after counts.
7. **Residual risks.** Remaining divergences, owner repo, upstream trigger,
   and whether they block NDS continuation.

## Ledger

| DUA | Work | Verifiable success criteria | Status |
| --- | --- | --- | --- |
| DUA0 | Baseline and control plane. Capture current Nimbus pins, local Deno fork state, dirty Deno files, local `rusty_v8` fork state, upstream release targets, active NDS relationship, and execution worktree/PR. Scaffold the verifier. | `dua0-baseline.md` and `dua0-control-plane.md` exist; proof records Nimbus `Cargo.toml`/`Cargo.lock` Deno and `rusty_v8` pins, Deno fork branch/tag/SHA/dirty files, `rusty_v8` fork branch/tag/SHA, upstream `denoland/deno@v2.8.1`, upstream `denoland/rusty_v8@v149.2.0`, and the rule that DUA rebaselines before further serious NDS3 greening; verifier exists and fails every unimplemented gate; worktree, branch, and draft PR URL or approved bootstrap substitute are recorded. | done |
| DUA1 | Deno 2.8.1 overlap audit. Compare all `v2.8.0-nimbus.*` patches and current dirty Deno changes against upstream `v2.8.1`. | `dua1-deno-overlap-audit.md` contains a complete patch disposition table; every Deno commit from `v2.8.0-nimbus.1` through the latest local tag is classified; every dirty file is classified; upstream 2.8.1 release items relevant to Nimbus are mapped to local code; `module.enableCompileCache` is explicitly classified as `upstream-replaced` or has a written product-specific exception with tests; no patch proceeds without disposition. | done |
| DUA2 | Lockstep `rusty_v8` substrate alignment. Before replaying Deno patches, rebase Nimbus `rusty_v8` from `v149.0.0-nimbus.1` to the Deno-compatible upstream `v149.2.0` line and preserve Nimbus locker safety. | `dua2-rusty-v8-alignment.md` proves `denoland/deno@v2.8.1` expects the `v149.2.0` V8 line; direct bump to upstream is rejected if it drops Nimbus locker safety; the expected output is a `v149.2.0-nimbus.*` candidate containing the required `Locker` / `UnenteredIsolate` stack and safety tests. A hold is valid only if the rebase attempt cannot pass build, safety, or runtime verification, and the proof records the exact blocker. "No immediate Node fixture benefit" is not a valid hold reason. | done |
| DUA3 | Deno fork rebase on the selected V8 substrate. Rebuild the Deno fork from upstream `v2.8.1`, replay only justified patches, and remove upstream-replaced code while consuming the DUA2 `rusty_v8` decision. | Deno fork is on a clean branch based on upstream `v2.8.1`; its `v8` dependency points at the DUA2-selected `v149.2.0-nimbus.*` candidate or the proof records the exact approved hold; dropped patches are absent from the diff; replayed patches carry disposition notes in proof; fork-side format/check commands named in proof pass or any failure is classified with owner and blocker status; a candidate tag name such as `v2.8.1-nimbus.1` is recorded but not consumed by Nimbus until DUA5. | in_progress |
| DUA4 | Dirty Node compatibility work reevaluation. Re-test current local Deno dirty work against the rebased Deno candidate and carry only the pieces still needed after upstream 2.8.1 and the selected V8 substrate. | `dua4-dirty-work-reevaluation.md` records focused pre/post results for the dirty areas: CommonJS global path resolution, `node:v8` serializer/deserializer and host-object behavior, crypto random/cipher behavior, and any `internal_binding` additions; each dirty change is either dropped, replaced with upstream, or committed with a disposition; no diagnostic-only code is counted as a positive compatibility claim. | pending |
| DUA5 | Nimbus repin. Point Nimbus at the published Deno and matching `rusty_v8` fork tags, update lockfile, and remove all temporary local path overrides. | Deno and the Deno-compatible `rusty_v8` candidate tags are committed, tagged, and pushed before Nimbus repin; `Cargo.toml` and `Cargo.lock` resolve to immutable `nimbus/deno` and matching `nimbus/rusty_v8` tags and SHAs; `bash scripts/verify-deno-fork-provenance.sh` and `bash scripts/verify-deno-fork-upstream-policy.sh` pass or the proof records the exact missing gate to add before closeout; no `/private/tmp` or local path override remains. | pending |
| DUA6 | Node compatibility rebaseline. Re-run focused and broad Node compatibility evidence against the repinned upstream-aligned fork stack. | `dua6-node-compat-rebaseline.md` records before/after counts for the relevant NDS3 fixture groups; current loader/context, crypto, V8, async-hooks, networking, and fs/stream outcomes are compared to the pre-DUA baseline; newly green fixtures are promoted only if broad reruns pass; remaining failures are classified as Nimbus runtime, Deno fork, `rusty_v8`, upstream/platform, or non-isolate boundary. | pending |
| DUA7 | Docs and ledgers. Update operating docs, fork bump ledger, compatibility dashboards, and NDS handoff notes. | `docs/architecture/runtime/deno-fork-bump-ledger.md` records the new fork tag(s), SHAs, upstream bases, dispositions, removal triggers, and verification; generated Node compatibility dashboards and summaries are updated if counts changed; `docs/operating/deno-fork-workflow.md` stays consistent; NDS proof or plan state records the upstream-aligned baseline for the next row. | pending |
| DUA8 | Closeout and archive readiness. Prove all rows are complete and hand control back to NDS. | `dua8-closeout.md` exists; every DUA row is `done`; verifier exits 0 with `0 failed`; `cargo fmt --all --check`, strict docs refs, fork provenance, fork upstream policy, and `git diff --check` pass; PR checks are green or exact hosted blockers are recorded; DUA is archived after merge approval and NDS resumes from the upstream-aligned baseline. | pending |

## Completion Gate

`bash scripts/verify-deno-rusty-v8-upstream-alignment.sh` exits 0 with a
summary line that includes `0 failed` and the actual number of required checks
passed. The verifier must check at least:

1. Plan is ready, active, or archived and every ledger row is `done` at
   closeout.
2. Every required DUA proof file exists and follows the Proof Contract.
3. DUA0 records current Nimbus pins, fork SHAs, upstream targets, dirty fork
   files, worktree, branch, PR URL or approved substitute, and NDS handoff.
4. DUA1 classifies every Deno fork patch and dirty Deno change with exactly
   one allowed disposition.
5. `module.enableCompileCache` is not carried unless a product-specific
   exception, permission proof, and tests are recorded.
6. DUA2 proves the selected Deno base and `rusty_v8` target are treated as a
   lockstep compatible runtime stack before the Deno fork is rebuilt.
7. The `rusty_v8` candidate is rebased to the Deno-compatible `v149.2.0` line
   and preserves `Locker` / `UnenteredIsolate` APIs plus safety tests.
8. If `rusty_v8` is not rebased, the row is blocked with exact build, safety,
   or runtime verification blockers. Lack of immediate Node compatibility
   fixture improvement is explicitly rejected as a hold reason.
9. Deno fork candidate is based on upstream `v2.8.1` and consumes the DUA2
   `rusty_v8` substrate decision before replaying patches.
10. No upstream-replaced Deno patch remains in the fork diff.
11. Every replayed Deno patch has source locations, owner repo, test evidence,
   and removal/upstream trigger unless permanently Nimbus-specific.
12. Dirty Deno changes are either dropped or committed with disposition and
   focused proof.
13. Nimbus `Cargo.toml` and `Cargo.lock` use immutable published fork tags and
    SHAs for both the aligned Deno fork and matching `rusty_v8` fork, not local
    paths.
14. Fork provenance and upstream-policy verifiers pass or DUA blocks closeout
    until the missing policy gate is added.
15. Focused Nimbus Node compatibility tests pass for every changed behavior
    claimed as supported.
16. Broad Node compatibility reruns compare pre-DUA and post-DUA counts.
17. Newly green fixtures are not promoted from focused tests alone.
18. Remaining fixture failures have owner repo and follow-up path.
19. Generated compatibility dashboards and summaries are updated when counts
    move.
20. Fork bump ledger records tag, SHA, upstream base, disposition, removal
    trigger, changelog mapping, and verification.
21. Docs links and operating workflow stay consistent with the new fork tags.
22. `cargo fmt --all --check`, strict docs refs, and `git diff --check` pass.
23. Closeout proof records green local verifier output, PR status, and the
    handoff point back to NDS.

## Goal Prompt

Use this prompt after creating the dedicated worktree and draft PR:

```text
/goal Complete docs/plans/deno-rusty-v8-upstream-alignment-plan.md
autonomously. Use the plan as the control plane. Work from the dedicated
worktree and PR recorded in DUA0. Do not continue Node fixture greening on a
stale fork baseline. First classify every Deno patch against upstream
denoland/deno@v2.8.1 and confirm the matching
denoland/rusty_v8@v149.2.0 V8 line. Before rebuilding the Deno fork, rebase
the Nimbus rusty_v8 fork to the Deno-compatible v149.2.0 line, or record an
exact build, safety, or runtime verification blocker. Then rebase the Deno
fork on that selected V8 substrate and replay only justified patches.
Prefer upstream implementations whenever they provide the same logical
behavior; do not carry duplicate local code. Holding the old rusty_v8 fork is
allowed only for a proven build, safety, or runtime verification blocker, never
because the newer compatible V8 line has no immediate fixture-count gain.
Publish fork tags before repinning Nimbus, remove all local path overrides before
closeout, rerun focused tests for changed behavior, then rerun the broad Node
compatibility groups and update evidence. Keep every DUA proof file current,
record exact commands and counts, and do not mark the goal complete until
scripts/verify-deno-rusty-v8-upstream-alignment.sh exits 0 with 0 failed.
```

## Out Of Scope

- Rewriting the Node default-runtime support target or lowering NDS pass gates.
- Claiming broad Node parity from the Deno bump without Nimbus evidence.
- Carrying upstream-replaced code for convenience.
- Holding `rusty_v8` at the older fork pin merely because the latest compatible
  upstream V8 line does not immediately improve Node compatibility numbers.
- Directly bumping `rusty_v8` if it removes Nimbus locker safety.
- Using `/private/tmp` checkouts, copied Cargo sources, or local path overrides
  as progress state.
- Publishing fork tags before focused and broad verification produce useful
  evidence.
- Treating non-isolate behavior as positive in-process support.

## Risks

| Risk | Mitigation |
| --- | --- |
| Upstream 2.8.1 fixes overlap local patches but not perfectly. | Use `upstream-adjacent` only with source boundaries and tests that show both behaviors. |
| Rebase conflicts silently preserve old Nimbus behavior over upstream. | DUA1 disposition and DUA3 conflict proof must explain every replayed patch. |
| Dropping compile-cache API surprises packages that expect Node's surface. | Follow upstream unless Nimbus proves a product-specific need; document unsupported or upstream-divergent behavior honestly. |
| `rusty_v8` rebase loses locker safety. | Completion gate requires replaying the locker safety stack on the Deno-compatible `v149.2.0` line, or a blocked hold with exact build/safety/runtime blockers. |
| Compatibility numbers move but docs stay stale. | DUA7 requires generated dashboard and summary updates when counts change. |
| Work continues in NDS using stale fork state. | DUA0 records DUA as the prerequisite baseline for the next serious NDS3 greening wave. |
| Latest compatible `rusty_v8` has no immediate Node fixture gain. | Still update in lockstep with Deno; upstream currency is a trust requirement, not just a fixture-greening tactic. |

## References

- `docs/plans/node-default-runtime-support-hardening-plan.md`
- `docs/operating/deno-fork-workflow.md`
- `docs/architecture/runtime/deno-fork-bump-ledger.md`
- `docs/architecture/runtime/node-compat-evidence/latest/status-summary.md`
- `docs/runtimes/nodejs/compatibility.md`
- `~/src/github.com/nimbus/deno`
- `~/src/github.com/nimbus/rusty_v8`
- `~/src/github.com/denoland/deno`
- `https://deno.com/blog/v2.8`
- `https://github.com/denoland/deno/releases/tag/v2.8.1`
- `https://github.com/denoland/deno/pull/34348`
- `https://github.com/denoland/deno/pull/34380`
- `https://github.com/denoland/rusty_v8/releases/tag/v149.2.0`
