# SUC0.2 — Fail-Before Inventory

Date: 2026-07-29.

| Defect | Fail-before artifact | Disposition |
| --- | --- | --- |
| Postgres lease owner-id guard absent (accepted >191-byte ids that mysql/libsql reject) | `postgres_lease_owner_id_guard_matches_provider_parity` fails on the pre-fix tree (guard did not exist; only the empty check) | FIXED in SUC1.1 (this PR) |
| Lease-duration representation drift (mysql µs vs postgres/libsql ms) | `mysql_lease_validation_is_canonical_millis_bound_as_micros` pins canonical millis with the ×1000 MICROSECOND SQL edge | UNIFIED in SUC1.1; behavior verified identical (both stacks were internally consistent — maintenance hazard, not a live bug; recorded honestly against the review's framing) |
| Dual engine transcription of the queued commit sequence | Located: `engine/mutations/publisher.rs:456` (`persist_assigned_batch_once`) and `engine/mutations/journal.rs:974` (`process_serial_queued_mutation_batch`, serial kill-switch arm) — no compiler linkage between them | SUC2.1 unifies under one definition |
| Objects-on-read-executor race (audit F3) | Deterministic characterization deferred to SUC2.2 where the fenced path exists to assert against; the audit's source-level proof (objects.rs → TenantPointWrite, no committer) stands as the inventory entry | SUC2.2 |
| Provider fault-injection-point drift (review claim) | Site survey: postgres checks both journal fault points in two flows; mysql in one (its fenced path composes the checked append); libsql checks both at transaction commit plus two points others lack. Exact-parity assertion is deferred to SUC3's facade, which deletes the triplication that permits drift (decision U4) | SUC3 |
