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

The new helper creates a green fixture for each per-path group. It then removes
the required evidence from one path at a time. Each removal must make the
`--require-each` check fail.

The helper covered five groups and 18 individual omissions:

- two SQL compaction paths.
- two typed-error paths.
- five optimistic page-check paths.
- three provider lease-test paths.
- six concurrent-prune test paths.

## Verification

| Command or gate | Result |
| --- | --- |
| `bash scripts/verify-storage-metadata-retention-helper.sh` | 5 groups passed; all 18 omission mutations failed closed. |
| `bash scripts/verify-storage-metadata-retention.sh` | `Summary: 18 passed, 0 failed`. |
| `bash -n` on both scripts | Passed. |
| `shellcheck` on both scripts | Passed with zero diagnostics. |
| `git diff --check` | Passed. |

## Remaining Uncertainty

SRR5 still owns the complete repository and hosted gates. The source verifier
does not replace runtime tests. It protects the control-plane evidence claims
from weakening through aggregate path searches.
