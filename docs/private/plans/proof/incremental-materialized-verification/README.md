# Incremental materialized verification proof

This directory contains the retained evidence for the incremental materialized
verification plan. Run the fixed verifier from the repository root:

```bash
bash docs/private/plans/proof/incremental-materialized-verification/verify.sh
```

The verifier always evaluates 16 conditions. It is intentionally red after
IMV0 and can become green only after the accepted IMV2 branch and IMV7 are
complete.

## Evidence map

- `imv0.md` records the baseline, dirty-state attribution, resolved Cargo
  features, review-only probe results, and quick full-verifier measurement.
- `imv0-raw.json` is the retained machine-readable quick measurement.
- `imv2-raw.json` and `imv2-verdict.md` own the continuation decision.
- `imv3.md` records the deterministic root contract, dependency screen, and
  million-leaf memory and depth evidence.
- `imv4.md` records the writer inventory, applied-delta contract, replacement
  invalidation, and focused acceptance results.
- `imv5.md` records the bounded-session contract, full-scrub anchor semantics,
  escalation behavior, cache limits, and focused acceptance results.
- `imv6.md` records provider-root parity, fault recovery, hard session bounds,
  fixed metrics, operator controls, and external-provider qualification state.
- `imv7-raw.json` is the final 36-coordinate full-verifier matrix.
  `imv7-tail-raw.json` retains the capacity-limited four-coordinate tail run.
- `imv7-candidate-raw.json` retains measured production-index latency and
  resident bytes at the two decisive candidate rungs.
- `imv7-performance.md` records the matched comparison, accepted verdict,
  measured margins, production memory result, and remaining uncertainty.
- `verify-imv7-performance.py` validates the two performance artifacts.
  `verify-imv7-performance-helper.sh` proves malformed, missing, censored,
  slow, and high-memory candidate evidence fails closed.

The execution baseline is
`137cc632a1c8585545d200ea49f44bd236478175`. The baseline was clean before
IMV0 began. The plan and BLI proposal commits were already present on the task
branch and are not IMV0 implementation changes.
