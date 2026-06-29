# NKV0 F4 Conformance Harness Proof

Date: 2026-06-27
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/nkv-cloudflare-foundation`
Branch: `codex/nkv-cloudflare-foundation`
Commit: `0fb43652d`

## Delivered Surface

- Added `crates/nimbus-kv/tests/spawn_harness.rs`, an ignored integration test
  that uses `REDISRS_SERVER_BIN` / `NIMBUS_KV_SERVER_BIN`, spawns the real
  `nimbus kv` binary, waits for readiness through redis-rs `PING`, and drives
  `GET`/`SET`/`DEL`/`EXPIRE`/`TTL`/`INCR` through RESP2 plus a raw RESP3
  `HELLO 3 AUTH ...` smoke path.
- Added `scripts/nimbus-kv-conformance.sh`, which pins Valkey to
  `9.1.0` / `c9e8005e9d0ec817e26c7db318861cb821409249`, spawns authenticated
  `nimbus kv`, patches only the temporary Valkey TCL client helper to AUTH each
  external-mode client, and runs the `unit/type/nimbus_kv_smoke` slice with
  `runtest --host --port --single ... --skipfile ...` under RESP2 and RESP3.
- Added `tests/nimbus-kv-skip.txt` with explicit `encoding` and `behavioral`
  sections. The NKV0 behavioral skip budget is 0.
- Added `.github/workflows/nimbus-kv-conformance.yml`, which installs `tcl`,
  builds the Nimbus binary, runs the redis-rs spawned-binary harness, and runs
  the Valkey external-mode script.
- Added the harmless Valkey harness setup verbs that external mode needs:
  `FLUSHALL` clears the tenant KV store through the same scan + batch delete
  trait path, including an expired-key sweep first; `FUNCTION FLUSH` acknowledges
  the empty NKV0 function registry.

## Verification

Command:

```text
cargo fmt --all --check
```

Output: passed with no diff.

Command:

```text
cargo test -p nimbus-kv
```

Output:

```text
cache_tiering: 5 passed; 0 failed; 0 ignored; finished in 2.96s
resp_server: 6 passed; 0 failed; 0 ignored; finished in 0.36s
spawn_harness: 0 passed; 0 failed; 1 ignored
doc-tests: 0 passed; 0 failed
```

Command:

```text
cargo test -p nimbus-storage kv::tests
```

Output:

```text
running 6 tests
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 304 filtered out; finished in 1.15s
```

Command:

```text
cargo build -p nimbus-bin --bin nimbus
```

Output:

```text
Finished `dev` profile [unoptimized + debuginfo] target(s) in 36.86s
```

Command:

```text
REDISRS_SERVER_BIN=target/debug/nimbus cargo test -p nimbus-kv --test spawn_harness -- --ignored --nocapture
```

Output:

```text
running 1 test
test redis_rs_spawned_nimbus_kv_binary_smoke_resp2_and_resp3 ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.90s
```

Command:

```text
REDISRS_SERVER_BIN=/Users/jack/src/github.com/nimbus/nimbus-worktrees/nkv-cloudflare-foundation/target/debug/nimbus bash scripts/nimbus-kv-conformance.sh
```

Output:

```text
Pinned Valkey checkout: c9e8005e9d0ec817e26c7db318861cb821409249

RESP2:
Test Summary: 1 passed, 0 failed
RESP2 passing behavioral assertions: 1 (minimum 1)

RESP3:
Test Summary: 1 passed, 0 failed
RESP3 passing behavioral assertions: 1 (minimum 1)

Nimbus KV Valkey conformance smoke passed: RESP2+RESP3, Valkey c9e8005e9d0ec817e26c7db318861cb821409249, slice unit/type/nimbus_kv_smoke
```

Command:

```text
git diff --check
```

Output: passed with no whitespace errors.

Command:

```text
bash scripts/verify-nimbus-kv-foundation.sh
```

Output:

```text
[7] F4: conformance harness and skip accounting
  PASS  redis-rs spawn harness + Valkey runner + two-section skipfile + minimum pass assertion

8 passed, 1 failed
```

The remaining verifier failure is expected F5 closeout (`doc=0 ledger=0 ci=0`).
