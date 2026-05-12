#!/usr/bin/env python3

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
import subprocess
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from schema import default_schema_path, validate_payload_against_schema


TEST_FILE_SUFFIXES = {".js", ".mjs", ".cjs"}
SYNC_REPORT_SCHEMA_PATH = default_schema_path("fixture-sync-report.schema.json")


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


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
    return repo_root() / "target" / "node-compat" / "sync"


def load_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def load_lane(lane: str) -> dict[str, Any]:
    path = manifest_root() / "lanes" / f"{lane}.json"
    if not path.is_file():
        known = ", ".join(sorted(path.stem for path in (manifest_root() / "lanes").glob("*.json")))
        raise ValueError(f"unknown lane {lane!r}; known lanes: {known}")
    return load_json(path)


def is_node_test_file(path: Path) -> bool:
    return path.name.startswith("test-") and path.suffix in TEST_FILE_SUFFIXES


def fixture_files(root: Path) -> list[str]:
    if not root.is_dir():
        return []
    return sorted(
        str(path.relative_to(root))
        for path in root.rglob("*")
        if path.is_file() and is_node_test_file(path)
    )


def file_digest(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def tree_snapshot(root: Path) -> dict[str, str]:
    if not root.is_dir():
        return {}
    return {
        str(path.relative_to(root)): file_digest(path)
        for path in sorted(root.rglob("*"))
        if path.is_file()
    }


def diff_snapshots(local: dict[str, str], upstream: dict[str, str]) -> dict[str, Any]:
    local_paths = set(local)
    upstream_paths = set(upstream)
    common = local_paths & upstream_paths
    return {
        "added_by_upstream": sorted(upstream_paths - local_paths),
        "removed_by_upstream": sorted(local_paths - upstream_paths),
        "modified_by_upstream": sorted(
            path for path in common if local[path] != upstream[path]
        ),
        "unchanged": len([path for path in common if local[path] == upstream[path]]),
    }


def run(command: list[str], cwd: Path | None = None) -> None:
    subprocess.run(command, cwd=cwd, check=True)


def github_url(repo: str) -> str:
    if repo.startswith("https://"):
        return repo
    return f"https://github.com/{repo}.git"


def fetch_upstream_fixture_tree(lane: dict[str, Any], temp_root: Path) -> Path:
    upstream = lane["upstream"]
    checkout = temp_root / f"{lane['lane']}-{upstream['tag']}"
    run(
        [
            "git",
            "clone",
            "--depth",
            "1",
            "--branch",
            upstream["tag"],
            "--single-branch",
            "--filter=blob:none",
            "--sparse",
            github_url(upstream["repo"]),
            str(checkout),
        ]
    )
    run(["git", "sparse-checkout", "set", upstream["fixture_subtree"]], cwd=checkout)
    return checkout / upstream["fixture_subtree"]


def git_dirty_paths(path: Path) -> list[str]:
    relative = path.relative_to(repo_root())
    result = subprocess.run(
        ["git", "status", "--porcelain", "--", str(relative)],
        cwd=repo_root(),
        check=True,
        text=True,
        capture_output=True,
    )
    return [line for line in result.stdout.splitlines() if line.strip()]


def sync_apply(local_root: Path, upstream_root: Path, force: bool) -> None:
    dirty_paths = git_dirty_paths(local_root)
    if dirty_paths and not force:
        raise RuntimeError(
            "refusing to replace fixture root with uncommitted local changes; "
            "rerun with --force after reviewing the sync diff"
        )
    if local_root.exists():
        shutil.rmtree(local_root)
    shutil.copytree(upstream_root, local_root)


def command_plan(lane: dict[str, Any]) -> dict[str, Any]:
    upstream = lane["upstream"]
    return {
        "fetch": [
            "git",
            "clone",
            "--depth",
            "1",
            "--branch",
            upstream["tag"],
            "--single-branch",
            "--filter=blob:none",
            "--sparse",
            github_url(upstream["repo"]),
            "<tempdir>",
        ],
        "sparse_checkout": [
            "git",
            "sparse-checkout",
            "set",
            upstream["fixture_subtree"],
        ],
        "local_fixture_root": lane["vendored_fixture_root"],
    }


def lane_with_overrides(args: argparse.Namespace) -> dict[str, Any]:
    lane = load_lane(args.lane)
    if args.upstream_tag:
        lane["upstream"] = dict(lane["upstream"])
        lane["upstream"]["tag"] = args.upstream_tag
    return lane


def build_report(args: argparse.Namespace) -> dict[str, Any]:
    lane = lane_with_overrides(args)
    local_root = repo_root() / lane["vendored_fixture_root"]
    mode = "apply" if args.apply else "compare" if args.compare_upstream else "dry_run"
    report: dict[str, Any] = {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "report_kind": "node_compat_fixture_sync",
        "mode": mode,
        "lane": lane["lane"],
        "upstream": lane["upstream"],
        "upstream_tag_override": args.upstream_tag,
        "vendored_fixture_root": lane["vendored_fixture_root"],
        "local_fixture_root_exists": local_root.is_dir(),
        "local_test_file_count": len(fixture_files(local_root)),
        "command_plan": command_plan(lane),
        "dry_run": mode == "dry_run",
        "diff": None,
        "applied": False,
    }
    if mode == "dry_run":
        return report

    with tempfile.TemporaryDirectory(prefix=f"node-compat-sync-{lane['lane']}-") as tmp:
        upstream_root = fetch_upstream_fixture_tree(lane, Path(tmp))
        local_snapshot = tree_snapshot(local_root)
        upstream_snapshot = tree_snapshot(upstream_root)
        report["upstream_test_file_count"] = len(fixture_files(upstream_root))
        report["diff"] = diff_snapshots(local_snapshot, upstream_snapshot)
        if args.apply:
            sync_apply(local_root, upstream_root, args.force)
            report["applied"] = True
            report["local_test_file_count_after_apply"] = len(fixture_files(local_root))
    return report


def build_markdown(report: dict[str, Any]) -> str:
    lines = [
        "# Node Compatibility Fixture Sync",
        "",
        f"- lane: `{report['lane']}`",
        f"- upstream: `{report['upstream']['repo']}@{report['upstream']['tag']}`",
        f"- subtree: `{report['upstream']['fixture_subtree']}`",
        f"- local fixture root: `{report['vendored_fixture_root']}`",
        f"- mode: `{report['mode']}`",
        f"- local test files: {report['local_test_file_count']}",
        f"- applied: `{str(report['applied']).lower()}`",
        "",
        "## Command Plan",
        "",
        f"- fetch: `{' '.join(report['command_plan']['fetch'])}`",
        f"- sparse checkout: `{' '.join(report['command_plan']['sparse_checkout'])}`",
    ]
    if report.get("upstream_test_file_count") is not None:
        lines.append(f"- upstream test files: {report['upstream_test_file_count']}")
    diff = report.get("diff")
    lines.extend(["", "## Diff Summary"])
    if diff is None:
        lines.append("- not fetched in dry-run mode")
    else:
        lines.append(f"- added by upstream: {len(diff['added_by_upstream'])}")
        lines.append(f"- removed by upstream: {len(diff['removed_by_upstream'])}")
        lines.append(f"- modified by upstream: {len(diff['modified_by_upstream'])}")
        lines.append(f"- unchanged: {diff['unchanged']}")
    lines.append("")
    return "\n".join(lines)


def write_outputs(report: dict[str, Any], output_root: Path) -> tuple[Path, Path]:
    output_root.mkdir(parents=True, exist_ok=True)
    stem = f"{report['lane']}-sync"
    json_path = output_root / f"{stem}.json"
    markdown_path = output_root / f"{stem}.md"
    json_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    markdown_path.write_text(build_markdown(report), encoding="utf-8")
    return json_path, markdown_path


def validate_report(report: dict[str, Any]) -> list[dict[str, Any]]:
    return validate_payload_against_schema(report, SYNC_REPORT_SCHEMA_PATH)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Plan, compare, or apply vendored upstream Node fixture syncs"
    )
    parser.add_argument("--lane", required=True, help="lane id, for example node22")
    parser.add_argument(
        "--upstream-tag",
        help="override the lane metadata tag for this sync plan or operation",
    )
    parser.add_argument("--output-root", default=str(default_output_root()))
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--dry-run", action="store_true", help="write a local sync plan only")
    mode.add_argument(
        "--compare-upstream",
        action="store_true",
        help="fetch upstream into a temporary sparse checkout and write a diff report",
    )
    mode.add_argument(
        "--apply",
        action="store_true",
        help="fetch upstream and replace the local vendored fixture root",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="allow --apply when the local fixture root has uncommitted changes",
    )
    return parser


def main() -> int:
    args = build_parser().parse_args()
    if not args.dry_run and not args.compare_upstream and not args.apply:
        args.dry_run = True
    report = build_report(args)
    errors = validate_report(report)
    if errors:
        for error in errors:
            print(f"error: {json.dumps(error, sort_keys=True)}")
        return 1
    json_path, markdown_path = write_outputs(report, Path(args.output_root).resolve())
    print(f"wrote node-compat fixture sync report to {json_path}")
    print(f"wrote node-compat fixture sync markdown to {markdown_path}")
    if report["dry_run"]:
        print("dry-run only; upstream was not fetched and local fixtures were not changed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
