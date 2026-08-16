# Third-Party Dependency Patches

This directory contains local crates.io patches used only when an upstream crate
has no released version carrying a required fix.

- `object_store-0.14.0` and `s3s-0.14.0` are copied from crates.io releases and
  keep their upstream source unchanged except for the `quick-xml` dependency
  floor. Nimbus pins them locally so `quick-xml >= 0.41.0` resolves for
  RUSTSEC-2026-0194 while upstream releases still require `quick-xml 0.40.x`.
  Remove once the upstream crates publish compatible releases with the fixed
  `quick-xml` dependency.

- `brotli-3.5.0` is copied verbatim from its crates.io release with a single
  change: `ffi-api` is removed from the crate's `default` features (it stays
  defined as an opt-in), so pingora's brotli copy exports no `BrotliEncoder*` C
  symbols and never references `brotli-decompressor`'s (gated) ffi module.
- `brotli-decompressor-2.5.1` is copied verbatim from its crates.io release
  with upstream's own 4.x fix backported: `pub mod ffi;` is gated behind a new
  `ffi-api` feature (in 2.5.1 the module — and its `#[no_mangle]
  BrotliDecoder*` C exports — was unconditional). This is the actual fix for
  the `rust-lld: duplicate symbol: BrotliDecoder*` link failures: pingora-core
  0.8.1 → `brotli 3` → `brotli-decompressor 2.5.1` exported the same C symbols
  the Deno runtime's `brotli-decompressor 4.0.3` (gated ffi, legitimately
  enabled) exports, and lld rejects the duplicate definitions in any binary
  linking both (GNU ld tolerates them, which is why the failure was CI-only).
  Both brotli patches apply only to pingora's `^3`/`^2` nodes; the Deno
  runtime's brotli 6/8 and brotli-decompressor 4/5 stay on crates.io. Remove
  both once Pingora moves off the brotli 3.x line.

- `lru-0.16.4` is copied from its crates.io release because Pingora 0.8.1 and
  `mysql_async` 0.36 still require the 0.16 line. Nimbus backports the exact
  panic-safety fix and regression test from upstream commit
  `f9a7f00fcf2d33e00adb03758cb350aaaa52cddb`, which fixes
  `RUSTSEC-2026-0253`. It also backports upstream commit
  `a615a5b29f21de6dd222394da91ab4e2c6918016`, which binds the returned mutable
  reference to the cache borrow. Nimbus adds state assertions to the upstream
  panic-safety regression and a compile-fail lifetime proof. Remove this patch
  after every consumer accepts `lru` 0.18.2 or later.
