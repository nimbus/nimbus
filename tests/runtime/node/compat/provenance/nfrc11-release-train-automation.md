# NFRC11 Release-Train Automation

Initial date: 2026-05-28
Last refreshed: 2026-09-22
Authoring agent: Codex
Initial repository baseline: `e7e8b9d6`
Refresh repository baseline: `cd3cef4fd`
Relevant Node lanes: Node20 `v20.20.2`, Node22 `v22.23.2`, Node24 `v24.21.0`, Node26 `v26.10.0`

## Git Status Summary

The worktree contains the active NFRC0-NFRC11 implementation wave. The
NFRC11-specific changes add release-train automation that validates checked-in
Node lane metadata, latest official tags, dashboard role separation, and a
proof digest gate. It also adds an optional live probe against official Node
release feeds. Future scheduled automation can detect new patch/minor tags or
lifecycle changes before public docs move.

## Source Digest Gate

Future changes to release metadata must update this proof because the verifier
requires these digest markers:

- tests/runtime/node/compat/node-lts-compat/node-lts-lanes.json sha256: 74d6e560865153183b2b46ffed8396ed4722e75531c12b6bdfd2e190194610dd
- tests/runtime/node/compat/node-lts-compat/node-latest-suite-tags.json sha256: 243a9686bfc38a26e673286f9f7cad4165171de87645148a24470165783675ee

## Files Changed

- Release-train automation and verifier:
  `scripts/runtime/node/release_train.py`,
  `scripts/verify-node-release-train.sh`,
  `Makefile`
- Release-train schema and generated summary:
  `tests/runtime/node/schemas/node-release-train.schema.json`,
  `tests/runtime/node/compat/node-lts-compat/node-release-train.json`,
  `tests/runtime/node/compat/node-lts-compat/node-release-train.md`
- Release metadata docs:
  `tests/runtime/node/compat/node-lts-compat/node-lts-lanes.md`,
  `tests/runtime/node/compat/node-lts-compat/node-latest-suite-tags.md`
- Control plane:
  `docs/private/plans/node-faas-runtime-compatibility-plan.md`,
  `docs/private/plans/proof/node-faas-runtime-compatibility/README.md`,
  this proof file

## Strategy

NFRC11 followed the wide-then-focused loop for release drift:

1. Add a broad release-train analyzer over lane registry, latest-suite tags,
   generated status/dashboard evidence, and optional official live feeds.
2. Run offline self-tests and a live probe to capture tag, lifecycle, role, and
   proof-gate feedback.
3. Fix the specific lifecycle mismatch exposed by live official schedule data.
4. Rerun the live probe and offline verifier, then publish a checked-in
   release-train summary.

## Release-Train Contract

`scripts/runtime/node/release_train.py` validates:

- Node24 is the product default.
- Nimbus supports Node22 as Maintenance LTS.
- Node20 is legacy-grace/EOL regression coverage.
- Node26 is Current/non-LTS, not product default and not supported LTS.
- Latest official tags match the lane registry and fixture corpus tags.
- In the live probe, the dist index `openssl` field of each lane's release
  matches the registry `openssl_version`, which the runtime reports as
  `process.versions.openssl`.
- Dashboard lane roles match registry roles: `legacy`, `supported`, `default`,
  and `current`.
- Generated release-train docs match current inputs.
- This proof file exists and contains the current lane/latest-tag source digest
  markers. The proof README lists this file.

The optional live probe reads:

- `https://nodejs.org/dist/index.json`
- `https://raw.githubusercontent.com/nodejs/Release/main/schedule.json`

Those are the official machine-readable feeds used to detect new tags and
release lifecycle changes. Web research on 2026-05-28 also confirmed Node.js
`v26.2.0` is a Current release and the Node Release Working Group schedule is
the canonical lifecycle source.

## 2026-09-01 Release-Readiness Refresh

The release-readiness replay detected newer official patch releases after the
offline checks passed. The live probe found `v22.23.2`, `v24.20.0`, and
`v26.8.1`, while the checked-in corpora still used `v22.22.3`, `v24.16.0`, and
`v26.2.0`. The refresh resolved each annotated tag to its tag object and peeled
commit. It synchronized the official fixture subtree. It also regenerated the
identity and classification catalogs and republished the evidence.

The refreshed evidence contains `20,621` official vendored test files, `7,768`
documented manifested green files, `150` explicit Rust watchpoints, `37` active
canaries, `79` canary claims, and `101` current canary checks. Every official
file is either in the measured green subset or has an explicit expected-failure,
known-gap, or skipped classification. The refresh does not convert known gaps
into pass claims.

A representative live replay covered core, process, stream, network, and
loader slices. It retained observed incompatibilities as failures in the raw
reports. The release-train gate verifies metadata and evidence integrity. It
does not claim complete Node compatibility.

## 2026-09-22 Release-Train Refresh

The scheduled live probe reported tag drift for two lanes. The official
`dist/index.json` listed `v24.21.0` and `v26.10.0`, while the checked-in
registry still recorded `v24.20.0` and `v26.8.1`. Node20 `v20.20.2` and Node22
`v22.23.2` had no drift. The refresh resolved each annotated tag to its tag
object and peeled commit from a local `nodejs/node` checkout after
`git fetch --tags`. It recorded the official fixture identities, synchronized
the official fixture subtree for both lanes, and regenerated the classification
catalogs with existing classifications preserved. New fixtures received the
default non-green classification. The refresh does not convert known gaps into
pass claims.

The refreshed evidence contains `20940` official vendored test files,
`7794` documented manifested green files, `129` explicit Rust
watchpoints, `79` canary claims, and `101` current canary
checks. Every official file is either in the measured green subset or has an
explicit expected-failure, known-gap, or skipped classification.

Oracle runs used official Node binaries that match each lane tag exactly:
`v20.20.2`, `v22.23.2`, `v24.21.0`, and `v26.10.0`. The representative live
replay covered the core, process, stream, network, and loader slices.

## Wide Feedback And Focused Fixes

Initial live probe:

```bash
python3 scripts/runtime/node/release_train.py probe-live
```

The first sandboxed run failed on DNS, as expected under restricted network
execution. The escalated live run then reached the official feeds and exposed a
real lifecycle mismatch:

| Surface | Initial feedback | Resolution |
| --- | --- | --- |
| Node26 maintenance date | A search-result snippet suggested `2027-10-27`, but the official schedule JSON returned `2027-10-20`. | Keep the registry aligned with the official schedule JSON and record the live-probe result as the source of truth. |

Final live probe:

```bash
python3 scripts/runtime/node/release_train.py probe-live
```

Result: pass, `4` lanes matched official release feeds.

## Generated Summary

The repository stores the generated summary at
`tests/runtime/node/compat/node-lts-compat/node-release-train.md` and
`node-release-train.json`.

The initial 2026-05-28 publication reported:

- Node20: `legacy_grace`, `eol_legacy`, dashboard role `legacy`.
- Node22: `supported_lts`, `maintenance_lts`, dashboard role `supported`.
- Node24: `product_default`, `active_lts`, dashboard role `default`.
- Node26: `current_non_lts`, `current_non_lts`, dashboard role `current`.
- Canary claims: `37`.
- Canary checks: `101`.
- Required canary gaps: `0`.
- Release-train drift: none.

The 2026-09-01 refresh reports the same lane roles with Node22 `v22.23.2`,
Node24 `v24.20.0`, and Node26 `v26.8.1`. It reports `79` canary claims, no
required canary gaps, and no release-train drift. Two fresh canary reports
contain `101` checks for the current candidate.

The 2026-09-22 refresh reports the same lane roles with Node24 `v24.21.0` and
Node26 `v26.10.0`. It reports `79` canary claims, `101` canary
checks, no required canary gaps, and no release-train drift.

## Verification

Results from the 2026-09-22 refresh:

- `python3 scripts/runtime/node/release_train.py publish --check-proof`: pass.
  The summary has a proof file, a proof README entry, and all digest markers.
- `bash scripts/verify-node-release-train.sh`: pass. Four lanes and zero drift
  entries. The negative self-tests passed.
- `make node-compat-release-train CHECK=1`: pass. The generated release-train
  summary is current.
- `python3 scripts/runtime/node/release_train.py self-test`: pass. The negative
  tests detected tag, lifecycle, dashboard-role, product-default, and missing
  canary-execution drift.
- `python3 scripts/runtime/node/release_train.py probe-live`: pass with
  network approval, 4 lanes matched official release feeds. The first run
  reported OpenSSL drift for Node24 and Node26, because `v24.21.0` and
  `v26.10.0` ship OpenSSL `3.5.8`. The registry now records `3.5.8` for both
  lanes.
- `make node-compat-refresh LANE=node24 TAG=v24.21.0 APPLY=1 FORCE=1` and
  `make node-compat-refresh LANE=node26 TAG=v26.10.0 APPLY=1 FORCE=1`: pass
  for every pipeline step.
- `bash scripts/runtime/node/canaries-run.sh --preset application`: pass,
  91 canaries passed and 0 failed, with current application and host-heavy
  evidence for Node20, Node22, Node24, and Node26.
- `bash scripts/runtime/node/canaries-run.sh --preset tooling`: pass,
  10 canaries passed and 0 failed for Node22 and Node24.
- Representative slices with `--capture-live`: pass for the core, process,
  stream, network, and loader slices.
- `bash scripts/runtime/node/oracle-run.sh --lane <lane>` with
  `test/parallel/test-buffer-alloc.js`: pass for all four lanes against the
  official binaries `v20.20.2`, `v22.23.2`, `v24.21.0`, and `v26.10.0`.
- `python3 scripts/runtime/node/fixture_provenance.py validate`: pass, 4
  vendored corpora and 4 strict identity manifests.
- `bash scripts/runtime/node/validate-claims.sh`: pass, 79 active claim
  mappings across 16 categories against 37 registered canaries.
- `bash scripts/verify-node-latest-suite-tags.sh`: pass, 4 lanes, 0 needing
  fixture sync, negative self-tests passed.
- `bash scripts/verify-node-lts-lanes.sh`: pass, 4 lanes, product default
  `node24`.
- `make node-compat-baseline-verify`: pass, 1328 recorded gaps across 5 lanes.
- `make node-compat-classifications CHECK=1`: pass, all four catalogs current.
- `git diff --check`: pass.

## Decisions

- Keep live official feed reads out of the default offline verifier. NFRC12
  owns scheduled/nightly placement for live checks. NFRC11 provides and proves
  the live probe.
- Require source digest markers in this proof. A future lane/tag metadata edit
  cannot pass release-train verification without updating the proof.
- Reject active canary claims when the current dashboard has no canary checks.
  Also reject any required canary gap.
- Treat search snippets as advisory only. The official schedule JSON is the
  source of truth when snippets disagree.

## Remaining Risks

- NFRC12 still owns wiring this verifier into PR and scheduled CI lanes.
- NFRC13 still owns the final all-row verifier and closeout pass.
