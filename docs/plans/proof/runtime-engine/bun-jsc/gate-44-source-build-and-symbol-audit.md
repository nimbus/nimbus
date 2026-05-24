# Gate 44: BJA4L2 Source Build And Symbol Audit

Date: 2026-05-24

## Purpose

Gate 43 proved that the Bun build seam can configure a private simdutf
namespace for Bun C++, Bun Rust FFI, and local WebKit. This gate proves the
next property: the source-owned WebKit/Bun build completes on Debian and the
resulting static artifacts actually contain the isolated symbols Nimbus needs
for an in-process Bun/JSC backend beside the existing Deno/V8 backend.

This gate is still not the final BJA4L closeout. BJA4L2 remains open until the
Bun source changes are reproducible from a committed source revision or tag.

## Source Build

Host:

```text
nimbus@192.168.4.29
Debian 13 minicloud
```

Source revisions:

```text
Bun:    a409f596e8e1394d8860e2cd8b2bb558ff1afcac
WebKit: 782504c968e2ae06a511c9e7a4d48318b2a23263
```

The Bun worktree contained the same seven-file source namespace patch recorded
in Gate 43. The build used the existing home-backed cache and local WebKit
configuration:

```sh
export PATH=$HOME/.cargo/bin:$HOME/.bun/bin:$PATH

ninja -C $HOME/.cache/nimbus-bun-proof/configure-namespaced \
  -j4 check-bun-embed-probe
```

Result:

```text
[WebKit] [117/118] Linking CXX static library lib/libJavaScriptCore.a
[WebKit] [118/118] Linking CXX executable bin/jsc
[1152/1154] link bun-embed-probe
[1153/1154] bun-embed-probe
gate44_end=2026-05-24T04:41:01-05:00
status=0
```

The native proof output still passed the expected embedder behavior probes:

```text
cancellation policy: owner entry denial, sync-loop acknowledgement, deadline
and external-cancel recovery all passed
permission surface: fs, spawn, serve, listen, connect, plugin, FFI.dlopen,
fetch, WebSocket, timers, worker, eval, Function, and dynamic import denied
package/module policy: program-wrapper lane selected; resolver policy denies
node:fs, package root, virtual plugin module, Bun.resolve, and native addon
lifecycle reuse: fresh create/invoke/destroy, retained invocation before
cancel, external-cancel recovery, microtask progress, and teardown passed
memory behavior: no hard JSC heap limit observed; fresh/discard or outer
quota remains the product-first policy
```

The completed artifacts were:

```text
/home/nimbus/.cache/nimbus-bun-proof/configure-namespaced/bun-embed-probe
/home/nimbus/.cache/nimbus-bun-proof/configure-namespaced/deps/WebKit/lib/libJavaScriptCore.a
/home/nimbus/.cache/nimbus-bun-proof/configure-namespaced/deps/WebKit/lib/libWTF.a
/home/nimbus/.cache/nimbus-bun-proof/configure-namespaced/obj/src/simdutf_sys/bun-simdutf.cpp.o
/home/nimbus/.cache/nimbus-bun-proof/configure-namespaced/rust-target/x86_64-unknown-linux-gnu/release/deps/libbun_embed_probe-4d6fdb7cd13c0296.a
/home/nimbus/.cache/nimbus-bun-proof/configure-namespaced/rust-target/x86_64-unknown-linux-gnu/release/libbun_embed_probe.a
```

Artifact sizes:

```text
libWTF.a                47M
libJavaScriptCore.a    532M
bun-simdutf.cpp.o      59K
bun-embed-probe        285M
```

## Symbol Audit

The key audit command shape was:

```sh
nm -g --defined-only -C <artifact> | grep ...
nm -g --defined-only <artifact> | grep ...
```

Built Bun/WebKit artifacts:

```text
libWTF.a nimbus_bun_simdutf:: definitions: 526
libWTF.a plain simdutf:: definitions:       0

libJavaScriptCore.a nimbus_bun_simdutf:: definitions: 0
libJavaScriptCore.a plain simdutf:: definitions:       0

bun-simdutf.cpp.o nimbus_bun_simdutf__ definitions: 60
bun-simdutf.cpp.o plain simdutf__ definitions:       0

bun-embed-probe exported nimbus_bun_simdutf__ definitions: 0
bun-embed-probe exported plain simdutf__ definitions:       0
```

Representative WebKit/WTF definitions:

```text
nimbus_bun_simdutf::count_utf8(char const*, unsigned long)
nimbus_bun_simdutf::count_utf16(char16_t const*, unsigned long)
nimbus_bun_simdutf::validate_utf8(char const*, unsigned long)
nimbus_bun_simdutf::base64_to_binary(...)
```

Representative Bun C wrapper definitions:

```text
nimbus_bun_simdutf__base64_decode_from_binary
nimbus_bun_simdutf__base64_encode
nimbus_bun_simdutf__convert_utf8_to_utf16le
nimbus_bun_simdutf__convert_utf8_to_utf32
nimbus_bun_simdutf__count_utf8
nimbus_bun_simdutf__detect_encodings
```

Remote Linux V8/rusty_v8 artifacts from the Nimbus worktree still own the
plain V8-side symbols and do not overlap the new Bun namespace:

```text
target/debug/gn_out/obj/librusty_v8.a
  plain simdutf:: definitions:             686
  nimbus_bun_simdutf:: definitions:        0
  plain simdutf__ definitions:             43
  nimbus_bun_simdutf__ definitions:        0

target/debug/deps/libv8-60dc74d54503132f.rlib
  plain simdutf:: definitions:             686
  nimbus_bun_simdutf:: definitions:        0
  plain simdutf__ definitions:             43
  nimbus_bun_simdutf__ definitions:        0

target/debug/deps/libv8-aa795fb60df703c3.rlib
  plain simdutf:: definitions:             686
  nimbus_bun_simdutf:: definitions:        0
  plain simdutf__ definitions:             43
  nimbus_bun_simdutf__ definitions:        0
```

Local macOS V8 artifacts also showed no overlap with the new Bun namespace:

```text
target/debug/gn_out/obj/librusty_v8.a
  plain simdutf:: definitions:             339
  nimbus_bun_simdutf:: definitions:        0
  plain simdutf__ definitions:             0
  nimbus_bun_simdutf__ definitions:        0
```

## Decision

The source-owned static namespace lane is now build-proven and symbol-proven
on Debian 13:

- local WebKit built `libJavaScriptCore.a` and `libWTF.a` from source;
- WebKit/WTF's former colliding C++ definitions are now
  `nimbus_bun_simdutf::`;
- Bun's former colliding C wrappers are now `nimbus_bun_simdutf__*`;
- the existing V8/rusty_v8 artifacts still own plain `simdutf::` and
  `simdutf__`, but do not own the Nimbus Bun namespace;
- the Gate 40 `--allow-multiple-definition` workaround remains rejected.

BJA4L2 should not be marked done yet because the Bun patch is still local to
the proof worktrees. The next step is source ownership: commit/tag the smallest
maintainable Nimbus-owned Bun source revision, then make the linked verification
gate consume that revision and audit these symbol properties automatically.

## Next

- Create or adopt the reproducible Bun source revision/tag for the namespace
  patch.
- Update `scripts/verify-bun-jsc-linked-adapter.sh` or its `make` wrapper to
  run the namespaced source build/audit path and reject unsafe duplicate-symbol
  workarounds.
- Add the same-process V8 plus Bun/JSC link/invocation proof on Debian 13
  `minicloud`.
