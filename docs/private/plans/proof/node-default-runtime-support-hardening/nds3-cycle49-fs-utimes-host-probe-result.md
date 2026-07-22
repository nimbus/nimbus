# NDS3 cycle 49 - fs utimes host probe reclassification

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

`test/parallel/test-fs-utimes-y2K38.js` was reclassified out of
`v8_isolate_required` for both node22 and node24 as
`diagnostic_only_non_isolate` / `exact_host_process_control_surface`.

Gate movement:

- node22: 53 -> 52 gaps, 97.8% pass rate
- node24: 61 -> 60 gaps, 97.51% pass rate

No fork changes were made.

## Source Evidence

The node22 and node24 fixture bodies are identical. Before the `fs.utimesSync()`
precision assertion, the fixture performs a host-platform capability probe:

- creates a file under the Node harness temp directory;
- imports `child_process.spawnSync`;
- runs host `touch -t 204001020304 <file>` to set a Y2K38 timestamp;
- runs host `date -r <file> +%Y%m%d%H%M` to validate the host filesystem's
  timestamp behavior;
- conditionally calls `common.skip()` based on those host subprocess results.

Ambient host subprocess execution and host filesystem capability probing are not
portable Application API behavior inside the default multi-tenant V8 isolate.
The isolate must fail closed rather than running host `touch`/`date`, so the
fixture is structurally outside the default required surface.

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
node22 52 97.8
node24 60 97.51
```
