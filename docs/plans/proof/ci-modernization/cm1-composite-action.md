# CM1: setup-rust-cached composite action

CM1 extracts the Rust toolchain + sccache + Swatinem cache + googlesource
credentials bootstrap into a single composite action at
`.github/actions/setup-rust-cached/action.yml` and migrates all 12 sites
across the workflow tree to call it.

## Why this is CM1

CC9 had to bump twelve duplicated `mozilla-actions/sccache-action@v0.0.6`
references to `@v0.0.10` because the Rust setup block was copy-pasted
across 3 workflow files. The duplication is the root cause of CC9's
blast radius. CM1 cures it: the next sccache-action / Swatinem /
dtolnay/rust-toolchain bump becomes a 1-line edit.

## Before (per site, ~24 lines)

```yaml
- name: Configure googlesource credentials
  if: env.GOOGLESOURCE_COOKIE != ''
  env:
    GOOGLESOURCE_COOKIE: ${{ secrets.GOOGLESOURCE_COOKIE }}
  run: |
    touch ~/.gitcookies && chmod 0600 ~/.gitcookies
    git config --global http.cookiefile ~/.gitcookies
    echo ".googlesource.com\tTRUE\t/\tTRUE\t2147483647\to\t${GOOGLESOURCE_COOKIE}" >> ~/.gitcookies
    git config --global url."https://chromium.googlesource.com/a/".insteadOf "https://chromium.googlesource.com/"

- name: Install Rust toolchain
  uses: dtolnay/rust-toolchain@stable
  with:
    components: clippy

- name: Install sccache
  uses: mozilla-actions/sccache-action@v0.0.10

- name: Cache cargo artifacts
  uses: Swatinem/rust-cache@v2
  with:
    shared-key: ci-ubuntu-stable-clippy-no-bin-v2
    cache-directories: ~/.cargo/.rusty_v8
    cache-on-failure: "true"
    cache-bin: "false"
    save-if: ${{ github.ref == 'refs/heads/main' }}
```

## After (per site, 6 lines)

```yaml
- name: Set up Rust with cache
  uses: ./.github/actions/setup-rust-cached
  with:
    shared-key: ci-ubuntu-stable-clippy-no-bin-v2
    toolchain-components: clippy
    googlesource-cookie: ${{ secrets.GOOGLESOURCE_COOKIE }}
```

The composite preserves the canonical CC caching contract:
`cache-directories: ~/.cargo/.rusty_v8`, `cache-on-failure: true`,
`cache-bin: "false"`, `save-if: refs/heads/main`. Each call site only
needs to supply the variable inputs (`shared-key`, optional
`toolchain-components`, optional `cache-bin` override for desktop-ui).

## Composite action inputs

| Input | Required | Default | Notes |
|-------|----------|---------|-------|
| `shared-key` | yes | — | Swatinem cache partition; unique per job |
| `toolchain-components` | no | `""` | rustup components (`clippy`, `llvm-tools-preview`) |
| `cache-bin` | no | `"false"` | CC contract default; desktop-ui still overrides to `"true"` |
| `googlesource-cookie` | no | `""` | `${{ secrets.GOOGLESOURCE_COOKIE }}` passthrough |

Composite actions cannot read repository secrets directly, so each
caller passes the secret as an input. The googlesource step is skipped
when the cookie input is empty, preserving the previous behavior of the
`if: env.GOOGLESOURCE_COOKIE != ''` gate.

## Migration inventory (12 sites)

`.github/workflows/ci.yml` (9 sites):

| Job | shared-key | toolchain-components |
|-----|------------|----------------------|
| `rust-clippy` | `ci-ubuntu-stable-clippy-no-bin-v2` | `clippy` |
| `deny` | `ci-ubuntu-stable-deny-no-bin-v2` | — |
| `rust-runtime-tests` | `ci-ubuntu-stable-runtime-no-bin-v2` | — |
| `rust-workspace-tests` | `ci-ubuntu-stable-workspace-no-bin-v2` | — |
| `external-providers` | `ci-ubuntu-stable-external-providers-no-bin-v2` | — |
| `warm-sccache` | `ci-ubuntu-stable-warm-sccache-no-bin-v2` | — |
| `harness` | `ci-ubuntu-stable-harness-${{ matrix.surface }}-no-bin-v2` | `llvm-tools-preview` |
| `harness-nightly` | `ci-ubuntu-stable-harness-nightly-${{ matrix.surface }}-no-bin-v2` | — |
| `coverage` | `ci-ubuntu-stable-coverage-no-bin-v2` | — |

`.github/workflows/desktop-ui.yml` (1 site):

| Job | shared-key | cache-bin |
|-----|------------|-----------|
| `desktop-ui` | `ci-ubuntu-stable-desktop-ui-v2` | `"true"` (preserves pre-CM1 default-true behavior) |

`.github/workflows/node-compat-nightly.yml` (2 sites):

| Job | shared-key |
|-----|------------|
| `node-compat-rust-corpus` | `node-compat-rust-corpus-ubuntu-stable-no-bin-v2` |
| `node-compat-evidence` | `node-compat-nightly-ubuntu-stable-no-bin-v2` |

## Sites intentionally not migrated

- `ci.yml::rust-format` (line 32): uses only `dtolnay/rust-toolchain` —
  no sccache or Swatinem cache. Format checks are fast and do not need
  the cargo cache; the toolchain step alone is not the CM1 pattern.
- `release.yml::*`: release jobs use Swatinem + googlesource without
  sccache, and the release surface is owned by `distribution-plan.md`.
  Out of CM scope.

## Verifier deltas

Before CM1:

- Condition 2 (composite action exists): FAIL
- Condition 3 (zero inline sccache-action references): FAIL — 12 hits

After CM1:

- Condition 2: PASS
- Condition 3: PASS — `grep -rn "mozilla-actions/sccache-action"
  .github/workflows/` returns no matches.

The remaining 8 ledger rows (CM2-CM8) gate the other 5 failing
conditions; they will be flipped in subsequent waves.
