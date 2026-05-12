#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from schema import default_schema_path, validate_payload_against_schema


TREND_SCHEMA_PATH = default_schema_path("trend-snapshot.schema.json")


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def default_artifacts_root() -> Path:
    return repo_root() / "target" / "node-compat"


def default_baseline_root() -> Path:
    return repo_root() / "docs" / "architecture" / "runtime" / "node-compat-evidence" / "latest"


def default_output_root() -> Path:
    return default_artifacts_root() / "trends"


def display_path(path: Path) -> str:
    try:
        return str(path.relative_to(repo_root()))
    except ValueError:
        return str(path)


def load_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        payload = json.load(handle)
    if not isinstance(payload, dict):
        raise ValueError(f"{display_path(path)} must contain a JSON object")
    return payload


def load_optional_json(path: Path) -> dict[str, Any] | None:
    if not path.is_file():
        return None
    return load_json(path)


def metric_delta(current: int | float, baseline: int | float | None) -> int | float | None:
    if baseline is None:
        return None
    return current - baseline


def lane_summary_by_id(status: dict[str, Any] | None) -> dict[str, dict[str, Any]]:
    if status is None:
        return {}
    return {
        lane["lane"]: lane
        for lane in status.get("lane_summaries", [])
        if isinstance(lane, dict) and isinstance(lane.get("lane"), str)
    }


def build_lane_trends(
    current_status: dict[str, Any],
    baseline_status: dict[str, Any] | None,
) -> list[dict[str, Any]]:
    baseline_by_lane = lane_summary_by_id(baseline_status)
    lane_trends: list[dict[str, Any]] = []
    for current in current_status["lane_summaries"]:
        lane = current["lane"]
        baseline = baseline_by_lane.get(lane)
        current_ratio = current["documented_manifested_green_ratio"]
        baseline_ratio = (
            baseline["documented_manifested_green_ratio"] if baseline else None
        )
        lane_trends.append(
            {
                "lane": lane,
                "upstream_tag": current["upstream"]["tag"],
                "baseline_upstream_tag": baseline["upstream"]["tag"] if baseline else None,
                "vendored_test_file_count": current["vendored_test_file_count"],
                "vendored_test_file_count_delta": metric_delta(
                    current["vendored_test_file_count"],
                    baseline["vendored_test_file_count"] if baseline else None,
                ),
                "documented_manifested_green_count": current[
                    "documented_manifested_green_count"
                ],
                "documented_manifested_green_count_delta": metric_delta(
                    current["documented_manifested_green_count"],
                    baseline["documented_manifested_green_count"] if baseline else None,
                ),
                "unmanifested_or_unclassified_count": current[
                    "unmanifested_or_unclassified_count"
                ],
                "unmanifested_or_unclassified_count_delta": metric_delta(
                    current["unmanifested_or_unclassified_count"],
                    baseline["unmanifested_or_unclassified_count"] if baseline else None,
                ),
                "documented_manifested_green_ratio": current_ratio,
                "documented_manifested_green_ratio_delta": metric_delta(
                    current_ratio,
                    baseline_ratio,
                ),
            }
        )
    return lane_trends


def dashboard_metrics(dashboard: dict[str, Any] | None) -> dict[str, int]:
    if dashboard is None:
        return {}
    return {
        "slice_report_count": dashboard.get("slice_report_count", 0),
        "canary_report_count": dashboard.get("canary_report_count", 0),
        "oracle_report_count": dashboard.get("oracle_report_count", 0),
        "required_canary_gap_count": len(dashboard.get("required_canary_gaps", [])),
    }


def status_metrics(status: dict[str, Any] | None) -> dict[str, int]:
    if status is None:
        return {}
    expectation_catalog = status.get("expectation_catalog", {})
    return {
        "rust_ignore_count": status.get("rust_ignore_count", 0),
        "expectation_catalog_entry_count": expectation_catalog.get(
            "catalog_entry_count", 0
        ),
        "unexpected_pass_count": len(expectation_catalog.get("unexpected_passes", [])),
        "warning_count": len(status.get("warnings", [])),
    }


def build_evidence_trends(
    current_status: dict[str, Any],
    current_dashboard: dict[str, Any],
    baseline_status: dict[str, Any] | None,
    baseline_dashboard: dict[str, Any] | None,
) -> dict[str, Any]:
    current = {
        **status_metrics(current_status),
        **dashboard_metrics(current_dashboard),
    }
    baseline = {
        **status_metrics(baseline_status),
        **dashboard_metrics(baseline_dashboard),
    }
    return {
        key: {
            "current": value,
            "baseline": baseline.get(key),
            "delta": metric_delta(value, baseline.get(key)),
        }
        for key, value in sorted(current.items())
    }


def build_report(
    artifacts_root: Path,
    baseline_root: Path,
) -> dict[str, Any]:
    current_status_path = artifacts_root / "status" / "status-summary.json"
    current_dashboard_path = artifacts_root / "dashboard" / "dashboard-summary.json"
    baseline_status_path = baseline_root / "status-summary.json"
    baseline_dashboard_path = baseline_root / "dashboard-summary.json"

    current_status = load_json(current_status_path)
    current_dashboard = load_json(current_dashboard_path)
    baseline_status = load_optional_json(baseline_status_path)
    baseline_dashboard = load_optional_json(baseline_dashboard_path)
    baseline_available = baseline_status is not None and baseline_dashboard is not None

    return {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "report_kind": "node_compat_trend_snapshot",
        "baseline_available": baseline_available,
        "current_sources": {
            "status": display_path(current_status_path),
            "dashboard": display_path(current_dashboard_path),
        },
        "baseline_sources": {
            "status": display_path(baseline_status_path) if baseline_status else None,
            "dashboard": (
                display_path(baseline_dashboard_path) if baseline_dashboard else None
            ),
        },
        "lane_trends": build_lane_trends(current_status, baseline_status),
        "evidence_trends": build_evidence_trends(
            current_status,
            current_dashboard,
            baseline_status,
            baseline_dashboard,
        ),
    }


def signed_delta(value: int | float | None, *, ratio: bool = False) -> str:
    if value is None:
        return "n/a"
    scaled = value * 100 if ratio else value
    if isinstance(scaled, float):
        return f"{scaled:+.1f}" if ratio else f"{scaled:+.3f}"
    return f"{scaled:+d}"


def build_markdown(report: dict[str, Any]) -> str:
    lines = [
        "# Node Compatibility Trend Snapshot",
        "",
        f"- baseline available: `{str(report['baseline_available']).lower()}`",
        f"- current status: `{report['current_sources']['status']}`",
        f"- current dashboard: `{report['current_sources']['dashboard']}`",
        "",
        "## Lane Trends",
        "",
        "| Lane | Upstream | Passed | Passed Delta | Pass Rate | Pass Rate Delta Points | Unclassified | Unclassified Delta |",
        "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for lane in report["lane_trends"]:
        ratio = lane["documented_manifested_green_ratio"] * 100
        lines.append(
            f"| `{lane['lane']}` | `{lane['upstream_tag']}` | "
            f"{lane['documented_manifested_green_count']} | "
            f"{signed_delta(lane['documented_manifested_green_count_delta'])} | "
            f"{ratio:.1f}% | "
            f"{signed_delta(lane['documented_manifested_green_ratio_delta'], ratio=True)} | "
            f"{lane['unmanifested_or_unclassified_count']} | "
            f"{signed_delta(lane['unmanifested_or_unclassified_count_delta'])} |"
        )
    lines.extend(
        [
            "",
            "## Evidence Trends",
            "",
            "| Metric | Current | Baseline | Delta |",
            "| --- | ---: | ---: | ---: |",
        ]
    )
    for metric, trend in report["evidence_trends"].items():
        baseline = "n/a" if trend["baseline"] is None else str(trend["baseline"])
        lines.append(
            f"| `{metric}` | {trend['current']} | {baseline} | "
            f"{signed_delta(trend['delta'])} |"
        )
    lines.append("")
    return "\n".join(lines)


def write_outputs(report: dict[str, Any], output_root: Path) -> tuple[Path, Path]:
    output_root.mkdir(parents=True, exist_ok=True)
    json_path = output_root / "trend-summary.json"
    markdown_path = output_root / "trend-summary.md"
    json_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    markdown_path.write_text(build_markdown(report), encoding="utf-8")
    return json_path, markdown_path


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Build Node compatibility trend snapshots from current and published evidence"
    )
    parser.add_argument("--artifacts-root", default=str(default_artifacts_root()))
    parser.add_argument("--baseline-root", default=str(default_baseline_root()))
    parser.add_argument("--output-root", default=str(default_output_root()))
    return parser


def main() -> int:
    args = build_parser().parse_args()
    report = build_report(
        artifacts_root=Path(args.artifacts_root).resolve(),
        baseline_root=Path(args.baseline_root).resolve(),
    )
    errors = validate_payload_against_schema(report, TREND_SCHEMA_PATH)
    if errors:
        for error in errors:
            print(f"error: {json.dumps(error, sort_keys=True)}")
        return 1
    json_path, markdown_path = write_outputs(report, Path(args.output_root).resolve())
    print(f"wrote node-compat trend summary to {json_path}")
    print(f"wrote node-compat trend markdown to {markdown_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
