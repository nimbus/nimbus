# NLRT5 Equal Lane Evidence Docs

Date: 2026-05-28

Agent: Codex

## Git Status Summary

NLRT5 changes are present in the working tree but not yet committed at proof
write time. The unrelated pre-existing dirty file remains
`docs/plans/dynamodb-adapter-plan.md` and is intentionally excluded from this
slice.

## Decisions

- Split node-compat lane role vocabulary into `default`, `supported`, and
  `legacy`, with matching public contract roles. Node20 now renders as
  `legacy` / `legacy_contract` in manifest-derived evidence because the LTS
  registry marks it `eol_legacy`.
- Regenerated public Node evidence pages from `publish_docs.py` so support
  roles are overlaid from the lane registry:
  - Node20: legacy grace; EOL
  - Node22: product default; Maintenance LTS
  - Node24: supported; Active LTS
- Added `scripts/verify-node-lts-docs.sh` and
  `scripts/runtime/node/docs_guard.py`. The guard verifies generated public
  evidence is current and rejects hand-written Node docs that reintroduce stale
  pass-rate percentages, Node22 pass-count prose, or Node20 active-LTS support
  claims.
- Rewrote the old Deno/Nimbus comparison document into a qualitative
  architecture note. Current numeric support claims now belong only to
  generated evidence and the lane registry.
- Kept Node22-named internal bootstrap roots in place. Those filenames are
  currently tied to the Deno `ext/node` bootstrap integration and renaming them
  would add churn without improving lane semantics. Lane-specific runtime
  metadata now comes from the registry and bootstrap contract, so the name no
  longer implies evidence priority.

## Changed Files

- `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/lanes/node20.json`
- `crates/nimbus-runtime/src/runtime/tests/node_compat_manifests/schema.json`
- `crates/nimbus-runtime/src/runtime/tests/node/manifest_catalog.rs`
- `crates/nimbus-runtime/src/runtime/tests/node/manifest_metadata.rs`
- `crates/nimbus-runtime/src/runtime/tests/node/manifest_report.rs`
- `crates/nimbus-runtime/src/runtime/tests/node/manifest_report_tests.rs`
- `crates/nimbus-runtime/src/runtime/tests/node/oracle.rs`
- `docs/architecture/runtime/deno-vs-nimbus-node-compat.md`
- `docs/architecture/runtime/node-compat-surface-matrix.md`
- `docs/architecture/runtime/permission-model.md`
- `docs/runtimes/nodejs/README.md`
- `docs/runtimes/nodejs/compatibility.md`
- `docs/runtimes/nodejs/configuration.md`
- `docs/runtimes/nodejs/evidence/latest.md`
- `docs/runtimes/nodejs/evidence/node20.md`
- `docs/runtimes/nodejs/evidence/node22.md`
- `docs/runtimes/nodejs/evidence/node24.md`
- `scripts/runtime/node/publish_docs.py`
- `scripts/runtime/node/docs_guard.py`
- `scripts/verify-node-lts-docs.sh`

## Verification

- `bash scripts/verify-node-lts-docs.sh`: pass; generated public evidence docs
  are current and the hand-written docs guard passed.
- `cargo test -p nimbus-runtime manifest_metadata -- --nocapture`: 3 passed,
  0 failed, 0 ignored.
- `cargo test -p nimbus-runtime manifest_report -- --nocapture`: 11 passed,
  0 failed, 1 ignored manual artifact entrypoint.
- `bash scripts/verify-node-lts-lanes.sh`: pass; validated 4 lanes and product
  default `node22`.
- `cargo fmt --all --check`: pass.
- `npm run docs:validate-refs:strict`: pass, 219 working-tree Markdown files.
- `git diff --check`: pass.

## Acceptance Evidence

- Public Node docs no longer call Node20 an active supported LTS lane.
- Public docs explicitly say product default is a routing default, not an
  evidence priority.
- Generated public evidence remains the place where pass-rate numbers appear.
  Hand-written Node support docs are guarded against reintroducing numeric
  pass-rate claims.
- Node22 and Node24 are described as the current supported LTS lanes. Node22 is
  still the product default, but the docs now present Node24 as a peer
  supported LTS lane rather than a secondary afterthought.
- Node22-shaped internal bootstrap filenames are intentionally retained and
  justified here because the semantics are no longer encoded by those names.

## Follow-On

Architecture-internal dashboard snapshots under
`docs/architecture/runtime/node-compat-evidence/latest/` still reflect the last
full evidence publication. NLRT11 owns the final full verifier and evidence
closeout; NLRT5 makes the public docs and generator registry-aware so future
publication does not reintroduce stale lane roles.
