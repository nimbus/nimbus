# Node.js Runtime Evidence

This directory contains generated Node.js runtime evidence pages plus the
canonical maintainer workflow for refreshing them.

Generated outputs:

- `latest.md`
- `node20.md`
- `node22.md`
- `node24.md`
- `node26.md`

The generated Deno-style public support pages next to this directory are:

- `../compatibility.md`
- `../reference/node-apis.md`
- `../reference/packages.md`

The generated pages in this directory are published by
`scripts/runtime/node/publish_docs.py` from the checked-in engineering
snapshots under `docs/private/architecture/runtime/node-compat-evidence/latest/`.

Maintainers should use [refreshing](refreshing.md) when updating lane metadata,
syncing against an upstream Node tag, regenerating dashboards, or preparing a
future `nodeNN` lane.
