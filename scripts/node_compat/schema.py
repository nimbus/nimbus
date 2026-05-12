#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def default_schema_root() -> Path:
    return repo_root() / "tests" / "node-compat" / "schemas"


def default_schema_path(name: str) -> Path:
    return default_schema_root() / name


def load_json(path: Path) -> Any:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)


def _json_type_name(value: Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, int):
        return "integer"
    if isinstance(value, float):
        return "number"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    if isinstance(value, dict):
        return "object"
    return type(value).__name__


def _matches_type(value: Any, expected_type: str) -> bool:
    if expected_type == "null":
        return value is None
    if expected_type == "boolean":
        return isinstance(value, bool)
    if expected_type == "integer":
        return isinstance(value, int) and not isinstance(value, bool)
    if expected_type == "number":
        return isinstance(value, (int, float)) and not isinstance(value, bool)
    if expected_type == "string":
        return isinstance(value, str)
    if expected_type == "array":
        return isinstance(value, list)
    if expected_type == "object":
        return isinstance(value, dict)
    return False


def _schema_path(path: str, segment: str) -> str:
    if path == "$":
        return f"$.{segment}"
    return f"{path}.{segment}"


def _array_path(path: str, index: int) -> str:
    return f"{path}[{index}]"


def validate_json_schema_subset(
    instance: Any,
    schema: dict[str, Any],
    path: str = "$",
) -> list[dict[str, Any]]:
    """Validate the repo's small, dependency-free JSON Schema subset."""
    errors: list[dict[str, Any]] = []

    if "const" in schema and instance != schema["const"]:
        errors.append(
            {
                "kind": "schema_const_mismatch",
                "path": path,
                "expected": schema["const"],
                "actual": instance,
            }
        )

    if "enum" in schema and instance not in schema["enum"]:
        errors.append(
            {
                "kind": "schema_enum_mismatch",
                "path": path,
                "allowed": schema["enum"],
                "actual": instance,
            }
        )

    type_spec = schema.get("type")
    if type_spec is not None:
        expected_types = type_spec if isinstance(type_spec, list) else [type_spec]
        if not any(_matches_type(instance, expected_type) for expected_type in expected_types):
            errors.append(
                {
                    "kind": "schema_type_mismatch",
                    "path": path,
                    "expected": expected_types,
                    "actual": _json_type_name(instance),
                }
            )
            return errors

    if isinstance(instance, str):
        min_length = schema.get("minLength")
        if isinstance(min_length, int) and len(instance) < min_length:
            errors.append(
                {
                    "kind": "schema_min_length_mismatch",
                    "path": path,
                    "minimum": min_length,
                    "actual": len(instance),
                }
            )

    if isinstance(instance, (int, float)) and not isinstance(instance, bool):
        minimum = schema.get("minimum")
        if isinstance(minimum, (int, float)) and instance < minimum:
            errors.append(
                {
                    "kind": "schema_minimum_mismatch",
                    "path": path,
                    "minimum": minimum,
                    "actual": instance,
                }
            )

    if isinstance(instance, dict):
        required = schema.get("required", [])
        if isinstance(required, list):
            for key in required:
                if key not in instance:
                    errors.append(
                        {
                            "kind": "schema_required_property_missing",
                            "path": path,
                            "property": key,
                        }
                    )
        properties = schema.get("properties", {})
        if isinstance(properties, dict):
            for key, child_schema in properties.items():
                if key in instance and isinstance(child_schema, dict):
                    errors.extend(
                        validate_json_schema_subset(
                            instance[key], child_schema, _schema_path(path, key)
                        )
                    )
        additional_properties = schema.get("additionalProperties", True)
        if additional_properties is False and isinstance(properties, dict):
            for key in sorted(set(instance) - set(properties)):
                errors.append(
                    {
                        "kind": "schema_additional_property",
                        "path": _schema_path(path, key),
                        "property": key,
                    }
                )
        elif isinstance(additional_properties, dict) and isinstance(properties, dict):
            for key in sorted(set(instance) - set(properties)):
                errors.extend(
                    validate_json_schema_subset(
                        instance[key],
                        additional_properties,
                        _schema_path(path, key),
                    )
                )

    if isinstance(instance, list):
        item_schema = schema.get("items")
        if isinstance(item_schema, dict):
            for index, value in enumerate(instance):
                errors.extend(
                    validate_json_schema_subset(value, item_schema, _array_path(path, index))
                )

    return errors


def validate_payload_against_schema(
    payload: Any,
    schema_path: Path,
) -> list[dict[str, Any]]:
    schema = load_json(schema_path)
    if not isinstance(schema, dict):
        return [
            {
                "kind": "schema_file_invalid",
                "path": str(schema_path),
                "actual": _json_type_name(schema),
            }
        ]
    return validate_json_schema_subset(payload, schema)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Validate Node compatibility JSON artifacts with repo-local schemas"
    )
    subparsers = parser.add_subparsers(dest="command", required=True)
    validate_parser = subparsers.add_parser("validate")
    validate_parser.add_argument("--schema", required=True)
    validate_parser.add_argument("--instance", required=True)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    if args.command != "validate":
        raise AssertionError(f"unhandled command {args.command}")
    schema_path = Path(args.schema)
    if not schema_path.is_absolute():
        schema_path = default_schema_root() / schema_path
    instance_path = Path(args.instance).resolve()
    errors = validate_payload_against_schema(load_json(instance_path), schema_path.resolve())
    if errors:
        for error in errors:
            print(f"error: {json.dumps(error, sort_keys=True)}")
        return 1
    print(f"validated {instance_path} against {schema_path.resolve()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
