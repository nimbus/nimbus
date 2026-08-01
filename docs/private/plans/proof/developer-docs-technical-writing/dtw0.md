# DTW0 baseline proof

Date: 2026-08-01
Baseline: main @ `1ba104f876078551207ef0eabf90e214c9d1e4e3`

## Inventory

- Files: 23 Markdown pages under `docs/developers/`.
- Lines: 2,984.
- Source-map coverage: 23 of 23 pages have one or more rows in
  `docs/source-map.md`.

## Fail-before result

Command:

```bash
/Users/jack/.agents/skills/technical-writing/scripts/technical-writing lint \
  docs/developers --mode developer --format json
```

Result: exit status 1 with 406 diagnostics. The result contained 317 errors
and 89 warnings across 23 files.

The primary error groups were long sentences, em dashes, semicolons,
contractions, and imprecise vocabulary. The warning groups included passive
voice, indirect verb forms, and nominalizations.

## Source-map coverage check

The check stripped the `docs/` prefix from each developer page and searched
for the exact backtick-delimited path in `docs/source-map.md`.

Result: `missing_source_map_files=0`.
