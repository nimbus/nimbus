#!/usr/bin/env python3
"""Tests for the selected runtime crossover trace contract."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from scripts.verify_profile_aware_isolate_runtime_crossover_trace import (
    TRACE_SCHEMA,
    TraceContractError,
    validate_trace,
)


class CrossoverTraceContractTests(unittest.TestCase):
    def write_trace(self, records: list[dict[str, object]]) -> Path:
        temp_dir = tempfile.TemporaryDirectory()
        self.addCleanup(temp_dir.cleanup)
        path = Path(temp_dir.name) / "trace.jsonl"
        path.write_text(
            "".join(f"{json.dumps(record)}\n" for record in records),
            encoding="utf-8",
        )
        return path

    @staticmethod
    def record(
        pool_kind: str,
        strategy_label: str,
        actual_construction_mode: str,
        *,
        run_id: str = "run-a",
    ) -> dict[str, object]:
        startup_snapshot_constructions = int(
            actual_construction_mode == "startup_snapshot"
        )
        unsnapshotted_constructions = int(
            actual_construction_mode == "unsnapshotted"
        )
        return {
            "schema": TRACE_SCHEMA,
            "run_id": run_id,
            "benchmark_group": "runtime_pool_modes_web_selected",
            "benchmark_id": (
                "web_standard/hostless_trivial/cooperative_locker/"
                f"{strategy_label}"
            ),
            "profile": "web_standard",
            "workload": "hostless_trivial",
            "pool_kind": pool_kind,
            "strategy": strategy_label,
            "actual_v8_construction_mode": actual_construction_mode,
            "v8_startup_snapshot_runtime_constructions": (
                startup_snapshot_constructions
            ),
            "v8_unsnapshotted_runtime_constructions": unsnapshotted_constructions,
            "execution_model": "cooperative_locker",
            "measured_iterations": 1,
        }

    def validate_web_trace(self, path: Path) -> frozenset[str]:
        return validate_trace(
            path,
            benchmark_group="runtime_pool_modes_web_selected",
            profile="web_standard",
            workload="hostless_trivial",
            execution_model="cooperative_locker",
            actual_construction_mode="unsnapshotted",
            startup_strategy_label="unsnapshotted_runtime_cache",
        ).pool_kinds

    def test_accepts_complete_rows(self) -> None:
        path = self.write_trace(
            [
                self.record(
                    "startup_snapshot_cache",
                    "unsnapshotted_runtime_cache",
                    "unsnapshotted",
                ),
                self.record("warm_pool", "warm_pool", "unsnapshotted"),
            ]
        )

        self.assertEqual(
            self.validate_web_trace(path),
            frozenset({"startup_snapshot_cache", "warm_pool"}),
        )

    def test_rejects_fields_mixed_across_rows(self) -> None:
        path = self.write_trace(
            [
                self.record(
                    "startup_snapshot_cache",
                    "unsnapshotted_runtime_cache",
                    "startup_snapshot",
                ),
                self.record("warm_pool", "warm_pool", "unsnapshotted"),
            ]
        )

        with self.assertRaisesRegex(
            TraceContractError,
            "startup_snapshot_cache actual_v8_construction_mode",
        ):
            self.validate_web_trace(path)

    def test_rejects_missing_pool_row(self) -> None:
        path = self.write_trace(
            [
                self.record(
                    "startup_snapshot_cache",
                    "unsnapshotted_runtime_cache",
                    "unsnapshotted",
                )
            ]
        )

        with self.assertRaisesRegex(TraceContractError, "missing exact crossover rows"):
            self.validate_web_trace(path)

    def test_rejects_mislabeled_strategy(self) -> None:
        startup_row = self.record(
            "startup_snapshot_cache",
            "unsnapshotted_runtime_cache",
            "unsnapshotted",
        )
        startup_row["strategy"] = "startup_snapshot_cache"
        path = self.write_trace(
            [
                startup_row,
                self.record("warm_pool", "warm_pool", "unsnapshotted"),
            ]
        )

        with self.assertRaisesRegex(
            TraceContractError,
            "startup_snapshot_cache strategy",
        ):
            self.validate_web_trace(path)

    def test_rejects_unknown_expected_construction_mode(self) -> None:
        path = self.write_trace(
            [
                self.record(
                    "startup_snapshot_cache",
                    "unsnapshotted_runtime_cache",
                    "mixed",
                ),
                self.record("warm_pool", "warm_pool", "mixed"),
            ]
        )

        with self.assertRaisesRegex(
            TraceContractError,
            "unsupported actual construction mode 'mixed'",
        ):
            validate_trace(
                path,
                benchmark_group="runtime_pool_modes_web_selected",
                profile="web_standard",
                workload="hostless_trivial",
                execution_model="cooperative_locker",
                actual_construction_mode="mixed",
                startup_strategy_label="unsnapshotted_runtime_cache",
            )

    def test_rejects_append_contaminated_duplicate_rows(self) -> None:
        startup_row = self.record(
            "startup_snapshot_cache",
            "unsnapshotted_runtime_cache",
            "unsnapshotted",
        )
        warm_row = self.record("warm_pool", "warm_pool", "unsnapshotted")
        path = self.write_trace([startup_row, warm_row, startup_row])

        with self.assertRaisesRegex(
            TraceContractError,
            "duplicate or non-increasing sample row",
        ):
            self.validate_web_trace(path)

    def test_rejects_rows_from_multiple_runs(self) -> None:
        startup_row = self.record(
            "startup_snapshot_cache",
            "unsnapshotted_runtime_cache",
            "unsnapshotted",
            run_id="run-a",
        )
        warm_row = self.record(
            "warm_pool",
            "warm_pool",
            "unsnapshotted",
            run_id="run-b",
        )
        warm_row["measured_iterations"] = 10
        path = self.write_trace([startup_row, warm_row])

        with self.assertRaisesRegex(TraceContractError, "multiple run IDs"):
            self.validate_web_trace(path)

    def test_rejects_a_different_expected_run(self) -> None:
        path = self.write_trace(
            [
                self.record(
                    "startup_snapshot_cache",
                    "unsnapshotted_runtime_cache",
                    "unsnapshotted",
                ),
                self.record("warm_pool", "warm_pool", "unsnapshotted"),
            ]
        )

        with self.assertRaisesRegex(TraceContractError, "expected shared run ID"):
            validate_trace(
                path,
                benchmark_group="runtime_pool_modes_web_selected",
                profile="web_standard",
                workload="hostless_trivial",
                execution_model="cooperative_locker",
                actual_construction_mode="unsnapshotted",
                startup_strategy_label="unsnapshotted_runtime_cache",
                expected_run_id="run-b",
            )


if __name__ == "__main__":
    unittest.main()
