#!/usr/bin/env python3
"""Validate vendored Node fixture provenance and published LTS coverage."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import stat
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from schema import load_json  # noqa: E402


HEX_SHA = re.compile(r"^[0-9a-f]{40}$")
SUPPORTED_LTS_PHASES = {"maintenance_lts", "active_lts"}
SUPPORTED_LTS_POLICY = "supported_lts_lane_local_evidence"
SOURCE_KIND = "vendored_official_fixture_corpus"

REPO_ROOT = Path(__file__).resolve().parents[3]
LANE_MANIFEST_ROOT = (
    REPO_ROOT
    / "crates"
    / "nimbus-runtime"
    / "src"
    / "runtime"
    / "tests"
    / "node_compat_manifests"
    / "lanes"
)
REGISTRY_PATH = (
    REPO_ROOT
    / "tests"
    / "runtime"
    / "node"
    / "compat"
    / "node-lts-compat"
    / "node-lts-lanes.json"
)
PUBLISHED_STATUS_PATH = (
    REPO_ROOT
    / "tests"
    / "runtime"
    / "node"
    / "compat"
    / "node-compat-evidence"
    / "latest"
    / "status-summary.json"
)
IDENTITY_ROOT = REPO_ROOT / "tests" / "runtime" / "node" / "official-fixture-identities"
LATEST_SUITE_TAGS_PATH = (
    REPO_ROOT
    / "docs"
    / "architecture"
    / "runtime"
    / "node-lts-compat"
    / "node-latest-suite-tags.json"
)
TEST_FILE_SUFFIXES = {".js", ".mjs", ".cjs"}


def display_path(path: Path) -> str:
    try:
        return str(path.relative_to(REPO_ROOT))
    except ValueError:
        return str(path)


def as_object(value: Any, context: str, errors: list[str]) -> dict[str, Any]:
    if isinstance(value, dict):
        return value
    errors.append(f"{context} must be a JSON object")
    return {}


def required_string(
    payload: dict[str, Any],
    key: str,
    context: str,
    errors: list[str],
) -> str:
    value = payload.get(key)
    if not isinstance(value, str) or not value:
        errors.append(f"{context}.{key} must be a non-empty string")
        return ""
    return value


def load_lane_manifests() -> list[tuple[Path, dict[str, Any]]]:
    return [
        (path, load_json(path))
        for path in sorted(LANE_MANIFEST_ROOT.glob("*.json"))
    ]


def registry_lanes_by_name() -> dict[str, dict[str, Any]]:
    registry = load_json(REGISTRY_PATH)
    lanes = registry.get("lanes", []) if isinstance(registry, dict) else []
    return {
        lane["lane_name"]: lane
        for lane in lanes
        if isinstance(lane, dict) and isinstance(lane.get("lane_name"), str)
    }


def status_lanes_by_name(status_path: Path) -> dict[str, dict[str, Any]]:
    status = load_json(status_path)
    lanes = status.get("lane_summaries", []) if isinstance(status, dict) else []
    return {
        lane["lane"]: lane
        for lane in lanes
        if isinstance(lane, dict) and isinstance(lane.get("lane"), str)
    }


def validate_sha(value: str, context: str, errors: list[str]) -> None:
    if not HEX_SHA.match(value):
        errors.append(f"{context} must be a 40-character lowercase hex SHA")


def expected_selection_command(lane: str, tag: str) -> str:
    return (
        "python3 scripts/runtime/node/sync.py "
        f"--lane {lane} --upstream-tag {tag} --apply"
    )


def is_node_test_file(path: str) -> bool:
    name = Path(path).name
    return name.startswith("test-") and Path(path).suffix in TEST_FILE_SUFFIXES


def git_blob_oid(data: bytes) -> str:
    digest = hashlib.sha1()
    digest.update(f"blob {len(data)}\0".encode("ascii"))
    digest.update(data)
    return digest.hexdigest()


def filesystem_entry(root: Path, relative_path: Path) -> dict[str, str] | None:
    path = root / relative_path
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return None
    if stat.S_ISLNK(metadata.st_mode):
        target = os.readlink(path)
        data = target.encode("utf-8")
        mode = "120000"
    elif stat.S_ISREG(metadata.st_mode):
        data = path.read_bytes()
        mode = "100755" if metadata.st_mode & 0o111 else "100644"
    else:
        return None
    relative = relative_path.as_posix()
    return {
        "path": relative,
        "mode": mode,
        "oid": git_blob_oid(data),
    }


def local_fixture_identity_entries(root: Path) -> list[dict[str, str]]:
    entries: list[dict[str, str]] = []
    if not root.is_dir():
        return entries
    for dirpath, dirnames, filenames in os.walk(root, topdown=True, followlinks=False):
        current = Path(dirpath)
        symlink_dirs: list[str] = []
        for dirname in list(dirnames):
            path = current / dirname
            if path.is_symlink():
                symlink_dirs.append(dirname)
                dirnames.remove(dirname)
        for name in sorted([*filenames, *symlink_dirs]):
            relative = (current / name).relative_to(root)
            entry = filesystem_entry(root, relative)
            if entry is not None:
                entries.append(entry)
    return sorted(entries, key=lambda entry: entry["path"])


def entries_by_path(entries: list[dict[str, str]]) -> dict[str, dict[str, str]]:
    return {entry["path"]: entry for entry in entries}


def test_file_count(entries: list[dict[str, str]]) -> int:
    return sum(1 for entry in entries if is_node_test_file(entry["path"]))


def load_latest_suite_local_checkout() -> Path | None:
    if not LATEST_SUITE_TAGS_PATH.is_file():
        return None
    payload = load_json(LATEST_SUITE_TAGS_PATH)
    checkout = payload.get("local_node_checkout") if isinstance(payload, dict) else None
    if not isinstance(checkout, str) or not checkout:
        return None
    path = Path(checkout).expanduser()
    if (path / ".git").is_dir():
        return path.resolve()
    return None


def configured_source_root(explicit: Path | None) -> Path | None:
    if explicit is not None:
        return explicit.expanduser().resolve()
    env_value = os.environ.get("NIMBUS_NODE_SOURCE_ROOT")
    if env_value:
        return Path(env_value).expanduser().resolve()
    return load_latest_suite_local_checkout()


def require_source_root(explicit: Path | None) -> Path | None:
    source_root = configured_source_root(explicit)
    if source_root is None:
        return None
    if not (source_root / ".git").is_dir():
        raise ValueError(
            f"configured Node source root is not a git checkout: {source_root}"
        )
    return source_root


def run_git(args: list[str], cwd: Path | None = None) -> bytes:
    return subprocess.check_output(args, cwd=cwd)


def upstream_identity_entries_from_source(
    source_root: Path,
    tag: str,
    fixture_subtree: str,
) -> list[dict[str, str]]:
    raw = run_git(
        [
            "git",
            "-C",
            str(source_root),
            "ls-tree",
            "-rz",
            "-r",
            tag,
            fixture_subtree,
        ]
    )
    prefix = f"{fixture_subtree}/"
    entries: list[dict[str, str]] = []
    for record in raw.split(b"\0"):
        if not record:
            continue
        metadata, raw_path = record.split(b"\t", 1)
        mode, object_type, oid = metadata.decode("ascii").split(" ")
        if object_type != "blob":
            continue
        path = raw_path.decode("utf-8", errors="surrogateescape")
        if not path.startswith(prefix):
            raise ValueError(f"unexpected upstream path outside {fixture_subtree}: {path}")
        entries.append({"path": path[len(prefix) :], "mode": mode, "oid": oid})
    return sorted(entries, key=lambda entry: entry["path"])


def github_url(repo: str) -> str:
    if repo.startswith("https://"):
        return repo
    return f"https://github.com/{repo}.git"


def upstream_identity_entries(
    lane: dict[str, Any],
    source_root: Path | None,
    *,
    allow_fetch: bool,
) -> list[dict[str, str]]:
    upstream = lane["upstream"]
    if source_root is not None:
        return upstream_identity_entries_from_source(
            source_root,
            upstream["tag"],
            upstream["fixture_subtree"],
        )
    if not allow_fetch:
        raise ValueError(
            "strict upstream identity comparison needs --source-root, "
            "NIMBUS_NODE_SOURCE_ROOT, an existing local_node_checkout, or --allow-fetch"
        )
    with tempfile.TemporaryDirectory(prefix=f"node-fixture-identity-{lane['lane']}-") as tmp:
        checkout = Path(tmp) / "node"
        subprocess.run(
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
            ],
            check=True,
        )
        subprocess.run(
            ["git", "sparse-checkout", "set", upstream["fixture_subtree"]],
            cwd=checkout,
            check=True,
        )
        return upstream_identity_entries_from_source(
            checkout,
            "HEAD",
            upstream["fixture_subtree"],
        )


def identity_manifest_path(identity_root: Path, lane: str) -> Path:
    return identity_root / f"{lane}.json"


def build_identity_manifest(
    lane: dict[str, Any],
    entries: list[dict[str, str]],
    source_description: str,
) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "manifest_kind": "node_official_fixture_identity",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "lane": lane["lane"],
        "upstream": lane["upstream"],
        "vendored_fixture_root": lane["vendored_fixture_root"],
        "identity_source": source_description,
        "entry_count": len(entries),
        "test_file_count": test_file_count(entries),
        "entries": entries,
    }


def validate_identity_manifest_shape(
    path: Path,
    manifest: dict[str, Any],
    lane: dict[str, Any],
) -> list[str]:
    errors: list[str] = []
    context = display_path(path)
    if manifest.get("schema_version") != 1:
        errors.append(f"{context}.schema_version must be 1")
    if manifest.get("manifest_kind") != "node_official_fixture_identity":
        errors.append(f"{context}.manifest_kind must be node_official_fixture_identity")
    if manifest.get("lane") != lane["lane"]:
        errors.append(f"{context}.lane must be {lane['lane']}")
    if manifest.get("upstream") != lane["upstream"]:
        errors.append(f"{context}.upstream must match {lane['lane']} lane manifest")
    if manifest.get("vendored_fixture_root") != lane["vendored_fixture_root"]:
        errors.append(
            f"{context}.vendored_fixture_root must match {lane['lane']} lane manifest"
        )
    entries = manifest.get("entries")
    if not isinstance(entries, list):
        errors.append(f"{context}.entries must be an array")
        return errors
    seen: set[str] = set()
    for index, entry in enumerate(entries):
        if not isinstance(entry, dict):
            errors.append(f"{context}.entries[{index}] must be an object")
            continue
        path_value = entry.get("path")
        mode = entry.get("mode")
        oid = entry.get("oid")
        if not isinstance(path_value, str) or not path_value:
            errors.append(f"{context}.entries[{index}].path must be a non-empty string")
        elif path_value in seen:
            errors.append(f"{context} contains duplicate path {path_value}")
        else:
            seen.add(path_value)
        if mode not in {"100644", "100755", "120000"}:
            errors.append(f"{context}.entries[{index}].mode has unsupported Git mode {mode!r}")
        if not isinstance(oid, str) or not HEX_SHA.match(oid):
            errors.append(f"{context}.entries[{index}].oid must be a Git SHA-1 blob id")
    if manifest.get("entry_count") != len(entries):
        errors.append(f"{context}.entry_count must equal entries length")
    if manifest.get("test_file_count") != test_file_count(entries):
        errors.append(f"{context}.test_file_count must equal test-* entry count")
    return errors


def compare_identity_entries(
    *,
    lane: str,
    expected_entries: list[dict[str, str]],
    actual_entries: list[dict[str, str]],
    source: str,
) -> tuple[list[str], dict[str, Any]]:
    errors: list[str] = []
    expected = entries_by_path(expected_entries)
    actual = entries_by_path(actual_entries)
    missing = sorted(set(expected) - set(actual))
    extra = sorted(set(actual) - set(expected))
    mode_mismatches = sorted(
        path
        for path in set(expected) & set(actual)
        if expected[path]["mode"] != actual[path]["mode"]
    )
    oid_mismatches = sorted(
        path
        for path in set(expected) & set(actual)
        if expected[path]["oid"] != actual[path]["oid"]
    )
    summary = {
        "lane": lane,
        "source": source,
        "expected_entry_count": len(expected_entries),
        "actual_entry_count": len(actual_entries),
        "expected_test_file_count": test_file_count(expected_entries),
        "actual_test_file_count": test_file_count(actual_entries),
        "missing_count": len(missing),
        "extra_count": len(extra),
        "mode_mismatch_count": len(mode_mismatches),
        "oid_mismatch_count": len(oid_mismatches),
        "missing_sample": missing[:20],
        "extra_sample": extra[:20],
        "mode_mismatch_sample": mode_mismatches[:20],
        "oid_mismatch_sample": oid_mismatches[:20],
    }
    for key, label in [
        ("missing_count", "missing upstream entries"),
        ("extra_count", "extra non-upstream entries"),
        ("mode_mismatch_count", "Git mode mismatches"),
        ("oid_mismatch_count", "blob identity mismatches"),
    ]:
        if summary[key]:
            sample_key = {
                "missing_count": "missing_sample",
                "extra_count": "extra_sample",
                "mode_mismatch_count": "mode_mismatch_sample",
                "oid_mismatch_count": "oid_mismatch_sample",
            }[key]
            errors.append(
                f"{lane} strict fixture identity has {summary[key]} {label} "
                f"against {source}; sample={summary[sample_key]}"
            )
    return errors, summary


def strict_identity_summaries(
    lanes: list[dict[str, Any]],
    identity_root: Path,
) -> tuple[list[str], list[dict[str, Any]]]:
    errors: list[str] = []
    summaries: list[dict[str, Any]] = []
    for lane in lanes:
        path = identity_manifest_path(identity_root, lane["lane"])
        if not path.is_file():
            errors.append(
                f"{display_path(path)} is required for vendored official fixture lane {lane['lane']}"
            )
            continue
        manifest = load_json(path)
        if not isinstance(manifest, dict):
            errors.append(f"{display_path(path)} must contain a JSON object")
            continue
        errors.extend(validate_identity_manifest_shape(path, manifest, lane))
        expected_entries = manifest.get("entries", [])
        if not isinstance(expected_entries, list):
            continue
        local_root = REPO_ROOT / lane["vendored_fixture_root"]
        local_entries = local_fixture_identity_entries(local_root)
        compare_errors, summary = compare_identity_entries(
            lane=lane["lane"],
            expected_entries=expected_entries,
            actual_entries=local_entries,
            source=display_path(path),
        )
        errors.extend(compare_errors)
        summaries.append(summary)
    return errors, summaries


def upstream_identity_summaries(
    lanes: list[dict[str, Any]],
    identity_root: Path,
    source_root: Path | None,
    *,
    allow_fetch: bool,
) -> tuple[list[str], list[dict[str, Any]]]:
    errors: list[str] = []
    summaries: list[dict[str, Any]] = []
    for lane in lanes:
        path = identity_manifest_path(identity_root, lane["lane"])
        if not path.is_file():
            errors.append(
                f"{display_path(path)} is required before upstream identity verification"
            )
            continue
        manifest = load_json(path)
        if not isinstance(manifest, dict):
            errors.append(f"{display_path(path)} must contain a JSON object")
            continue
        expected_entries = manifest.get("entries", [])
        if not isinstance(expected_entries, list):
            errors.append(f"{display_path(path)}.entries must be an array")
            continue
        try:
            upstream_entries = upstream_identity_entries(
                lane,
                source_root,
                allow_fetch=allow_fetch,
            )
        except (OSError, subprocess.CalledProcessError, ValueError) as error:
            errors.append(f"{lane['lane']} upstream identity verification failed: {error}")
            continue
        compare_errors, summary = compare_identity_entries(
            lane=lane["lane"],
            expected_entries=expected_entries,
            actual_entries=upstream_entries,
            source=f"{lane['upstream']['repo']}@{lane['upstream']['tag']}",
        )
        errors.extend(compare_errors)
        summaries.append(summary)
    return errors, summaries


def validate_lane_manifest(
    path: Path,
    manifest: dict[str, Any],
    registry_lane: dict[str, Any] | None,
) -> list[str]:
    errors: list[str] = []
    context = display_path(path)
    lane = required_string(manifest, "lane", context, errors)
    upstream = as_object(manifest.get("upstream"), f"{context}.upstream", errors)
    provenance = as_object(
        manifest.get("fixture_provenance"),
        f"{context}.fixture_provenance",
        errors,
    )

    if upstream.get("source_kind") != SOURCE_KIND:
        errors.append(f"{context}.upstream.source_kind must be {SOURCE_KIND}")

    repo = required_string(upstream, "repo", f"{context}.upstream", errors)
    if repo != "nodejs/node":
        errors.append(f"{context}.upstream.repo must be nodejs/node")

    tag = required_string(upstream, "tag", f"{context}.upstream", errors)
    commit = required_string(upstream, "commit", f"{context}.upstream", errors)
    tag_object = required_string(upstream, "tag_object", f"{context}.upstream", errors)
    fixture_subtree = required_string(
        upstream, "fixture_subtree", f"{context}.upstream", errors
    )
    if fixture_subtree != "test":
        errors.append(f"{context}.upstream.fixture_subtree must be test")
    validate_sha(commit, f"{context}.upstream.commit", errors)
    validate_sha(tag_object, f"{context}.upstream.tag_object", errors)

    synced_at = required_string(
        provenance, "synced_at", f"{context}.fixture_provenance", errors
    )
    if len(synced_at) < 10:
        errors.append(f"{context}.fixture_provenance.synced_at must include a date")
    selection_command = required_string(
        provenance,
        "selection_command",
        f"{context}.fixture_provenance",
        errors,
    )
    nimbus_sync_commit = required_string(
        provenance,
        "nimbus_sync_commit",
        f"{context}.fixture_provenance",
        errors,
    )
    validate_sha(
        nimbus_sync_commit,
        f"{context}.fixture_provenance.nimbus_sync_commit",
        errors,
    )
    required_string(
        provenance, "recorded_at", f"{context}.fixture_provenance", errors
    )
    required_string(
        provenance, "recorded_from", f"{context}.fixture_provenance", errors
    )

    expected_selection = expected_selection_command(lane, tag)
    if selection_command != expected_selection:
        errors.append(
            f"{context}.fixture_provenance.selection_command must be {expected_selection!r}"
        )

    vendored_fixture_root = required_string(
        manifest, "vendored_fixture_root", context, errors
    )
    if vendored_fixture_root and not (REPO_ROOT / vendored_fixture_root).is_dir():
        errors.append(f"{context}.vendored_fixture_root does not exist")

    if registry_lane is None:
        errors.append(f"{context} has no matching lane registry entry")
    else:
        if registry_lane.get("fixture_corpus_path") != vendored_fixture_root:
            errors.append(
                f"{context}.vendored_fixture_root does not match lane registry fixture_corpus_path"
            )
        if registry_lane.get("fixture_corpus_upstream_tag") != tag:
            errors.append(
                f"{context}.upstream.tag does not match lane registry fixture_corpus_upstream_tag"
            )

    return errors


def supported_lts_registry_lanes(
    lanes_by_name: dict[str, dict[str, Any]],
) -> list[dict[str, Any]]:
    return [
        lane
        for lane in lanes_by_name.values()
        if lane.get("support_phase") in SUPPORTED_LTS_PHASES
        and lane.get("evidence_policy") == SUPPORTED_LTS_POLICY
    ]


def validate_published_supported_lts_status(
    lanes_by_name: dict[str, dict[str, Any]],
    status_path: Path,
) -> list[str]:
    errors: list[str] = []
    status_lanes = status_lanes_by_name(status_path)
    for lane in supported_lts_registry_lanes(lanes_by_name):
        lane_name = lane["lane_name"]
        status = status_lanes.get(lane_name)
        if status is None:
            errors.append(
                f"{display_path(status_path)} is missing supported LTS lane {lane_name}"
            )
            continue
        unclassified = status.get("unmanifested_or_unclassified_count")
        if not isinstance(unclassified, int):
            errors.append(
                f"{display_path(status_path)} {lane_name} unmanifested_or_unclassified_count must be an integer"
            )
            continue
        if unclassified != 0:
            errors.append(
                f"{display_path(status_path)} {lane_name} has {unclassified} unclassified published fixtures"
            )
    return errors


def vendored_lane_manifests() -> list[dict[str, Any]]:
    lanes: list[dict[str, Any]] = []
    for _path, manifest in load_lane_manifests():
        if not isinstance(manifest, dict):
            continue
        upstream = manifest.get("upstream", {})
        if isinstance(upstream, dict) and upstream.get("source_kind") == SOURCE_KIND:
            lanes.append(manifest)
    lanes.sort(key=lambda lane: lane["lane"])
    return lanes


def validate_fixture_provenance(
    status_path: Path = PUBLISHED_STATUS_PATH,
    identity_root: Path = IDENTITY_ROOT,
) -> tuple[list[str], list[dict[str, Any]]]:
    errors: list[str] = []
    lanes_by_name = registry_lanes_by_name()
    vendored_count = 0

    for path, manifest in load_lane_manifests():
        if not isinstance(manifest, dict):
            errors.append(f"{display_path(path)} must contain a JSON object")
            continue
        upstream = manifest.get("upstream", {})
        if isinstance(upstream, dict) and upstream.get("source_kind") == SOURCE_KIND:
            vendored_count += 1
        lane_name = manifest.get("lane") if isinstance(manifest.get("lane"), str) else None
        registry_lane = lanes_by_name.get(lane_name) if lane_name else None
        errors.extend(validate_lane_manifest(path, manifest, registry_lane))

    if vendored_count == 0:
        errors.append("no vendored Node fixture corpora were found")

    identity_errors, identity_summaries = strict_identity_summaries(
        vendored_lane_manifests(),
        identity_root,
    )
    errors.extend(identity_errors)
    errors.extend(validate_published_supported_lts_status(lanes_by_name, status_path))
    return errors, identity_summaries


def build_validation_report(
    *,
    errors: list[str],
    identity_summaries: list[dict[str, Any]],
    upstream_summaries: list[dict[str, Any]] | None = None,
) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "report_kind": "node_official_fixture_provenance_validation",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "identity_root": display_path(IDENTITY_ROOT),
        "strict_identity_summaries": identity_summaries,
        "upstream_identity_summaries": upstream_summaries or [],
        "error_count": len(errors),
        "errors": errors,
    }


def write_validation_report(report: dict[str, Any], output_root: Path) -> tuple[Path, Path]:
    output_root.mkdir(parents=True, exist_ok=True)
    json_path = output_root / "fixture-provenance-validation.json"
    markdown_path = output_root / "fixture-provenance-validation.md"
    json_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    lines = [
        "# Node Official Fixture Provenance Validation",
        "",
        f"- generated at: `{report['generated_at']}`",
        f"- identity root: `{report['identity_root']}`",
        f"- errors: `{report['error_count']}`",
        "",
        "## Strict Identity",
        "",
        "| Lane | Expected entries | Actual entries | Expected test files | Actual test files | Missing | Extra | Mode mismatches | Blob mismatches |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for summary in report["strict_identity_summaries"]:
        lines.append(
            f"| `{summary['lane']}` | {summary['expected_entry_count']} | "
            f"{summary['actual_entry_count']} | {summary['expected_test_file_count']} | "
            f"{summary['actual_test_file_count']} | {summary['missing_count']} | "
            f"{summary['extra_count']} | {summary['mode_mismatch_count']} | "
            f"{summary['oid_mismatch_count']} |"
        )
    if report["upstream_identity_summaries"]:
        lines.extend(
            [
                "",
                "## Upstream Identity",
                "",
                "| Lane | Expected entries | Upstream entries | Expected test files | Upstream test files | Missing | Extra | Mode mismatches | Blob mismatches |",
                "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
            ]
        )
        for summary in report["upstream_identity_summaries"]:
            lines.append(
                f"| `{summary['lane']}` | {summary['expected_entry_count']} | "
                f"{summary['actual_entry_count']} | {summary['expected_test_file_count']} | "
                f"{summary['actual_test_file_count']} | {summary['missing_count']} | "
                f"{summary['extra_count']} | {summary['mode_mismatch_count']} | "
                f"{summary['oid_mismatch_count']} |"
            )
    lines.extend(["", "## Errors"])
    if report["errors"]:
        for error in report["errors"]:
            lines.append(f"- {error}")
    else:
        lines.append("- none")
    markdown_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return json_path, markdown_path


def record_identity_manifests(args: argparse.Namespace) -> int:
    source_root = require_source_root(args.source_root)
    lanes = vendored_lane_manifests()
    selected_lanes = set(args.lane or [])
    if selected_lanes:
        lanes = [lane for lane in lanes if lane["lane"] in selected_lanes]
    unknown = selected_lanes - {lane["lane"] for lane in vendored_lane_manifests()}
    if unknown:
        print(f"error: unknown lane(s): {', '.join(sorted(unknown))}")
        return 1
    output_root = Path(args.output_root).resolve()
    output_root.mkdir(parents=True, exist_ok=True)
    for lane in lanes:
        try:
            entries = upstream_identity_entries(
                lane,
                source_root,
                allow_fetch=args.allow_fetch,
            )
        except (OSError, subprocess.CalledProcessError, ValueError) as error:
            print(f"error: {lane['lane']} identity generation failed: {error}")
            return 1
        source_description = (
            f"{lane['upstream']['repo']}@{lane['upstream']['tag']} via "
            f"{source_root if source_root is not None else 'sparse fetch'}"
        )
        manifest = build_identity_manifest(lane, entries, source_description)
        path = identity_manifest_path(output_root, lane["lane"])
        path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        print(
            "wrote Node official fixture identity manifest: "
            f"{display_path(path)} ({len(entries)} entries, {test_file_count(entries)} test files)"
        )
    return 0


def verify_upstream_identities(args: argparse.Namespace) -> int:
    try:
        source_root = require_source_root(args.source_root)
    except ValueError as error:
        print(f"error: {error}")
        return 1
    identity_root = Path(args.identity_root).resolve()
    lanes = vendored_lane_manifests()
    selected_lanes = set(args.lane or [])
    if selected_lanes:
        lanes = [lane for lane in lanes if lane["lane"] in selected_lanes]
    errors, summaries = upstream_identity_summaries(
        lanes,
        identity_root,
        source_root,
        allow_fetch=args.allow_fetch,
    )
    report = build_validation_report(
        errors=errors,
        identity_summaries=[],
        upstream_summaries=summaries,
    )
    if args.output_root:
        json_path, markdown_path = write_validation_report(
            report,
            Path(args.output_root).resolve(),
        )
        print(f"wrote fixture provenance validation report to {json_path}")
        print(f"wrote fixture provenance validation markdown to {markdown_path}")
    if errors:
        for error in errors:
            print(f"error: {error}")
        return 1
    print(
        "verified checked-in Node official fixture identities against upstream: "
        f"{len(summaries)} lanes"
    )
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Validate Node fixture provenance and supported LTS coverage"
    )
    subparsers = parser.add_subparsers(dest="command", required=True)
    validate_parser = subparsers.add_parser("validate")
    validate_parser.add_argument(
        "--status-summary",
        type=Path,
        default=PUBLISHED_STATUS_PATH,
        help="published status-summary.json to guard for unclassified supported LTS results",
    )
    validate_parser.add_argument(
        "--identity-root",
        type=Path,
        default=IDENTITY_ROOT,
        help="checked-in strict official fixture identity manifests",
    )
    validate_parser.add_argument(
        "--output-root",
        type=Path,
        help="optional output directory for a strict provenance validation report",
    )
    record_parser = subparsers.add_parser(
        "record-identities",
        help="write checked-in official fixture identity manifests from upstream Node",
    )
    record_parser.add_argument("--lane", action="append", help="lane to record; repeatable")
    record_parser.add_argument("--source-root", type=Path, help="local nodejs/node checkout")
    record_parser.add_argument("--output-root", type=Path, default=IDENTITY_ROOT)
    record_parser.add_argument(
        "--allow-fetch",
        action="store_true",
        help="allow sparse GitHub fetch when no local Node source checkout is configured",
    )
    upstream_parser = subparsers.add_parser(
        "verify-upstream",
        help="compare checked-in identity manifests against upstream Node source",
    )
    upstream_parser.add_argument("--lane", action="append", help="lane to verify; repeatable")
    upstream_parser.add_argument("--source-root", type=Path, help="local nodejs/node checkout")
    upstream_parser.add_argument("--identity-root", type=Path, default=IDENTITY_ROOT)
    upstream_parser.add_argument("--output-root", type=Path)
    upstream_parser.add_argument(
        "--allow-fetch",
        action="store_true",
        help="allow sparse GitHub fetch when no local Node source checkout is configured",
    )
    return parser


def main() -> int:
    args = build_parser().parse_args()
    if args.command == "record-identities":
        return record_identity_manifests(args)
    if args.command == "verify-upstream":
        return verify_upstream_identities(args)
    if args.command != "validate":
        raise AssertionError(f"unhandled command {args.command}")

    status_path = args.status_summary.resolve()
    errors, identity_summaries = validate_fixture_provenance(
        status_path,
        args.identity_root.resolve(),
    )
    report = build_validation_report(
        errors=errors,
        identity_summaries=identity_summaries,
    )
    if args.output_root:
        json_path, markdown_path = write_validation_report(
            report,
            args.output_root.resolve(),
        )
        print(f"wrote fixture provenance validation report to {json_path}")
        print(f"wrote fixture provenance validation markdown to {markdown_path}")
    if errors:
        for error in errors:
            print(f"error: {error}")
        return 1

    registry_lanes = registry_lanes_by_name()
    vendored_lanes = [
        manifest["lane"]
        for _, manifest in load_lane_manifests()
        if isinstance(manifest, dict)
        and isinstance(manifest.get("upstream"), dict)
        and manifest["upstream"].get("source_kind") == SOURCE_KIND
    ]
    supported_lts = supported_lts_registry_lanes(registry_lanes)
    print(
        "validated Node fixture provenance: "
        f"{len(vendored_lanes)} vendored corpora, "
        f"{len(identity_summaries)} strict identity manifests, "
        f"{len(supported_lts)} supported LTS lanes with zero unclassified published results"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
