#!/usr/bin/env python3
"""Assert the coverage shard plan still covers every workspace member.

The coverage reducer instruments the whole workspace and then merges the
profiles the shards produced. A crate that no shard runs is therefore not
absent from the report -- it lands in the denominator at 0% and lowers the
reported number without anyone choosing that. This guard makes the choice
explicit: every member of `[workspace] members` must appear in exactly one
shard in .github/coverage-shards.json, or in that file's `excluded` list with
a reason.
"""

from __future__ import annotations

import json
import sys
import tomllib
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
PLAN_PATH = REPO_ROOT / ".github" / "coverage-shards.json"
MANIFEST_PATH = REPO_ROOT / "Cargo.toml"


def workspace_members() -> dict[str, str]:
    """Map each workspace member's package name to its manifest directory."""
    root = tomllib.loads(MANIFEST_PATH.read_text())
    members: dict[str, str] = {}
    for entry in root["workspace"]["members"]:
        if "*" in entry:
            # Every member is spelled out today. A glob would make the mapping
            # from directory to package name ambiguous here, so require the
            # explicit form rather than guess.
            sys.exit(f"{MANIFEST_PATH}: glob workspace member '{entry}' is not supported")
        manifest = REPO_ROOT / entry / "Cargo.toml"
        if not manifest.is_file():
            sys.exit(f"{MANIFEST_PATH}: workspace member '{entry}' has no Cargo.toml")
        name = tomllib.loads(manifest.read_text())["package"]["name"]
        members[name] = entry
    return members


def main() -> int:
    plan = json.loads(PLAN_PATH.read_text())
    members = workspace_members()

    errors: list[str] = []

    sharded: dict[str, str] = {}
    for shard in plan["shards"]:
        for package in shard["packages"]:
            if package in sharded:
                errors.append(
                    f"'{package}' is in both the '{sharded[package]}' and "
                    f"'{shard['shard']}' shards; a crate runs in exactly one shard"
                )
            sharded[package] = shard["shard"]

    excluded = {entry["package"]: entry["reason"] for entry in plan["excluded"]}

    for package in sorted(set(sharded) & set(excluded)):
        errors.append(f"'{package}' is both sharded and excluded; pick one")

    for package in sorted(set(sharded) | set(excluded)):
        if package not in members:
            errors.append(
                f"'{package}' is named in {PLAN_PATH.name} but is not a workspace member"
            )

    for package in sorted(members):
        if package in sharded or package in excluded:
            continue
        errors.append(
            f"workspace member '{package}' ({members[package]}) is in no coverage "
            f"shard. Add it to a shard in {PLAN_PATH.name}, or to that file's "
            f"'excluded' list with the reason its lines are not measured."
        )

    for package, reason in sorted(excluded.items()):
        if not reason.strip():
            errors.append(f"'{package}' is excluded without a reason")

    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1

    print(
        f"coverage scope ok: {len(sharded)} of {len(members)} workspace members "
        f"across {len(plan['shards'])} shards, {len(excluded)} excluded"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
