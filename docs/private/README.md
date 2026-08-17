# Private Contributor Documentation

This tree contains internal architecture records, implementation plans,
operating runbooks, test contracts, and review evidence. It is not published
on nimbusdocs.com. Public documentation must not link into this tree.

## Routing

| Work | Start here |
| --- | --- |
| Architecture, trust boundaries, crate ownership, or cross-seam design | [`architecture/README.md`](architecture/README.md) |
| Local development, verification, deployment operations, or incident diagnosis | [`operating/README.md`](operating/README.md) |
| Protocol adapters and compatibility surfaces | [`adapters/README.md`](adapters/README.md) |
| Active implementation order, durable plans, and status ledgers | [`plans/README.md`](plans/README.md) |
| Test taxonomy, deterministic harnesses, and shared fixtures | [`testing/TEST_CONVENTIONS.md`](testing/TEST_CONVENTIONS.md) |
| Historical code reviews | [`code-review/`](code-review/) |

Use the active plan that owns the current change. The plan status ledger and
the current worktree are the recovery record for work that spans sessions.
Source and tests remain the authority for current behavior. A proof document
records evidence, not a second implementation contract.

## Private fence

- Put product guidance for users under the published groups named in
  [`../README.md`](../README.md).
- Put source ownership claims in the public
  [`../source-map.md`](../source-map.md) when a public page depends on them.
- Do not add private pages to website navigation, generated `llms` outputs, or
  links from public pages.
- Do not copy active-plan status into bootstrap documents. Route to the plans
  index instead.

## Verification

For documentation changes, run:

```bash
bash scripts/check-docs.sh
bash scripts/verify-nimbus-docs-site.sh
npm --prefix website run build
```

The first two commands validate repository and publication contracts. The
build verifies the rendered site and generated language-model indexes. See
[`operating/verification.md`](operating/verification.md) for the full
repository verification contract.
