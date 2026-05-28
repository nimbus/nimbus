#!/usr/bin/env python3
"""Guard hand-written Node LTS docs against stale support claims."""

from __future__ import annotations

import re
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[3]

HAND_WRITTEN_SUPPORT_DOCS = (
    "docs/runtimes/nodejs/README.md",
    "docs/runtimes/nodejs/compatibility.md",
    "docs/runtimes/nodejs/configuration.md",
    "docs/architecture/runtime/node-compat-surface-matrix.md",
    "docs/architecture/runtime/deno-vs-neovex-node-compat.md",
    "docs/architecture/runtime/node-lts-compat/node-lts-lanes.md",
)

FORBIDDEN_PATTERNS = (
    (
        re.compile(r"\b\d+(?:\.\d+)?%"),
        "hand-written percentage support claims belong in generated evidence",
    ),
    (
        re.compile(r"\b\d+\s+Node22 tests green\b", re.IGNORECASE),
        "hand-written Node22 pass-count prose makes the product default look like evidence priority",
    ),
    (
        re.compile(r"Official Node test files green", re.IGNORECASE),
        "hand-written official fixture totals belong in generated evidence",
    ),
    (
        re.compile(r"Node20:\s*supported selectable target", re.IGNORECASE),
        "Node20 is EOL legacy-grace coverage, not active supported LTS",
    ),
    (
        re.compile(r"\|\s*Node20\s*\|\s*Supported\b", re.IGNORECASE),
        "Node20 table rows must not call the EOL lane supported",
    ),
    (
        re.compile(r"Supported Node lanes\s*\|[^\n]*Node20", re.IGNORECASE),
        "Node20 must not be listed as a supported LTS lane",
    ),
)

REQUIRED_SNIPPETS = {
    "docs/runtimes/nodejs/README.md": (
        "Product default is a routing default, not an evidence priority.",
        "Node20 remains selectable only as legacy-grace regression coverage",
    ),
    "docs/runtimes/nodejs/compatibility.md": (
        "Node22 and Node24 are supported LTS targets with lane-local evidence.",
        "Node20 remains selectable as legacy-grace regression coverage",
        "Product default is a routing default, not an evidence priority.",
    ),
    "docs/runtimes/nodejs/configuration.md": (
        "product default from the lane registry",
        "evidence priority",
    ),
    "docs/architecture/runtime/node-compat-surface-matrix.md": (
        "Product default is a routing default, not an evidence priority.",
        "Node22 and Node24 are the current supported LTS lanes",
    ),
    "docs/architecture/runtime/deno-vs-neovex-node-compat.md": (
        "it must not carry hand-maintained pass rates",
        "Product default is a routing default, not an evidence priority.",
    ),
}


def main() -> int:
    errors: list[str] = []
    for relative_path in HAND_WRITTEN_SUPPORT_DOCS:
        path = REPO_ROOT / relative_path
        if not path.is_file():
            errors.append(f"missing hand-written Node support doc: {relative_path}")
            continue
        text = path.read_text(encoding="utf-8")
        normalized_text = re.sub(r"\s+", " ", text)
        for pattern, reason in FORBIDDEN_PATTERNS:
            match = pattern.search(text)
            if match:
                errors.append(
                    f"{relative_path}: stale Node support prose `{match.group(0)}`: {reason}"
                )
        for snippet in REQUIRED_SNIPPETS.get(relative_path, ()):
            if snippet not in normalized_text:
                errors.append(
                    f"{relative_path}: missing required Node LTS support wording `{snippet}`"
                )

    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1

    print(
        "Node LTS docs guard passed: hand-written docs avoid stale pass-rate "
        "and support-priority claims"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
