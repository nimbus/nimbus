#!/usr/bin/env python3

from __future__ import annotations

import argparse
import copy
import json
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).resolve().parent))

from schema import default_schema_path, load_json, validate_payload_against_schema  # noqa: E402


REPO_ROOT = Path(__file__).resolve().parents[3]
PROFILE_PATH = (
    REPO_ROOT / "docs" / "private" / "architecture" / "runtime" / "node-faas-compatibility-profile.json"
)
SCHEMA_PATH = default_schema_path("node-faas-compatibility-profile.schema.json")

EXPECTED_STATUSES = {
    "supported_in_process",
    "supported_local_dev_only",
    "service_microvm_required",
    "import_compatible_stub",
    "unsupported",
    "not_applicable_to_faas",
}
EXPECTED_VERIFICATION_STATES = {
    "current_evidence",
    "planned_by_nfrc",
    "requires_service_route",
    "unsupported_boundary",
}
EXPECTED_FAILURE_CLASSES = {
    "supported_bug",
    "unsupported",
    "service_microvm_required",
    "supported_local_dev_only",
    "not_applicable_to_faas",
    "flaky",
    "harness_bug",
}
EXPECTED_LANES = {"node20", "node22", "node24", "node26"}
EXPECTED_PROOF_FIELDS = {
    "initial_wide_run_inventory",
    "focused_fix_or_classification_evidence",
    "final_wide_run_result",
}


def display_path(path: Path) -> str:
    try:
        return str(path.relative_to(REPO_ROOT))
    except ValueError:
        return str(path)


def load_profile(path: Path = PROFILE_PATH) -> dict[str, Any]:
    payload = load_json(path)
    if not isinstance(payload, dict):
        raise SystemExit(f"{display_path(path)} should contain a JSON object")
    return payload


def _ids(items: list[dict[str, Any]], field: str = "id") -> list[str]:
    ids: list[str] = []
    for item in items:
        value = item.get(field)
        if isinstance(value, str):
            ids.append(value)
    return ids


def _require_unique(errors: list[str], values: list[str], label: str) -> None:
    seen: set[str] = set()
    duplicates: set[str] = set()
    for value in values:
        if value in seen:
            duplicates.add(value)
        seen.add(value)
    if duplicates:
        errors.append(f"{label} must be unique: {', '.join(sorted(duplicates))}")


def _require_path(errors: list[str], relative_path: str, owner: str) -> None:
    path = REPO_ROOT / relative_path
    if not path.exists():
        errors.append(f"{owner} references missing path: {relative_path}")


def _validate_status(
    errors: list[str],
    status: Any,
    status_ids: set[str],
    owner: str,
) -> None:
    if not isinstance(status, str) or status not in status_ids:
        errors.append(f"{owner} uses unknown support status: {status!r}")


def _validate_state(
    errors: list[str],
    state: Any,
    states: set[str],
    owner: str,
) -> None:
    if not isinstance(state, str) or state not in states:
        errors.append(f"{owner} uses unknown verification_state: {state!r}")


def _validate_evidence_refs(
    errors: list[str],
    refs: Any,
    evidence_ids: set[str],
    owner: str,
    *,
    required: bool = True,
) -> None:
    if refs is None:
        refs = []
    if not isinstance(refs, list) or not all(isinstance(ref, str) for ref in refs):
        errors.append(f"{owner} evidence_refs must be a string array")
        return
    if required and not refs:
        errors.append(f"{owner} must cite at least one evidence_ref")
    unknown = sorted(set(refs) - evidence_ids)
    if unknown:
        errors.append(f"{owner} references unknown evidence refs: {', '.join(unknown)}")


def validate_profile_payload(profile: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    for schema_error in validate_payload_against_schema(profile, SCHEMA_PATH):
        errors.append(json.dumps(schema_error, sort_keys=True))

    statuses = profile.get("support_statuses", [])
    if not isinstance(statuses, list):
        return errors
    status_ids = set(_ids(statuses))
    if status_ids != EXPECTED_STATUSES:
        errors.append(
            "support_statuses must exactly match expected status vocabulary: "
            + ", ".join(sorted(EXPECTED_STATUSES))
        )
    _require_unique(errors, _ids(statuses), "support_status ids")

    states = profile.get("verification_states", [])
    if not isinstance(states, list) or set(states) != EXPECTED_VERIFICATION_STATES:
        errors.append(
            "verification_states must exactly match expected values: "
            + ", ".join(sorted(EXPECTED_VERIFICATION_STATES))
        )

    failure_classes = profile.get("failure_classes", [])
    if not isinstance(failure_classes, list) or set(failure_classes) != EXPECTED_FAILURE_CLASSES:
        errors.append(
            "failure_classes must exactly match wide-run inventory values: "
            + ", ".join(sorted(EXPECTED_FAILURE_CLASSES))
        )

    strategy = profile.get("wide_then_focused_strategy", {})
    if isinstance(strategy, dict):
        for field in (
            "required",
            "wide_run_inventory_required",
            "focused_fix_required_for_supported_bugs",
            "final_wide_rerun_required",
        ):
            if strategy.get(field) is not True:
                errors.append(f"wide_then_focused_strategy.{field} must be true")
        proof_fields = set(strategy.get("proof_fields", []))
        if proof_fields != EXPECTED_PROOF_FIELDS:
            errors.append(
                "wide_then_focused_strategy.proof_fields must exactly match: "
                + ", ".join(sorted(EXPECTED_PROOF_FIELDS))
            )

    owning_plan = profile.get("owning_plan")
    if isinstance(owning_plan, str):
        _require_path(errors, owning_plan, "owning_plan")

    engine = profile.get("engine", {})
    if isinstance(engine, dict):
        harness = engine.get("new_engine_proof_harness")
        if isinstance(harness, str):
            _require_path(errors, harness, "engine.new_engine_proof_harness")
        claim = str(engine.get("implementation_claim", ""))
        if "libnode" not in claim or "compatibility" not in claim:
            errors.append("engine.implementation_claim must name libnode and compatibility")

    evidence_refs = profile.get("evidence_refs", [])
    evidence_ids = set(_ids(evidence_refs))
    _require_unique(errors, _ids(evidence_refs), "evidence_ref ids")
    for ref in evidence_refs if isinstance(evidence_refs, list) else []:
        owner = f"evidence_ref {ref.get('id', '<unknown>')}"
        path = ref.get("path")
        if isinstance(path, str):
            _require_path(errors, path, owner)
        else:
            errors.append(f"{owner} must include path")

    lane_targets = profile.get("lane_targets", [])
    if isinstance(lane_targets, list):
        lane_ids = {
            lane.get("lane")
            for lane in lane_targets
            if isinstance(lane, dict) and isinstance(lane.get("lane"), str)
        }
        if lane_ids != EXPECTED_LANES:
            errors.append(
                "lane_targets must include exactly node20, node22, node24, and node26"
            )
        default_lanes = [
            lane.get("lane")
            for lane in lane_targets
            if isinstance(lane, dict) and lane.get("product_default_after_nfrc") is True
        ]
        if default_lanes != ["node24"]:
            errors.append("lane_targets must name node24 as the only product_default_after_nfrc")
        for lane in lane_targets:
            if not isinstance(lane, dict):
                continue
            owner = f"lane_target {lane.get('lane', '<unknown>')}"
            _validate_status(errors, lane.get("public_status"), status_ids, owner)
            _validate_state(errors, lane.get("verification_state"), set(states), owner)
            _validate_evidence_refs(errors, lane.get("evidence_refs"), evidence_ids, owner)
            if not isinstance(lane.get("doc_generation"), dict):
                errors.append(f"{owner} must include doc_generation")

    for collection_name in ("api_families", "package_classes"):
        collection = profile.get(collection_name, [])
        if not isinstance(collection, list):
            continue
        _require_unique(errors, _ids(collection), f"{collection_name} ids")
        for item in collection:
            if not isinstance(item, dict):
                continue
            owner = f"{collection_name} {item.get('id', '<unknown>')}"
            _validate_status(errors, item.get("required_status"), status_ids, owner)
            _validate_state(errors, item.get("verification_state"), set(states), owner)
            _validate_evidence_refs(errors, item.get("evidence_refs"), evidence_ids, owner)
            if not isinstance(item.get("doc_section"), str) or not item["doc_section"]:
                errors.append(f"{owner} must include doc_section")

    doc_generation = profile.get("doc_generation", {})
    if isinstance(doc_generation, dict):
        source_manifest = doc_generation.get("source_manifest")
        if source_manifest != display_path(PROFILE_PATH):
            errors.append(
                f"doc_generation.source_manifest must be {display_path(PROFILE_PATH)}"
            )
        targets = doc_generation.get("generated_targets", [])
        if not isinstance(targets, list) or not targets:
            errors.append("doc_generation.generated_targets must be a non-empty array")
        else:
            for target in targets:
                if not isinstance(target, dict):
                    continue
                owner = f"generated_target {target.get('path', '<unknown>')}"
                if not isinstance(target.get("path"), str) or not target["path"]:
                    errors.append(f"{owner} must include path")
                source_arrays = target.get("source_arrays")
                if not isinstance(source_arrays, list) or not source_arrays:
                    errors.append(f"{owner} must include source_arrays")
                else:
                    for source_array in source_arrays:
                        if source_array not in profile:
                            errors.append(
                                f"{owner} source_arrays references unknown field {source_array!r}"
                            )

    doc_claims = profile.get("doc_claims", [])
    if isinstance(doc_claims, list):
        _require_unique(errors, _ids(doc_claims), "doc_claim ids")
        for claim in doc_claims:
            if not isinstance(claim, dict):
                continue
            owner = f"doc_claim {claim.get('id', '<unknown>')}"
            _validate_status(errors, claim.get("status"), status_ids, owner)
            if not isinstance(claim.get("claim"), str) or not claim["claim"]:
                errors.append(f"{owner} must include claim text")
            _validate_evidence_refs(errors, claim.get("evidence_refs"), evidence_ids, owner)

    return errors


def validate_profile(path: Path = PROFILE_PATH) -> list[str]:
    return validate_profile_payload(load_profile(path))


def command_validate(_: argparse.Namespace) -> int:
    errors = validate_profile()
    if errors:
        for error in errors:
            print(f"error: {error}")
        return 1
    profile = load_profile()
    print(
        "validated Node FaaS compatibility profile: "
        f"{len(profile['support_statuses'])} statuses, "
        f"{len(profile['lane_targets'])} lanes, "
        f"{len(profile['api_families'])} API families, "
        f"{len(profile['package_classes'])} package classes, "
        f"{len(profile['doc_claims'])} doc claims"
    )
    return 0


def expect_invalid(mutator_name: str, mutator) -> list[str]:
    payload = copy.deepcopy(load_profile())
    mutator(payload)
    errors = validate_profile_payload(payload)
    if not errors:
        return [f"negative self-test failed: {mutator_name} was accepted"]
    return []


def command_self_test(_: argparse.Namespace) -> int:
    errors: list[str] = []

    def unknown_status(payload: dict[str, Any]) -> None:
        payload["api_families"][0]["required_status"] = "surprisingly_supported"

    def doc_claim_without_evidence(payload: dict[str, Any]) -> None:
        payload["doc_claims"][0]["evidence_refs"] = []

    def unknown_doc_claim_evidence(payload: dict[str, Any]) -> None:
        payload["doc_claims"][0]["evidence_refs"] = ["ghost-evidence"]

    def missing_wide_rerun(payload: dict[str, Any]) -> None:
        payload["wide_then_focused_strategy"]["final_wide_rerun_required"] = False

    for name, mutator in (
        ("unknown_status", unknown_status),
        ("doc_claim_without_evidence", doc_claim_without_evidence),
        ("unknown_doc_claim_evidence", unknown_doc_claim_evidence),
        ("missing_wide_rerun", missing_wide_rerun),
    ):
        errors.extend(expect_invalid(name, mutator))

    if errors:
        for error in errors:
            print(f"error: {error}")
        return 1
    print("Node FaaS compatibility profile negative self-tests passed")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Validate the Nimbus Node FaaS compatibility profile"
    )
    subparsers = parser.add_subparsers(dest="command", required=True)
    subparsers.add_parser("validate").set_defaults(func=command_validate)
    subparsers.add_parser("self-test").set_defaults(func=command_self_test)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
