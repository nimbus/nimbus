# CW4 — warm-sccache compile-cost reduction

CW4 investigated two lanes to compress the `warm-sccache` job's 10.2m
wall on the CW0 baseline. Lane (a) — drop `--tests` from the warm
command — landed. Lane (b) — bespoke per-target cache layer — is
deferred because Swatinem v2 already provides target/ caching.

## Lane (a, landed) — drop `--tests` from warm-sccache

### What changed

`.github/workflows/ci.yml` `warm-sccache:` step:

```diff
-      - name: Warm sccache via workspace check
-        run: cargo check --workspace --tests
+      - name: Warm sccache via workspace check
+        run: cargo check --workspace
```

`docs/operating/ci-caching.md`'s `warm-sccache` section is updated to
match: the description now says "every workspace crate's lib/bin dep
graph gets rustc'd into the shared sccache pool" rather than "every
workspace crate (and its dev-deps)".

### Why dropping `--tests` is a wall win

The `warm-sccache` leader exists to populate sccache so that *parallel*
downstream Rust jobs hit the same cache instead of N runners each
cold-compiling identical dep crates. The relevant question for any
rustc invocation `warm-sccache` makes is: do downstream jobs emit the
same sccache key, and how often?

For **lib/bin rustc invocations**: yes, downstream test jobs that
`cargo test -p <crate>` will emit the same lib rustc call with the
same sccache key. The benefit is real and proportional to N (with N
parallel downstream jobs, the warm pass saves N-1 cold compiles).
**This is preserved by lane (a)** — `cargo check --workspace` still
rustc's every workspace lib/bin crate and all transitive deps.

For **integration test binary rustc invocations** (the work `--tests`
adds): each `--test foo` binary is its own rustc call. Downstream test
jobs do compile the same test binary, so the keys match — but
*they only match the test binary for the specific test surface that
job runs*. Harness shards run their specific corpus crate's test bins;
coverage shards run their lane's test bins; workspace-tests shards run
nextest with `--partition hash:N/M` which picks one third of the
binaries. So the warm pass's --tests work overlaps with at most one
downstream shard's test-bin compile per surface. With CW1+CA3+CW2's
sharding, each downstream shard compiles only a fraction of the
workspace's test binaries anyway, so the cross-job reuse benefit of
warming all of them upstream is structurally small.

Meanwhile the cost of `--tests` on warm-sccache is the rustc invocations
for every integration test in every workspace crate — a sizable chunk
of compile work added directly to the critical-path wall pole.

### Wall-delta expectation

Without CI measurement against the CW0 baseline we can't quantify the
exact delta, but the qualitative breakdown is:

- `warm-sccache` wall shrinks by the time previously spent on
  integration-test-binary rustc invocations
- harness / coverage / workspace-tests shards each gain a small
  per-shard delta for their own test-bin compile (parallelized across
  runners, so the wall pole among them does not move proportionally)

The structural argument is that the warm leader is on the critical
path while the downstream shards are parallel — shifting work from the
former to the latter reduces wall.

### Correctness

`cargo check --workspace` (without `--tests`) still type-checks every
workspace lib/bin crate and all transitive deps. It does *not*
type-check `#[cfg(test)]` modules or integration test files, but those
are type-checked by the downstream test jobs as part of running the
tests. CI failure surface is unchanged: a broken test compile fails
the downstream test job, not the warm leader.

The shared-key (`ci-ubuntu-stable-warm-sccache-no-bin-v2`) is *not*
bumped. Swatinem's restore brings in whatever was in target/ at the
end of the previous main run; if that target/ contains test-bin
artifacts from the pre-CW4 warm command, they linger as dead weight
but don't affect the new warm command's output. Eventually they age
out via Swatinem's pruning. Not bumping the key keeps the next post-CW4
run benefiting from cached lib/bin artifacts immediately.

## Lane (b, deferred) — per-target cache layer

The CW plan considered prototyping a per-target Swatinem cache slot
for `warm-sccache` so its `target/` is restored between runs (rather
than just `~/.cargo`).

**This lane is redundant given the current cache stack.**
`Swatinem/rust-cache@v2` *already* caches `target/` (the action's
built-in behavior; see
`.github/actions/setup-rust-cached/action.yml:101-108`). The composite
wires the v2 action onto `warm-sccache` with
`shared-key: ci-ubuntu-stable-warm-sccache-no-bin-v2`. Swatinem's v2
implementation walks `target/` and caches the curated subset that
benefits cross-run incremental compile while excluding the bloat that
would push the cache slot over GH Actions' 10 GB org cap.

The CW0 baseline's 10.2m warm-sccache wall therefore already reflects
target/ being restored. Adding a second, bespoke `actions/cache@v4`
layer on top of Swatinem would either:

- Duplicate what v2 already caches (wasted restore time, no compile
  savings), or
- Cache target/ content Swatinem deliberately excludes (likely bloat
  that hurts cache restore wall-time more than it helps compile
  wall-time)

Either direction needs **measurement** to justify, which requires
running CI with the candidate cache shape, comparing wall against the
CW0 control commit, and proving the delta is real and stable across
multiple runs. That measurement work is out of scope for the CW wave.

**Deferred to a future investigation** if `warm-sccache` becomes the
wall pole again post-CW4 and lane (a)'s gains plateau.

## Verifier evidence

```
[8] Warm-sccache lane documented in ci-modernization.md and landed
  PASS  Warm-sccache lane documented and landed
```

Condition 8 (`scripts/verify-ci-wall-acceleration.sh`) checks two
things:

1. `docs/operating/ci-modernization.md` contains either the phrase "PR
   critical-path acceleration" or a warm-sccache + --tests / target
   mention. CW4 promotes a new section by exactly that name.
2. The warm-sccache job block in `ci.yml` matches `cargo check
   --workspace` without `--tests` (lane a) OR contains a bespoke
   `actions/cache@...target/` step (lane b). CW4 lands lane (a).

## Notes

- The two lanes are not mutually exclusive in principle. CW4 lands
  lane (a) and defers lane (b) because lane (b)'s payoff requires
  measurement we cannot run here. A follow-on plan can add lane (b)
  on top if a real wall-time gap motivates it.
- The verifier intentionally accepts either lane individually — the
  plan was framed as "land whichever holds up against the CW0 baseline;
  document the other as deferred scope" and the verifier reflects that
  structure.
- `make warm-sccache` is not a real Makefile target — the warm pass is
  invoked directly from `ci.yml` as `cargo check --workspace`. Local
  developers warm their sccache implicitly via the `make check` /
  `make test-rust-*` pipeline.
