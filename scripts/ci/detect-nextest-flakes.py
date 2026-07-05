#!/usr/bin/env python3
"""Emit CI annotations for nextest retry-pass flaky detections."""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from pathlib import Path
from typing import Iterable


ATTEMPT_SUFFIX_RE = re.compile(r"^(?P<test_id>.+)#(?P<attempts>[0-9]+)$")
HUMAN_FLAKY_RE = re.compile(
    r"^\s*FLKY-\S+\s+(?P<attempts>[0-9]+)/[0-9]+\s+\[[^\]]+\]\s+\([^)]+\)\s+"
    r"(?P<package>\S+)\s+(?P<name>.+?)\s*$"
)


def iter_lines(paths: Iterable[Path]) -> Iterable[str]:
    for path in paths:
        with path.open(encoding="utf-8", errors="replace") as handle:
            yield from handle


def structured_flakes(lines: Iterable[str]) -> dict[str, int]:
    flakes: dict[str, int] = {}
    for line in lines:
        stripped = line.strip()
        if not stripped.startswith("{"):
            continue
        try:
            event = json.loads(stripped)
        except json.JSONDecodeError:
            continue
        if event.get("type") != "test" or event.get("event") != "failed":
            continue
        reason = str(event.get("reason") or "")
        if "flaky" not in reason.lower():
            continue
        raw_name = str(event.get("name") or "")
        match = ATTEMPT_SUFFIX_RE.match(raw_name)
        test_id = match.group("test_id") if match else raw_name
        attempts = int(match.group("attempts")) if match else 2
        flakes[test_id] = max(flakes.get(test_id, 0), attempts)
    return flakes


def human_flakes(lines: Iterable[str]) -> dict[str, int]:
    flakes: dict[str, int] = {}
    for line in lines:
        match = HUMAN_FLAKY_RE.match(line)
        if not match:
            continue
        test_id = f"{match.group('package')}::{match.group('name')}"
        attempts = int(match.group("attempts"))
        flakes[test_id] = max(flakes.get(test_id, 0), attempts)
    return flakes


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("logs", nargs="+", type=Path, help="nextest output logs to parse")
    parser.add_argument(
        "--summary",
        type=Path,
        default=Path(os.environ["GITHUB_STEP_SUMMARY"])
        if os.environ.get("GITHUB_STEP_SUMMARY")
        else None,
        help="GitHub step summary path",
    )
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    for path in args.logs:
        if not path.is_file():
            print(f"::error::nextest log not found: {path}", file=sys.stderr)
            return 1

    text_lines = list(iter_lines(args.logs))
    flakes = structured_flakes(text_lines)
    if not flakes:
        flakes = human_flakes(text_lines)

    if not flakes:
        print("no nextest flaky retry detections found")
        return 0

    details = ", ".join(f"{test_id} attempts={attempts}" for test_id, attempts in sorted(flakes.items()))
    print(f"::error::nextest flaky retry detections: {details}")
    for test_id, attempts in sorted(flakes.items()):
        print(f"::error::FLAKY {test_id} attempts={attempts}")

    if args.summary is not None:
        with args.summary.open("a", encoding="utf-8") as summary:
            for test_id, attempts in sorted(flakes.items()):
                summary.write(f"FLAKY-DETECTED {test_id} {attempts}\n")

    return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
