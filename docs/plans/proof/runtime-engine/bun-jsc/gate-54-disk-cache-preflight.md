# Gate 54: BJA8 Disk And Cache Verification Preflight

Date: 2026-05-24

## Purpose

`BJA8` needs broad local and Debian verification without accidentally turning
disk pressure into a false product blocker. This preflight records the current
local and `minicloud` space state, identifies which heavy artifacts are
canonical caches to reuse, and records the local WebKit environment gate for
source-backed Bun/JSC linked verification.

## Local Mac State

Command:

```sh
df -h / /private/tmp /Users/jack/src/github.com/nimbus/nimbus
du -sh target /private/tmp/nimbus-bun-cache \
  /private/tmp/nimbus-bun-linked-adapter-release \
  /private/tmp/nimbus-bun-proof-target-release \
  /private/tmp/nimbus-bja7-product-metadata.patch \
  /private/tmp/nimbus-bja7-product-metadata-code.patch
```

Observed:

```text
/System/Volumes/Data: 926Gi size, 634Gi used, 201Gi available, 76% capacity
target: 58G
/private/tmp/nimbus-bun-cache: 2.3G
/private/tmp/nimbus-bun-linked-adapter-release: 3.5G
/private/tmp/nimbus-bun-proof-target-release: 0B
/private/tmp/nimbus-bja7-product-metadata.patch: 52K
/private/tmp/nimbus-bja7-product-metadata-code.patch: 32K
```

Local Bun source-backed linked verification remains environment-gated:

```text
/Users/jack/src/github.com/nimbus/bun/vendor/WebKit: absent
BUN_WEBKIT_PATH: unset
```

Decision: do not clone or install WebKit locally for `BJA8` unless explicitly
requested. The local Mac has enough free space for broad Nimbus checks, and
the small BJA7 patch files plus old `/private/tmp` Bun proof artifacts can be
pruned later if space pressure returns. The local `target` directory is a
canonical build cache for the final local gates and should be reused.

## Debian 13 Minicloud State

Command:

```sh
ssh nimbus@192.168.4.29 'df -h / /home /tmp'
ssh nimbus@192.168.4.29 'du -sh \
  /home/nimbus/.cache/nimbus-bun-proof \
  /home/nimbus/src/github.com/nimbus/nimbus-worktrees/bja5-hostbridge/target \
  /home/nimbus/src/github.com/nimbus/bun-worktrees/bja5-hostbridge \
  /home/nimbus/.cargo \
  /home/nimbus/.rustup'
```

Observed:

```text
/dev/sda2: 449G size, 270G used, 157G available, 64% capacity
/tmp: 3.9G size, 643M used, 3.3G available, 17% capacity
/home/nimbus/.cache/nimbus-bun-proof: 37G
/home/nimbus/src/github.com/nimbus/nimbus-worktrees/bja5-hostbridge/target: 35G
/home/nimbus/src/github.com/nimbus/bun-worktrees/bja5-hostbridge: 737M
/home/nimbus/.cargo: 5.8G
/home/nimbus/.rustup: 3.5G
```

The remote Bun worktree is at the expected source revision:

```text
7c6dd4312e437c67a6c4c8cbb252f0d7ae898db8
```

The remote worktree does not contain `vendor/WebKit`, but the verified shared
adapter artifact exists at:

```text
/home/nimbus/.cache/nimbus-bun-proof/shared-adapter-release-local-namespaced/libnimbus_bun_jsc_embedder.so
```

Decision: keep using home-backed proof paths on `minicloud`. Do not use `/tmp`
for Bun/WebKit builds or Cargo targets. Do not prune
`/home/nimbus/.cache/nimbus-bun-proof` or the remote Nimbus `target` before
the final linked proof because they are the warmed canonical caches that made
Gate 53 pass. If a later cleanup is needed, prune only after BJA8 evidence is
recorded.

## Verification Strategy

- Local broad gates should use the canonical Nimbus worktree and shared
  `target`.
- Local `make verify-bun-jsc-linked-adapter` cannot count as source-backed
  loaded-adapter evidence until local WebKit source is installed or
  `BUN_WEBKIT_PATH` is set.
- Debian `minicloud` remains the source-backed loaded-adapter verifier for
  `BJA8`, using `/home/nimbus/.cache/nimbus-bun-proof/*` and the canonical
  Nimbus/Bun proof worktrees.
- Existing unrelated dirty files in the local Nimbus worktree remain outside
  Bun/JSC commits.

## Decision

The disk/cache preflight is complete. There is enough space to continue BJA8
without deleting proof artifacts. The next step is final broad verification:
local default contract and broad repo gates, plus the Debian linked proof using
home-backed caches.
