#!/usr/bin/env python3
"""Publish user-facing Node.js runtime evidence docs from checked-in evidence."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def default_evidence_root() -> Path:
    return repo_root() / "docs" / "architecture" / "runtime" / "node-compat-evidence" / "latest"


def default_output_root() -> Path:
    return repo_root() / "docs" / "runtimes" / "nodejs" / "evidence"


def lane_registry_path() -> Path:
    return (
        repo_root()
        / "docs"
        / "architecture"
        / "runtime"
        / "node-lts-compat"
        / "node-lts-lanes.json"
    )


def load_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as file:
        return json.load(file)


def percent(value: float | int | None) -> str:
    if value is None:
        return "n/a"
    return f"{value * 100:.1f}%"


def count_percent(numerator: int, denominator: int) -> str:
    if denominator == 0:
        return "n/a"
    return f"{(numerator / denominator) * 100:.1f}%"


def lane_title(lane: str) -> str:
    if lane.startswith("node"):
        return f"Node{lane.removeprefix('node')}"
    return lane


def role_label(role: str) -> str:
    labels = {
        "default": "Default",
        "supported": "Supported",
        "legacy": "Legacy",
        "preview": "Preview",
        "validation": "Validation",
    }
    return labels.get(role, role.replace("_", " ").title())


def support_phase_label(phase: str) -> str:
    labels = {
        "eol_legacy": "EOL legacy",
        "maintenance_lts": "Maintenance LTS",
        "active_lts": "Active LTS",
        "preview_current": "Current preview",
    }
    return labels.get(phase, phase.replace("_", " ").title())


def evidence_policy_label(policy: str) -> str:
    labels = {
        "legacy_grace_regression_only": "legacy-grace regression only",
        "supported_lts_lane_local_evidence": "lane-local LTS evidence",
        "preview_no_enterprise_support_until_lts_and_evidence": "preview; no enterprise support until LTS evidence exists",
    }
    return labels.get(policy, policy.replace("_", " "))


def lane_registry_by_name() -> dict[str, dict[str, Any]]:
    registry = load_json(lane_registry_path())
    return {
        lane["lane_name"]: lane
        for lane in registry.get("lanes", [])
        if isinstance(lane, dict) and isinstance(lane.get("lane_name"), str)
    }


def registry_lane(lane: dict[str, Any], registry: dict[str, dict[str, Any]]) -> dict[str, Any]:
    return registry.get(str(lane.get("lane")), {})


def public_lane_role_label(lane: dict[str, Any], registry: dict[str, dict[str, Any]]) -> str:
    metadata = registry_lane(lane, registry)
    support_phase = metadata.get("support_phase")
    if metadata.get("product_default") is True:
        return f"Product default; {support_phase_label(str(support_phase))}"
    if support_phase == "eol_legacy":
        return "Legacy grace; EOL"
    if support_phase in {"maintenance_lts", "active_lts"}:
        return f"Supported; {support_phase_label(str(support_phase))}"
    return role_label(str(lane.get("lane_role", "")))


def status_label(status: str) -> str:
    labels = {
        "pass": "Passed",
        "passed": "Passed",
        "fail": "Failed",
        "failed": "Failed",
        "skip": "Skipped",
        "skipped": "Skipped",
    }
    return labels.get(status, status.replace("_", " ").title())


def expectation_label(expectation: str) -> str:
    labels = {
        "expected_failure": "Expected failure",
        "expected_gap": "Known gap",
        "expected_skip": "Skipped / excluded",
    }
    return labels.get(expectation, expectation.replace("_", " ").title())


def lane_summaries(status: dict[str, Any]) -> list[dict[str, Any]]:
    return list(status.get("lane_summaries", []))


def canary_results(dashboard: dict[str, Any]) -> list[dict[str, Any]]:
    results: list[dict[str, Any]] = []
    for report in dashboard.get("canary_reports", []):
        results.extend(report.get("canary_results", []))
    return results


def write(path: Path, lines: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")


def lane_row(lane: dict[str, Any], registry: dict[str, dict[str, Any]]) -> str:
    vendored = int(lane.get("vendored_test_file_count", 0))
    documented = int(lane.get("documented_manifested_green_count", 0))
    classified_total = int(lane.get("documented_or_classified_count", 0))
    return (
        f"| {lane_title(lane['lane'])} | {public_lane_role_label(lane, registry)} | "
        f"`{lane.get('upstream', {}).get('tag', 'unknown')}` | {vendored} | "
        f"{documented} | {lane.get('known_red_or_gap_count', 0)} | "
        f"{lane.get('skipped_or_excluded_count', 0)} | "
        f"{lane.get('unmanifested_or_unclassified_count', 0)} | "
        f"{percent(lane.get('documented_manifested_green_ratio'))} | "
        f"{count_percent(classified_total, vendored)} |"
    )


def latest_lines(
    status: dict[str, Any],
    dashboard: dict[str, Any],
    trends: dict[str, Any] | None,
) -> list[str]:
    registry = lane_registry_by_name()
    lines = [
        "# Node.js Runtime Evidence",
        "",
        "This page is generated from the checked-in Node.js runtime support evidence snapshots.",
        "It is a support summary, not a blanket Node.js compatibility claim.",
        "",
        "## Snapshot",
        "",
        f"- generated at: `{status.get('generated_at', 'unknown')}`",
        "- status source: `docs/architecture/runtime/node-compat-evidence/latest/status-summary.json`",
        "- dashboard source: `docs/architecture/runtime/node-compat-evidence/latest/dashboard-summary.json`",
    ]
    if trends:
        lines.append("- trend source: `docs/architecture/runtime/node-compat-evidence/latest/trend-summary.json`")
    lines.extend(
        [
            "",
            "## Node Test Results",
            "",
            "| Target | Role | Upstream | Vendored official fixtures | Passed | Expected failure / known gap | Skipped / excluded | Unclassified | Official fixture pass rate | Classified coverage |",
            "| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
        ]
    )
    lines.extend(lane_row(lane, registry) for lane in lane_summaries(status))
    lines.extend(
        [
            "",
            "## Package/Framework Canaries",
            "",
            "| Package | Preset | Lane | Pinned version | Status |",
            "| --- | --- | --- | --- | --- |",
        ]
    )
    for result in canary_results(dashboard):
        lines.append(
            f"| `{result.get('package', result.get('id', 'unknown'))}` | "
            f"{result.get('runtime_preset', 'unknown')} | "
            f"{lane_title(result.get('lane', 'unknown'))} | "
            f"`{result.get('pinned_version', 'unknown')}` | "
            f"{status_label(result.get('status', 'unknown'))} |"
        )
    lines.extend(
        [
            "",
            "## Oracle Checks",
            "",
            "| Lane | Fixture | Runtime | Oracle | Drift | Node oracle |",
            "| --- | --- | --- | --- | --- | --- |",
        ]
    )
    for report in dashboard.get("oracle_reports", []):
        lines.append(
            f"| {lane_title(report.get('lane', 'unknown'))} | "
            f"`{report.get('fixture', 'unknown')}` | "
            f"{status_label(report.get('runtime_state', 'unknown'))} | "
            f"{status_label(report.get('oracle_state', 'unknown'))} | "
            f"{status_label(report.get('drift_class', 'unknown'))} | "
            f"`{report.get('node_version', 'unknown')}` |"
        )
    lines.extend(
        [
            "",
            "## Notes",
            "",
            "- `Passed` fixtures and canaries may support public claims.",
            "- Expected failures, known gaps, skips, and unclassified fixtures are not pass claims.",
            "- Product default is a routing default, not an evidence priority.",
            "- Node22 and Node24 are the current supported LTS lanes; Node20 is legacy-grace regression coverage after its 2026-04-30 EOL.",
        ]
    )
    return lines


def per_lane_lines(
    lane: dict[str, Any],
    dashboard: dict[str, Any],
    registry: dict[str, dict[str, Any]],
) -> list[str]:
    vendored = int(lane.get("vendored_test_file_count", 0))
    documented = int(lane.get("documented_manifested_green_count", 0))
    classified_total = int(lane.get("documented_or_classified_count", 0))
    lane_id = lane["lane"]
    registry_metadata = registry.get(lane_id, {})
    support_phase = str(registry_metadata.get("support_phase", "unknown"))
    evidence_policy = str(registry_metadata.get("evidence_policy", "unknown"))
    product_default = registry_metadata.get("product_default") is True
    lines = [
        f"# {lane_title(lane_id)} Runtime Evidence",
        "",
        "This page is generated from the checked-in Node compatibility evidence snapshots.",
        "",
        "## Summary",
        "",
        f"- role: `{public_lane_role_label(lane, registry)}`",
        f"- support phase: `{support_phase_label(support_phase)}`",
        f"- product default: `{'yes' if product_default else 'no'}`",
        f"- evidence policy: `{evidence_policy_label(evidence_policy)}`",
        f"- upstream fixture line: `{lane.get('upstream', {}).get('tag', 'unknown')}`",
        f"- runtime execution target: `{lane.get('runtime_execution_target', 'unknown')}`",
        f"- vendored official fixtures: `{vendored}`",
        f"- passed official fixtures: `{documented}`",
        f"- expected failure / known gap fixtures: `{lane.get('known_red_or_gap_count', 0)}`",
        f"- skipped / excluded fixtures: `{lane.get('skipped_or_excluded_count', 0)}`",
        f"- unclassified fixtures: `{lane.get('unmanifested_or_unclassified_count', 0)}`",
        f"- official fixture pass rate: `{percent(lane.get('documented_manifested_green_ratio'))}`",
        f"- classified coverage: `{count_percent(classified_total, vendored)}`",
        "",
        "## Classification Catalog",
        "",
        f"- catalog: `{lane.get('classification_catalog', {}).get('catalog_path', 'unknown')}`",
        "",
        "| Expectation | Count |",
        "| --- | ---: |",
    ]
    for key, value in sorted(
        lane.get("classification_catalog", {}).get("by_expectation", {}).items()
    ):
        lines.append(f"| {expectation_label(key)} | {value} |")
    lines.extend(
        [
            "",
            "## Canary Coverage",
            "",
            "| Package | Preset | Pinned version | Status |",
            "| --- | --- | --- | --- |",
        ]
    )
    lane_canaries = [result for result in canary_results(dashboard) if result.get("lane") == lane_id]
    if lane_canaries:
        for result in lane_canaries:
            lines.append(
                f"| `{result.get('package', result.get('id', 'unknown'))}` | "
                f"{result.get('runtime_preset', 'unknown')} | "
                f"`{result.get('pinned_version', 'unknown')}` | "
                f"{status_label(result.get('status', 'unknown'))} |"
            )
    else:
        lines.append("| none in current snapshot | n/a | n/a | n/a |")
    lines.extend(["", "## Claim Boundary", ""])
    if support_phase == "eol_legacy":
        lines.extend(
            [
                "This lane remains selectable as legacy-grace regression coverage, but it",
                "is not an active enterprise LTS support target after Node20 EOL on",
                "2026-04-30.",
            ]
        )
    else:
        lines.extend(
            [
                "This lane is supported only for the measured surfaces represented by its",
                "passed fixtures, canaries, and explicit classifications.",
            ]
        )
    lines.extend(
        [
            "Known gaps and expected failures are intentionally not support claims.",
        ]
    )
    return lines


def rendered_docs(evidence_root: Path) -> dict[Path, list[str]]:
    status = load_json(evidence_root / "status-summary.json")
    dashboard = load_json(evidence_root / "dashboard-summary.json")
    trend_path = evidence_root / "trend-summary.json"
    trends = load_json(trend_path) if trend_path.exists() else None
    registry = lane_registry_by_name()

    rendered = {Path("latest.md"): latest_lines(status, dashboard, trends)}
    for lane in lane_summaries(status):
        rendered[Path(f"{lane['lane']}.md")] = per_lane_lines(lane, dashboard, registry)
    return rendered


def publish(evidence_root: Path, output_root: Path) -> None:
    for relative_path, lines in rendered_docs(evidence_root).items():
        write(output_root / relative_path, lines)


def check(evidence_root: Path, output_root: Path) -> bool:
    ok = True
    for relative_path, lines in rendered_docs(evidence_root).items():
        path = output_root / relative_path
        expected = "\n".join(lines).rstrip() + "\n"
        actual = path.read_text(encoding="utf-8") if path.exists() else ""
        if actual != expected:
            print(f"stale Node.js runtime evidence doc: {path}")
            ok = False
    return ok


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Publish user-facing Node.js runtime evidence Markdown"
    )
    parser.add_argument("--evidence-root", type=Path, default=default_evidence_root())
    parser.add_argument("--output-root", type=Path, default=default_output_root())
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify checked-in generated docs match the current evidence snapshots",
    )
    args = parser.parse_args()
    if args.check:
        if check(args.evidence_root, args.output_root):
            print(f"Node.js runtime evidence docs are current in {args.output_root}")
            return 0
        return 1
    publish(args.evidence_root, args.output_root)
    print(f"published Node.js runtime evidence docs to {args.output_root}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
