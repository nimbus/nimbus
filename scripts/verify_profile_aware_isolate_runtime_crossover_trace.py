#!/usr/bin/env python3
"""Validate selected runtime crossover rows as complete JSON records."""

from __future__ import annotations

import argparse
import json
from dataclasses import dataclass
from pathlib import Path
from typing import Any


TRACE_SCHEMA = "nimbus.profile_aware_isolate_runtime.pir0.trace.v3"


class TraceContractError(ValueError):
    """A crossover trace does not contain the required row contract."""


@dataclass(frozen=True)
class TraceValidation:
    """Validated pool kinds and the single trace generation identity."""

    pool_kinds: frozenset[str]
    run_id: str


def load_records(trace_path: Path) -> list[dict[str, Any]]:
    records: list[dict[str, Any]] = []
    for line_number, raw_line in enumerate(
        trace_path.read_text(encoding="utf-8").splitlines(), start=1
    ):
        if not raw_line.strip():
            continue
        try:
            record = json.loads(raw_line)
        except json.JSONDecodeError as error:
            raise TraceContractError(
                f"{trace_path}:{line_number}: invalid JSON: {error.msg}"
            ) from error
        if not isinstance(record, dict):
            raise TraceContractError(
                f"{trace_path}:{line_number}: trace record must be a JSON object"
            )
        records.append(record)
    if not records:
        raise TraceContractError(f"{trace_path}: trace has no JSON records")
    return records


def validate_trace(
    trace_path: Path,
    *,
    benchmark_group: str,
    profile: str,
    workload: str,
    execution_model: str,
    actual_construction_mode: str,
    startup_strategy_label: str,
    expected_run_id: str | None = None,
) -> TraceValidation:
    supported_construction_modes = {"startup_snapshot", "unsnapshotted"}
    if actual_construction_mode not in supported_construction_modes:
        supported = ", ".join(sorted(supported_construction_modes))
        raise TraceContractError(
            f"unsupported actual construction mode {actual_construction_mode!r}; "
            f"expected one of: {supported}"
        )
    if expected_run_id is not None and not expected_run_id:
        raise TraceContractError("expected run ID must be nonempty")

    expected_ids = {
        "startup_snapshot_cache": (
            f"{profile}/{workload}/{execution_model}/{startup_strategy_label}"
        ),
        "warm_pool": f"{profile}/{workload}/{execution_model}/warm_pool",
    }
    expected_strategies = {
        "startup_snapshot_cache": startup_strategy_label,
        "warm_pool": "warm_pool",
    }
    seen_pool_kinds: set[str] = set()
    trace_run_id: str | None = None
    last_measured_iterations: dict[str, int] = {}

    for line_number, record in enumerate(load_records(trace_path), start=1):
        if (
            record.get("benchmark_group") != benchmark_group
            or record.get("profile") != profile
            or record.get("workload") != workload
            or record.get("execution_model") != execution_model
        ):
            continue

        pool_kind = record.get("pool_kind")
        if pool_kind not in expected_ids:
            raise TraceContractError(
                f"{trace_path}:{line_number}: unexpected pool_kind {pool_kind!r}"
            )

        run_id = record.get("run_id")
        if not isinstance(run_id, str) or not run_id:
            raise TraceContractError(
                f"{trace_path}:{line_number}: run_id is {run_id!r}; "
                "expected a nonempty string"
            )
        if trace_run_id is None:
            trace_run_id = run_id
        elif run_id != trace_run_id:
            raise TraceContractError(
                f"{trace_path}:{line_number}: trace contains multiple run IDs: "
                f"{trace_run_id!r} and {run_id!r}"
            )
        if expected_run_id is not None and run_id != expected_run_id:
            raise TraceContractError(
                f"{trace_path}:{line_number}: run_id is {run_id!r}; "
                f"expected shared run ID {expected_run_id!r}"
            )

        measured_iterations = record.get("measured_iterations")
        if (
            isinstance(measured_iterations, bool)
            or not isinstance(measured_iterations, int)
            or measured_iterations <= 0
        ):
            raise TraceContractError(
                f"{trace_path}:{line_number}: {pool_kind} measured_iterations is "
                f"{measured_iterations!r}; expected a positive integer"
            )
        previous_iterations = last_measured_iterations.get(pool_kind)
        if previous_iterations is not None and measured_iterations <= previous_iterations:
            raise TraceContractError(
                f"{trace_path}:{line_number}: {pool_kind} duplicate or non-increasing "
                f"sample row {measured_iterations}; previous value was "
                f"{previous_iterations}"
            )
        last_measured_iterations[pool_kind] = measured_iterations

        expected_id = expected_ids[pool_kind]
        exact_fields = {
            "schema": TRACE_SCHEMA,
            "benchmark_id": expected_id,
            "strategy": expected_strategies[pool_kind],
            "actual_v8_construction_mode": actual_construction_mode,
        }
        for field, expected_value in exact_fields.items():
            actual_value = record.get(field)
            if actual_value != expected_value:
                raise TraceContractError(
                    f"{trace_path}:{line_number}: {pool_kind} {field} is "
                    f"{actual_value!r}; expected {expected_value!r}"
                )

        counters = {
            "startup_snapshot": record.get(
                "v8_startup_snapshot_runtime_constructions"
            ),
            "unsnapshotted": record.get("v8_unsnapshotted_runtime_constructions"),
        }
        for mode, count in counters.items():
            if isinstance(count, bool) or not isinstance(count, int) or count < 0:
                raise TraceContractError(
                    f"{trace_path}:{line_number}: {pool_kind} {mode} construction "
                    f"count is {count!r}; expected a nonnegative integer"
                )
        for mode, count in counters.items():
            if mode == actual_construction_mode and count == 0:
                raise TraceContractError(
                    f"{trace_path}:{line_number}: {pool_kind} observed no successful "
                    f"{mode} runtime construction"
                )
            if mode != actual_construction_mode and count != 0:
                raise TraceContractError(
                    f"{trace_path}:{line_number}: {pool_kind} observed unexpected "
                    f"{mode} runtime constructions: {count}"
                )
        seen_pool_kinds.add(pool_kind)

    missing = set(expected_ids) - seen_pool_kinds
    if missing:
        missing_list = ", ".join(sorted(missing))
        raise TraceContractError(
            f"{trace_path}: missing exact crossover rows for {missing_list}"
        )
    if trace_run_id is None:
        raise TraceContractError(f"{trace_path}: selected rows have no run identity")
    return TraceValidation(frozenset(seen_pool_kinds), trace_run_id)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--trace", required=True, type=Path)
    parser.add_argument("--benchmark-group", required=True)
    parser.add_argument("--profile", required=True)
    parser.add_argument("--workload", required=True)
    parser.add_argument("--execution-model", required=True)
    parser.add_argument("--actual-construction-mode", required=True)
    parser.add_argument("--startup-strategy-label", required=True)
    parser.add_argument("--expected-run-id")
    parser.add_argument("--print-run-id", action="store_true")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        validation = validate_trace(
            args.trace,
            benchmark_group=args.benchmark_group,
            profile=args.profile,
            workload=args.workload,
            execution_model=args.execution_model,
            actual_construction_mode=args.actual_construction_mode,
            startup_strategy_label=args.startup_strategy_label,
            expected_run_id=args.expected_run_id,
        )
    except (OSError, TraceContractError) as error:
        print(f"crossover trace validation failed: {error}")
        return 1
    if args.print_run_id:
        print(validation.run_id)
        return 0
    validated = ", ".join(sorted(validation.pool_kinds))
    print(
        f"crossover trace validation passed: {args.trace} "
        f"({len(validation.pool_kinds)} pool kinds: {validated}; "
        f"run ID: {validation.run_id})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
