# SRR3 Retention Verifier Proof

Date: 2026-08-26.
Implementation commit: `e83899824`.

## Outcome

The storage metadata retention verifier now requires a match in every path for
each multi-backend claim. A match in one backend cannot satisfy a missing
neighbor.

The stricter checks cover SQL compaction, typed cursor and PITR errors,
optimistic page checks, provider lease tests, and concurrent-prune tests. The
final metrics condition also checks its engine and storage diagnostics sources
separately.

## Fail-Before Evidence

The former `contains` helper passed all target paths to one `rg -q` command.
Ripgrep returns success when any target contains the pattern. The other named
paths can omit the required evidence without changing that result.

## Mutation Evidence

The helper copies the real storage, engine, and closeout-proof inputs into an
isolated repository tree. It first requires the full verifier to pass all 18
conditions. It then removes one real path at a time and reruns the full
verifier. Each removal must fail the owning condition, not only a shared search
primitive.

The helper covered five groups and 18 individual omissions:

- two SQL compaction paths.
- two typed-error paths.
- five optimistic page-check paths.
- three provider lease-test paths.
- six concurrent-prune test paths.

## Verification

| Command or gate | Result |
| --- | --- |
| `bash scripts/verify-storage-metadata-retention-helper.sh` | 5 real condition groups passed; all 18 path omissions failed their owning condition. |
| `bash scripts/verify-storage-metadata-retention.sh` | `Summary: 18 passed, 0 failed`. |
| `bash -n` on both scripts | Passed. |
| `shellcheck` on both scripts | Passed with zero diagnostics. |
| `git diff --check` | Passed. |

## Remaining Uncertainty

SRR5 still owns the complete repository and hosted gates. The source verifier
does not replace runtime tests. It protects the control-plane evidence claims
from weakening through aggregate path searches.
