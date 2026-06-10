# NFRC2 Latest Node Suite Tags

Date: 2026-05-28
Authoring agent: Codex
Baseline commit: `e7e8b9d6`

## Git Status Summary

NFRC work remains uncommitted. The pre-existing unrelated dirty file is still
present and untouched:

```text
 M docs/plans/dynamodb-adapter-plan.md
```

## Files Changed

- `docs/architecture/runtime/node-lts-compat/node-latest-suite-tags.json`
- `docs/architecture/runtime/node-lts-compat/node-latest-suite-tags.md`
- `tests/runtime/node/schemas/node-latest-suite-tags.schema.json`
- `scripts/runtime/node/latest_suite_tags.py`
- `scripts/verify-node-latest-suite-tags.sh`
- `docs/architecture/runtime/node-lts-compat/node-lts-lanes.md`
- `docs/architecture/runtime/node-compat-surface-matrix.md`
- `tests/runtime/node/README.md`
- `docs/plans/node-faas-runtime-compatibility-plan.md`
- `docs/plans/proof/node-faas-runtime-compatibility/nfrc2-latest-node-suite-tags.md`

## Latest Tags Recorded

| Lane | Latest official tag | Tag object | Commit | Current fixture corpus | Sync required |
| --- | --- | --- | --- | --- | --- |
| `node20` | `v20.20.2` | `35e07843146797923006aa01c6daabf4f53a4fb9` | `3626fea570e44896ad99aaf3bf6e59def5adede5` | `v20.20.2` | no |
| `node22` | `v22.22.3` | `354ef4b9bd94d5b662a9c300ddacc67f95a1bbe8` | `fdfa0ff0dbaf0fbf4d7d6d89a2ab807f3177fa5c` | `v22.15.0` | yes |
| `node24` | `v24.16.0` | `75143a8d75629c5d429dd0becb0d725e955f48fb` | `c7d10158bc31036de6783d66beaaaf551e3167aa` | `v24.15.0` | yes |
| `node26` | `v26.2.0` | `30ffe3cfc2fda3684c38ec43aa79c381d398bf14` | `cfd7920d5a2d84905c4292362d01d07870047e93` | none | yes |

Sources recorded in the registry:

- `https://raw.githubusercontent.com/nodejs/Release/main/schedule.json`
- `https://nodejs.org/dist/index.json`
- `https://github.com/nodejs/node/releases`

Local tag objects and commits were checked against
`/Users/jack/src/github.com/nodejs/node`.

## Decisions

- Added `node-latest-suite-tags.json` beside the lane registry instead of
  editing fixture manifests to claim unsynced corpora are current.
- The regular verifier checks latest official tag metadata, lane-registry
  alignment, current fixture-manifest alignment, intended sync commands, and
  negative self-tests.
- The opt-in enforcement mode fails while Node22, Node24, and Node26 corpora
  are stale or missing. This satisfies the stale-tag guard without making
  NFRC2 pretend NFRC4 has already synced fixture files.
- NFRC4 remains responsible for running the actual fixture sync/apply work and
  writing wide-run issue inventories.

## Verification

```bash
bash scripts/verify-node-latest-suite-tags.sh
```

Result: pass. Output summary:

```text
validated Node latest suite tags: 4 lanes, 3 needing fixture sync
Node latest suite tag negative self-tests passed
```

```bash
python3 scripts/runtime/node/schema.py validate --schema node-latest-suite-tags.schema.json --instance docs/architecture/runtime/node-lts-compat/node-latest-suite-tags.json
```

Result: pass.

```bash
NIMBUS_ENFORCE_CURRENT_NODE_CORPORA=1 bash scripts/verify-node-latest-suite-tags.sh
```

Result: expected fail proving stale corpora are not silently accepted:

```text
error: node22 fixture corpus is not current: v22.15.0 -> v22.22.3
error: node24 fixture corpus is not current: v24.15.0 -> v24.16.0
error: node26 fixture corpus is not current: none -> v26.2.0
```

```bash
bash scripts/verify-node-lts-lanes.sh
```

Result: pass, 4 lanes with product default `node22`.

```bash
npm run docs:validate-refs:strict
```

Result: pass, 225 working-tree Markdown files.

```bash
git diff --check
```

Result: pass.

## Remaining Risks

- NFRC4 must sync Node22 and Node24 fixture corpora to their latest tags and
  add the first Node26 fixture corpus.
- NFRC5 must classify the resulting wide-run fixture inventory to zero
  unclassified targeted fixtures.
- NFRC6 must later move product default from Node22 to Node24.
