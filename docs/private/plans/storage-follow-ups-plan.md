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
| FU10 nimbus-system projection reconciliation liveness flake | FU1 attribution (ABBA interleaved: fails in no-fix arm) | `planned` — post-restart reconciliation wait_for_row_count 10s timeout, visible_row=None with frontiers caught up; ci-pr retries=0 so it fails the MySQL lane when it fires |
| FU11 batch.rs comment falsely labels partial application as DynamoDB semantics | FU3 pass-5 observation | `planned` — real BatchWriteItem validates the whole request before applying anything; the comment (and its pinning test) present a Nimbus divergence as AWS compliance; needs whole-request validation or an honest divergence note |
| FU8 CLAUDE.md storage-features gotcha | SUC3.1 step 5 | `complete` (this commit) |
