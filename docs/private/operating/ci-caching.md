# CI caching contract

Hosted CI uses two cache layers. Both layers draw on one budget.

## The budget

GitHub gives each repository 10 GiB of Actions cache. When the total goes
above 10 GiB, GitHub deletes entries in least-recently-used order. The limit
covers the whole repository. No key and no layer holds a reservation.

Every cache entry therefore competes with every other cache entry.

## The two layers

**Swatinem/rust-cache** keeps `target/`, `~/.cargo/.rusty_v8`, and
`~/.cargo/git/checkouts` in one entry for each `shared-key`. An entry holds
multiple gibibytes. The `.github/actions/setup-rust-cached` composite owns
the pin and the inputs.

**sccache over the GitHub Actions cache** keeps one small entry for each
rustc call. Entries are tens of mebibytes. `RUSTC_WRAPPER` and
`SCCACHE_GHA_ENABLED` turn it on for each workflow that needs it.

The layers are complements, not alternatives. rust-cache restores a warm
`target/` to a lane that owns its `shared-key`. sccache covers the lanes that
rust-cache cannot keep warm. PW4c measured an uneven rust-cache hit rate
across jobs. The libsql external-provider shard showed 0% and the storage
harness showed 77%. Retiring sccache would give the consistently cold lanes a
full cold-compile cost on each pull request.

## Rules

1. Each `shared-key` must stay unique for each job. Bump the trailing `-vN`
   to invalidate.
2. `save-cache: auto` is the default. It saves only on pushes to `main`. This
   keeps a pull request from writing a cold cache over a warm one.
3. A scheduled workflow runs on `refs/heads/main`. `auto` therefore saves on
   each scheduled run. Set `save-cache: never` for a scheduled lane that does
   not need to hold a multi-gibibyte slot.
4. Prefer `consume-prebuilt-v8` where the lane supports it. The prebuilt V8
   payload then goes to a separate content-keyed cache that all such lanes
   share, and it stays out of each lane's private slot.
5. A large rust-cache slot is not free. It takes space from sccache, and
   sccache serves every lane.

## Measurement, 2026-09-16

| Layer | Size | Entries |
| --- | --- | --- |
| rust-cache | 8.80 GiB | 2 |
| sccache | 1.84 GiB | 3,862 |
| npm | 0.19 GiB | 1 |
| **Total** | **10.84 GiB** | **3,865** |

The pool was above the 10 GiB limit. Two rust-cache entries held 81% of the
budget:

- `v0-rust-node-compat-rust-corpus-ubuntu-stable-no-bin-v2-...` at 4.54 GiB
- `v0-rust-ci-ubuntu-stable-coverage-no-bin-v2-...` at 4.26 GiB

Only these two rust-cache entries stayed. The repository declares more than
ten `shared-key` values, so LRU had already deleted the slots for every other
lane.

sccache was the victim, not the cause. Its 3,862 objects shared the 1.84 GiB
that remained, so GitHub deleted them almost as fast as CI wrote them. The
coverage reducer reported 207 write errors out of 207 and a 0% hit rate. The
warm-sccache lane reported 1,276 write errors out of 1,555 and spent 13m51s.

### Action taken

The node-compat nightly corpus lane moved to `save-cache: never`. It runs
only on a schedule and on manual dispatch, so its wall-clock time is off the
pull-request critical path, and it was the largest single consumer. The lane
keeps sccache, and it gains an sccache pool that works, so it does not fall
back to a fully cold build. Recent runs take 48 to 58 minutes against a
120-minute timeout.

This returns about 4.54 GiB. The expected pool is near 6.3 GiB, which leaves
about 3.7 GiB of headroom for sccache.

The coverage slot stays. It serves the pull-request path. Its size will grow
when the coverage lane covers all 41 sharded workspace members, because the
lane then builds more instrumented test binaries. The headroom above absorbs
that growth.

### How to measure again

```bash
gh api repos/nimbus/nimbus/actions/cache/usage
gh api --paginate "repos/nimbus/nimbus/actions/caches?per_page=100" \
  --jq '.actions_caches[] | [.size_in_bytes, .key] | @tsv' \
  | sort -rn | head -20
```

Compare `active_caches_size_in_bytes` against 10 GiB. A total at or above the
limit means sccache is losing entries, whatever the hit-rate logs report.
