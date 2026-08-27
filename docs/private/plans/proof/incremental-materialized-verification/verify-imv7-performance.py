#!/usr/bin/env python3
"""Validate the final IMV performance matrix and production candidate proof."""

from __future__ import annotations

import itertools
import json
import math
import sys
from pathlib import Path
from typing import Any


DECISIVE_COORDINATE = (100_000, 1_024, 10)
MILLION_COORDINATE = (1_000_000, 1_024, 10)
EXPECTED_COORDINATES = set(
    itertools.product(
        (10_000, 100_000, 1_000_000),
        (256, 1_024, 8 * 1_024),
        (0, 10, 100, 1_000),
    )
)
CANDIDATE_LIMITS_NS = {
    100_000: 1_000_000_000,
    1_000_000: 60_000_000_000,
}
CANDIDATE_SAMPLES = 21
MAX_RESIDENT_BYTES_PER_LEAF = 192


class ValidationError(Exception):
    """A proof artifact does not satisfy the IMV7 contract."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValidationError(message)


def require_int(value: Any, label: str, *, minimum: int = 0) -> int:
    require(type(value) is int, f"{label} must be an integer")
    require(value >= minimum, f"{label} must be at least {minimum}")
    return value


def require_number(value: Any, label: str) -> float:
    require(
        type(value) in (int, float) and math.isfinite(value),
        f"{label} must be a finite number",
    )
    return float(value)


def percentile(samples: list[int], percent: int) -> int:
    require(samples, "percentile samples must not be empty")
    ordered = sorted(samples)
    index = ((len(ordered) - 1) * percent + 99) // 100
    return ordered[index]


def validate_summary(summary: Any, samples: list[int], label: str) -> None:
    require(type(summary) is dict, f"{label} summary must be an object")
    expected = {
        "sample_count": len(samples),
        "p50_ns": percentile(samples, 50),
        "p95_ns": percentile(samples, 95),
        "p99_ns": percentile(samples, 99),
    }
    require(summary == expected, f"{label} summary does not match its raw samples")


def load_json(path: Path, label: str) -> dict[str, Any]:
    with path.open(encoding="utf-8") as artifact:
        value = json.load(artifact)
    require(type(value) is dict, f"{label} must contain a JSON object")
    return value


def coordinate(row: dict[str, Any]) -> tuple[int, int, int]:
    return (
        require_int(row.get("documents"), "matrix documents", minimum=1),
        require_int(row.get("payload_bytes"), "matrix payload_bytes", minimum=1),
        require_int(
            row.get("churn_basis_points"),
            "matrix churn_basis_points",
        ),
    )


def find_matrix_row(
    rows: list[dict[str, Any]],
    expected: tuple[int, int, int],
) -> dict[str, Any]:
    matches = [row for row in rows if coordinate(row) == expected]
    require(len(matches) == 1, f"matrix must contain coordinate {expected} exactly once")
    return matches[0]


def validate_full_matrix(report: dict[str, Any]) -> tuple[int, int, float, float]:
    require(report.get("format_version") == 2, "full matrix format_version must be 2")
    require(report.get("interval_seconds") == 60, "verification interval must be 60 seconds")
    rows = report.get("matrix")
    require(type(rows) is list and len(rows) == 36, "full matrix must contain 36 rows")
    require(all(type(row) is dict for row in rows), "every matrix row must be an object")
    actual = [coordinate(row) for row in rows]
    require(len(set(actual)) == len(actual), "full matrix coordinates must be unique")
    require(set(actual) == EXPECTED_COORDINATES, "full matrix coordinates are incomplete")

    decisive = find_matrix_row(rows, DECISIVE_COORDINATE)
    require(decisive.get("churn_setup_status") == "measured", "decisive churn must be measured")
    full = decisive.get("full")
    require(type(full) is dict, "decisive full measurement must be an object")
    require(full.get("status") == "measured", "decisive full status must be measured")
    samples = full.get("samples")
    require(type(samples) is list and samples, "decisive full samples must not be empty")
    elapsed_samples: list[int] = []
    extra_rss_samples: list[int] = []
    for index, sample in enumerate(samples):
        require(type(sample) is dict, f"decisive full sample {index} must be an object")
        elapsed_samples.append(
            require_int(sample.get("elapsed_ns"), f"decisive sample {index} elapsed_ns", minimum=1)
        )
        extra_rss_samples.append(
            require_int(
                sample.get("extra_peak_rss_bytes"),
                f"decisive sample {index} extra_peak_rss_bytes",
                minimum=1,
            )
        )
        require(sample.get("report_ok") is True, f"decisive sample {index} must report success")
        require(sample.get("mismatch_count") == 0, f"decisive sample {index} must have no mismatch")
        require(
            sample.get("authoritative_document_count") == DECISIVE_COORDINATE[0],
            f"decisive sample {index} has the wrong document count",
        )
    validate_summary(full.get("summary"), elapsed_samples, "decisive full")
    require(full.get("timed_out_samples") == 0, "decisive full measurement must not be censored")
    require(full.get("failures") == [], "decisive full measurement must not contain failures")
    full_p95 = percentile(elapsed_samples, 95)
    extra_rss_p95 = percentile(extra_rss_samples, 95)
    require(
        full_p95 > 1_000_000_000 or extra_rss_p95 > 256 * 1024 * 1024,
        "decisive full measurement no longer justifies the accepted candidate branch",
    )

    million = find_matrix_row(rows, MILLION_COORDINATE)
    require(million.get("churn_setup_status") == "measured", "million-rung churn must be measured")

    write = report.get("write_overhead")
    require(type(write) is dict, "write_overhead must be measured")
    throughput_change = require_number(
        write.get("throughput_change_percent"),
        "write throughput_change_percent",
    )
    latency_change = require_number(
        write.get("p99_commit_latency_change_percent"),
        "write p99_commit_latency_change_percent",
    )
    require(throughput_change >= -5, "candidate degrades write throughput by more than 5 percent")
    require(latency_change <= 5, "candidate increases p99 commit latency by more than 5 percent")
    return full_p95, extra_rss_p95, throughput_change, latency_change


def validate_candidate(report: dict[str, Any]) -> dict[int, tuple[int, int]]:
    require(report.get("format_version") == 1, "candidate format_version must be 1")
    require(
        report.get("measurement") == "production_materialized_verification_index",
        "candidate must measure the production materialized verification index",
    )
    require(
        report.get("samples_per_rung") == CANDIDATE_SAMPLES,
        f"candidate must retain {CANDIDATE_SAMPLES} samples per rung",
    )
    rungs = report.get("rungs")
    require(type(rungs) is list and len(rungs) == 2, "candidate must contain two decisive rungs")
    require(all(type(rung) is dict for rung in rungs), "every candidate rung must be an object")
    by_documents = {rung.get("documents"): rung for rung in rungs}
    require(set(by_documents) == set(CANDIDATE_LIMITS_NS), "candidate rungs must be 100k and 1m")

    measurements: dict[int, tuple[int, int]] = {}
    for documents, latency_limit_ns in CANDIDATE_LIMITS_NS.items():
        rung = by_documents[documents]
        label = f"candidate {documents}"
        require(rung.get("payload_bytes") == 1_024, f"{label} payload must be 1 KiB")
        require(rung.get("churn_basis_points") == 10, f"{label} churn must be 0.1 percent")
        expected_churn = (documents * 10 + 9_999) // 10_000
        require(rung.get("churn_documents") == expected_churn, f"{label} churn count is wrong")
        require(rung.get("status") == "measured", f"{label} status must be measured")
        samples = rung.get("samples_ns")
        require(
            type(samples) is list and len(samples) == CANDIDATE_SAMPLES,
            f"{label} must contain {CANDIDATE_SAMPLES} samples",
        )
        measured_samples = [
            require_int(sample, f"{label} sample {index}", minimum=1)
            for index, sample in enumerate(samples)
        ]
        validate_summary(rung.get("summary"), measured_samples, label)
        candidate_p95 = percentile(measured_samples, 95)
        require(
            candidate_p95 <= latency_limit_ns,
            f"{label} p95 exceeds the absolute {latency_limit_ns} ns limit",
        )

        require(rung.get("leaf_count") == documents, f"{label} leaf count is wrong")
        require(
            rung.get("resident_bytes_status") == "measured",
            f"{label} resident bytes status must be measured",
        )
        require(
            rung.get("memory_source") == "MaterializedVerificationIndex::resident_bytes",
            f"{label} must use the production resident-byte measurement",
        )
        resident_bytes = require_int(
            rung.get("resident_bytes"),
            f"{label} resident_bytes",
            minimum=1,
        )
        measured_per_leaf = (resident_bytes + documents - 1) // documents
        require(
            rung.get("resident_bytes_per_leaf") == measured_per_leaf,
            f"{label} resident bytes per leaf does not match the measured total",
        )
        require(
            resident_bytes <= documents * MAX_RESIDENT_BYTES_PER_LEAF,
            f"{label} resident bytes exceed the absolute total",
        )
        measurements[documents] = (candidate_p95, resident_bytes)
    return measurements


def main() -> int:
    if len(sys.argv) != 3:
        print(
            "usage: verify-imv7-performance.py FULL_MATRIX CANDIDATE_PROOF",
            file=sys.stderr,
        )
        return 2
    try:
        full = load_json(Path(sys.argv[1]), "full matrix")
        candidate = load_json(Path(sys.argv[2]), "candidate proof")
        full_p95, rss_p95, throughput, commit_latency = validate_full_matrix(full)
        measurements = validate_candidate(candidate)
    except Exception as error:  # A proof gate must fail closed without a traceback.
        print(f"IMV7 performance proof invalid: {error}", file=sys.stderr)
        return 1

    decisive_p95, decisive_bytes = measurements[100_000]
    million_p95, million_bytes = measurements[1_000_000]
    print(
        "IMV7 performance proof valid: "
        f"full_p95_ns={full_p95} full_extra_rss_p95={rss_p95} "
        f"candidate_100k_p95_ns={decisive_p95} candidate_100k_bytes={decisive_bytes} "
        f"candidate_1m_p95_ns={million_p95} candidate_1m_bytes={million_bytes} "
        f"throughput_change_percent={throughput:.6f} "
        f"p99_commit_latency_change_percent={commit_latency:.6f}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
