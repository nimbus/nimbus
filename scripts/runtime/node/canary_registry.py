#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def registry_path() -> Path:
    return repo_root() / "tests" / "runtime" / "node" / "canary-registry.json"


def load_registry() -> dict:
    with registry_path().open("r", encoding="utf-8") as handle:
        return json.load(handle)


def active_canaries(registry: dict, preset: str | None = None) -> list[dict]:
    canaries = [
        canary
        for canary in registry["canaries"]
        if canary.get("status") == "active"
        and (preset is None or canary["runtime_preset"].lower() == preset.lower())
    ]
    if preset is not None and not canaries:
        raise SystemExit(f"no active canaries found for preset={preset}")
    return canaries


def default_report_output_root() -> Path:
    return repo_root() / "target" / "node-compat" / "canaries"

def lane_metadata_root() -> Path:
    return (
        repo_root()
        / "crates"
        / "neovex-runtime"
        / "src"
        / "runtime"
        / "tests"
        / "node_compat_manifests"
        / "lanes"
    )


def load_lane_metadata_map() -> dict[str, dict]:
    metadata_by_lane: dict[str, dict] = {}
    for path in sorted(lane_metadata_root().glob("*.json")):
        metadata = json.loads(path.read_text(encoding="utf-8"))
        metadata_by_lane[metadata["lane"]] = metadata
    return metadata_by_lane


def canary_report_path(output_root: Path, preset: str) -> Path:
    slug = preset.lower().replace(" ", "-")
    return output_root / f"preset-{slug}.json"


def ensure_bootstrapped(root: Path) -> None:
    package_json = root / "package.json"
    if not package_json.is_file():
        raise SystemExit(f"missing package.json at {package_json}")
    node_modules = root / "node_modules"
    if node_modules.is_dir():
        print(f"node-compat canaries already bootstrapped at {root}")
        return
    subprocess.run(
        ["npm", "ci", "--prefix", str(root)],
        check=True,
        cwd=repo_root(),
    )


def command_bootstrap(args: argparse.Namespace) -> None:
    registry = load_registry()
    roots = {
        repo_root() / canary["root"]
        for canary in active_canaries(registry, args.preset)
    }
    for root in sorted(roots):
        ensure_bootstrapped(root)


def command_run(args: argparse.Namespace) -> None:
    registry = load_registry()
    lane_metadata_by_lane = load_lane_metadata_map()
    canaries = active_canaries(registry, args.preset)
    roots = {repo_root() / canary["root"] for canary in canaries}
    for root in sorted(roots):
        ensure_bootstrapped(root)

    cargo_tests: list[str] = []
    lane_runs: list[dict] = []
    for canary in canaries:
        for lane_run in canary["lane_runs"]:
            if args.lane and lane_run["lane"] != args.lane:
                continue
            lane_runs.append(
                {
                    "canary_id": canary["id"],
                    "package": canary["package"],
                    "pinned_version": canary["pinned_version"],
                    "runtime_preset": canary["runtime_preset"],
                    "claim_ids": canary["claim_ids"],
                    "lane": lane_run["lane"],
                    "compatibility_target": lane_run["compatibility_target"],
                    "cargo_test": lane_run["cargo_test"],
                    "lane_metadata": lane_metadata_by_lane[lane_run["lane"]],
                }
            )
            cargo_test = lane_run["cargo_test"]
            if cargo_test not in cargo_tests:
                cargo_tests.append(cargo_test)

    if not cargo_tests:
        raise SystemExit(
            f"no active canary cargo tests matched preset={args.preset} lane={args.lane or 'all'}"
        )

    cargo_test_status: dict[str, str] = {}
    any_failures = False
    for cargo_test in cargo_tests:
        completed = subprocess.run(
            [
                "cargo",
                "test",
                "-p",
                "neovex-runtime",
                cargo_test,
                "--",
                "--nocapture",
                "--test-threads=1",
                "--ignored",
            ],
            check=False,
            cwd=repo_root(),
        )
        if completed.returncode == 0:
            cargo_test_status[cargo_test] = "pass"
        else:
            cargo_test_status[cargo_test] = "fail"
            any_failures = True

    lane_summary_map: dict[tuple[str, str], dict] = {}
    canary_results: list[dict] = []
    for lane_run in lane_runs:
        status = cargo_test_status[lane_run["cargo_test"]]
        canary_results.append(
            {
                "id": lane_run["canary_id"],
                "package": lane_run["package"],
                "pinned_version": lane_run["pinned_version"],
                "runtime_preset": lane_run["runtime_preset"],
                "claim_ids": lane_run["claim_ids"],
                "lane": lane_run["lane"],
                "compatibility_target": lane_run["compatibility_target"],
                "cargo_test": lane_run["cargo_test"],
                "upstream_fixture_line": lane_run["lane_metadata"]["upstream_fixture_line"],
                "lane_role": lane_run["lane_metadata"]["lane_role"],
                "public_contract_role": lane_run["lane_metadata"]["public_contract_role"],
                "runtime_execution_target": lane_run["lane_metadata"]["runtime_execution_target"],
                "runtime_limits_preset": lane_run["lane_metadata"]["runtime_limits_preset"],
                "status": status,
            }
        )
        lane_key = (lane_run["lane"], lane_run["compatibility_target"])
        summary = lane_summary_map.setdefault(
            lane_key,
            {
                "lane": lane_run["lane"],
                "compatibility_target": lane_run["compatibility_target"],
                "upstream_fixture_line": lane_run["lane_metadata"]["upstream_fixture_line"],
                "lane_role": lane_run["lane_metadata"]["lane_role"],
                "public_contract_role": lane_run["lane_metadata"]["public_contract_role"],
                "runtime_execution_target": lane_run["lane_metadata"]["runtime_execution_target"],
                "runtime_limits_preset": lane_run["lane_metadata"]["runtime_limits_preset"],
                "cargo_tests": [],
                "canary_count": 0,
                "passed": 0,
                "failed": 0,
            },
        )
        if lane_run["cargo_test"] not in summary["cargo_tests"]:
            summary["cargo_tests"].append(lane_run["cargo_test"])
        summary["canary_count"] += 1
        if status == "pass":
            summary["passed"] += 1
        else:
            summary["failed"] += 1

    output_root = (
        Path(args.output_root).resolve()
        if args.output_root
        else default_report_output_root()
    )
    output_root.mkdir(parents=True, exist_ok=True)
    report_path = canary_report_path(output_root, args.preset)
    report = {
        "schema_version": 1,
        "runtime_preset": canaries[0]["runtime_preset"],
        "canary_count": len(canary_results),
        "passed": sum(1 for result in canary_results if result["status"] == "pass"),
        "failed": sum(1 for result in canary_results if result["status"] == "fail"),
        "lane_summaries": sorted(
            lane_summary_map.values(),
            key=lambda summary: (summary["lane"], summary["compatibility_target"]),
        ),
        "canary_results": sorted(
            canary_results,
            key=lambda result: (result["lane"], result["id"]),
        ),
    }
    report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"wrote canary report to {report_path}")

    if any_failures:
        raise SystemExit(1)


def command_validate_claims(_: argparse.Namespace) -> None:
    registry = load_registry()
    claims = registry["claims"]
    canaries = registry["canaries"]
    active_claim_ids = {claim["id"] for claim in claims}
    mapped_claim_ids = {
        claim_id
        for canary in canaries
        if canary.get("status") == "active"
        for claim_id in canary["claim_ids"]
    }
    missing = sorted(active_claim_ids - mapped_claim_ids)
    if missing:
        raise SystemExit(f"claims missing active canary mappings: {', '.join(missing)}")

    for claim in claims:
        doc_path = repo_root() / claim["doc_path"]
        if not doc_path.is_file():
            raise SystemExit(f"missing doc path for claim {claim['id']}: {doc_path}")
        doc_text = doc_path.read_text(encoding="utf-8")
        if claim["package"] not in doc_text:
            raise SystemExit(
                f"doc path for claim {claim['id']} does not mention package {claim['package']}"
            )

    print(
        f"validated {len(claims)} active claim mappings against {len(canaries)} registered canaries"
    )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Node-compat package canary registry helper"
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    bootstrap = subparsers.add_parser("bootstrap")
    bootstrap.add_argument("--preset", default=None)
    bootstrap.set_defaults(func=command_bootstrap)

    run = subparsers.add_parser("run")
    run.add_argument("--preset", required=True)
    run.add_argument("--lane", default=None)
    run.add_argument("--output-root", default=None)
    run.set_defaults(func=command_run)

    validate = subparsers.add_parser("validate-claims")
    validate.set_defaults(func=command_validate_claims)
    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()
    args.func(args)
    return 0


if __name__ == "__main__":
    sys.exit(main())
