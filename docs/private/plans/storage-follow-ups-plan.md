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
| FU5 `arm_selection::opaque_internal_job_cannot_overtake_ordered_publisher` flake | 3 campaigns of sightings + isolation repro 1/9 | `planned` — root-cause the third journal record (suspect: background commit racing `shutdown_trigger_candidates_for_testing`); fix test or product accordingly |
| FU6 nimbus-fs `ObjectMetaStore` unfenced-write seam + sqlite-module libsql fns | SUC2.2 + SUC3.1 step 5 | `planned` — smallest correct closure of the dormant seam (no production wiring exists) + relocate the 3 cfg'd libsql replica fns out of sqlite modules |
| FU7 SUC6.2 literal measurement (U7 override) | owner 2026-07-30 | `planned` — rebuild the SWT4 binding ablation on current main; paired protocol on minicloud; record the measured verdict superseding U7's arithmetic |
| FU8 CLAUDE.md storage-features gotcha | SUC3.1 step 5 | `complete` (this commit) |
