# Third-Party Dependency Patches

This directory contains local crates.io patches used only when an upstream crate
has no released version carrying a required fix.

## object_store and s3s

The `object_store-0.14.0` and `s3s-0.14.0` patches copy the crates.io releases.
They change only the `quick-xml` dependency floor and the `crc-fast` pin.
Nimbus requires `quick-xml >= 0.41.0` for RUSTSEC-2026-0194. The upstream
releases still require `quick-xml 0.40.x`.

Nimbus pins `crc-fast` at version `1.6.0`. Newer `crc-fast` 1.x releases
require yanked `spin` version `0.10.0`. Version `1.6.0` provides the same CRC
digest API that both patches use. Remove the pin after `crc-fast` drops the
yanked dependency. Remove both patches after upstream supports the fixed
`quick-xml` version.

## libsql

The `libsql-0.9.30` patch copies its crates.io release. Nimbus removes only the
unused Hyper `http2` feature. The TLS connector calls `enable_http1()` and never
enables HTTP/2. This change preserves remote HTTP/1 behavior. It also removes
the unsupported `h2` 0.3 line affected by RUSTSEC-2026-0258.

The crates.io archive omits the license file. Nimbus copied `LICENSE.md` from
upstream source revision `0653c5788d77ef16a97c56ff3e9fdc11717a72d9`. Remove
this patch after libsql publishes a fixed transport dependency.

## flume

The `flume-0.12.0` patch copies its crates.io release. Nimbus updates only its
`spin` dependency from version `0.9.8` to `0.12.3`. The flume mutex API stays
unchanged. Remove this patch after flume publishes a non-yanked dependency.

## lazy_static

The `lazy_static-1.5.0` patch copies its crates.io release. Nimbus updates its
optional `spin` dependency from version `0.9.8` to `0.12.3`. The vendored copy
also makes the return lifetimes explicit in both `Lazy::get` implementations to
remain clean under the current Rust `mismatched_lifetime_syntaxes` lint. Nimbus
patched the no-std implementation earlier; the current patch completes the
std-backed implementation. The no-std implementation uses the unchanged
`spin::Once` API. Remove this patch after lazy_static publishes a non-yanked
dependency.

## brotli

The `brotli-3.5.0` patch copies its crates.io release. Nimbus removes `ffi-api`
from the crate's `default` features. The feature stays available as an opt-in.
Pingora's brotli copy exports no `BrotliEncoder*` C symbols. It never references
the gated `brotli-decompressor` FFI module.

The `brotli-decompressor-2.5.1` patch copies its crates.io release. Nimbus
backports upstream's 4.x fix. The patch gates `pub mod ffi;` behind a new
`ffi-api` feature. Version 2.5.1 otherwise exported the module and its
`#[no_mangle] BrotliDecoder*` C symbols unconditionally.

This change fixes the `rust-lld: duplicate symbol: BrotliDecoder*` failures.
Pingora-core 0.8.1 uses `brotli 3` and `brotli-decompressor 2.5.1`. Those
dependencies exported the same C symbols as Deno's `brotli-decompressor 4.0.3`.
The Deno runtime legitimately enables the gated FFI module.

LLD rejects duplicate definitions in binaries that link both dependency lines.
GNU ld accepts the definitions. This difference made the failure CI-only. Both
patches apply only to Pingora's `^3` and `^2` nodes. The Deno runtime's brotli
6/8 and brotli-decompressor 4/5 stay on crates.io. Remove both patches after
Pingora leaves the brotli 3.x line.

## lru

The `lru-0.16.4` patch copies its crates.io release. Pingora 0.8.1 and
`mysql_async` 0.36 still require the 0.16 line.

Nimbus backports the exact panic-safety fix and regression test from upstream
commit `f9a7f00fcf2d33e00adb03758cb350aaaa52cddb`. That commit fixes
RUSTSEC-2026-0253. Nimbus also backports upstream commit
`a615a5b29f21de6dd222394da91ab4e2c6918016`. That commit binds the returned
mutable reference to the cache borrow.

Nimbus adds state assertions to the upstream panic-safety regression. Nimbus
also adds a compile-fail lifetime proof. Remove this patch after every consumer
accepts `lru` 0.18.2 or later.
