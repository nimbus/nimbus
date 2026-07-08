# Third-Party Code in nimbus-blob

This crate adapts code and durability recipes from
[RustFS](https://github.com/rustfs/rustfs) (Apache-2.0) at revision
`bd5d3c5d92a0aa70a7d92da3e48761d6e61f0dc9`
(`1.0.0-beta.8-879-gbd5d3c5d`, 2026-07-08). RustFS is a trademark of
RustFS, Inc.; the name appears here and in provenance headers only.

Upstream ships no NOTICE file; each adapted file preserves the upstream
`Copyright 2024 RustFS Team` header per Apache-2.0 §4. The upstream license
text is at `LICENSE-APACHE-rustfs` in this crate.

| File | Upstream source | Kind |
| --- | --- | --- |
| `src/disk.rs` | `crates/ecstore/src/disk/os.rs` (directory-fsync helpers, rename-retry predicate and its tests) and the `SyncMode` durable-write recipe from `crates/ecstore/src/disk/local.rs` (`write_all_meta`/`write_all_internal`) | Adapted (reimplemented against `LocalPackStore`; no verbatim lift) |

Architecture patterns borrowed without code (root/format ownership
discipline from `crates/ecstore/src/disk/local.rs` and
`crates/ecstore/src/store/init_format.rs`) are credited in module docs
(`src/root_guard.rs`) and are not lifted files.

Provenance and security-review requirements for this table are enforced by
`scripts/verify-third-party-attribution.sh` and
`scripts/verify-rustfs-storage-hardening.sh`.
