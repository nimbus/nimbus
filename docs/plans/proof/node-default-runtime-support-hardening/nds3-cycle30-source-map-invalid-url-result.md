# NDS3 Cycle 30: source-map invalid URL tolerance

Date: 2026-06-13

## Scope

This checkpoint promotes `test/parallel/test-source-map-invalid-url.js` for both
required lanes.

The fixture checks that eval and dynamic `data:` module imports tolerate invalid
`sourceMappingURL` / `sourceURL` comments. The failure was not a host-capability
boundary. Nimbus rejected the dynamic import before source-map handling because
the fixture encodes the module body with `Buffer.toString("base64url")` and then
uses a `data:text/javascript;base64,...` URL. Node accepts that unpadded
URL-safe base64 form.

## Fix Summary

Nimbus-local module loading now decodes `;base64` data-module payloads with
Node-compatible tolerance for:

- URL-safe base64 characters (`-` and `_`).
- Missing trailing padding.
- ASCII whitespace inside the encoded payload.

This does not add host filesystem, network, subprocess, signal, cwd, or native
capability. It only broadens data URL payload decoding to match Node's tolerated
encoding shape.

## Proof Commands

Initial census:

```text
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-source-map-invalid-url.js" NIMBUS_RECENSUS_LANE=node24 NIMBUS_RECENSUS_EXTRA_DIRS="test/common" cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

Result:

```text
node_compat nds-probe node24 summary: selected=1, passed=0, skipped=0, failed=1
runtime JavaScript error: invalid base64 data module specifier data:text/javascript;base64,...: Invalid padding
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 812 filtered out; finished in 2.02s
```

```text
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-source-map-invalid-url.js" NIMBUS_RECENSUS_LANE=node22 NIMBUS_RECENSUS_EXTRA_DIRS="test/common" cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

Result:

```text
node_compat nds-probe node22 summary: selected=1, passed=0, skipped=0, failed=1
runtime JavaScript error: invalid base64 data module specifier data:text/javascript;base64,...: Invalid padding
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 812 filtered out; finished in 1.92s
```

Focused unit proof:

```text
gtimeout -s KILL 90 cargo test -p nimbus-runtime --lib data_url_module_source_decodes_unpadded_base64url_javascript -- --nocapture
```

Result:

```text
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 813 filtered out; finished in 0.00s
```

Post-fix dynamic census:

```text
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-source-map-invalid-url.js" NIMBUS_RECENSUS_LANE=node24 NIMBUS_RECENSUS_EXTRA_DIRS="test/common" cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

Result:

```text
node_compat nds-probe node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 813 filtered out; finished in 2.01s
```

```text
gtimeout -s KILL 90 env NIMBUS_RECENSUS_FIXTURE="test/parallel/test-source-map-invalid-url.js" NIMBUS_RECENSUS_LANE=node22 NIMBUS_RECENSUS_EXTRA_DIRS="test/common" cargo test -p nimbus-runtime --lib nds_probe -- --ignored --nocapture
```

Result:

```text
node_compat nds-probe node22 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 813 filtered out; finished in 1.93s
```

Static promotion proof:

```text
gtimeout -s KILL 90 cargo test -p nimbus-runtime --lib cycle30_source_map_invalid_url -- --nocapture
```

Result:

```text
node_compat node22-supported-lane-executes-cycle30-source-map-invalid-url-batch node22 summary: selected=1, passed=1, skipped=0, failed=0
node_compat node24-default-lane-executes-cycle30-source-map-invalid-url-batch node24 summary: selected=1, passed=1, skipped=0, failed=0
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 813 filtered out; finished in 3.88s
```

Generated evidence refresh:

```text
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for script in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do /opt/homebrew/bin/python3.12 "scripts/runtime/node/${script}.py" >/dev/null || exit $?; done
```

Result:

- `tests/runtime/node/classifications/node22.json`: removed
  `test/parallel/test-source-map-invalid-url.js` from required gaps.
- `tests/runtime/node/classifications/node24.json`: removed
  `test/parallel/test-source-map-invalid-url.js` from required gaps.
- `docs/private/architecture/runtime/node-default-support-posture.json`:
  `node22.v8_isolate_required` = 69 gaps, 97.09% pass rate.
- `docs/private/architecture/runtime/node-default-support-posture.json`:
  `node24.v8_isolate_required` = 77 gaps, 96.81% pass rate.

## Guardrails

- No official Node fixture or checker edits.
- No false-green hand edits to generated JSON.
- No V8 or rusty_v8 native binding changes.
- No local Deno path pin was used or left behind.
- Scratch probe include and file were removed before promotion.
- PR #10 remains draft and unmerged.
