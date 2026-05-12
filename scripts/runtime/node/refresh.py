#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import subprocess
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from schema import default_schema_path, validate_payload_against_schema


REPRESENTATIVE_SLICES: tuple[tuple[str, str], ...] = (
    ("core-semantics", "assert-and-buffer-foundation"),
    ("process-and-timing", "process-foundation"),
    ("streams-and-local-io", "os-tty-readline-foundation"),
    ("networking", "dns-net-foundation"),
    ("loader-context", "module-and-async-foundation"),
)
REFRESH_REPORT_SCHEMA_PATH = default_schema_path("refresh-report.schema.json")


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def manifest_root() -> Path:
    return (
        repo_root()
        / "crates"
        / "neovex-runtime"
        / "src"
        / "runtime"
        / "tests"
        / "node_compat_manifests"
    )


def default_output_root() -> Path:
    return repo_root() / "target" / "node-compat" / "refresh"


def lane_path(lane: str) -> Path:
    return manifest_root() / "lanes" / f"{lane}.json"


def load_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def write_json(path: Path, payload: dict[str, Any]) -> None:
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def update_lane_tag(lane: str, tag: str | None, dry_run: bool) -> dict[str, Any]:
    path = lane_path(lane)
    if not path.is_file():
        raise ValueError(f"unknown lane {lane!r}: {path} does not exist")
    payload = load_json(path)
    before = payload["upstream"]["tag"]
    after = tag or before
    changed = before != after
    if changed and not dry_run:
        payload["upstream"]["tag"] = after
        write_json(path, payload)
    return {
        "path": str(path.relative_to(repo_root())),
        "before_tag": before,
        "after_tag": after,
        "changed": changed,
        "written": changed and not dry_run,
    }


def command_output_tail(value: str, limit: int = 4000) -> str:
    if len(value) <= limit:
        return value
    return value[-limit:]


def run_step(name: str, command: list[str]) -> dict[str, Any]:
    result = subprocess.run(
        command,
        cwd=repo_root(),
        text=True,
        capture_output=True,
    )
    return {
        "name": name,
        "command": command,
        "returncode": result.returncode,
        "stdout_tail": command_output_tail(result.stdout),
        "stderr_tail": command_output_tail(result.stderr),
        "status": "passed" if result.returncode == 0 else "failed",
    }


def sync_mode_args(args: argparse.Namespace) -> list[str]:
    if args.apply:
        mode = ["--apply"]
    elif args.compare_upstream:
        mode = ["--compare-upstream"]
    else:
        mode = ["--dry-run"]
    if args.force:
        mode.append("--force")
    return mode


def build_steps(args: argparse.Namespace, metadata: dict[str, Any]) -> list[tuple[str, list[str]]]:
    sync_command = [
        "python3",
        "scripts/runtime/node/sync.py",
        "--lane",
        args.lane,
        "--output-root",
        str(repo_root() / "target" / "node-compat" / "sync"),
        *sync_mode_args(args),
    ]
    if args.tag and args.apply:
        sync_command.extend(["--upstream-tag", metadata["after_tag"]])
    elif args.tag and not metadata["written"]:
        sync_command.extend(["--upstream-tag", args.tag])

    steps: list[tuple[str, list[str]]] = [("sync", sync_command)]
    if args.run_representative_slices:
        for family, slice_id in REPRESENTATIVE_SLICES:
            steps.append(
                (
                    f"report:{family}:{slice_id}",
                    [
                        "bash",
                        "scripts/runtime/node/report.sh",
                        "--family",
                        family,
                        "--slice",
                        slice_id,
                        "--capture-live",
                    ],
                )
            )
    steps.extend(
        [
            (
                "expectations",
                ["python3", "scripts/runtime/node/expectations.py", "validate"],
            ),
            ("status", ["python3", "scripts/runtime/node/status.py"]),
            (
                "inventory",
                ["python3", "scripts/runtime/node/inventory.py", "--lane", args.lane],
            ),
            ("dashboard", ["python3", "scripts/runtime/node/dashboard.py"]),
            ("trends", ["python3", "scripts/runtime/node/trends.py"]),
            ("publish", ["python3", "scripts/runtime/node/publish_evidence.py"]),
            ("publish_docs", ["python3", "scripts/runtime/node/publish_docs.py"]),
            ("claims", ["bash", "scripts/runtime/node/validate-claims.sh"]),
        ]
    )
    return steps


def build_markdown(report: dict[str, Any]) -> str:
    lines = [
        "# Node Compatibility Refresh",
        "",
        f"- lane: `{report['lane']}`",
        f"- requested tag: `{report['requested_tag'] or 'unchanged'}`",
        f"- mode: `{report['mode']}`",
        f"- lane metadata: `{report['lane_metadata']['before_tag']}` -> `{report['lane_metadata']['after_tag']}`",
        f"- metadata written: `{str(report['lane_metadata']['written']).lower()}`",
        f"- status: `{report['status']}`",
        "",
        "## Steps",
        "",
        "| Step | Status | Return code |",
        "| --- | --- | ---: |",
    ]
    for step in report["steps"]:
        lines.append(
            f"| `{step['name']}` | `{step['status']}` | {step['returncode']} |"
        )
    lines.append("")
    return "\n".join(lines)


def write_outputs(report: dict[str, Any], output_root: Path) -> tuple[Path, Path]:
    output_root.mkdir(parents=True, exist_ok=True)
    stem = f"{report['lane']}-refresh"
    json_path = output_root / f"{stem}.json"
    markdown_path = output_root / f"{stem}.md"
    json_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    markdown_path.write_text(build_markdown(report), encoding="utf-8")
    return json_path, markdown_path


def validate_report(report: dict[str, Any]) -> list[dict[str, Any]]:
    return validate_payload_against_schema(report, REFRESH_REPORT_SCHEMA_PATH)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Run a coordinated Node compatibility fixture/evidence refresh"
    )
    parser.add_argument("--lane", required=True, help="lane id, for example node22")
    parser.add_argument("--tag", help="new upstream Node tag for the lane")
    parser.add_argument("--output-root", default=str(default_output_root()))
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--dry-run", action="store_true", help="plan only; do not edit lane metadata")
    mode.add_argument("--compare-upstream", action="store_true", help="fetch upstream and diff")
    mode.add_argument("--apply", action="store_true", help="write tag metadata and apply fixture sync")
    parser.add_argument("--force", action="store_true", help="allow fixture apply over dirty local paths")
    parser.add_argument(
        "--run-representative-slices",
        action="store_true",
        help="run the representative live slice reports before dashboard generation",
    )
    return parser


def main() -> int:
    args = build_parser().parse_args()
    if not args.dry_run and not args.compare_upstream and not args.apply:
        args.dry_run = True
    mode = "apply" if args.apply else "compare" if args.compare_upstream else "dry_run"
    metadata = update_lane_tag(args.lane, args.tag, dry_run=not args.apply)
    report: dict[str, Any] = {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "report_kind": "node_compat_refresh",
        "lane": args.lane,
        "requested_tag": args.tag,
        "mode": mode,
        "lane_metadata": metadata,
        "steps": [],
        "status": "passed",
    }
    for name, command in build_steps(args, metadata):
        step = run_step(name, command)
        report["steps"].append(step)
        if step["returncode"] != 0:
            report["status"] = "failed"
            break
    errors = validate_report(report)
    if errors:
        for error in errors:
            print(f"error: {json.dumps(error, sort_keys=True)}")
        return 1
    json_path, markdown_path = write_outputs(report, Path(args.output_root).resolve())
    print(f"wrote node-compat refresh report to {json_path}")
    print(f"wrote node-compat refresh markdown to {markdown_path}")
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
