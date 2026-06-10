---
status: done
phase: SATH5
---

# SATH5 Read Visibility

Read freshness is now represented by typed visibility DTOs:
`ReadVisibility`, `RequiredSequence`, and `PinnedServingSnapshot`. These encode
the existing latest-row contract: reads wait for an applied sequence and do not
overlay journal-only records.

Evidence:

- `crates/nimbus-core/src/visibility.rs`
- Existing engine serving code waits through the serving snapshot manager
  (`wait_for_snapshot_covering_cancellable`).
- `read_visibility_waits_for_required_sequence`
- `cargo check -p nimbus-engine --all-targets`: passed.
