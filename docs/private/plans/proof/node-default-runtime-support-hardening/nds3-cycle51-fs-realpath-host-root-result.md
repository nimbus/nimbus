# NDS3 cycle 51 - fs realpath host-root reclassification

Date: 2026-06-13
Worktree: `/Users/jack/src/github.com/nimbus/nimbus-worktrees/node-default-runtime-support-hardening`
Branch / PR: `codex/node-default-runtime-support-hardening` / PR #10

## Result

`test/parallel/test-fs-realpath.js` was reclassified out of
`v8_isolate_required` for both node22 and node24 as
`diagnostic_only_non_isolate` / `absolute_host_path_policy_boundary`.

Gate movement:

- node22: 51 -> 50 gaps, 97.89% pass rate
- node24: 59 -> 58 gaps, 97.59% pass rate

No fork changes were made.

## Source Evidence

The node22 and node24 fixture bodies are identical for the relevant assertions.
The official fixture:

- sets `root = "/"` on non-Windows hosts;
- asserts `realpathSync("/")` and async `realpath("/")` both resolve to that
  host root;
- computes `upone = path.join(process.cwd(), "..")` and asserts `realpath("..")`
  and `realpathSync("..")` against that host-cwd parent;
- creates absolute symlink graphs under the harness temp directory and asserts
  realpath resolution through those absolute `/tmp/...` targets.

Those assertions are host-root filesystem topology. The default multi-tenant V8
isolate must fail closed for unbounded absolute host filesystem probes and
host-cwd parent traversal; greening this fixture by exposing or remapping host
root realpath behavior would weaken the sandbox boundary. The honest disposition
is therefore the existing absolute host path policy boundary.

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
```

Generated counts and disposition:

```text
node22 50 97.89
  diagnostic_only_non_isolate absolute_host_path_policy_boundary
node24 58 97.59
  diagnostic_only_non_isolate absolute_host_path_policy_boundary
```
