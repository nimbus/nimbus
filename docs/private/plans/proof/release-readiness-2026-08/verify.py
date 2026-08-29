#!/usr/bin/env python3
import json
import re
import sys
from pathlib import Path

EXPECTED_IDS = (
    "tenant_lifecycle", "document_crud", "query_pagination",
    "schema_validation", "indexes", "subscriptions", "scheduler_cron",
    "user_auth", "ts_functions", "bundle_integrity",
    "runtime_subscriptions", "node_compat", "runtime_permissions",
    "diagnostics", "adapter_convex", "adapter_cloud_functions",
    "adapter_firestore", "adapter_mongodb", "adapter_dynamodb",
    "adapter_s3", "adapter_cloudflare_kv", "adapter_resp_kv",
    "adapter_native", "javascript_sdk", "storage_sqlite",
    "storage_postgres", "storage_mysql", "storage_libsql", "storage_redb",
    "encryption", "backup_restore", "object_plane", "server_deployment",
    "network_control", "resource_apis", "sandbox_backends", "machines",
    "compose_services", "desktop_app", "release_archives",
    "install_channels", "oci_image", "docs_contract",
    "security_dependencies", "full_ci", "independent_reviews",
)
STATES = ("pass", "unverified", "fail", "blocked")
CANDIDATE_KEYS = ("nimbus", "desktop", "deno", "main")
PASS_CANDIDATE_KEYS = ("nimbus", "deno", "main")
PASS_CANDIDATE_EXTRAS = {
    "desktop_app": ("desktop",),
    "independent_reviews": ("desktop",),
}
FULL_SHA = re.compile(r"^[0-9a-f]{40}$")
MARKDOWN_HEADING = re.compile(r"^(#{1,6})[ \t]+\S.*$")
MARKDOWN_FENCE = re.compile(r"^(`{3,}|~{3,})(.*)$")


class UnterminatedMarkdownFence(ValueError):
    pass


def markdown_indentation(line: str) -> tuple[int, int]:
    columns = 0
    offset = 0
    for character in line:
        if character == " ":
            columns += 1
        elif character == "\t":
            columns += 4 - (columns % 4)
        else:
            break
        offset += 1
    return columns, offset


def markdown_headings(lines: list[str]) -> list[tuple[int, int, str]]:
    headings: list[tuple[int, int, str]] = []
    fence_character: str | None = None
    fence_length = 0
    for index, line in enumerate(lines):
        indentation, content_offset = markdown_indentation(line)
        content = line[content_offset:]
        if fence_character is not None:
            marker = content.rstrip(" \t")
            if (
                indentation <= 3
                and marker
                and set(marker) == {fence_character}
                and len(marker) >= fence_length
            ):
                fence_character = None
                fence_length = 0
            continue
        fence = MARKDOWN_FENCE.fullmatch(content) if indentation <= 3 else None
        if fence is not None and not (
            fence.group(1).startswith("`") and "`" in fence.group(2)
        ):
            marker = fence.group(1)
            fence_character = marker[0]
            fence_length = len(marker)
            continue
        if indentation >= 4:
            continue
        stripped = content.strip()
        heading = MARKDOWN_HEADING.fullmatch(stripped)
        if heading is not None:
            headings.append((index, len(heading.group(1)), stripped))
    if fence_character is not None:
        raise UnterminatedMarkdownFence
    return headings


def anchored_evidence_section(proof: str, anchor: str) -> str | None:
    anchor = anchor.strip()
    heading = MARKDOWN_HEADING.fullmatch(anchor)
    if heading is None:
        return None
    lines = proof.splitlines()
    headings = markdown_headings(lines)
    matches = [index for index, _level, text in headings if text == anchor]
    if len(matches) != 1:
        return None
    start = matches[0]
    level = len(heading.group(1))
    end = len(lines)
    for index, candidate_level, _text in headings:
        if index > start and candidate_level <= level:
            end = index
            break
    return "\n".join(lines[start:end])


def main() -> int:
    matrix_path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).with_name("matrix.json")
    root = matrix_path.resolve().parent
    errors: list[str] = []
    try:
        data = json.loads(matrix_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        print(f"matrix error: {error}")
        return 1

    if data.get("schemaVersion") != 1:
        errors.append("schemaVersion must equal 1")
    candidate = data.get("candidate")
    if not isinstance(candidate, dict):
        errors.append("candidate must be an object")
        candidate = {}
    elif set(candidate) != set(CANDIDATE_KEYS):
        errors.append("candidate keys must be exactly nimbus, desktop, deno, and main")
    for key in CANDIDATE_KEYS:
        revision = candidate.get(key)
        if not isinstance(revision, str) or FULL_SHA.fullmatch(revision) is None:
            errors.append(f"candidate.{key} must be a full lowercase commit SHA")
    conditions = data.get("conditions")
    if not isinstance(conditions, list):
        print("matrix error: conditions must be an array")
        return 1
    ids = tuple(row.get("id") for row in conditions if isinstance(row, dict))
    if ids != EXPECTED_IDS:
        errors.append("condition IDs or order differ from the fixed 46-condition contract")

    counts = {state: 0 for state in STATES}
    for row in conditions:
        if not isinstance(row, dict):
            errors.append("each condition must be an object")
            continue
        condition_id = row.get("id", "<missing>")
        state = row.get("state")
        if state not in STATES:
            errors.append(f"{condition_id}: invalid state {state!r}")
            continue
        counts[state] += 1
        if state != "pass":
            print(f"{condition_id}: {state}")
            continue
        evidence = row.get("evidence")
        if not isinstance(evidence, dict):
            errors.append(f"{condition_id}: pass requires evidence")
            continue
        relative_path = evidence.get("path")
        anchor = evidence.get("anchor")
        if not isinstance(relative_path, str) or not relative_path:
            errors.append(f"{condition_id}: evidence path is required")
            continue
        if not isinstance(anchor, str) or not anchor:
            errors.append(f"{condition_id}: evidence anchor is required")
            continue
        proof_path = (root / relative_path).resolve()
        if root not in proof_path.parents:
            errors.append(f"{condition_id}: evidence path escapes the proof root")
            continue
        try:
            proof = proof_path.read_text(encoding="utf-8")
        except OSError as error:
            errors.append(f"{condition_id}: cannot read evidence: {error}")
            continue
        try:
            evidence_section = anchored_evidence_section(proof, anchor)
        except UnterminatedMarkdownFence:
            errors.append(f"{condition_id}: evidence proof has an unterminated Markdown fence")
            continue
        if evidence_section is None:
            errors.append(
                f"{condition_id}: evidence anchor must identify exactly one Markdown section"
            )
            continue
        required_candidate_keys = PASS_CANDIDATE_KEYS + PASS_CANDIDATE_EXTRAS.get(
            condition_id, ()
        )
        for key in required_candidate_keys:
            revision = candidate.get(key)
            if isinstance(revision, str) and revision not in evidence_section:
                errors.append(
                    f"{condition_id}: evidence is not bound to the {key} candidate {revision}"
                )

    for error in errors:
        print(f"ERROR: {error}")
    print(
        "Summary: "
        f"{counts['pass']} passed, {counts['unverified']} unverified, "
        f"{counts['fail']} failed, {counts['blocked']} blocked, "
        f"{len(errors)} structural errors"
    )
    return 0 if counts["pass"] == len(EXPECTED_IDS) and not errors else 1


if __name__ == "__main__":
    raise SystemExit(main())
