#!/usr/bin/env python3
import json
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
        if anchor not in proof:
            errors.append(f"{condition_id}: evidence anchor not found")

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
