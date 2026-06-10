# Node Latest Suite Tags

Status: latest official suite tag registry

The checked-in latest-suite registry is
[`node-latest-suite-tags.json`](node-latest-suite-tags.json). It records the
latest official Node tag that Nimbus should use for each targeted lane and the
currently vendored fixture corpus tag, if one exists.

Validate the registry with:

```bash
bash scripts/verify-node-latest-suite-tags.sh
bash scripts/verify-node-release-train.sh
```

To enforce that every targeted corpus is already synced to its latest official
tag, run:

```bash
NIMBUS_ENFORCE_CURRENT_NODE_CORPORA=1 bash scripts/verify-node-latest-suite-tags.sh
```

That enforcement mode must pass after NFRC4 syncs Node22, Node24, and Node26
fixture corpora.

## Contract

- `latest_official_tag` is the current upstream Node release tag for the lane.
- `fixture_corpus_current_tag` is the tag currently represented by the vendored
  test corpus, or `null` if no corpus is vendored yet.
- `fixture_sync_required` must be `true` whenever the vendored corpus is absent
  or older than the latest official tag.
- `intended_sync_command` records the exact sync command NFRC4 should use.

The regular verifier keeps release facts and registry metadata honest without
claiming stale corpora are already current. The optional enforcement mode is
the guard that fails on stale fixture tags.

Release-train automation publishes
[`node-release-train.md`](node-release-train.md), which separates Node24
product default, Node22 supported LTS, Node20 legacy-grace, and Node26
Current/non-LTS, then fails if lane metadata, latest tags, dashboard roles, or
proof digests drift.
