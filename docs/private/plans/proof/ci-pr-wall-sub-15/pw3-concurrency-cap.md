# PW3 — concurrency cancellation excludes main

## The bug

`ci.yml` already had a top-level concurrency block (added in an
earlier wave):

```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
```

`group` is per-branch (good — PR branches deduplicate themselves)
but `cancel-in-progress: true` is unconditional. When `main`
receives back-to-back pushes (e.g. PW1 + PW1-backfill landing
seconds apart), the earlier run is cancelled. A cancelled run
abandons cache-save side effects: sccache GHA cache, Swatinem
target cache, and the libsql-image cache the PW1 step would
have written.

This is most expensive on PW0..PW5 sequencing where each PW lands
two commits (the change + the SHA backfill). The first commit's
warm cache, which the SHA-backfill commit's run would have hit,
gets thrown away mid-flight.

## The fix

```yaml
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: ${{ github.ref != 'refs/heads/main' }}
```

PR branch behaviour is unchanged (rapid pushes still cancel
themselves). On `main` the expression evaluates to `false`, so
back-to-back main pushes serialise instead of cancelling.

Same pattern was applied to `.github/workflows/coverage.yml` in
PW2. Both workflow files now protect main from cache-save abortion.

## Saturation attack

The CW5 baseline (`pw0-baseline.md`) showed libsql sitting in a
27m 35s queue waiting for an ubuntu runner. PW3 alone does not
shrink that queue — it preserves the cache state on the rare
back-to-back main pushes where the queue wait is reset by a
cancellation.

The bulk of the saturation relief comes from PW2: by moving the
3 coverage shards + reducer + their leader jobs off the PR path,
PRs free up ~4 concurrent runner slots (3 coverage shards + 1
reducer; the leader jobs are short). With ~24 simultaneous jobs
peaking on a CW5-style run, dropping 4 from the PR side gives the
queue depth measurably more headroom for the libsql shard to
start sooner.

## Expected impact

- Direct: main runs never cancel themselves; the
  caches-stay-warm guarantee that CC9 / Swatinem v2 / CW4 all
  depend on now extends across rapid-fire main pushes.
- Indirect: post-PW2 PR runs need ~4 fewer simultaneous runners.
  The libsql 27m queue wait should drop to single-digit minutes
  in the common case.

PW5 measures both effects on three consecutive PR-branch runs.

## Verifier

Condition 6 passes after PW3:

```
[6] ci.yml concurrency cap protects main (cancel-in-progress branch-conditional)
  PASS  concurrency.cancel-in-progress excludes refs/heads/main
```

The verifier checks the literal `cancel-in-progress: true` form
and rejects it, then verifies the cancel-in-progress expression
references `refs/heads/main`.
