#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
from pathlib import Path


REQUIRED_CANARY_PACKAGES: tuple[tuple[str, str], ...] = (
    ("express", "Application"),
    ("fastify", "Application"),
    ("socket.io", "Application"),
    ("undici", "Application"),
    ("axios", "Application"),
    ("jest", "Tooling"),
    ("tsx", "Tooling"),
    ("ts-node", "Tooling"),
    ("prisma", "Tooling"),
    ("next", "Tooling"),
)


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def default_artifacts_root() -> Path:
    return repo_root() / "target" / "node-compat"


def default_output_root() -> Path:
    return default_artifacts_root() / "dashboard"


def registry_path() -> Path:
    return repo_root() / "tests" / "runtime" / "node" / "canary-registry.json"


def load_json(path: Path) -> dict:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def required_key(report: dict, path: Path, key: str) -> object:
    if key not in report:
        raise SystemExit(
            f"{path.relative_to(repo_root())} is missing required key {key!r}; "
            "regenerate node-compat artifacts before building the dashboard"
        )
    return report[key]


def discover_slice_reports(artifacts_root: Path) -> list[dict]:
    reports: list[dict] = []
    for path in sorted(artifacts_root.glob("*/*/slice-observed-*.json")):
        report = load_json(path)
        reports.append(
            {
                "path": str(path.relative_to(repo_root())),
                "family": report["family"],
                "slice": report["slice"],
                "nlc_item": report["nlc_item"],
                "execution_class": report["execution_class"],
                "presets": required_key(report, path, "presets"),
                "capabilities": report["capabilities"],
                "counts": report["slice_summary"]["counts"],
                "total_expected_results": report["slice_summary"][
                    "total_expected_results"
                ],
                "total_observed_results": report["slice_summary"][
                    "total_observed_results"
                ],
                "lane_summaries": report["lane_summaries"],
            }
        )
    return reports


def discover_canary_reports(artifacts_root: Path) -> list[dict]:
    reports: list[dict] = []
    for path in sorted((artifacts_root / "canaries").glob("preset-*.json")):
        report = load_json(path)
        reports.append(
            {
                "path": str(path.relative_to(repo_root())),
                "runtime_preset": report["runtime_preset"],
                "canary_count": report["canary_count"],
                "passed": report["passed"],
                "failed": report["failed"],
                "lane_summaries": report["lane_summaries"],
                "canary_results": report["canary_results"],
            }
        )
    return reports


def discover_oracle_reports(artifacts_root: Path) -> list[dict]:
    reports: list[dict] = []
    oracle_root = artifacts_root / "oracle"
    if not oracle_root.is_dir():
        return reports
    for path in sorted(oracle_root.glob("**/oracle-*.json")):
        report = load_json(path)
        reports.append(
            {
                "path": str(path.relative_to(repo_root())),
                "family": report["family"],
                "slice": report["slice"],
                "lane": report["lane"],
                "upstream_fixture_line": report["upstream_fixture_line"],
                "lane_role": report["lane_role"],
                "public_contract_role": report["public_contract_role"],
                "runtime_execution_target": report["runtime_execution_target"],
                "runtime_limits_preset": report["runtime_limits_preset"],
                "fixture": report["fixture"],
                "runtime_state": report["runtime_state"],
                "oracle_state": report["oracle_state"],
                "drift_class": report["drift_class"],
                "node_version": report["node_version"],
            }
        )
    return reports


def discover_suite_status_report(artifacts_root: Path) -> dict | None:
    path = artifacts_root / "status" / "status-summary.json"
    if not path.is_file():
        path = artifacts_root / "status-summary.json"
    if not path.is_file():
        return None
    report = load_json(path)
    return {
        "path": str(path.relative_to(repo_root())),
        "lane_summaries": report["lane_summaries"],
        "rust_ignore_count": report.get("rust_ignore_count", 0),
        "warnings": report["warnings"],
    }


def discover_inventory_reports(artifacts_root: Path) -> list[dict]:
    reports: list[dict] = []
    for path in sorted((artifacts_root / "inventory").glob("*-inventory.json")):
        report = load_json(path)
        reports.append(
            {
                "path": str(path.relative_to(repo_root())),
                "lane": report["lane"],
                "upstream": report["upstream"],
                "counts": report["counts"],
                "warnings": report["warnings"],
            }
        )
    return reports


def build_claim_summaries(registry: dict, canary_reports: list[dict]) -> list[dict]:
    claim_summaries: list[dict] = []
    observed_by_claim: dict[str, list[dict]] = {}
    for report in canary_reports:
        for result in report["canary_results"]:
            for claim_id in result["claim_ids"]:
                observed_by_claim.setdefault(claim_id, []).append(result)

    for claim in registry["claims"]:
        observed = observed_by_claim.get(claim["id"], [])
        observed_lanes = sorted({result["lane"] for result in observed})
        observed_lane_metadata = sorted(
            {
                (
                    result["lane"],
                    result["upstream_fixture_line"],
                    result["lane_role"],
                    result["public_contract_role"],
                )
                for result in observed
            }
        )
        observed_statuses = {result["status"] for result in observed}
        missing_lanes = sorted(set(claim["lane_coverage"]) - set(observed_lanes))
        if missing_lanes:
            summary_status = "missing_observation"
        elif "fail" in observed_statuses:
            summary_status = "failed"
        elif observed:
            summary_status = "passed"
        else:
            summary_status = "missing_observation"
        claim_summaries.append(
            {
                "id": claim["id"],
                "package": claim["package"],
                "runtime_preset": claim["runtime_preset"],
                "lane_coverage": claim["lane_coverage"],
                "nlc_family": claim["nlc_family"],
                "status": summary_status,
                "missing_lanes": missing_lanes,
                "observed_lane_metadata": [
                    {
                        "lane": lane,
                        "upstream_fixture_line": upstream_fixture_line,
                        "lane_role": lane_role,
                        "public_contract_role": public_contract_role,
                    }
                    for (
                        lane,
                        upstream_fixture_line,
                        lane_role,
                        public_contract_role,
                    ) in observed_lane_metadata
                ],
                "observed_results": observed,
            }
        )
    return claim_summaries


def build_required_canary_gaps(registry: dict) -> list[dict]:
    active_claim_pairs = {
        (claim["package"], claim["runtime_preset"]) for claim in registry["claims"]
    }
    gaps: list[dict] = []
    for package, runtime_preset in REQUIRED_CANARY_PACKAGES:
        if (package, runtime_preset) not in active_claim_pairs:
            gaps.append(
                {
                    "package": package,
                    "runtime_preset": runtime_preset,
                    "status": "missing_registry_claim",
                }
            )
    return gaps


def build_dashboard_summary(artifacts_root: Path) -> dict:
    registry = load_json(registry_path())
    slice_reports = discover_slice_reports(artifacts_root)
    canary_reports = discover_canary_reports(artifacts_root)
    oracle_reports = discover_oracle_reports(artifacts_root)
    suite_status_report = discover_suite_status_report(artifacts_root)
    inventory_reports = discover_inventory_reports(artifacts_root)
    claim_summaries = build_claim_summaries(registry, canary_reports)
    required_canary_gaps = build_required_canary_gaps(registry)
    canary_check_count = sum(
        len(report["canary_results"]) for report in canary_reports
    )

    return {
        "schema_version": 1,
        "artifacts_root": str(artifacts_root.relative_to(repo_root())),
        "slice_report_count": len(slice_reports),
        "canary_report_count": len(canary_reports),
        "canary_claim_count": len(claim_summaries),
        "canary_check_count": canary_check_count,
        "oracle_report_count": len(oracle_reports),
        "inventory_report_count": len(inventory_reports),
        "suite_status_report": suite_status_report,
        "inventory_reports": inventory_reports,
        "slice_reports": slice_reports,
        "canary_reports": canary_reports,
        "oracle_reports": oracle_reports,
        "claim_summaries": claim_summaries,
        "required_canary_gaps": required_canary_gaps,
    }


def build_markdown(summary: dict) -> str:
    def render_lane_summary(summary: dict) -> str:
        return (
            f"{summary['lane']}:{summary['upstream_fixture_line']}/"
            f"{summary['lane_role']}/{summary['public_contract_role']}"
        )

    def label(value: str) -> str:
        labels = {
            "pass": "Passed",
            "passed": "Passed",
            "fail": "Failed",
            "failed": "Failed",
            "skip": "Skipped",
            "skipped": "Skipped",
        }
        return labels.get(value, value.replace("_", " ").capitalize())

    lines = [
        "# Node.js Runtime Support Dashboard",
        "",
        f"- Representative Node test checks: {summary['slice_report_count']}",
        f"- Package/framework canary claims: {summary['canary_claim_count']}",
        f"- Package/framework canary checks: {summary['canary_check_count']}",
        f"- Canary artifact bundles: {summary['canary_report_count']}",
        f"- Oracle reports: {summary['oracle_report_count']}",
        f"- Inventory reports: {summary['inventory_report_count']}",
        "",
        "## Suite Status",
    ]
    if summary["suite_status_report"] is None:
        lines.append("- none; run `make node-compat-status` before `make node-compat-dashboard`")
    else:
        status = summary["suite_status_report"]
        lines.append(f"- source: `{status['path']}`")
        lines.append(f"- rust ignored tests: `{status['rust_ignore_count']}`")
        lines.extend(
            [
                "",
                "| Lane | Upstream | Role | Passed | Expected failure / known gap | Skipped / excluded | Classified total | Classified coverage count | Vendored | Unclassified | Pass rate |",
                "| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
            ]
        )
        for lane in status["lane_summaries"]:
            ratio = lane["documented_manifested_green_ratio"] * 100
            lines.append(
                f"| `{lane['lane']}` | `{lane['upstream']['tag']}` | "
                f"`{lane['lane_role']}` | "
                f"{lane['documented_manifested_green_count']} | "
                f"{lane['known_red_or_gap_count']} | "
                f"{lane['skipped_or_excluded_count']} | "
                f"{lane['classified_total_count']} | "
                f"{lane['documented_or_classified_count']} | "
                f"{lane['vendored_test_file_count']} | "
                f"{lane['unmanifested_or_unclassified_count']} | "
                f"{ratio:.1f}% |"
            )
        lines.extend(["", "### Suite Warnings"])
        if status["warnings"]:
            lines.extend(["", "| Kind | Details |", "| --- | --- |"])
            for warning in status["warnings"]:
                lines.append(
                    f"| `{warning['kind']}` | "
                    f"`{json.dumps(warning, sort_keys=True)}` |"
                )
        else:
            lines.append("- none")
    lines.extend(
        [
            "",
            "## Fixture Inventory",
            "",
            "| Lane | Upstream | Vendored | Passed | Expected failure / known gap / skipped total | Classified coverage count | Unclassified | Path-owned passed | Rust-referenced passed | Rust-unreferenced expected / skipped | Rust-unreferenced unclassified | Passed reconstructability gap | Warnings |",
            "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
        ]
    )
    if summary["inventory_reports"]:
        for report in summary["inventory_reports"]:
            counts = report["counts"]
            lines.append(
                f"| `{report['lane']}` | `{report['upstream']['tag']}` | "
                f"{counts['vendored_test_file_count']} | "
                f"{counts['documented_manifested_green_count']} | "
                f"{counts['classified_red_skip_count']} | "
                f"{counts['documented_or_classified_count']} | "
                f"{counts['documented_unmanifested_or_unclassified_count']} | "
                f"{counts['path_owned_green_test_count']} | "
                f"{counts['rust_referenced_test_file_count']} | "
                f"{counts['rust_unreferenced_classified_red_skip_count']} | "
                f"{counts['rust_unreferenced_unclassified_count']} | "
                f"{counts['documented_green_reconstructability_gap_count']} | "
                f"{len(report['warnings'])} |"
            )
    else:
        lines.append("| none | - | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |")
    lines.extend(
        [
            "",
            "## Representative Node Test Checks",
            "",
            "| API family | Check | NLC | Execution | Passed | Skipped | Failed | Missing | Lanes |",
            "| --- | --- | --- | --- | ---: | ---: | ---: | ---: | --- |",
        ]
    )
    for report in summary["slice_reports"]:
        counts = report["counts"]
        lane_details = ", ".join(
            render_lane_summary(lane_summary) for lane_summary in report["lane_summaries"]
        )
        lines.append(
            f"| `{report['family']}` | `{report['slice']}` | "
            f"`{report['nlc_item']}` | "
            f"{label(report['execution_class'])} | "
            f"{counts['passed']} | {counts['skipped']} | "
            f"{counts['failed']} | {counts['missing']} | {lane_details} |"
        )
    lines.extend(
        [
            "",
            "## Package/Framework Canaries",
            "",
            "| Claim | Preset | Status | Required lanes | Observed lanes |",
            "| --- | --- | --- | --- | --- |",
        ]
    )
    for claim in summary["claim_summaries"]:
        lane_details = ", ".join(
            render_lane_summary(lane_summary)
            for lane_summary in claim["observed_lane_metadata"]
        )
        lines.append(
            f"| `{claim['id']}` | `{claim['runtime_preset']}` | "
            f"{label(claim['status'])} | {', '.join(claim['lane_coverage'])} | "
            f"{lane_details or 'none'} |"
        )
    lines.extend(["", "## Required Canary Gaps"])
    if summary["required_canary_gaps"]:
        lines.extend(["", "| Package | Preset | Status |", "| --- | --- | --- |"])
        for gap in summary["required_canary_gaps"]:
            lines.append(
                f"| `{gap['package']}` | `{gap['runtime_preset']}` | "
                f"{label(gap['status'])} |"
            )
    else:
        lines.append("- none")
    lines.extend(["", "## Oracle Reports"])
    if summary["oracle_reports"]:
        lines.extend(
            [
                "",
                "| Lane | Fixture | Runtime | Oracle | Drift | Node | Role |",
                "| --- | --- | --- | --- | --- | --- | --- |",
            ]
        )
        for report in summary["oracle_reports"]:
            lines.append(
                f"| `{report['lane']}` | `{report['fixture']}` | "
                f"{label(report['runtime_state'])} | {label(report['oracle_state'])} | "
                f"{label(report['drift_class'])} | `{report['node_version']}` | "
                f"`{report['lane_role']}/{report['public_contract_role']}` |"
            )
    else:
        lines.append("- none")
    lines.append("")
    return "\n".join(lines)


def write_outputs(summary: dict, output_root: Path) -> tuple[Path, Path]:
    output_root.mkdir(parents=True, exist_ok=True)
    summary_path = output_root / "dashboard-summary.json"
    markdown_path = output_root / "dashboard-summary.md"
    summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    markdown_path.write_text(build_markdown(summary), encoding="utf-8")
    return summary_path, markdown_path


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Aggregate node-compat reports, canaries, and oracle artifacts"
    )
    parser.add_argument("--artifacts-root", default=str(default_artifacts_root()))
    parser.add_argument("--output-root", default=str(default_output_root()))
    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    artifacts_root = Path(args.artifacts_root).resolve()
    output_root = Path(args.output_root).resolve()
    summary = build_dashboard_summary(artifacts_root)
    summary_path, markdown_path = write_outputs(summary, output_root)
    print(f"wrote dashboard summary to {summary_path}")
    print(f"wrote dashboard markdown to {markdown_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
