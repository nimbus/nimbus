#!/usr/bin/env python3
"""Own the recorded-expectations baseline for the Rust Node compatibility lane.

The Rust corpus lane compares each observed fixture result with
`tests/runtime/node/expectations/corpus-baseline.json`. The Rust side of that
contract lives in
`crates/nimbus-runtime/src/runtime/tests/node/corpus_baseline.rs`. This script
owns the three operations that happen outside the test process:

  aggregate  merge the per-partition JSONL shards into one observed-results
             document, which is the format `watchpoints.py` already reads
  refresh    rewrite the baseline from an observed-results document
  verify     guard the baseline against the required surface and the vendored
             corpus

A baseline entry is only ever generated from a real run. Nothing here invents
an entry, and `refresh` refuses an observed-results document that does not
cover the lane it would rewrite.

Contract: docs/private/operating/node-compat-nightly.md.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any

BASELINE_RELATIVE_PATH = "tests/runtime/node/expectations/corpus-baseline.json"
POSTURE_RELATIVE_PATH = (
    "docs/private/architecture/runtime/node-default-support-posture.json"
)
FIXTURE_ROOT_RELATIVE_PATH = "crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures"

LANES = ("node20", "node22", "node24", "node26")

# A fixture that is vendored once and runs without a declared lane. The Rust
# reader already treats this as a real baseline key
# (NODE_COMPAT_LANELESS_BASELINE_KEY in corpus_baseline.rs), so this side must
# agree. Without it, a laneless failure has no legal way into the baseline, and
# the lane stays red on a finding the documented workflow cannot record.
LANELESS_LANE = "unspecified"

# Every key the baseline document carries. The laneless bucket is a key like
# any other, so the document always states all five and never leaves one
# implicit.
BASELINE_LANE_KEYS = (*LANES, LANELESS_LANE)

# Outcomes that the Rust seam writes. See NodeCompatReconciliation.
OUTCOME_FAILED = "failed"
OUTCOME_KNOWN_GAP = "known_gap"
OUTCOME_UNEXPECTED_PASS = "unexpected_pass"
OUTCOME_PASSED = "passed"
OUTCOME_SKIPPED = "skipped"

# Records that report the progress of a batch rather than the result of a
# fixture. `corpus_baseline.rs` writes them.
RECORD_KIND_BATCH_START = "batch_start"
RECORD_KIND_BATCH_COMPLETE = "batch_complete"
RECORD_KIND_BATCH_ABORT = "batch_abort"

# An entry is recorded for a fixture that did not pass. `known_gap` is included
# because a fixture already recorded keeps failing and must stay recorded.
RECORDABLE_OUTCOMES = frozenset({OUTCOME_FAILED, OUTCOME_KNOWN_GAP})

CONTRACT = (
    "Records which vendored upstream fixtures are known to fail in the "
    "non-ignored Rust compatibility lane. An entry keeps the lane green for "
    "that fixture and nothing else. A fixture that is not recorded and fails "
    "is a regression, and it fails the lane. A recorded fixture that passes "
    "is an unexpected pass, and it also fails the lane until the entry is "
    "removed. Entries record observed behavior only. Generate them with "
    "`make node-compat-baseline-refresh`; never write one by hand. Reports and "
    "the dashboard keep counting a recorded gap as a measured failure, so the "
    "published pass rate stays honest. Contract and refresh procedure: "
    "docs/private/operating/node-compat-nightly.md."
)


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def load_json(path: Path) -> Any:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, payload: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(payload, handle, indent=2, sort_keys=False)
        handle.write("\n")


# ---------------------------------------------------------------- aggregate


def check_shard_completeness(files: list[Path], partitions: int) -> None:
    """Refuses a measurement that is missing any partition's shard.

    The corpus job writes two shards per partition: `partition-<i>.jsonl` from
    the main run and `watchpoints-<i>.jsonl` from the ignored-test run. A
    partition that dies before its tests start writes neither, and the merge
    would still succeed on the shards of its siblings.

    That is the dangerous case. The fixtures of the missing partition are
    simply absent, so an unmeasured fixture reads exactly like a fixture with
    no finding, and seeding from it would leave those fixtures unrecorded. Their
    next failure would then be reported as a fresh regression.

    On 2026-09-16 partition 4 of 6 hit a transient `sccache` 503 from the
    Actions cache, cargo exited before compiling, and only the watchpoints
    shard reached the artifact.
    """
    present = {path.name for path in files}
    missing = [
        name
        for index in range(partitions)
        for name in (f"partition-{index}.jsonl", f"watchpoints-{index}.jsonl")
        if name not in present
    ]
    if missing:
        raise SystemExit(
            "error: the measurement is incomplete. Missing shard(s): "
            + ", ".join(missing)
            + ". Every partition must contribute both shards before the results "
            "can seed or reconcile a baseline. Rerun the corpus."
        )


def read_jsonl_shards(
    inputs: list[Path], partitions: int | None = None
) -> list[dict[str, Any]]:
    """Collects every JSON object from the given JSONL files and directories.

    `partitions` names how many corpus partitions the run used. When it is
    given, every partition must have contributed both of its shards.
    """
    files: list[Path] = []
    for item in inputs:
        if item.is_dir():
            files.extend(sorted(item.rglob("*.jsonl")))
        elif item.is_file():
            files.append(item)
        else:
            raise SystemExit(f"error: no such observed-results input: {item}")

    if partitions is not None:
        check_shard_completeness(files, partitions)

    records: list[dict[str, Any]] = []
    for path in files:
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            line = line.strip()
            if not line:
                continue
            try:
                record = json.loads(line)
            except json.JSONDecodeError as error:
                raise SystemExit(f"error: {path}:{number}: {error}") from error
            if not isinstance(record, dict):
                raise SystemExit(f"error: {path}:{number}: expected a JSON object")
            records.append(record)
    print(f"read {len(records)} observed results from {len(files)} shard(s)")
    return records


def check_batch_completeness(records: list[dict[str, Any]]) -> None:
    """Refuses a measurement that a killed batch truncated.

    One batch test executes hundreds of fixtures in a single process. The test
    runner kills a test that outruns its timeout, and every fixture the batch
    already measured is already in the shard, so a kill removes the rest of the
    batch from the measurement without removing the batch. Seeding from that
    run records where the kill landed. The next run reaches further, and it
    reports the fixtures behind the old kill point as fresh regressions.

    A batch writes one record when it starts and one when it ends. The end is
    either a completion, when the fixture loop ran to its last fixture, or an
    abort, when a panic or an early return unwound out of the loop. An abort is
    a reported failure: the test process stayed alive, and the runner names the
    failing test. A kill leaves neither record, because the process dies
    without unwinding, and that silence is what this check refuses.

    A completion also carries how many fixtures the loop executed, so a shard
    that lost records is refused as well. An aborted batch carries no count,
    because the loop never reached its end.

    On 2026-09-16 run 35171841643 recorded 6908 fixtures and run 35167962571
    recorded 6930, with 99 fixtures only in the first and 77 only in the
    second, all in contiguous alphabetical groups. About 32 batch tests had
    been killed at 135 seconds.
    """
    started: dict[str, dict[str, Any]] = {}
    completed: dict[str, dict[str, Any]] = {}
    aborted: dict[str, dict[str, Any]] = {}
    observed: Counter[str] = Counter()
    for record in records:
        kind = str(record.get("kind", ""))
        batch = str(record.get("batch", ""))
        if not batch:
            continue
        if kind == RECORD_KIND_BATCH_START:
            started[batch] = record
        elif kind == RECORD_KIND_BATCH_COMPLETE:
            completed[batch] = record
        elif kind == RECORD_KIND_BATCH_ABORT:
            aborted[batch] = record
        elif not kind:
            observed[batch] += 1

    errors: list[str] = []
    for batch in sorted(started):
        if batch in completed or batch in aborted:
            continue
        test_name = str(started[batch].get("test_name", "?"))
        errors.append(
            f"{batch}: the batch started and neither finished nor unwound. "
            f"Its test ({test_name}) measured {observed[batch]} fixture(s) "
            "and the process was killed, most likely by the nextest timeout. "
            "The rest of the batch is missing from this measurement."
        )
    for batch in sorted(aborted):
        if batch not in started:
            errors.append(
                f"{batch}: the batch unwound and never started. The shard "
                "holding its first records is missing."
            )
    for batch in sorted(completed):
        if batch not in started:
            errors.append(
                f"{batch}: the batch finished and never started. The shard "
                "holding its first records is missing."
            )
            continue
        expected = completed[batch].get("fixture_count")
        if not isinstance(expected, int) or observed[batch] != expected:
            errors.append(
                f"{batch}: the batch executed {expected} fixture(s) and the "
                f"shards hold {observed[batch]}. Records were lost between the "
                "test process and the merge."
            )
    if errors:
        raise SystemExit(
            "error: the measurement is truncated. A batch that does not run to "
            "its end measures only part of the corpus, and seeding or "
            "reconciling from it would report the unmeasured fixtures as "
            "regressions on the next run. Rerun the corpus.\n"
            + "\n".join(f"  {line}" for line in errors)
        )
    print(
        f"verified {len(completed)} complete batch(es) "
        f"and {len(aborted)} reported failure(s)"
    )


def merge_observed_records(records: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Reduces repeated attempts of one fixture to a single worst-case result.

    A fixture can run more than once across batch and watchpoint entry points.
    A pass does not erase a failure, because the lane must react to the failure.
    """
    # `unexpected_pass` reports a pass, so it must rank below the two failure
    # outcomes. Ranking it above `known_gap` let one passing entry point erase
    # a failing one, and the refresh then deleted an entry that the corpus had
    # just observed failing.
    severity = {
        OUTCOME_PASSED: 0,
        OUTCOME_SKIPPED: 1,
        OUTCOME_UNEXPECTED_PASS: 2,
        OUTCOME_KNOWN_GAP: 3,
        OUTCOME_FAILED: 4,
    }
    merged: dict[tuple[str, str], dict[str, Any]] = {}
    for record in records:
        lane = str(record.get("lane", ""))
        test_path = str(record.get("test_relative_path", ""))
        if not lane or not test_path:
            continue
        key = (lane, test_path)
        previous = merged.get(key)
        if previous is None or severity.get(
            str(record.get("outcome")), 0
        ) > severity.get(str(previous.get("outcome")), 0):
            merged[key] = record
    return [merged[key] for key in sorted(merged)]


def command_aggregate(args: argparse.Namespace) -> int:
    raw_records = read_jsonl_shards(
        [Path(p) for p in args.input], args.expect_partitions
    )
    check_batch_completeness(raw_records)
    records = merge_observed_records(raw_records)
    if not records:
        # Shards that exist but hold nothing are not the same as a missing
        # input, which `read_jsonl_shards` already rejects. An empty merge from
        # real files means the corpus executed and measured nothing, and
        # writing the document anyway would let the reconciliation job compare
        # the catalog against silence and report success. Refuse instead, and
        # name the likely cause.
        print(
            "no observed results in the shards. The corpus produced no "
            "measurement, so there is nothing to reconcile. Check that "
            "NIMBUS_NODE_COMPAT_OBSERVED_RESULTS reached the test process and "
            "that its shards are the files passed to --input.",
            file=sys.stderr,
        )
        return 1
    counts = Counter(str(record.get("outcome")) for record in records)
    payload = {
        "catalog_kind": "node_compat_observed_results",
        "generated_from": "instrumented_rust_corpus_run",
        "counts": dict(sorted(counts.items())),
        # `results` is the key that watchpoints.observed_result_entries reads.
        "results": records,
    }
    write_json(Path(args.output), payload)
    print(f"wrote {len(records)} merged results to {args.output}")
    for outcome, count in sorted(counts.items()):
        print(f"  {outcome}: {count}")
    return 0


# ------------------------------------------------------------------ refresh


def command_refresh(args: argparse.Namespace) -> int:
    root = repo_root()
    observed = load_json(Path(args.observed))
    records = observed.get("results") if isinstance(observed, dict) else observed
    if not isinstance(records, list):
        raise SystemExit("error: the observed-results document has no `results` list")

    seen_lanes = {str(r.get("lane")) for r in records if isinstance(r, dict)}
    lanes_to_write = list(args.lane) if args.lane else list(BASELINE_LANE_KEYS)
    missing = [lane for lane in lanes_to_write if lane not in seen_lanes]
    if missing:
        # Refusing here is the point. Rewriting a lane from a run that never
        # executed it would silently empty that lane's baseline and turn every
        # future failure there into a fresh regression.
        raise SystemExit(
            "error: the observed results contain no fixture for "
            f"{', '.join(missing)}. Rerun the corpus for those lanes, or pass "
            "--lane for only the lanes the run covered."
        )

    baseline_path = root / BASELINE_RELATIVE_PATH
    existing = load_json(baseline_path) if baseline_path.is_file() else {"lanes": {}}
    lanes: dict[str, list[dict[str, str]]] = {
        lane: list(existing.get("lanes", {}).get(lane, []))
        for lane in BASELINE_LANE_KEYS
    }

    # `verify` rejects a required-surface entry and a non-vendored entry, so
    # `refresh` must not write one. Without this, a refresh from a real run
    # writes a baseline that the guard immediately rejects, and the operator is
    # left with a bad file on disk and an error naming a fixture that they did
    # not choose to record.
    #
    # Skipping is not muting. These two kinds are exactly the failures that the
    # baseline must never absorb, so they are reported as work to do.
    surface = required_surface(root)
    fixture_root = root / FIXTURE_ROOT_RELATIVE_PATH
    not_recordable: list[str] = []

    for lane in lanes_to_write:
        entries: dict[str, tuple[str, str]] = {}
        for record in records:
            if not isinstance(record, dict) or str(record.get("lane")) != lane:
                continue
            if str(record.get("outcome")) not in RECORDABLE_OUTCOMES:
                continue
            test_path = str(record.get("test_relative_path", ""))
            if not test_path:
                continue
            # The posture document is lane-keyed, so the laneless bucket has
            # no required surface and this lookup is empty for it.
            if test_path in surface.get(lane, set()):
                not_recordable.append(
                    f"{lane}: {test_path} is required surface (v8_isolate_required). "
                    "Fix the runtime; the baseline may not record it."
                )
                continue
            source = str(record.get("fixture_source_relative_path", "")).strip()
            if not source:
                not_recordable.append(
                    f"{lane}: {test_path} names no vendored source. A test that "
                    "supplies its own source is a Nimbus probe, not a "
                    "measurement of upstream compatibility, so fix it rather "
                    "than record it."
                )
                continue
            if not (fixture_root / source).is_file():
                not_recordable.append(
                    f"{lane}: {test_path} names {source}, which is not vendored "
                    f"under {FIXTURE_ROOT_RELATIVE_PATH}. Vendor the fixture, or "
                    "fix the test that points at nothing."
                )
                continue
            entries[test_path] = (summarize_reason(record), source)
        lanes[lane] = [
            {
                "test_relative_path": path,
                "fixture_source_relative_path": entries[path][1],
                "reason": entries[path][0],
            }
            for path in sorted(entries)
        ]
        print(f"{lane}: {len(lanes[lane])} recorded gaps")

    write_json(
        baseline_path,
        {
            "catalog_kind": "node_compat_corpus_baseline",
            "contract": CONTRACT,
            "generated_from": "instrumented_rust_corpus_run",
            "lanes": lanes,
        },
    )
    print(f"wrote {baseline_path.relative_to(root)}")
    if not_recordable:
        print(
            f"\n{len(not_recordable)} observed failure(s) were left out because "
            "the baseline may not record them:",
            file=sys.stderr,
        )
        for line in sorted(not_recordable):
            print(f"  {line}", file=sys.stderr)
        print(
            "These keep the lane red until the runtime changes. That is the "
            "intended behavior.",
            file=sys.stderr,
        )
    return verify_baseline(root)


def summarize_reason(record: dict[str, Any]) -> str:
    """Turns a failure detail into one short, stable line.

    A full stack trace would make the baseline churn on every unrelated line
    change, so the reason keeps the first meaningful line only.
    """
    detail = str(record.get("detail", "")).strip()
    if not detail:
        return "observed failure with no detail"
    for line in detail.splitlines():
        line = line.strip()
        if not line or line.startswith("at "):
            continue
        # Drop the repeated prefix the Rust seam adds.
        marker = "should execute: "
        if marker in line:
            line = line.split(marker, 1)[1]
        return line[:200]
    return detail.splitlines()[0][:200]


# ------------------------------------------------------------------- verify


def required_surface(root: Path) -> dict[str, set[str]]:
    """Fixtures that the required surface owns, per lane.

    A required-surface fixture must never enter the baseline. The required
    surface is the gate that already means something, and recording a gap
    against it would quietly retire that gate.
    """
    posture_path = root / POSTURE_RELATIVE_PATH
    if not posture_path.is_file():
        raise SystemExit(f"error: missing support posture at {posture_path}")
    posture = load_json(posture_path)
    surface: dict[str, set[str]] = {}
    for lane, lane_data in posture.get("lanes", {}).items():
        surface[str(lane)] = {
            str(entry.get("test_path"))
            for entry in lane_data.get("entries", [])
            if entry.get("support_denominator") == "v8_isolate_required"
        }
    return surface


def verify_baseline(root: Path) -> int:
    baseline_path = root / BASELINE_RELATIVE_PATH
    if not baseline_path.is_file():
        print(f"error: missing baseline at {baseline_path}", file=sys.stderr)
        return 1
    baseline = load_json(baseline_path)
    errors: list[str] = []

    if baseline.get("catalog_kind") != "node_compat_corpus_baseline":
        errors.append("catalog_kind must be node_compat_corpus_baseline")
    if not str(baseline.get("contract", "")).strip():
        errors.append("the baseline must carry its contract")

    lanes = baseline.get("lanes")
    if not isinstance(lanes, dict):
        return fail(["`lanes` must be an object"])
    for lane in BASELINE_LANE_KEYS:
        if lane not in lanes:
            errors.append(f"lane `{lane}` is missing; write an empty list instead")

    surface = required_surface(root)
    fixture_root = root / FIXTURE_ROOT_RELATIVE_PATH
    total = 0

    for lane, entries in lanes.items():
        if lane not in BASELINE_LANE_KEYS:
            errors.append(f"unknown lane `{lane}`")
            continue
        if not isinstance(entries, list):
            errors.append(f"lane `{lane}` must hold a list")
            continue
        seen: set[str] = set()
        previous = ""
        for entry in entries:
            total += 1
            if not isinstance(entry, dict):
                errors.append(f"{lane}: every entry must be an object")
                continue
            test_path = str(entry.get("test_relative_path", ""))
            reason = str(entry.get("reason", "")).strip()
            if not test_path:
                errors.append(f"{lane}: an entry has no test_relative_path")
                continue
            if not reason:
                errors.append(f"{lane}: {test_path} has no reason")
            if test_path in seen:
                errors.append(f"{lane}: {test_path} is recorded twice")
            seen.add(test_path)
            if test_path < previous:
                errors.append(f"{lane}: {test_path} is out of order")
            previous = test_path
            if test_path in surface.get(lane, set()):
                errors.append(
                    f"{lane}: {test_path} is on the required surface "
                    "(support_denominator == v8_isolate_required) and must not "
                    "be recorded as a known gap. Fix the fixture, or "
                    "reclassify it through the posture first."
                )
            source = str(entry.get("fixture_source_relative_path", "")).strip()
            if not source:
                errors.append(
                    f"{lane}: {test_path} has no fixture_source_relative_path. "
                    "A recorded gap must name where its fixture is vendored."
                )
            elif not (fixture_root / source).is_file():
                errors.append(
                    f"{lane}: {test_path} names {source}, which is not vendored "
                    f"under {FIXTURE_ROOT_RELATIVE_PATH}"
                )

    if errors:
        return fail(errors)
    print(
        f"node-compat corpus baseline ok: {total} recorded gaps across "
        f"{len(BASELINE_LANE_KEYS)} lanes"
    )
    return 0


def fail(errors: list[str]) -> int:
    for error in errors:
        print(f"error: {error}", file=sys.stderr)
    return 1


def command_verify(_args: argparse.Namespace) -> int:
    return verify_baseline(repo_root())


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    aggregate = sub.add_parser(
        "aggregate", help="merge per-partition JSONL shards into one document"
    )
    aggregate.add_argument("--input", nargs="+", required=True)
    aggregate.add_argument("--output", required=True)
    aggregate.add_argument(
        "--expect-partitions",
        type=int,
        default=None,
        help=(
            "the number of corpus partitions the run used. Each one must have "
            "contributed both its partition and watchpoints shard."
        ),
    )
    aggregate.set_defaults(handler=command_aggregate)

    refresh = sub.add_parser(
        "refresh", help="rewrite the baseline from an observed-results document"
    )
    refresh.add_argument("--observed", required=True)
    refresh.add_argument(
        "--lane",
        action="append",
        choices=BASELINE_LANE_KEYS,
        help="restrict the rewrite to one lane key; repeatable",
    )
    refresh.set_defaults(handler=command_refresh)

    verify = sub.add_parser("verify", help="guard the checked-in baseline")
    verify.set_defaults(handler=command_verify)

    args = parser.parse_args()
    return int(args.handler(args))


if __name__ == "__main__":
    sys.exit(main())
