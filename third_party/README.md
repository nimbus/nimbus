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
  defined as an opt-in). `pingora-core 0.8.1` hard-depends on `brotli 3` with
  default features, whose `ffi-api` C exports (`BrotliDecoder*` /
  `BrotliEncoder*`) collide at link time with the pinned Deno runtime's own
  brotli C exports, producing `rust-lld: duplicate symbol` errors when
  `nimbus-server` links both. Pingora uses only brotli's Rust API, so dropping
  the C exports is behaviorally inert. Remove once Pingora no longer pulls a
  brotli whose default features export the C FFI (e.g. it gates brotli behind a
  feature, or moves to a version that unifies with the Deno runtime's brotli).
