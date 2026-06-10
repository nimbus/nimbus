---
status: done
phase: SATH9
---

# SATH9 Generated Conformance

The generated storage-history lane now includes explicit conformance markers
for required seed replay, schema/index/lifecycle/scheduler/retention coverage,
and crash/replay diagnostics.

Evidence:

- `storage_conformance_required_seed_corpus_matches_model`
- `generated_storage_history_includes_schema_index_lifecycle_scheduler_retention`
- `crash_replay_diagnostic_and_retention_snapshot_diagnostic_are_seed_replayable`
- `NIMBUS_STORAGE_CONFORMANCE_SEED`
- `NIMBUS_VERIFY_CASE` remains the harness repro selector.
