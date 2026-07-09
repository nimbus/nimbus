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

## reed-solomon-simd

`nimbus-blob` uses `reed-solomon-simd` 3.1.0 for erasure-coding math. The
crate is licensed as `MIT AND BSD-3-Clause` and bundles the following notices:

```text
Copyright (c) 2023 Anders Trier Olesen

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

```text
Copyright (c) 2022 Markus Laire

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

```text
Copyright (c) 2017 Christopher A. Taylor.  All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

* Redistributions of source code must retain the above copyright notice,
  this list of conditions and the following disclaimer.
* Redistributions in binary form must reproduce the above copyright notice,
  this list of conditions and the following disclaimer in the documentation
  and/or other materials provided with the distribution.
* Neither the name of Leopard-RS nor the names of its contributors may be
  used to endorse or promote products derived from this software without
  specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED.  IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE
LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
POSSIBILITY OF SUCH DAMAGE.
```
