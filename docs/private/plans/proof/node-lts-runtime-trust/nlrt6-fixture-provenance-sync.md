# NLRT6 Fixture Provenance Sync

Date: 2026-05-28

Agent: Codex

## Git Status Summary

NLRT6 changes are present in the working tree but not yet committed at proof
write time. The unrelated pre-existing dirty file remains
`docs/plans/dynamodb-adapter-plan.md` and is intentionally excluded from this
slice.

## Decisions

- Added explicit upstream commit identity to every vendored Node fixture lane.
  The human-readable tag remains, but the manifest now also records the commit
  reached by that tag and the annotated tag object SHA.
- Added a separate `fixture_provenance` block to lane manifests. It records the
  sync date, canonical selection command, Nimbus sync baseline commit, and the
  local command source used to record the upstream identity.
- Added `scripts/runtime/node/fixture_provenance.py` plus
  `scripts/verify-node-fixture-provenance.sh`. The verifier checks all vendored
  fixture corpora for provenance, cross-checks fixture tags and paths against
  the Node LTS lane registry, and rejects supported LTS lanes with unclassified
  published results.
- Wired the provenance verifier into `scripts/runtime/node/refresh.py` before
  sync and after evidence publication. This makes refresh fail before operating
  on unknown provenance and fail after publication if a supported LTS lane
  publishes unclassified results.
- Tightened `scripts/runtime/node/sync.py` so dry-run reports include commit,
  tag object, and fixture provenance. Upstream tag overrides now fail unless
  the lane metadata already records matching provenance.

## Upstream Tag Evidence

Resolved from local Node source at `/Users/jack/src/github.com/nodejs/node`:

| Lane | Tag | Commit | Tag object |
| --- | --- | --- | --- |
| `node20` | `v20.20.2` | `3626fea570e44896ad99aaf3bf6e59def5adede5` | `35e07843146797923006aa01c6daabf4f53a4fb9` |
| `node22` | `v22.15.0` | `b009466555c360513b8012ce549f716501090ee5` | `fba004eabc89c2b92d21e56d8ba24c23a952119f` |
| `node24` | `v24.15.0` | `848430679556aed0bd073f2bc263331ad84fa119` | `a20a24415694b80361d661d6ecc1ea0e260d9c32` |

The fixture baseline sync commit recorded in the manifests is
`17a6bf48e3d69a5c153ffc89300629cc798346a5`, dated
`2026-05-11T19:29:29-05:00`.

## Changed Files

- `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/lanes/node20.json`
- `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/lanes/node22.json`
- `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/lanes/node24.json`
- `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/schema.json`
- `crates/nimbus-runtime/src/runtime/tests/node/manifest_catalog.rs`
- `crates/nimbus-runtime/src/runtime/tests/node/manifest_metadata.rs`
- `scripts/runtime/node/fixture_provenance.py`
- `scripts/runtime/node/refresh.py`
- `scripts/runtime/node/sync.py`
- `scripts/verify-node-fixture-provenance.sh`
- `tests/runtime/node/schemas/fixture-sync-report.schema.json`

## Verification

- `python3 scripts/runtime/node/fixture_provenance.py validate`: pass; validated
  3 vendored corpora and 2 supported LTS lanes with zero unclassified published
  results.
- `bash scripts/verify-node-fixture-provenance.sh`: pass; same verifier result.
- `python3 scripts/runtime/node/sync.py --lane node22 --dry-run --output-root target/node-compat/nlrt6-sync-dry-run`:
  pass; wrote `node22-sync.json` and `node22-sync.md`; reported 1283 local
  test files and did not fetch upstream.
- `python3 scripts/runtime/node/publish_docs.py --check`: pass; generated
  public evidence docs are current.
- `python3 scripts/runtime/node/sync.py --lane node22 --upstream-tag v22.16.0 --dry-run --output-root target/node-compat/nlrt6-negative-override`:
  expected failure; sync rejected an unproven tag override and required
  upstream tag, commit, tag object, and fixture provenance to be recorded first.
- Synthetic missing-provenance probe against `node22`: pass; the validator
  emitted missing `fixture_provenance` field errors and the probe asserted that
  the missing-provenance condition is caught.
- Synthetic unclassified-status probe with `node22` set to 1 unclassified
  fixture: expected failure; verifier reported
  `node22 has 1 unclassified published fixtures`.
- `cargo test -p nimbus-runtime manifest_metadata -- --nocapture`: 3 passed,
  0 failed, 0 ignored.
- `bash scripts/verify-node-lts-lanes.sh`: pass; validated 4 lanes and product
  default `node22`.
- `bash scripts/verify-node-lts-docs.sh`: pass; public evidence docs current
  and stale prose guard passed.
- `cargo fmt --all --check`: pass.
- `npm run docs:validate-refs:strict`: pass, 219 working-tree Markdown files.
- `git diff --check`: pass.

## Acceptance Evidence

- Every vendored Node fixture corpus records upstream tag, commit, tag object,
  sync date, and selection command.
- Refresh now calls the provenance verifier before sync and after evidence
  publication, so missing provenance blocks and unclassified supported-LTS
  published results fail the coordinated refresh path.
- The dry-run proof wrote a provenance-bearing Node22 sync report without
  mutating fixtures.
- The checked generated-output comparison proved public Node evidence docs
  still match the checked-in evidence snapshots.

## Follow-On

NLRT7 owns harness timeout and hang diagnostics. NLRT6 deliberately does not
reclassify watchpoints or alter fixture expectations; it makes fixture identity
and published-support classification enforceable before those next hardening
steps.
