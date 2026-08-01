# Developer documentation technical-writing pass

Status: `active` | Owner: this plan | Created: 2026-08-01
Baseline: main @ 1ba104f876078551207ef0eabf90e214c9d1e4e3
Proof root: `docs/private/plans/proof/developer-docs-technical-writing/`
Next action: push the branch and open the ready pull request against main

## Outcome

> All pages under `docs/developers/` preserve their verified technical
> claims and pass the technical-writing linter in developer mode. The Nimbus
> documentation gates also pass.

## Architecture

This plan does not change the documentation structure. It edits prose in the
existing Developers group and keeps each page in its current Diataxis mode.

## Scope

- Owns: the 23 Markdown pages under `docs/developers/` and their existing
  rows in `docs/source-map.md`.
- Does not own: other public documentation groups, repository READMEs,
  product behavior, or source code. Their current owner plans retain them.
- Non-goals: new product claims, new examples, navigation changes, and
  documentation structure changes.

## Invariants

1. Preserve every fact, limit, condition, identifier, command, and code sample.
2. Keep each behavior claim supported by its existing source-map evidence.
3. Keep one Diataxis mode on each page.
4. Keep private paths and internal contributor workflows out of public pages.
5. Do not change code or product behavior.

## Status ledger

| ID | Task | Status | Evidence |
|---|---|---|---|
| DTW0 | Capture the developer-doc baseline and source-map coverage. | `done` | 2026-08-01: 23 files, 2,984 lines, and 406 diagnostics. All 23 files have source-map rows. See `proof/developer-docs-technical-writing/dtw0.md`. |
| DTW1 | Revise developer documentation without changing technical meaning. | `done` | 2026-08-01: all 23 pages reviewed, 22 revised, 23 passed the linter with zero errors and 30 warnings. See `proof/developer-docs-technical-writing/dtw1.md`. |
| DTW2 | Run conformance, documentation, build, and review gates. | `done` | 2026-08-01: all writing, source-map, protected-content, link, private-fence, site-build, and review gates passed. Autoreview classified the branch as documentation-only and reported no findings. See `proof/developer-docs-technical-writing/dtw2.md`. |
| DTW9 | Clean up after the final pull request merges. | `todo` | Trigger: merge of the final pull request. |

## Tasks

### DTW0 Capture the baseline

- Problem: the writing pass needs a measured fail-before state and complete
  source-map coverage.
- Owning seam and paths: `docs/developers/`, `docs/source-map.md`, and the
  DTW0 proof file.
- Steps: count the pages and lines, run the technical-writing linter, and
  compare every page with the source map.
- Acceptance: the proof records the file count, line count, diagnostic count,
  and source-map coverage.
- Fail-before: the developer-mode linter reports at least one error.
- Verification: run the developer-mode linter and the source-map coverage
  check recorded in the proof.

### DTW1 Revise developer documentation

- Problem: the Developers group contains mechanical writing errors and prose
  that is harder to scan than required.
- Owning seam and paths: `docs/developers/` and existing developer rows in
  `docs/source-map.md`.
- Steps: perform the protected-content rewrite workflow on each page.
- Acceptance: all pages preserve protected content and retain their current
  information architecture.
- Fail-before: DTW0 records 317 errors and 89 warnings across 23 files.
- Verification: run the technical-writing linter in developer mode and inspect
  the final diff against the protected source content.

### DTW2 Run final gates

- Problem: mechanical lint alone cannot prove technical accuracy, link
  integrity, rendering, or human conformance.
- Owning seam and paths: the changed documentation, this plan, and the DTW2
  proof file.
- Steps: run all final checks, complete the human conformance checklist, and
  run the Nimbus pre-PR review.
- Acceptance: every named gate passes, the build emits three LLM files, and
  review has no unresolved findings.
- Fail-before: DTW0 records the red linter baseline.
- Verification: run the technical-writing linter, `bash scripts/check-docs.sh`,
  `bash scripts/verify-nimbus-docs-site.sh`,
  `npm --prefix website run build`, and the Nimbus pre-PR review gate.

### DTW9 Clean up

- Problem: a merged plan must not remain as an active control plane.
- Owning seam and paths: this plan, its proof root, and
  `docs/private/plans/README.md`.
- Steps: confirm the final pull request merged, then archive the completed
  plan according to repository convention.
- Acceptance: the archive records the completed plan and the plans index has
  no active entry for this work.
- Fail-before: not applicable because the ledger row records the merge.
- Verification: search the repository for the plan slug and confirm that only
  archive and proof references remain.

## Goal

```text
Execute docs/private/plans/developer-docs-technical-writing-plan.md to
completion. This is a whole-plan goal, not a single-task goal. Read the plan
fully, then read: AGENTS.md, docs/README.md, docs/source-map.md, the Nimbus
docs skill, and the technical-writing skill. Work in
/Users/jack/src/github.com/nimbus/nimbus-technical-writing-docs on branch
codex/technical-writing-developer-docs. Chat history is not progress state.
Resume from the status ledger, the execution log, and git state. If compaction
happens, continue from the plan and git state. Keep one task in_progress.
Implement at the owning seam. Capture fail-before evidence. Run the named
verification commands. Commit the work after all non-cleanup tasks become
terminal. Write each proof file and update the ledger before the commit. Mark
a wrong or satisfied task no-action with one reason. Record a blocker and
continue with the next eligible task. Preserve facts, code, commands,
identifiers, links, source-map evidence, and Diataxis boundaries. Do not add
product claims or edit code. Stop only at a valid stop state from the plans
skill. Before stopping, update the ledger, the log, and the next action. The
goal is met when DTW0 through DTW2 are terminal, all final gates pass, and the
ready pull request targets main. DTW9 waits for that pull request to merge.
```

## Execution log

| Date | Item | Action | Evidence |
|---|---|---|---|
| 2026-08-01 | DTW0 | Captured the developer documentation baseline. | The developer-mode linter reported 406 diagnostics: 317 errors and 89 warnings. The 23 files contain 2,984 lines, and all 23 have source-map rows. |
| 2026-08-01 | DTW1 | Revised the Developers group with protected-content checks. | All 23 pages passed the developer-mode linter with zero errors and 30 warnings. `git diff --check` passed. |
| 2026-08-01 | DTW2 | Ran the documentation, conformance, and review gates. | The developer corpus has zero lint errors, all 23 pages retain source-map coverage, fenced content is unchanged, the site built 109 pages, and all 17 site conditions passed. Autoreview classified the branch as documentation-only and reported no findings. |
