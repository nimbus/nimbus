# MBA1 Technical Debt Seed

Date: 2026-05-27

source_scope: Nimbus-owned source, current active docs, scripts, packages, and
demos inspected from the repo root.

excluded_paths: generated protobuf and SDK outputs, Node compatibility fixture
corpora, vendored or upstream copies, archived plans, private/generated HTML,
snapshot files, dependency directories, and false-positive `XXXXXX` temp-file
patterns.

ledger_entries: 31

## Seed Command

The owned TODO scan used word boundaries so shell temp patterns such as
`mktemp ... XXXXXX` did not count as `XXX` debt.

```bash
rg -n -S "\b(TODO|FIXME|XXX|HACK)\b" crates packages docs scripts demos \
  --glob '!**/_generated/**' \
  --glob '!**/node_modules/**' \
  --glob '!**/target/**' \
  --glob '!**/vendor/**' \
  --glob '!**/fixtures/**' \
  --glob '!**/testdata/**' \
  --glob '!**/*.snap' \
  --glob '!packages/firebase/src/gen/**' \
  --glob '!crates/nimbus-server/proto/**' \
  --glob '!crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures/**' \
  --glob '!docs/private/**' \
  --glob '!docs/plans/archive/**'
```

## Actionable Hits

The scoped command produced three owned hits:

| Source | Ledger ID | Rationale |
|--------|-----------|-----------|
| `crates/nimbus-runtime/src/runtime/bootstrap/js/perf_hooks.js:7` | C-001 | Runtime polyfill hygiene is owned source, not an upstream fixture. |
| `crates/nimbus-runtime/src/runtime/bootstrap/js/perf_hooks.js:508` | F-005 | Stubbed perf hook values affect compatibility claims. |
| `docs/design-review/2026-05-18-operator-console-review.md:192` | C-002 | Current design review calls out user-facing copy that still reads like placeholder text. |

## MBA0-Derived Entries

Most seed entries come from the MBA0 baseline, rigor review, and current
verifier failures because those are higher-signal than generated TODO comments:

- MBA2 storage trait segregation gaps: A-001, A-006.
- MBA4 backend-coupled worker hook gap: A-002, C-003, O-002.
- MBA5 dual-target adapter coverage gaps: F-001 through F-004 and T-001
  through T-004.
- MBA6 auth caching policy gaps: S-001, S-005.
- MBA7 SQL safety ADR gaps: S-002, S-003.
- MBA8 latency budget and metrics gaps: P-001, P-002, O-001.
- MBA9 object-erased trait audit gap: T-006.
- MBA10 stable table identity gap: A-003, S-004.
- MBA11 typed-key ordering gap: T-005.
- MBA12 consistency routing gap: A-004, P-003.
- MBA13 event-capture contract gap: A-005.
- MBA14 closeout evidence gap: O-003.

## Exclusion Rationale

Generated protobuf and Firebase SDK comments preserve upstream source context,
not Nimbus-owned debt. Node compatibility fixture TODOs preserve upstream Node
test behavior and should remain in fixture context. Archived plan TODOs are
historical execution records. Private/generated HTML and binary-like embedded
assets are not source-maintained backlog. Shell `XXXXXX` temp-file templates are
not `XXX` comments.
