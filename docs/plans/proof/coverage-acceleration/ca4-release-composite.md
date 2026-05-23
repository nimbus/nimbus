# CA4 — migrate release.yml to setup-rust-cached composite

Lands the holdout that CM1 explicitly deferred: the 5 inline
`dtolnay/rust-toolchain` + `Swatinem/rust-cache` + googlesource
bootstrap sites in `release.yml` now route through the
`setup-rust-cached` composite. Release tag builds pick up sccache
for the first time, gaining the same cold-build acceleration that
PR CI has had since CC1.

## Sites migrated

| Job | Runner | matrix.target | Shared key (new) |
|-----|--------|---------------|------------------|
| `build-linux-arm64` | `ubuntu-24.04-arm` | aarch64-unknown-linux-gnu | `release-aarch64-unknown-linux-gnu-no-bin-v1` |
| `build` (matrix entry 1) | `ubuntu-22.04` | x86_64-unknown-linux-gnu | `release-x86_64-unknown-linux-gnu-no-bin-v1` |
| `build` (matrix entry 2) | `macos-14` | aarch64-apple-darwin | `release-aarch64-apple-darwin-no-bin-v1` |
| `build` (matrix entry 3) | `windows-latest` | x86_64-pc-windows-msvc | `release-x86_64-pc-windows-msvc-no-bin-v1` |

Each site replaces its previous shape:

- `actions/checkout`
- `Configure googlesource credentials` (inline)
- `Install Rust toolchain` (`dtolnay/rust-toolchain@<sha>`)
- `Cache cargo artifacts` (`Swatinem/rust-cache@<sha>` with explicit
  `key:` and no sccache)

with a single composite call:

```yaml
- name: Set up Rust with cache
  uses: ./.github/actions/setup-rust-cached
  with:
    shared-key: release-${{ matrix.target }}-no-bin-v1
    save-cache: always
    googlesource-cookie: ${{ secrets.GOOGLESOURCE_COOKIE }}
```

The Windows-Perl + long-paths bootstrap steps stay where they are in
the `build` matrix; only the toolchain + cache + googlesource sites
collapse into the composite.

## New composite input: `save-cache`

The composite previously hardcoded
`save-if: ${{ github.ref == 'refs/heads/main' }}` — the CC9
retraction that prevents PR cold-cache poisoning of main warm caches.
That logic is correct for PR CI but wrong for release.yml: tag refs
never match `refs/heads/main`, so a naive migration would leave
release builds saving to nothing.

CA4 introduces a `save-cache` input with three values:

| Value | save-if expression | Used by |
|-------|--------------------|---------|
| `auto` (default) | `github.ref == 'refs/heads/main'` | PR CI workflows (ci.yml, desktop-ui.yml, node-compat-nightly.yml) — preserves CC9 behavior unchanged |
| `always` | `true` | release.yml tag builds — each release tag is intentional and per-target shared keys isolate the cache namespace |
| `never` | `false` | reserved (no caller yet) |

Existing callers don't set the input → resolve to `auto` → behavior
unchanged.

## Why this is safe

- **Cache namespace isolation**: PR CI keys use the
  `ci-{platform}-{role}-no-bin-vN` shape (e.g.
  `ci-ubuntu-stable-clippy-no-bin-v2`). Release keys use
  `release-{target}-no-bin-v1`. These shared-key prefixes don't
  collide, so release tag saves can't pollute PR CI warm caches.
- **sccache cross-platform**: `mozilla-actions/sccache-action@v0.0.10`
  works on Linux, macOS, and Windows. The CC plan's per-job
  `SCCACHE_GHA_ENABLED=true` toggle is exported by the composite
  uniformly across all runners.
- **mold gate**: the Linux-only `Install mold linker` step has
  `if: runner.os == 'Linux'` so it cleanly no-ops on the `macos-14`
  and `windows-latest` matrix entries.

## sccache adoption cold/warm pattern

Baseline (`ca0-baseline.md`): release.yml has zero sccache references
workflow-wide; every release tag does a full cold compile.

Expected post-CA4 trajectory:

- **First release tag after CA4 lands**: cold sccache (no prior
  cache to restore from). sccache stats published in the per-job
  log show `cache hits: 0`, `cache misses: <N>`. Wall-clock close
  to baseline (no acceleration yet because sccache populates).
- **Second release tag after CA4 lands**: warm sccache (restores
  the previous tag's saved cache). sccache stats show appreciable
  hit rate. Wall-clock expected to drop meaningfully on the cold
  Linux/macOS lanes; Windows wall is the CA5 follow-up
  investigation pole.

Actual cold/warm numbers will be appended here after the first two
post-CA4 release tags fire.

## Cross-workflow regression gate

Condition 8 of the CA verifier reasserts the CM-era invariant:

```bash
grep -cE 'uses:[[:space:]]*mozilla-actions/sccache-action' .github/workflows/*.yml
# Must equal 0 across every workflow file
```

Migrating release.yml without breaking this invariant proves the
composite is the single entry point for sccache wiring repo-wide.

## Verifier delta

Before CA4:
- Condition 7 (release.yml composite-only): FAIL —
  `release.yml still has 2 inline Swatinem/rust-cache reference(s)`

After CA4:
- Condition 7: PASS — `Zero inline Swatinem/rust-cache references
  in .github/workflows/release.yml`
