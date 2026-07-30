#!/usr/bin/env python3
"""Static contract check for the NNC4.7 sovereignty proof adapter."""

from __future__ import annotations

import argparse
import ast
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))

from nimbus_network_sovereignty_tripwire.evidence import (  # noqa: E402
    EvidenceValidationError,
    validate_evidence,
    validate_reentry_pair,
)


def fail(message: str) -> None:
    raise SystemExit(f"sovereignty tripwire contract failed: {message}")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Verify the NNC4.7 tripwire source and optional live evidence"
    )
    parser.add_argument("--evidence-dir", type=Path)
    parser.add_argument("--predecessor-evidence-dir", type=Path)
    return parser


def load_evidence(evidence_dir: Path, source_root: Path) -> dict[str, object]:
    evidence_path = evidence_dir / "evidence.json"
    if not evidence_path.is_file():
        fail(f"missing live evidence {evidence_path}")
    try:
        document = json.loads(evidence_path.read_text(encoding="utf-8"))
        validate_evidence(
            document,
            evidence_root=evidence_dir,
            source_root=source_root,
        )
    except (OSError, json.JSONDecodeError, EvidenceValidationError) as error:
        fail(f"live evidence is invalid: {error}")
    return document


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if args.predecessor_evidence_dir is not None and args.evidence_dir is None:
        fail("--predecessor-evidence-dir requires --evidence-dir")
    root = Path(__file__).resolve().parent.parent
    required = (
        root / "scripts/nimbus-network-sovereignty-tripwire.sh",
        root / "scripts/nimbus_network_sovereignty_tripwire/__init__.py",
        root / "scripts/nimbus_network_sovereignty_tripwire/__main__.py",
        root / "scripts/nimbus_network_sovereignty_tripwire/environment.py",
        root / "scripts/nimbus_network_sovereignty_tripwire/evidence.py",
        root / "scripts/nimbus_network_sovereignty_tripwire/integrity.py",
        root / "scripts/nimbus_network_sovereignty_tripwire/isolation.py",
        root / "scripts/nimbus_network_sovereignty_tripwire/probe.py",
        root / "scripts/nimbus_network_sovereignty_tripwire/runner.py",
        root / "scripts/nimbus_network_sovereignty_tripwire/synchronization.py",
        root / "scripts/nimbus_network_sovereignty_tripwire/workspace.py",
        root
        / "scripts/nimbus-network-control-plane/sovereignty-tripwire-self-tests.py",
        root
        / "scripts/nimbus-network-control-plane/sovereignty_tripwire_wrapper_harness.py",
        root
        / "scripts/nimbus-network-control-plane/sovereignty-tripwire-self-tests.sh",
        root
        / "docs/private/plans/proof/nimbus-network-control-plane/nnc4.7-local-sovereignty-tripwire.md",
    )
    missing = [
        path.relative_to(root).as_posix() for path in required if not path.is_file()
    ]
    if missing:
        fail("missing required paths: " + ", ".join(missing))

    for path in required:
        if path.suffix == ".py":
            try:
                ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
            except SyntaxError as error:
                fail(f"invalid Python source {path.relative_to(root)}: {error}")

    wrapper = required[0].read_text(encoding="utf-8")
    if "set -euo pipefail" not in wrapper:
        fail("live wrapper is not fail closed")
    wrapper_anchors = (
        "#!/bin/bash -p",
        "unset BASH_ENV ENV CDPATH GLOBIGNORE",
        'PYTHON_BIN="/usr/bin/python3"',
        "unset PYTHONHOME PYTHONPATH",
        'exec "${PYTHON_BIN}" -I -S -c',
        "from nimbus_network_sovereignty_tripwire.runner import main",
        "raise SystemExit(main())",
    )
    for anchor in wrapper_anchors:
        if anchor not in wrapper:
            fail(f"live wrapper is missing isolated-entry anchor {anchor!r}")
    makefile = (root / "Makefile").read_text(encoding="utf-8")
    if "/bin/bash -p scripts/nimbus-network-sovereignty-tripwire.sh" not in makefile:
        fail("canonical Make target does not use the fixed privileged shell")

    evidence = (
        root / "scripts/nimbus_network_sovereignty_tripwire/evidence.py"
    ).read_text(encoding="utf-8")
    for anchor in (
        '"PASS": 0',
        '"SKIPPED": 77',
        "REQUIRED_PASS_ASSERTIONS",
        "artifact digest mismatch",
        "forbidden install/download/network command",
    ):
        if anchor not in evidence:
            fail(f"evidence contract is missing anchor {anchor!r}")

    isolation = (
        root / "scripts/nimbus_network_sovereignty_tripwire/isolation.py"
    ).read_text(encoding="utf-8")
    for anchor in (
        "preflight_decision",
        "OwnedResources",
        "trace=%network",
        "denied_ipv4",
        "denied_ipv6",
        "dns_udp",
        "dns_tcp",
        "cleanup.same_identity_reentry",
    ):
        if anchor not in isolation:
            fail(f"live isolation adapter is missing anchor {anchor!r}")

    print("sovereignty tripwire contract: PASS")
    if args.evidence_dir is not None:
        document = load_evidence(args.evidence_dir, root)
        print("sovereignty tripwire live evidence: PASS")
        if args.predecessor_evidence_dir is not None:
            predecessor = load_evidence(args.predecessor_evidence_dir, root)
            try:
                validate_reentry_pair(predecessor, document)
            except EvidenceValidationError as error:
                fail(f"fresh-process re-entry evidence is invalid: {error}")
            print("sovereignty tripwire fresh-process re-entry: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
