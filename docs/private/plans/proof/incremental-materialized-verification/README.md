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
- `imv2-raw.json` and `imv2-verdict.md` will own the continuation decision.
- Later task proofs stay beside these files and remain inputs to `verify.sh`.

The execution baseline is
`137cc632a1c8585545d200ea49f44bd236478175`. The baseline was clean before
IMV0 began. The plan and BLI proposal commits were already present on the task
branch and are not IMV0 implementation changes.
