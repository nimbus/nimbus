# SWT4.1 Forward-Apply Attribution — Disposition: REJECT Implementation (D17)

Date: 2026-07-28. Binary `2e756b88…` at `3d9aad324` (bench-only lanes on
merged main `4665dcf48`). Attempt 1 rejected whole (minus-preimage lane CV
25.5%; retained as `attribution-raw-rejected-a1.md`). Accepted attempt 2
(`attribution-raw-accepted.md`, SHA-256 `0e6003b8…`): all 8 lanes CV≤6.9%.

## Attributed components (guarded fixture, 4.648 ms)

| Component | Cost | Share of guarded |
| --- | ---: | ---: |
| Live preimage SELECT (per record) | 0.466 ms | 10.0% |
| Rust JSON decode + previous-document compare | 0.201 ms | 4.3% |
| Delete-side resource-binding DELETE | 0.108 ms | 2.3% |

The old combined 7–11.5% range decomposes cleanly; the SQL read dominates.

## End-to-end projection

Removable by a conditional forward guard (preimage + decode/compare):
0.667 ms = 10.6% of the production-storage lane (6.303 ms; the lane now
measures 121,939 mut/s on merged main, ≈2.9× `B_ref`). Engine retention
≈36% (≈44k Engine N=256 ÷ 121,939). Point projection **≈3.8%**;
conservative CI-edge projection **≈2.6%** — the gate requires ≥3% with a
positive lower confidence bound, which this does not clearly meet.

## Decision (D17)

SWT4.2/4.3 implementation **rejected**: the safe projected gain sits below
the bar, the mechanism carries the plan's highest corruption/replay risk,
and the campaign target is already exceeded after SWT1+SWT2. The
attribution instrumentation (opt-in `NIMBUS_SWO_ATTRIBUTION=1`) and this
proof are retained for any future campaign; binding cleanup (2.3%) is
recorded as a separately measurable future candidate.

## Review correction: production-equivalent decode

Review flagged that the decode lane compared only `fields`. The lane now
rebuilds the full `Document` (both JSON columns, timestamps, typed fields)
and compares it whole, matching production. Two corrected runs were taken:
a3 rejected whole (host noise; retained) and a4
(`attribution-raw-a4-partial.md`, SHA-256 `81fb02cd…`) whose *decision pair*
is clean — guarded 4.670 ms (CV 1.0%) vs decode+compare 4.899 ms (CV 2.6%)
→ **corrected decode cost 0.229 ms (4.9% of guarded)** vs 0.201 fields-only
— while two unrelated side lanes breached CV (storage 12.1%,
minus-binding 22.0%), so a4 is not a whole-run acceptance artifact.

Corrected projection: (0.466 + 0.229) / 6.303 × 36% ≈ **4.0% point,
≈2.8% conservative** — still below the safe ≥3% positive-lower-bound gate.
D17 is unchanged under both the original and corrected decode measurements;
the correction is recorded because it tightens the estimate the decision
rests on.
