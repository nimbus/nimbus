# SRR0 Baseline And Adjudication

Date: 2026-08-26
Baseline: `b57a2d680891de852d5576e65ccaea787b005431`
Branch: `codex/storage-review-repairs`

## Review command

```text
nimbus-autoreview --mode branch --base ff87c5028 \
  --engine claude --model claude-opus-5 --thinking high \
  --max-priority P2 --no-review-cache \
  --prompt "Perform one aggregate, end-to-end review..."
```

The review covered 169 files, 18,724 additions, and 791 deletions. It used
three passes. The secret scan was clean.

## Confirmed findings

1. `CanonicalMaterializedState` excludes `resource_path_bindings` and
   `trigger_delivery_cursor`.
2. Embedded PITR import commits restored state before it installs the
   checkpoint and retained-history floors.
3. A nonzero-base PITR import installs document and index floors at the base
   sequence without MVCC anchors for the snapshot state.
4. `scripts/verify-storage-metadata-retention.sh` treats one match across
   multiple backend paths as evidence for all named backends.
5. IMV7 condition 15 accepts static memory estimates and censored baseline
   comparisons without measured absolute candidate limits.

## Rejected findings

1. `ControllerCompletion::wait` has no lost wake-up. Tokio 1.52.3 retains a
   `notify_waiters` permit for a created `Notified` future before its first poll.
2. Synchronous tenant deletion does not block its caller's Tokio worker. The
   retention controller uses the dedicated two-thread `BackgroundExecutor`.
3. libSQL does not silently apply a divergent journal suffix. Gap checks reject
   it, and same-process retention requests a full local snapshot refresh.
4. Historical SMR proof transcripts match the verifier versions at their proof
   commits. Comparing each transcript to the current script produced the false
   mismatch.

## Baseline result

Five findings require repairs. Four claims require no code change. This proof
file is the durable scope boundary for the plan.
