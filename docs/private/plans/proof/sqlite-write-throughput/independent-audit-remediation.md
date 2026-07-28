# Independent Audit Remediation

Date: 2026-07-28

Audited branch head:
`f6fc2cd86ea742b8c8780b3af4a44f82449f36b0`

The read-only Fable audit independently reconstructed the committed
statistics, statement model, SQLite topology, source paths, and external
comparison claims. Its rerun artifacts remained in the review session
scratchpad and were not transferred to this branch, so the reported rerun
numbers are diagnostic corroboration rather than campaign acceptance proof.
SWT0 still owns the cryptographically bound baseline source/protocol. SWT5
freezes the exact final commit, and final acceptance reruns both immutable
binaries contemporaneously.

## Diagnostic rerun

- CRUD N=256: 25,862 logical mutations/s, 95% CI 25,335–26,390,
  CV 3.7%, versus the historical 21,433 observation.
- Guarded-to-lower-bound delta: 6.8% in the accepted audit rerun, versus 11.5%
  in the planning reference.
- Hot-key N=256: three of three attempts failed on `CommitterFull` because the
  harness did not retry `RetryableAfterBackoff`.

## Finding disposition

| Finding | Disposition |
| --- | --- |
| F1 hot-key backpressure panic | Benchmark retries `RetryableAfterBackoff`, honoring an explicit retry delay or sleeping 1 ms. The exact canonical N=1/32/256 validation completed; its noisy, rejected performance report is retained in `hotkey-backpressure-validation.md`. |
| F2 host-sensitive absolute target | SWT0 freezes baseline source/protocol `B_ref`, not a permanent numeric denominator; SWT5 freezes exact final commit `F_ref`. The final session runs six predeclared balanced full-protocol block pairs and requires a mean `F_ref`/`B_ref` ratio ≥1.40, `F_ref` 30k/28k absolute floors, and a positive paired-delta lower CI. |
| F3 unscoped three-route claim | Research, plan, and `AGENTS.md` scope the invariant to client document mutations and enumerate internal/non-committer writers. SWT2 must coexist with them. |
| F4 unstable forward-apply magnitude | Research and plan use an approximately 7–11.5% cross-run range and retain the attribution-first ≥3% end-to-end gate. |
| F5 fixture-only checkpoint evidence | SWT0 merges the Engine-scale WAL high-water, checkpoint-count, and checkpoint-time observation seam before freezing `B_ref`, so the exact baseline can supply the same resource evidence required at final disposition. |
| F6 publication order | Research now records write-log publication → cache invalidation → applied head → fan-out. |
| F7 target arithmetic | Research uses matching units: 30k is 19.8% of guarded logical throughput; approximately 90k core row changes/s is 19.7% of guarded row throughput. |
| F8 missing rejected raw report | `layered-noisy-diagnostic-raw.md` is retained byte-for-byte with SHA-256 `83aa26c5665c9fc7180d6c450ed82598c636973b63aa543140a11d147e96b45c`. |
| F9 schema-validation wording | Direct/queued prepare and reprepare placement is distinguished from execution-unit serial validation. |
| F10 methodology cleanup | Research states fixed lane order, precomputed MessagePack timing, non-no-op updates, fixture shape, and Engine-owned coverage for indexed/multi-write behavior. |
