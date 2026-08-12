# NNC6.5g final teardown convergence

Status: `in_progress; recovery checkpoint only; product work not started`

Owner: `docs/private/plans/nimbus-network-control-plane-plan.md`

## Recovery checkpoint

| Field | Value |
| --- | --- |
| Dependency | NNC6.5e, NNC6.5f2, and NNC6.5f3 are complete. The commit containing the NNC6.5f3 ledger transition is the clean item base. |
| Current state | No NNC6.5g product edit has started. NNCV035 retains exactly four diagnostics: service retirement, tenant retirement, failed-provision or restart compensation, and attributed end-to-end behavior. |
| Owned outcome | Route every remaining teardown caller through the existing durable compute saga, close exact compensation races, delete every frozen legacy bypass, and make NNCV035 green. |
| Owned paths | None yet. The read-only census must freeze exact product, test, verifier, proof, plan, and routing paths before the first product edit. |
| Forbidden scope | Do not move provider effects, service naming, tenant policy, proxy behavior, or cluster transport into `nimbus-network`. Do not weaken retained ambiguity or cleanup fences. |
| Last green | NNC6.5f3 CLI `1007 + 4 ignored`; strict Clippy and Rustdoc; NNCV035 physical `1/1`; helper `150/150`; aggregate `552/552`; live verifier `35/36`; docs `108`; site `17/17`. |
| Next action | Re-read the frozen NNC6.5 audit, inspect the four live diagnostics and current callers, then freeze the exact source census, deletion list, race matrix, and acceptance ledger before product edits. |
| Blocker | None. |

The canonical plan owns NNC6.5g acceptance. This file is only the compaction
recovery checkpoint until the read-only census freezes the implementation
ledger and proof commands.
