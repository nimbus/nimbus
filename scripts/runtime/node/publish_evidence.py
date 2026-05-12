#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import shutil
from pathlib import Path
from typing import Any


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def default_artifacts_root() -> Path:
    return repo_root() / "target" / "node-compat"


def default_publish_root() -> Path:
    return repo_root() / "docs" / "architecture" / "runtime" / "node-compat-evidence" / "latest"


def load_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def require_file(path: Path) -> Path:
    if not path.is_file():
        raise FileNotFoundError(
            f"missing {path}; run make node-compat-status and make node-compat-dashboard first"
        )
    return path


def display_path(path: Path) -> str:
    try:
        return str(path.relative_to(repo_root()))
    except ValueError:
        return str(path)


def copy_artifact(source: Path, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)


def optional_file(path: Path) -> Path | None:
    return path if path.is_file() else None


def build_index(
    status: dict[str, Any],
    dashboard: dict[str, Any],
    publish_root: Path,
    trend: dict[str, Any] | None,
) -> str:
    lines = [
        "# Node Compatibility Evidence Snapshot",
        "",
        "This directory is the checked-in latest snapshot of the generated Node compatibility evidence outputs.",
        "",
        f"- evidence_generated_at: `{status.get('generated_at', 'unknown')}`",
        f"- publish_root: `{display_path(publish_root)}`",
        f"- status source: `target/node-compat/status/status-summary.json`",
        f"- dashboard source: `target/node-compat/dashboard/dashboard-summary.json`",
        "",
        "## Lane Denominators",
        "",
        "| Lane | Upstream | Vendored test files | Documented passed | Unclassified | Pass rate |",
        "| --- | --- | ---: | ---: | ---: | ---: |",
    ]
    for lane in status["lane_summaries"]:
        ratio = lane["documented_manifested_green_ratio"] * 100
        lines.append(
            f"| `{lane['lane']}` | `{lane['upstream']['tag']}` | "
            f"{lane['vendored_test_file_count']} | "
            f"{lane['documented_manifested_green_count']} | "
            f"{lane['unmanifested_or_unclassified_count']} | "
            f"{ratio:.1f}% |"
        )
    expectation = status["expectation_catalog"]
    lines.extend(
        [
            "",
            "## Expectation Coverage",
            "",
            f"- Rust ignored tests: {status['rust_ignore_count']}",
            f"- catalog entries: {expectation['catalog_entry_count']}",
            f"- catalog path: `{expectation['catalog_path']}`",
            f"- unexpected passes: {len(expectation['unexpected_passes'])}",
            "",
            "## Dashboard Coverage",
            "",
            f"- slice reports: {dashboard['slice_report_count']}",
            f"- canary reports: {dashboard['canary_report_count']}",
            f"- oracle reports: {dashboard['oracle_report_count']}",
            f"- required canary gaps: {len(dashboard['required_canary_gaps'])}",
            "",
            "## Trend Coverage",
            "",
        ]
    )
    if trend is None:
        lines.append("- trend snapshot: unavailable; run `make node-compat-trends` before publishing")
    else:
        lines.extend(
            [
                "- trend snapshot: `trend-summary.json` and `trend-summary.md`",
                f"- baseline available: `{str(trend['baseline_available']).lower()}`",
                f"- lane trend rows: {len(trend['lane_trends'])}",
                f"- evidence trend metrics: {len(trend['evidence_trends'])}",
            ]
        )
    lines.extend(
        [
            "",
            "## Files",
            "",
            "- `status-summary.json` and `status-summary.md` are copied from `make node-compat-status`.",
            "- `dashboard-summary.json` and `dashboard-summary.md` are copied from `make node-compat-dashboard`.",
            "- `trend-summary.json` and `trend-summary.md` are copied from `make node-compat-trends` when present.",
            "",
        ]
    )
    return "\n".join(lines)


def publish(artifacts_root: Path, publish_root: Path) -> list[Path]:
    status_json = require_file(artifacts_root / "status" / "status-summary.json")
    status_md = require_file(artifacts_root / "status" / "status-summary.md")
    dashboard_json = require_file(artifacts_root / "dashboard" / "dashboard-summary.json")
    dashboard_md = require_file(artifacts_root / "dashboard" / "dashboard-summary.md")
    trend_json = optional_file(artifacts_root / "trends" / "trend-summary.json")
    trend_md = optional_file(artifacts_root / "trends" / "trend-summary.md")
    if trend_json is None or trend_md is None:
        trend_json = None
        trend_md = None

    status = load_json(status_json)
    dashboard = load_json(dashboard_json)
    trend = load_json(trend_json) if trend_json is not None else None
    copied = [
        publish_root / "status-summary.json",
        publish_root / "status-summary.md",
        publish_root / "dashboard-summary.json",
        publish_root / "dashboard-summary.md",
    ]
    for source, destination in zip(
        [status_json, status_md, dashboard_json, dashboard_md],
        copied,
        strict=True,
    ):
        copy_artifact(source, destination)
    if trend_json is not None and trend_md is not None:
        for source, destination in (
            (trend_json, publish_root / "trend-summary.json"),
            (trend_md, publish_root / "trend-summary.md"),
        ):
            copy_artifact(source, destination)
            copied.append(destination)
    index_path = publish_root / "README.md"
    index_path.write_text(
        build_index(status, dashboard, publish_root, trend), encoding="utf-8"
    )
    copied.append(index_path)
    return copied


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Publish durable Node compatibility evidence summaries"
    )
    parser.add_argument("--artifacts-root", default=str(default_artifacts_root()))
    parser.add_argument("--publish-root", default=str(default_publish_root()))
    return parser


def main() -> int:
    args = build_parser().parse_args()
    copied = publish(
        artifacts_root=Path(args.artifacts_root).resolve(),
        publish_root=Path(args.publish_root).resolve(),
    )
    for path in copied:
        print(f"published {display_path(path)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
