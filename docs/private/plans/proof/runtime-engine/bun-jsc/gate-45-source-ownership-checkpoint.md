# Gate 45: BJA4L2 Source Ownership Checkpoint

Date: 2026-05-24

## Purpose

Gate 44 proved that the source-owned Bun/WebKit namespace build and symbol
audit work on Debian 13. This gate removes the last BJA4L2 blocker: the proof
patch is no longer only a dirty local checkout. It is committed and tagged in a
Nimbus-owned Bun repository.

## Nimbus Bun Source

Repository:

```text
https://github.com/nimbus/bun
```

Visibility:

```text
PUBLIC
```

Branch:

```text
nimbus/bja4l2-simdutf-namespace
```

Tag:

```text
bun-v1.4.0-nimbus.1
```

Revision:

```text
5ba54ccecdfabd857a7ca362c14c0f614d25b21b
```

Base upstream proof head:

```text
a409f596e8e1394d8860e2cd8b2bb558ff1afcac
```

Patch hash:

```text
eb9d7d047b44fdc7921a3447c6d155ee1c9f8a1cef9892a79d60280f6e589c85
```

The patch changes only:

```text
scripts/build.ts
scripts/build/config.ts
scripts/build/deps/webkit.ts
scripts/build/flags.ts
scripts/build/rust.ts
src/simdutf_sys/bun-simdutf.cpp
src/simdutf_sys/simdutf.rs
```

Commit:

```text
5ba54ccecd Add Nimbus simdutf namespace build option
```

Remote verification:

```text
5ba54ccecdfabd857a7ca362c14c0f614d25b21b refs/heads/nimbus/bja4l2-simdutf-namespace
5ba54ccecdfabd857a7ca362c14c0f614d25b21b refs/tags/bun-v1.4.0-nimbus.1
```

## Local Source Layout

The local source is available at:

```text
/Users/jack/src/github.com/nimbus/bun
```

It is a Git worktree of the local Bun object store rather than a duplicate
clone, to avoid wasting disk while keeping the `~/src/github.com/<org>/<repo>`
source layout. The worktree is clean at the Nimbus tag.

The original proof worktree remains at:

```text
/Users/jack/src/github.com/oven-sh/bun
```

It still contains the local dirty proof patch and should no longer be treated
as the product source identity. Future Nimbus gates should consume the
`nimbus/bun` branch or tag above.

## Verification

In `/Users/jack/src/github.com/nimbus/bun`:

```sh
git diff --check
cargo fmt --all --check
```

Both passed before the commit.

The repository was originally created empty. Because the local Bun checkout was
shallow, the first branch push failed with a missing-object remote unpack
error. The source repo was then unshallowed:

```text
shallow=false
```

After that, the branch and tag pushed successfully to `nimbus/bun`.

## Decision

BJA4L2 is now closed:

- Gate 43 proved the build seam and fail-closed namespace guards.
- Gate 44 proved the source-owned Debian build and symbol audit.
- Gate 45 provides a Nimbus-owned branch and tag for reproducible source
  identity.

The next slice is BJA4L3: make the linked verification gate consume this
source identity, reject unsafe duplicate-symbol workarounds, and audit the
namespaced artifacts automatically.
