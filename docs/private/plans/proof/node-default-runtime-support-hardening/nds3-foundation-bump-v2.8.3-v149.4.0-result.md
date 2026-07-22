# NDS3 Foundation Bump: Deno v2.8.3 and rusty_v8 v149.4.0

Date: 2026-06-13

## Scope

This checkpoint updates the foundation forks used by NDS PR #10:

- `nimbus/rusty_v8`: upstream `v149.4.0` with Nimbus fork metadata and release tag `v149.4.0-nimbus.1`.
- `nimbus/deno`: upstream Deno `v2.8.3` merged into the Nimbus fork, pinned to `v149.4.0-nimbus.1`, with release tag `v2.8.3-nimbus.1`.
- Nimbus PR branch: dependency pins updated to the immutable fork tags and adjusted for the Deno 2.8.3 API shape.

This checkpoint does not claim the NDS gate is green.

## Fork State

`nimbus/rusty_v8`:

- Branch: `nimbus/v149.4.0`
- Commit: `ad9a69167db7`
- Tag: `v149.4.0-nimbus.1`
- Default branch updated to `nimbus/v149.4.0`
- README updated for the `v149.4.0-nimbus.1` release URLs and `nimbus/v149.4.0` branch badge.
- Release workflow run: `27452871236`
- Release result: success, 5/5 jobs green.
- Published release asset count: 16.

`nimbus/deno`:

- Branch: `nimbus/v2.8.3`
- Commit: `53b797e0c9fc`
- Tag: `v2.8.3-nimbus.1`
- Default branch updated to `nimbus/v2.8.3`
- `Cargo.toml` pins `v8 = "149.4.0"` and patches `v8` to `nimbus/rusty_v8` tag `v149.4.0-nimbus.1`.
- `.cargo/config.toml` uses `RUSTY_V8_VERSION = "149.4.0-nimbus.1"`.

## Nimbus Changes

Nimbus now pins:

- Deno packages to `git+https://github.com/nimbus/deno?tag=v2.8.3-nimbus.1#53b797e0c9fc255bcd05432b049fc7073ca5fd18`
- `v8` to `git+https://github.com/nimbus/rusty_v8?tag=v149.4.0-nimbus.1#ad9a69167db75088292c993e56071a02f69599fd`

The runtime compile required three Nimbus-local API adjustments:

- Load hook results now carry `(source, format, effective_url)`. Nimbus now honors `effective_url` on fallthrough.
- `deno_web::deno_web::init` now receives `deno_web::BlobStore::default_arc()`.
- `ValidateImportAttributesCb` now receives an `ImportAttributesContext`; Nimbus ignores the new context and preserves the existing Node import-attribute validation behavior.

No V8 or rusty_v8 native binding changes were made beyond the fork release metadata already carried in `nimbus/rusty_v8`.

## Proof Commands

Rusty V8 release:

```text
gh run view 27452871236 --repo nimbus/rusty_v8 --json status,conclusion,jobs
```

Result: completed successfully. Jobs green:

- `release x86_64-pc-windows-msvc`
- `release aarch64-apple-darwin`
- `release aarch64-unknown-linux-gnu`
- `release x86_64-unknown-linux-gnu`
- `publish release assets`

```text
gh release view v149.4.0-nimbus.1 --repo nimbus/rusty_v8 --json tagName,isDraft,isPrerelease,publishedAt,assets
```

Result: release exists, non-draft, non-prerelease, published at `2026-06-13T04:26:52Z`, 16 assets.

Deno fork:

```text
CARGO_ENCODED_RUSTFLAGS=$'--cfg\x1ftokio_unstable' cargo check -p deno_core -p deno_node -p deno_crypto
```

First run before the rusty_v8 release published failed because `src_binding_simdutf_release_aarch64-apple-darwin.rs` was still a release-asset 404.

Final result after release publication: pass, `Finished dev profile [unoptimized + debuginfo] target(s) in 23.30s`.

Nimbus:

```text
cargo update -p deno_core -p deno_node -p deno_crypto -p v8
```

Final result: lockfile resolved to Deno `v2.8.3-nimbus.1` and rusty_v8 `v149.4.0-nimbus.1`.

```text
cargo check -p nimbus-runtime
```

First run exposed the Deno 2.8.3 API changes listed above.

Final result after Nimbus-local API adjustments: pass, `Finished dev profile [unoptimized + debuginfo] target(s) in 3.24s`.

```text
cargo fmt --all --check
```

Result: pass, no output.

```text
bash scripts/verify-node-default-runtime-support-hardening.sh
```

Result: fail, `13 passed, 21 failed`.

This remains expected for the still-red NDS PR. The verifier reported missing/unfinished plan proof rows and step 9 still red. This checkpoint does not promote support or claim zero gaps.

## Current Posture

Committed `docs/architecture/runtime/node-default-support-posture.json` was not regenerated in this checkpoint. Its current committed required-lane metrics are:

- `node22.v8_isolate_required`: 82 gaps, 96.55% pass rate, 2297 passed / 2379 total.
- `node24.v8_isolate_required`: 88 gaps, 96.36% pass rate, 2327 passed / 2415 total.

## Guardrails

- No official fixture or checker edits.
- No hand-edited support/posture JSON.
- No V8/rusty_v8 native binding changes in Nimbus.
- No local Deno path pin left in Nimbus.
- No `git add -A`; stage only named files for this checkpoint.
- PR #10 remains draft and unmerged.

## Recommended Next Action

Run the post-bump blocker probes against the broad high-ROI clusters before any further promotion:

- `vm` module import-meta and top-level-await blockers.
- WebCrypto KMAC / Ed448 fixtures, because Deno 2.8.3 pulls in modern crypto changes and new crypto dependencies.
- Module loader ESM/CJS hooks and builtin resolution.
- Remaining fs/stream cluster.

Only regenerate generated posture/classification artifacts after a meaningful broad-batch result.
