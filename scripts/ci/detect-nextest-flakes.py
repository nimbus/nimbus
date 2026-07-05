#!/usr/bin/env python3
"""Detect retry-pass flaky tests from nextest JUnit output (A9/F13).

Parses ONLY the documented JUnit flaky markers (<flakyFailure>/<flakyError>):
a test that failed and then passed on retry. Genuine failures (<failure>) and
rerun-still-failing tests (<rerunFailure>/<rerunError>) are NOT classified as
flaky — they are real reds and stay the run's responsibility.

Emits ::error:: per flaky test plus machine-readable summary lines:
    FLAKY-DETECTED <test-id> <attempts>
Disposition: a flaky-quarantine ledger row (owner/issue/expiry mandatory;
scripts/test-taxonomy.py check enforces expiry).
"""

from __future__ import annotations

import argparse
import glob
import os
import sys
import xml.etree.ElementTree as ET


def flaky_tests(junit_path: str) -> dict[str, int]:
    flakes: dict[str, int] = {}
    tree = ET.parse(junit_path)
    for case in tree.iter("testcase"):
        flaky_elems = case.findall("flakyFailure") + case.findall("flakyError")
        if not flaky_elems:
            continue
        classname = case.get("classname", "")
        name = case.get("name", "")
        test_id = f"{classname}::{name}" if classname else name
        # attempts = flaky retries observed + the final (passing) attempt
        flakes[test_id] = len(flaky_elems) + 1
    return flakes


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "junit_globs",
        nargs="+",
        help="JUnit XML paths or globs (nextest ci-nightly junit output)",
    )
    args = parser.parse_args()

    paths: list[str] = []
    for pattern in args.junit_globs:
        paths.extend(sorted(glob.glob(pattern, recursive=True)))
    if not paths:
        print(
            f"::warning::no JUnit files matched {args.junit_globs} — "
            "flake detection had nothing to scan"
        )
        return 0

    all_flakes: dict[str, int] = {}
    for path in paths:
        try:
            all_flakes.update(flaky_tests(path))
        except ET.ParseError as error:
            print(f"::warning::unparseable JUnit file {path}: {error}")

    if not all_flakes:
        print("no flaky detections")
        return 0

    summary_path = os.environ.get("GITHUB_STEP_SUMMARY")
    summary_lines = []
    for test_id, attempts in sorted(all_flakes.items()):
        print(
            f"::error::FLAKY test detected: {test_id} needed {attempts} attempts — "
            "file a flaky-quarantine ledger row (owner/issue/expiry) and fix or delete"
        )
        summary_lines.append(f"FLAKY-DETECTED {test_id} {attempts}")
    if summary_path:
        try:
            with open(summary_path, "a", encoding="utf-8") as handle:
                handle.write("\n".join(summary_lines) + "\n")
        except OSError as error:  # visibility must never mask the detection itself
            print(f"::warning::could not append step summary: {error}")
    # Under flaky-result=fail the run already failed; this exit code makes the
    # detection step itself red so the annotation is impossible to miss.
    return 1


if __name__ == "__main__":
    sys.exit(main())
