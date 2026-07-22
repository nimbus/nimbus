#!/usr/bin/env python3
"""Verify that every reachable Rust ambient wall-clock source is classified."""

from __future__ import annotations

import csv
import re
import sys
from dataclasses import dataclass
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
LEDGER = REPO_ROOT / "scripts/data/clock-sources.tsv"
FIXTURE = REPO_ROOT / "scripts/fixtures/clock-sources/disallowed.rs"
PATTERNS = {
    "Timestamp::now": "reads ambient epoch time through Timestamp",
    "SystemTime::now": "reads the process wall clock directly",
    "system_now_millis": "reads ambient epoch milliseconds through the system adapter",
    "system_now_secs": "reads ambient epoch seconds through the system adapter",
}
CLASSIFICATIONS = {
    "INJECT",
    "SAMPLE_AT_SHELL",
    "SYSTEM_ADAPTER",
    "UNIQUENESS_ONLY",
    "TEST_ONLY",
    "EXTERNAL_TYPE",
    "REMOVE",
}
SENSITIVE_PREFIXES = (
    "crates/nimbus-engine/src/engine/mutations/",
    "crates/nimbus-engine/src/engine/execution_units/",
    "crates/nimbus-engine/src/engine/scheduler/",
    "crates/nimbus-convex/src/auth/",
    "crates/nimbus-dynamodb/src/auth/",
)
SENSITIVE_FILES = {
    "crates/nimbus-engine/src/engine/transactions.rs",
    "crates/nimbus-engine/src/tenant/write_rate.rs",
    "crates/nimbus-firebase/src/grpc/listen_stream.rs",
    "crates/nimbus-firebase/src/grpc/write_stream.rs",
    "crates/nimbus-server/src/adapters/cloudflare/durable_objects/mod.rs",
    "crates/nimbus-server/src/adapters/convex/execution/async_ops/scheduling.rs",
    "crates/nimbus-server/src/adapters/convex/execution/sync_ops/scheduling.rs",
    "crates/nimbus-server/src/adapters/convex/handlers/scheduling.rs",
}


@dataclass(frozen=True)
class Hit:
    path: str
    line: int
    pattern: str


def is_structural_test_path(path: Path) -> bool:
    relative = path.relative_to(REPO_ROOT)
    parts = relative.parts
    return (
        "tests" in parts
        or "test_support" in parts
        or "benches" in parts
        or path.name == "tests.rs"
        or path.name.endswith("_tests.rs")
    )


def cfg_test_lines(lines: list[str]) -> set[int]:
    """Return zero-based lines owned by items whose cfg requires `test`."""
    excluded: set[int] = set()
    index = 0
    while index < len(lines):
        if not re.match(r"\s*#\s*\[\s*cfg\s*\(", lines[index]):
            index += 1
            continue

        start = index
        attribute = lines[index]
        while ")]" not in attribute and index + 1 < len(lines):
            index += 1
            attribute += lines[index]
        compact_attribute = re.sub(r"\s+", "", attribute)
        requires_test = bool(
            re.fullmatch(r"#\[cfg\(test\)\]", compact_attribute)
            or re.fullmatch(r"#\[cfg\(all\([^]]*\btest\b[^]]*\)\)\]", compact_attribute)
        )
        if not requires_test:
            index = start + 1
            continue

        index += 1
        while index < len(lines) and (
            not lines[index].strip()
            or lines[index].lstrip().startswith("#[")
            or lines[index].lstrip().startswith("///")
        ):
            index += 1
        if index >= len(lines):
            excluded.update(range(start, index))
            break

        item_start = index
        brace_depth = 0
        saw_brace = False
        while index < len(lines):
            brace_depth += lines[index].count("{") - lines[index].count("}")
            saw_brace = saw_brace or "{" in lines[index]
            index += 1
            if saw_brace and brace_depth <= 0:
                break
            if not saw_brace and ";" in lines[index - 1]:
                break
        excluded.update(range(start, index))
        if index == item_start:
            index += 1
    return excluded


def scan_file(path: Path, include_tests: bool = False) -> list[Hit]:
    relative = path.relative_to(REPO_ROOT).as_posix()
    lines = path.read_text(encoding="utf-8").splitlines()
    excluded = set() if include_tests else cfg_test_lines(lines)
    hits: list[Hit] = []
    for line_index, line in enumerate(lines):
        if line_index in excluded:
            continue
        for pattern in PATTERNS:
            if f"{pattern}(" in line:
                hits.append(Hit(relative, line_index + 1, pattern))
    return hits


def production_hits() -> list[Hit]:
    hits: list[Hit] = []
    for path in sorted((REPO_ROOT / "crates").rglob("*.rs")):
        if is_structural_test_path(path):
            continue
        if any(part in {"target", "src/gen", "node_compat_fixtures"} for part in path.parts):
            continue
        hits.extend(scan_file(path))
    return hits


def load_ledger() -> dict[tuple[str, str], dict[str, str]]:
    with LEDGER.open(encoding="utf-8", newline="") as handle:
        rows = csv.DictReader(
            (line for line in handle if not line.startswith("#")), delimiter="\t"
        )
        required = {
            "path",
            "pattern",
            "classification",
            "owner",
            "rationale",
            "removal_trigger",
        }
        if set(rows.fieldnames or ()) != required:
            raise ValueError(f"{LEDGER}: expected columns {sorted(required)}")
        ledger: dict[tuple[str, str], dict[str, str]] = {}
        for row in rows:
            key = (row["path"], row["pattern"])
            if key in ledger:
                raise ValueError(f"duplicate clock-source ledger entry: {key}")
            if row["classification"] not in CLASSIFICATIONS:
                raise ValueError(f"invalid clock-source classification at {key}")
            if any(not row[field].strip() for field in required):
                raise ValueError(f"blank required clock-source ledger field at {key}")
            ledger[key] = row
        return ledger


def is_sensitive(path: str) -> bool:
    return path in SENSITIVE_FILES or path.startswith(SENSITIVE_PREFIXES)


def main() -> int:
    errors: list[str] = []
    if not LEDGER.is_file():
        print(f"clock-source-check: missing ledger: {LEDGER.relative_to(REPO_ROOT)}", file=sys.stderr)
        return 1

    try:
        ledger = load_ledger()
    except (OSError, ValueError) as error:
        print(f"clock-source-check: {error}", file=sys.stderr)
        return 1

    hits = production_hits()
    observed = {(hit.path, hit.pattern) for hit in hits}
    for hit in hits:
        reason = PATTERNS[hit.pattern]
        if is_sensitive(hit.path):
            errors.append(
                f"{hit.path}:{hit.line}: correctness-sensitive source tree forbids "
                f"`{hit.pattern}(` because it {reason}; inject a clock or pass an explicit observation"
            )
        elif (hit.path, hit.pattern) not in ledger:
            errors.append(
                f"{hit.path}:{hit.line}: unclassified `{hit.pattern}(` ({reason}); "
                "record its semantic owner, rationale, and removal trigger"
            )

    stale = sorted(set(ledger) - observed)
    for path, pattern in stale:
        errors.append(f"{path}: stale clock-source allowlist entry for `{pattern}(`")

    fixture_hits = scan_file(FIXTURE, include_tests=True)
    if len(fixture_hits) != 1 or fixture_hits[0].pattern != "SystemTime::now":
        errors.append(
            "guard fixture must contain exactly one disallowed SystemTime::now source "
            "so file/line/semantic diagnostics remain proven"
        )

    if errors:
        for error in errors:
            print(f"clock-source-check: {error}", file=sys.stderr)
        return 1

    print("clock_source_allowlist_matches_reachable_production_sources: PASS")
    print("correctness_sensitive_source_tree_rejects_ambient_wall_time: PASS")
    print("ambient_clock_allowlist_has_no_stale_entries: PASS")
    fixture = fixture_hits[0]
    print(
        "clock-source guard fixture: PASS "
        f"({fixture.path}:{fixture.line}: {PATTERNS[fixture.pattern]})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
