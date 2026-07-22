# NDS3 cycle 50 - fs read stream host FIFO reclassification

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

`test/parallel/test-fs-read-stream.js` was reclassified out of
`v8_isolate_required` for both node22 and node24 as
`diagnostic_only_non_isolate` / `exact_host_process_control_surface`.

Gate movement:

- node22: 52 -> 51 gaps, 97.85% pass rate
- node24: 60 -> 59 gaps, 97.55% pass rate

No fork changes were made.

## Source Evidence

The node22 and node24 fixture bodies are identical in the relevant section. The
non-Windows non-seekable file descriptor subtest:

- refreshes the Node harness temp directory;
- creates a host FIFO path under that directory;
- runs `child_process.spawnSync('mkfifo', [filename])`;
- if that host command is available, runs `child_process.exec(...)` to write
  into the FIFO;
- creates an `fs.createReadStream()` over that FIFO and asserts the read range.

That subtest is mandatory on normal Unix hosts where `mkfifo` exists. It
requires ambient host subprocess execution plus host FIFO creation and shell-fed
I/O. The default multi-tenant V8 isolate must fail closed for host subprocesses
and special host filesystem devices, so the fixture is structurally outside the
default required Application surface.

## Verification

Regenerated lightweight posture/evidence pipeline:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/classifications.py sync --lane all
for s in status dashboard trends publish_evidence default_support_posture required_surface_blockers; do
  /opt/homebrew/bin/python3.12 scripts/runtime/node/$s.py >/dev/null
done
```

Checks:

```bash
/opt/homebrew/bin/python3.12 scripts/runtime/node/default_support_posture.py --check
# node default support posture: pass

/opt/homebrew/bin/python3.12 scripts/runtime/node/required_surface_blockers.py --check
# node required-surface blocker inventory: pass

/opt/homebrew/bin/python3.12 scripts/runtime/node/watchpoints.py validate
# validated node-compat watchpoint catalog: 134 entries
```

Generated counts:

```text
node22 51 97.85
node24 59 97.55
```
