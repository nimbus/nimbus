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
    return repo_root() / "tests" / "runtime" / "node" / "compat" / "node-compat-evidence" / "latest"


def default_output_root() -> Path:
    return repo_root() / "tests" / "runtime" / "node" / "published" / "nodejs" / "evidence"


def lane_registry_path() -> Path:
    return (
        repo_root()
        / "tests"
        / "runtime"
        / "node"
        / "compat"
        / "node-lts-compat"
        / "node-lts-lanes.json"
    )


def faas_profile_path() -> Path:
    return (
        repo_root()
        / "tests"
        / "runtime"
        / "node"
        / "compat"
        / "node-faas-compatibility-profile.json"
    )


def default_support_posture_path() -> Path:
    return (
        repo_root()
        / "docs"
        / "private"
        / "architecture"
        / "runtime"
        / "node-default-support-posture.json"
    )


def shim_inventory_path() -> Path:
    return (
        repo_root()
        / "docs"
        / "private"
        / "architecture"
        / "runtime"
        / "node-isolate-shim-inventory.json"
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
        "current_non_lts": "Current non-LTS",
    }
    return labels.get(phase, phase.replace("_", " ").title())


def evidence_policy_label(policy: str) -> str:
    labels = {
        "legacy_grace_regression_only": "legacy-grace regression only",
        "supported_lts_lane_local_evidence": "lane-local LTS evidence",
        "current_non_lts_lane_local_evidence_until_lts_promotion": "Current non-LTS; promote to LTS support only after LTS and lane-local evidence",
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


def evidence_label(evidence_kind: str) -> str:
    labels = {
        "positive_support": "Support",
        "diagnostic": "Diagnostic",
    }
    return labels.get(evidence_kind, evidence_kind.replace("_", " ").title())


def support_status_label(support_status: str) -> str:
    labels = {
        "supported": "Supported",
        "service_microvm_required": "Service/microVM required",
        "unsupported_boundary": "Unsupported boundary",
    }
    return labels.get(support_status, support_status.replace("_", " ").title())


def verification_state_label(verification_state: str) -> str:
    labels = {
        "current_evidence": "Current evidence",
        "planned_by_nfrc": "Planned by NFRC",
        "requires_service_route": "Requires service route",
        "unsupported_boundary": "Unsupported boundary",
    }
    return labels.get(verification_state, verification_state.replace("_", " ").title())


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


def claim_summaries(dashboard: dict[str, Any]) -> list[dict[str, Any]]:
    return list(dashboard.get("claim_summaries", []))


def inline_code_list(values: list[str] | tuple[str, ...]) -> str:
    if not values:
        return "none"
    return ", ".join(f"`{value}`" for value in values)


def inline_code_list_limited(values: list[str] | tuple[str, ...], limit: int = 5) -> str:
    if not values:
        return "none"
    rendered = [f"`{value}`" for value in values[:limit]]
    remaining = len(values) - limit
    if remaining > 0:
        rendered.append(f"{remaining} more")
    return ", ".join(rendered)


def evidence_refs(profile: dict[str, Any], refs: list[str]) -> str:
    refs_by_id = {ref["id"]: ref for ref in profile.get("evidence_refs", [])}
    rendered: list[str] = []
    for ref_id in refs:
        ref = refs_by_id.get(ref_id)
        if ref is None:
            rendered.append(f"`{ref_id}`")
            continue
        rendered.append(f"`{ref_id}`")
    return ", ".join(rendered) if rendered else "none"


def support_statuses_by_id(profile: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {status["id"]: status for status in profile.get("support_statuses", [])}


def generated_header(source: str) -> list[str]:
    return [
        "<!-- generated by scripts/runtime/node/publish_docs.py; do not edit by hand -->",
        "",
        f"Source: `{source}`",
        "",
    ]


def write(path: Path, lines: list[str]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines).rstrip() + "\n", encoding="utf-8")


def version_support_lines(
    profile: dict[str, Any],
    status: dict[str, Any],
    registry: dict[str, dict[str, Any]],
) -> list[str]:
    lanes_by_name = {lane["lane"]: lane for lane in lane_summaries(status)}
    lines = [
        "## Version Support",
        "",
        "| Target | Public status | Release phase | Product default | Enterprise LTS | Upstream | Evidence policy |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for target in profile.get("lane_targets", []):
        if not target.get("doc_generation", {}).get("show_in_version_table", True):
            continue
        lane = target["lane"]
        status_lane = lanes_by_name.get(lane, {})
        registry_lane = registry.get(lane, {})
        upstream = (
            status_lane.get("upstream", {}).get("tag")
            or registry_lane.get("runtime_version")
            or "unknown"
        )
        lines.append(
            f"| {lane_title(lane)} | "
            f"{support_status_label(target['public_status'])} | "
            f"{support_phase_label(target['node_release_phase'])} | "
            f"{'yes' if target.get('product_default_after_nfrc') else 'no'} | "
            f"{'yes' if target.get('enterprise_lts_support') else 'no'} | "
            f"`{upstream}` | "
            f"{evidence_policy_label(str(registry_lane.get('evidence_policy', 'unknown')))} |"
        )
    lines.extend(
        [
            "",
            "Product default is a routing default, not an evidence priority.",
            "Node22 and Node24 are supported LTS targets with lane-local evidence.",
            "Node20 remains selectable as legacy-grace regression coverage, but it is not active enterprise LTS support.",
            "Node26 is Current/non-LTS compatibility evidence and is not enterprise LTS support until Node itself enters LTS and supported-LTS gates pass.",
            "",
        ]
    )
    return lines


def support_vocabulary_lines(profile: dict[str, Any]) -> list[str]:
    lines = [
        "## Support Vocabulary",
        "",
        "| Status | Meaning |",
        "| --- | --- |",
    ]
    for status in profile.get("support_statuses", []):
        lines.append(f"| {support_status_label(status['id'])} | {status['description']} |")
    lines.append("")
    return lines


def posture_lanes(posture: dict[str, Any]) -> list[tuple[str, dict[str, Any]]]:
    lanes = posture.get("lanes", {})
    ordered = ["node20", "node22", "node24", "node26"]
    return [(lane, lanes[lane]) for lane in ordered if lane in lanes]


def default_support_posture_lines(posture: dict[str, Any]) -> list[str]:
    lines = [
        "## Default-Support Posture",
        "",
        "Source: `docs/private/architecture/runtime/node-default-support-posture.json`",
        "",
        "The default-support posture separates the full official fixture corpus from the V8-isolate-required surface, optional isolate gaps, diagnostic non-isolate behavior, test-harness-only fixtures, and upstream/platform boundaries.",
        "",
        "| Target | Role | Full official corpus | Current passed | V8-isolate required passed/total | Required gaps | Optional gaps | Diagnostic non-isolate | Test-harness-only | Upstream/platform |",
        "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for lane, payload in posture_lanes(posture):
        required = payload.get("v8_isolate_required", {})
        lines.append(
            f"| {lane_title(lane)} | "
            f"{role_label(str(payload.get('role', 'unknown')))} | "
            f"{payload.get('full_official_fixture_corpus', 0)} | "
            f"{payload.get('current_passed', 0)} | "
            f"{required.get('passed', 0)} / {required.get('total', 0)} | "
            f"{required.get('gaps', 0)} | "
            f"{payload.get('v8_isolate_optional', {}).get('gaps', 0)} | "
            f"{payload.get('diagnostic_only_non_isolate', {}).get('gaps', 0)} | "
            f"{payload.get('test_harness_only', {}).get('gaps', 0)} | "
            f"{payload.get('upstream_or_platform_boundary', {}).get('gaps', 0)} |"
        )
    lines.extend(
        [
            "",
            "Node24 remains the product-default routing target, but the well-supported default label is gated on NDS closeout. Until Node22 and Node24 are green for the V8-isolate-required surface, these generated counts are the public contract.",
            "Node26 is Current/non-LTS observation evidence and is shown separately from supported LTS claims.",
            "",
        ]
    )
    return lines


def capability_class_summary_lines(inventory: dict[str, Any], link_path: str) -> list[str]:
    lines = [
        "## Capability Classes",
        "",
        "Source: `docs/private/architecture/runtime/node-isolate-shim-inventory.json`",
        "",
        "| Class | Public label | Meaning | Entries |",
        "| --- | --- | --- | ---: |",
    ]
    entries = inventory.get("entries", [])
    for item in inventory.get("classification_vocabulary", []):
        class_id = item["class"]
        count = len([entry for entry in entries if entry.get("class") == class_id])
        lines.append(
            f"| `{class_id}` | {item['public_disclosure_label']} | {item['meaning']} | {count} |"
        )
    lines.extend(
        [
            "",
            f"See [shim and boundary inventory]({link_path}) for source locations, side-effect limits, evidence, and owner repository.",
            "",
        ]
    )
    return lines


def api_reference_lines(
    profile: dict[str, Any],
    status: dict[str, Any],
    dashboard: dict[str, Any],
    posture: dict[str, Any],
    inventory: dict[str, Any],
) -> list[str]:
    registry = lane_registry_by_name()
    lines = [
        "# Node API Reference",
        "",
        *generated_header("tests/runtime/node/compat/node-faas-compatibility-profile.json"),
        "This generated reference lists the Node API families Nimbus exposes, denies, or routes for functions-as-a-service workloads. It is a measured support contract, not a blanket Node.js compatibility claim.",
        "",
        *version_support_lines(profile, status, registry),
        *support_vocabulary_lines(profile),
        *default_support_posture_lines(posture),
        *capability_class_summary_lines(inventory, "shims-and-boundaries.md"),
        "## API Families",
        "",
        "| API family | Nimbus support | Verification | Evidence | Coverage requirements |",
        "| --- | --- | --- | --- | --- |",
    ]
    for family in profile.get("api_families", []):
        lines.append(
            f"| {family['label']} | "
            f"{support_status_label(family['required_status'])} | "
            f"{verification_state_label(family['verification_state'])} | "
            f"{evidence_refs(profile, family.get('evidence_refs', []))} | "
            f"{inline_code_list(family.get('coverage_requirements', []))} |"
        )
    diagnostic_claim_count = len(
        [
            claim
            for claim in claim_summaries(dashboard)
            if claim.get("evidence_kind") == "diagnostic"
        ]
    )
    lines.extend(
        [
            "",
            "## Host-Heavy Boundary",
            "",
            f"The current dashboard carries `{diagnostic_claim_count}` diagnostic canary claims. A diagnostic pass means Nimbus proved the denial or service/microVM route; it is not positive in-process support.",
            "Child processes, worker threads, inspector, REPL, `node --test`, native addons, persistent host filesystem assumptions, and raw listen behavior remain service/microVM-routed in production in-process application profiles.",
            "",
            "## Evidence Links",
            "",
            "- [Generated evidence summary](../evidence/latest.md)",
            "- [Architecture dashboard](../../../compat/node-compat-evidence/latest/dashboard-summary.md)",
            "- [FaaS compatibility profile](../../../compat/node-faas-compatibility-profile.md)",
        ]
    )
    return lines


def package_reference_lines(
    profile: dict[str, Any],
    _status: dict[str, Any],
    dashboard: dict[str, Any],
) -> list[str]:
    lines = [
        "# Node Package Reference",
        "",
        *generated_header("tests/runtime/node/canary-registry.json"),
        "This generated reference summarizes package and framework canaries by support boundary. `Diagnostic` rows prove an intentional denial or service route; they are not positive in-process package support.",
        "",
        *support_vocabulary_lines(profile),
        "## Package Classes",
        "",
        "| Package class | Nimbus support | Verification | Evidence |",
        "| --- | --- | --- | --- |",
    ]
    for package_class in profile.get("package_classes", []):
        lines.append(
            f"| {package_class['label']} | "
            f"{support_status_label(package_class['required_status'])} | "
            f"{verification_state_label(package_class['verification_state'])} | "
            f"{evidence_refs(profile, package_class.get('evidence_refs', []))} |"
        )
    lines.extend(
        [
            "",
            "## Canary Matrix",
            "",
            "| Package | Preset | Evidence | Support boundary | Result | Required lanes | Observed lanes | Claim |",
            "| --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
    )
    for claim in claim_summaries(dashboard):
        observed_lanes = ", ".join(
            lane_title(lane["lane"]) for lane in claim.get("observed_lane_metadata", [])
        )
        lines.append(
            f"| `{claim['package']}` | `{claim['runtime_preset']}` | "
            f"{evidence_label(claim.get('evidence_kind', 'positive_support'))} | "
            f"{support_status_label(claim.get('support_status', 'supported'))} | "
            f"{status_label(claim['status'])} | "
            f"{', '.join(lane_title(lane) for lane in claim.get('lane_coverage', []))} | "
            f"{observed_lanes or 'none'} | `{claim['id']}` |"
        )
    lines.extend(
        [
            "",
            "## Package Guidance",
            "",
            "- HTTP SDKs and Convex-compatible `\"use node\"` actions are supported only where Application canaries pass on Node22 and Node24.",
            "- Tooling packages are local-development or build-time evidence unless their row explicitly says Application support.",
            "- Native addons, package-owned binaries, child-process tools, and raw server listeners require service/microVM routing in production.",
            "- The generated evidence pages show lane-local results for Node26 Current/non-LTS separately from supported-LTS claims.",
        ]
    )
    return lines


def compatibility_lines(
    profile: dict[str, Any],
    status: dict[str, Any],
    dashboard: dict[str, Any],
    posture: dict[str, Any],
    inventory: dict[str, Any],
) -> list[str]:
    registry = lane_registry_by_name()
    lines = [
        "# Node.js Runtime Compatibility",
        "",
        *generated_header("tests/runtime/node/compat/node-faas-compatibility-profile.json"),
        "Nimbus's Node.js runtime compatibility is evidence-backed and deliberately bounded. A surface is considered supported only when it has checked-in fixture, canary, oracle, or classification evidence.",
        "",
        *version_support_lines(profile, status, registry),
        *support_vocabulary_lines(profile),
        *default_support_posture_lines(posture),
        *capability_class_summary_lines(inventory, "reference/shims-and-boundaries.md"),
        "## Current Public Contract",
        "",
        "- Node24 is the product-default compatibility target, but the well-supported default label is held until NDS closeout proves the required surface.",
        "- Node22 and Node24 are supported LTS targets with lane-local evidence.",
        "- Node20 remains selectable as legacy-grace regression coverage, but it is not active enterprise LTS support.",
        "- Node26 is selectable as Current/non-LTS compatibility, but it is not a supported LTS lane or product default.",
        "- Product default is a routing default, not an evidence priority.",
        "- Node target selection does not grant ambient host access. Runtime permission mode and explicit grants remain separate from Node compatibility target.",
        "- Convex-compatible `\"use node\"` action modules can select Node20, Node22, Node24, or Node26 through `convex.json`.",
        "- Nimbus does not currently claim full Node built-in compatibility for any target.",
        "- Runtime support is narrower than Node CLI parity; `node --test`, inspector, worker, child process, native addon, and host-heavy behavior are service/microVM-routed unless generated evidence says otherwise.",
        "",
        "## Canary Summary",
        "",
        f"- package/framework canary claims: `{dashboard.get('canary_claim_count', 0)}`",
        f"- package/framework canary checks: `{dashboard.get('canary_check_count', 0)}`",
        f"- diagnostic canary claims: `{len([claim for claim in claim_summaries(dashboard) if claim.get('evidence_kind') == 'diagnostic'])}`",
        f"- required canary gaps: `{len(dashboard.get('required_canary_gaps', []))}`",
        "",
        "## Reference Pages",
        "",
        "- [Node API reference](reference/node-apis.md)",
        "- [Node package reference](reference/packages.md)",
        "- [Shim and boundary inventory](reference/shims-and-boundaries.md)",
        "- [Generated evidence](evidence/latest.md)",
        "- [Evidence refresh workflow](evidence/refreshing.md)",
    ]
    return lines


def shim_reference_lines(inventory: dict[str, Any]) -> list[str]:
    lines = [
        "# Node Shim And Boundary Inventory",
        "",
        *generated_header("docs/private/architecture/runtime/node-isolate-shim-inventory.json"),
        "This generated reference lists the Node-compatible isolate shims, emulations, test-harness-only helpers, diagnostic stubs, and unsupported surfaces tracked for the Nimbus V8 isolate runtime and the `nimbus/deno` fork.",
        "",
        "## Capability Classes",
        "",
        "| Class | Public label | Meaning |",
        "| --- | --- | --- |",
    ]
    for item in inventory.get("classification_vocabulary", []):
        lines.append(
            f"| `{item['class']}` | {item['public_disclosure_label']} | {item['meaning']} |"
        )
    lines.extend(
        [
            "",
            "## Inventory Entries",
            "",
            "| Entry | Class | Owner | Lanes | Surfaces | Claimed capability | Limits | Evidence |",
            "| --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
    )
    for entry in inventory.get("entries", []):
        lines.append(
            f"| `{entry['id']}` | `{entry['class']}` | `{entry['owner_repository']}` | "
            f"{inline_code_list(entry.get('affected_lanes', []))} | "
            f"{inline_code_list_limited(entry.get('surfaces', []))} | "
            f"{entry['claimed_capability']} | "
            f"{entry['capability_limits']} | "
            f"{inline_code_list_limited(entry.get('evidence_paths', []), limit=3)} |"
        )
    lines.extend(
        [
            "",
            "## Boundary Rule",
            "",
            "Diagnostic entries are not positive support claims. Test-harness-only entries measure official fixtures and do not become user-facing runtime support. Unsupported entries remain visible gaps until an owner lands fixture-backed native, shimmed, or emulated behavior.",
        ]
    )
    return lines


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
        "- status source: `tests/runtime/node/compat/node-compat-evidence/latest/status-summary.json`",
        "- dashboard source: `tests/runtime/node/compat/node-compat-evidence/latest/dashboard-summary.json`",
    ]
    if trends:
        lines.append("- trend source: `tests/runtime/node/compat/node-compat-evidence/latest/trend-summary.json`")
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
            "| Package | Preset | Lane | Pinned version | Evidence | Support boundary | Status |",
            "| --- | --- | --- | --- | --- | --- | --- |",
        ]
    )
    canary_rows = canary_results(dashboard)
    if canary_rows:
        for result in canary_rows:
            lines.append(
                f"| `{result.get('package', result.get('id', 'unknown'))}` | "
                f"{result.get('runtime_preset', 'unknown')} | "
                f"{lane_title(result.get('lane', 'unknown'))} | "
                f"`{result.get('pinned_version', 'unknown')}` | "
                f"{evidence_label(result.get('evidence_kind', 'positive_support'))} | "
                f"{support_status_label(result.get('support_status', 'supported'))} | "
                f"{status_label(result.get('status', 'unknown'))} |"
            )
    else:
        seen_claim_rows: set[
            tuple[str, str, str, str, str, str]
        ] = set()
        for claim in claim_summaries(dashboard):
            required_lanes = ", ".join(
                lane_title(lane) for lane in claim.get("lane_coverage", [])
            )
            package = claim.get("package", claim.get("id", "unknown"))
            runtime_preset = claim.get("runtime_preset", "unknown")
            evidence = evidence_label(claim.get("evidence_kind", "positive_support"))
            support_boundary = support_status_label(
                claim.get("support_status", "supported")
            )
            status = status_label(claim.get("status", "unknown"))
            row_key = (
                package,
                runtime_preset,
                required_lanes,
                evidence,
                support_boundary,
                status,
            )
            if row_key in seen_claim_rows:
                continue
            seen_claim_rows.add(row_key)
            lines.append(
                f"| `{package}` | "
                f"{runtime_preset} | "
                f"{required_lanes or 'none'} | "
                "n/a | "
                f"{evidence} | "
                f"{support_boundary} | "
                f"{status} |"
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
            "| Package | Preset | Pinned version | Evidence | Support boundary | Status |",
            "| --- | --- | --- | --- | --- | --- |",
        ]
    )
    lane_canaries = [result for result in canary_results(dashboard) if result.get("lane") == lane_id]
    if lane_canaries:
        for result in lane_canaries:
            lines.append(
                f"| `{result.get('package', result.get('id', 'unknown'))}` | "
                f"{result.get('runtime_preset', 'unknown')} | "
                f"`{result.get('pinned_version', 'unknown')}` | "
                f"{evidence_label(result.get('evidence_kind', 'positive_support'))} | "
                f"{support_status_label(result.get('support_status', 'supported'))} | "
                f"{status_label(result.get('status', 'unknown'))} |"
            )
    else:
        lines.append("| none in current snapshot | n/a | n/a | n/a | n/a | n/a |")
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


def rendered_public_docs(evidence_root: Path) -> dict[Path, list[str]]:
    status = load_json(evidence_root / "status-summary.json")
    dashboard = load_json(evidence_root / "dashboard-summary.json")
    profile = load_json(faas_profile_path())
    posture = load_json(default_support_posture_path())
    inventory = load_json(shim_inventory_path())
    return {
        Path("compatibility.md"): compatibility_lines(
            profile, status, dashboard, posture, inventory
        ),
        Path("reference/node-apis.md"): api_reference_lines(
            profile, status, dashboard, posture, inventory
        ),
        Path("reference/packages.md"): package_reference_lines(profile, status, dashboard),
        Path("reference/shims-and-boundaries.md"): shim_reference_lines(inventory),
    }


def publish(evidence_root: Path, output_root: Path) -> None:
    for relative_path, lines in rendered_docs(evidence_root).items():
        write(output_root / relative_path, lines)
    public_root = output_root.parent
    for relative_path, lines in rendered_public_docs(evidence_root).items():
        write(public_root / relative_path, lines)


def check(evidence_root: Path, output_root: Path) -> bool:
    ok = True
    for relative_path, lines in rendered_docs(evidence_root).items():
        path = output_root / relative_path
        expected = "\n".join(lines).rstrip() + "\n"
        actual = path.read_text(encoding="utf-8") if path.exists() else ""
        if actual != expected:
            print(f"stale Node.js runtime evidence doc: {path}")
            ok = False
    public_root = output_root.parent
    for relative_path, lines in rendered_public_docs(evidence_root).items():
        path = public_root / relative_path
        expected = "\n".join(lines).rstrip() + "\n"
        actual = path.read_text(encoding="utf-8") if path.exists() else ""
        if actual != expected:
            print(f"stale Node.js runtime public doc: {path}")
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
    print(
        "published Node.js runtime evidence docs to "
        f"{args.output_root} and public docs to {args.output_root.parent}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
