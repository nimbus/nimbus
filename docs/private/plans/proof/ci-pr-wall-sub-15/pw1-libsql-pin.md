# PW1 — libsql image pin + docker-image cache lane

## Pin

`ghcr.io/tursodatabase/libsql-server:latest` → `:v0.24.33`.

Selected 2026-05-23: most recent v0.24.x present on GHCR for
`tursodatabase/libsql-server`. Probed via:

```
TOKEN=$(curl -s 'https://ghcr.io/token?scope=repository:tursodatabase/libsql-server:pull' | jq -r .token)
curl -s "https://ghcr.io/v2/tursodatabase/libsql-server/tags/list?n=1000" \
  -H "Authorization: Bearer ${TOKEN}" | jq -r '.tags[]' | grep -E '^v0\.24\.' | sort -V | tail -10
```

Result (highest tags):

```
v0.24.24 v0.24.25 v0.24.26 v0.24.27 v0.24.28
v0.24.29 v0.24.30 v0.24.31 v0.24.32 v0.24.33
```

### Tag selection notes

The first PW1 pin was `v0.24.26`. That tag has a sqld bug in
`--enable-namespaces` mode where the server extracts the first
dotted label of the `Host` header as the namespace and ignores the
`x-namespace` header that the libsql Rust client sends. A request
to `http://127.0.0.1:18080` is routed to namespace `127` and
returns 404. The bug is fixed by `v0.24.33`.

The failed `v0.24.26` pin reached PW2-backfill before being caught
(External Provider Tests (libsql) failed with
`Hrana: api error: status=404 Not Found, body={"error":"Namespace
\`127\` doesn't exist"}` for all 8 libsql tests). PW5 repins to
`v0.24.33` after verifying all 8 libsql provider tests pass locally
against that tag. Note: the original PW1 probe used `n=500` and
listed only up through `v0.24.26`; with `n=1000` the full set
through `v0.24.33` is visible. Always raise `n=` when probing for
tag selection.

## Cache lane

Two `libsql-server` usages in `ci.yml` today (PW2 will move the
second to `coverage.yml`):

- `external-provider-tests (libsql)` matrix shard (gate path)
- `coverage` libsql fixture (full-wall path)

Both gain three new steps before "Start libsql provider fixture":

1. `actions/cache@v5` keyed on `libsql-image-v0.24.33` with path
   `/tmp/libsql-image.tar.gz`.
2. On cache hit: `docker load --input /tmp/libsql-image.tar.gz`.
3. On cache miss: `docker pull` + `docker save | gzip > /tmp/...`.

The pin alone removes manifest churn and ensures behavioral
stability. The cache lane removes the cold-pull cost on warm
runs (cache hit). On cache miss the cost is `pull + save` instead
of just `pull`, but the next run is warm.

## Expected impact

CW5 libsql duration: **17m 45s** total
(27m 17s queue wait excluded — that's PW3 territory).

Decomposition estimate:
- Image pull from GHCR: ~30s..2m (varies by GHCR backend latency)
- `--enable-namespaces` startup overhead: ~10s
- Actual cargo test work: ~6m..8m (filtered to libsql provider)
- Test setup/teardown per case: ~remainder

Cold-pull factor was the gate-bound pole. With the cache lane:
- Warm runs (cache hit): pull → 0; load ≈ 5s
- Cold runs (cache miss): pull + save ≈ pull + 2s

Plus the pin removes the cold-pull resolving an arbitrary
`:latest` tag (which can shift mid-day).

Conservative estimate: libsql gate duration drops from **17m 45s
→ ≤ 10m** on warm runs. Gate floor drops from **18m → 11m**.

PW5 will measure this directly.

## Verifier

Verifier condition 4 passes after PW1:

```
[4] All libsql image refs pinned to vX.Y.Z (ci.yml + coverage.yml)
  PASS  All libsql refs pinned with vX.Y.Z comments
```

The verifier was loosened in this same commit from ±3 lines to
±10 lines around the libsql-server line for the `# vX.Y.Z`
comment search — comments naturally sit at step boundaries
(`- name:`) which can be 5–8 lines above the multi-line
`docker run` command. ±10 keeps the proximity requirement
meaningful while accommodating YAML step structure.

## Note on coverage.yml

PW1 pins both libsql references in `ci.yml` even though the
second (under `coverage:`) will be moved to `coverage.yml` in
PW2. This is deliberate: PW1 lands a working pinned configuration
that survives PW2 without requiring PW2 to re-pin. PW2's git
mv preserves the pin.
