# Storage Follow-Ups Closeout

Status: `active — executing all follow-up tickets from the archived storage-unification campaign`

Owner directive 2026-07-30: "do or clear all the followup items and bugs we
found with prs", plus run the SUC6.2 literal measurement (U7 override
exercised). Runs autonomously; PRs per concept; same fast-loop merge policy.

| Item | Source | Status |
| --- | --- | --- |
| FU1 MySQL `has_scheduled_work` outlier | SUC3.1 step 4 | `complete` (PR #264) — cron_jobs counted like the other five backends; six-backend conformance pin covering both halves of the enabled split; dead libsql filter alternative removed (52 selected before/after) |
| FU2 PPSC ack-loss arm-theft fault-interface fix | SUC3.1 steps 1-3 evidence | `planned` — durable-record identity through the fault check on all dialects (nimbus-storage + nimbus-testing); folds the mysql PPSC flake; the refuted `commit.is_some()` gate is the anti-pattern to avoid |
| FU3 DynamoDB batch stream staleness | SUC4.1 | `complete` (PR #265) — batch prior images read inside per-op transactions |
| FU4 policy-aware starting-at scan | SUC5.1 | `complete` (PR #265) — filter-then-fill; fail-before proved a live read-policy bypass at the seam; plus review-driven: stream-record authorization (both-images rule), real lifecycle times in stored events (fixing a nimbus-core planner bug that denied every row under lifecycle-referencing policies on every adapter), and a by-construction GetRecords store-read ceiling closing an authenticated DoS vector |
| FU5 `arm_selection` flake | 3 campaigns + isolation repro | `complete` (PR #262) — record #3 = trigger worker's zero-write cursor advance; the worker RESTARTS on every commit batch (lifecycle shutdown ≠ suppression); test-side fix via the permanent disable hook, assertion unweakened, 8/100→0/100; 3 sibling latent defects fixed |
| FU6 nimbus-fs seam + sqlite libsql fns | SUC2.2 + step 5 | `complete` (PR #263) — ObjectMetaStore write half deleted (trait now ObjectMetaRead; no publicly reachable unfenced object-write API); nimbus-fs owns its ObjectManifestStore capability w/ fencing contract doc; replica-cache fns relocated to sqlite/replica_cache.rs; build-time coverage pin |
| FU7 SUC6.2 literal measurement (U7 override) | owner 2026-07-30 | `complete` — retained SWT4.1 attribution rerun on current main @22c5cdd62 (minicloud, 12 rounds, all CVs ≤0.6%): binding = 0.095 ms = 1.42% of guarded = 0.90% of the storage lane → end-to-end <0.9%, a-fortiori REJECT vs the ≥3% bar; proof `proof/storage-follow-ups/fu7.md`; U10 appended to the archived plan |
| FU9 mutation-journal flake family | fu3 sighting + reproduction harness | `complete` (PR #266) — 4 tests, 2 causes: provider-catch-up reconciliation restarting the trigger worker (FU5 class; disable-hook fix ×3) and tests `.expect()`ing the deliberate retryable runtime-restarting refusal (bounded retry across the replacement boundary); 100× per test + 3× suite proof |
| FU10 nimbus-system projection reconciliation flake | FU1 attribution | `complete` (PR #268) — PRODUCT defect: every projection redeclared all 19 system schemas on `_nimbus`, each appending a durable record behind the committer (~10s fixed setup on MySQL); redeclarations now answered from the snapshot; lane 60-86s → 6.4-6.9s; second fix-induced test instability caught by matched stats and rebaselined without relaxation |
| FU11 BatchWriteItem validation ordering | FU3 pass-5 | `complete` (PR #267) — whole-request validation before any application (AWS's actual line: validation=whole-request, runtime=per-item); false 'DynamoDB semantics' comment corrected |
| FU12 BatchWriteItem missing rejection rules | FU11 survey | `planned` — duplicate hash+range keys in one request (incl. put+delete same item) applied last-writer-wins instead of rejected; 400KB per-item / 16MB per-request limits unenforced; lands as one pass over `prepare_batch_writes` output |
| FU8 CLAUDE.md storage-features gotcha | SUC3.1 step 5 | `complete` (this commit) |
