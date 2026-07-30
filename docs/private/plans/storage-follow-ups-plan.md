# Storage Follow-Ups Closeout

Status: `active — executing all follow-up tickets from the archived storage-unification campaign`

Owner directive 2026-07-30: "do or clear all the followup items and bugs we
found with prs", plus run the SUC6.2 literal measurement (U7 override
exercised). Runs autonomously; PRs per concept; same fast-loop merge policy.

| Item | Source | Status |
| --- | --- | --- |
| FU1 MySQL `has_scheduled_work` enabled=TRUE outlier | SUC3.1 step 4 | `planned` — real bug; align to the other four backends + regression test; also delete the dead libsql filter alternative in scripts/test-external-providers.sh |
| FU2 PPSC ack-loss arm-theft fault-interface fix | SUC3.1 steps 1-3 evidence | `planned` — durable-record identity through the fault check on all dialects (nimbus-storage + nimbus-testing); folds the mysql PPSC flake; the refuted `commit.is_some()` gate is the anti-pattern to avoid |
| FU3 DynamoDB `batch_write_item` stream-record staleness | SUC4.1 verification | `planned` — route batch writes through `execute_single_item_transaction`; INSERT/MODIFY classification + OldImage freshness tests |
| FU4 `scan_documents_by_id_starting_at_cancellable` policy-aware paging | SUC5.1 | `planned` — filter-then-fill paging so the limit-bearing scan can enforce ReadAuthorization; keep the adapter-owned stream sidecar's semantics |
| FU5 `arm_selection` flake | 3 campaigns + isolation repro | `complete` (PR #262) — record #3 = trigger worker's zero-write cursor advance; the worker RESTARTS on every commit batch (lifecycle shutdown ≠ suppression); test-side fix via the permanent disable hook, assertion unweakened, 8/100→0/100; 3 sibling latent defects fixed |
| FU6 nimbus-fs seam + sqlite libsql fns | SUC2.2 + step 5 | `complete` (PR #263) — ObjectMetaStore write half deleted (trait now ObjectMetaRead; no publicly reachable unfenced object-write API); nimbus-fs owns its ObjectManifestStore capability w/ fencing contract doc; replica-cache fns relocated to sqlite/replica_cache.rs; build-time coverage pin |
| FU7 SUC6.2 literal measurement (U7 override) | owner 2026-07-30 | `complete` — retained SWT4.1 attribution rerun on current main @22c5cdd62 (minicloud, 12 rounds, all CVs ≤0.6%): binding = 0.095 ms = 1.42% of guarded = 0.90% of the storage lane → end-to-end <0.9%, a-fortiori REJECT vs the ≥3% bar; proof `proof/storage-follow-ups/fu7.md`; U10 appended to the archived plan |
| FU10 nimbus-system projection reconciliation liveness flake | FU1 attribution (ABBA interleaved: fails in no-fix arm) | `planned` — post-restart reconciliation wait_for_row_count 10s timeout, visible_row=None with frontiers caught up; ci-pr retries=0 so it fails the MySQL lane when it fires |
| FU8 CLAUDE.md storage-features gotcha | SUC3.1 step 5 | `complete` (this commit) |
