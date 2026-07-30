#!/usr/bin/env python3
"""Deterministic, unprivileged mutation tests for the NNC4.7 tripwire."""

from __future__ import annotations

import copy
import hashlib
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest import mock

REPO_ROOT = Path(__file__).resolve().parents[2]
SELF_TEST_ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(REPO_ROOT / "scripts"))
sys.path.insert(0, str(SELF_TEST_ROOT))

from sovereignty_tripwire_wrapper_harness import run_isolated_wrapper  # noqa: E402

from nimbus_network_sovereignty_tripwire.evidence import (  # noqa: E402
    EVIDENCE_SCHEMA_VERSION,
    REQUIRED_HARNESS_PATHS,
    REQUIRED_PASS_ASSERTIONS,
    REQUIRED_RUNNER_TOOLS,
    EvidenceValidationError,
    validate_evidence,
    validate_reentry_pair,
)
from nimbus_network_sovereignty_tripwire.environment import (  # noqa: E402
    PreflightFacts,
    TripwireConfig,
    TripwireConfigurationError,
    TripwireInterrupted,
    TripwireProofFailure,
    _trusted_tool_path,
    collect_preflight,
    harness_paths,
    preflight_decision,
    source_facts,
)
from nimbus_network_sovereignty_tripwire.isolation import (  # noqa: E402
    CommandRecorder,
    OwnedResources,
    _assert_control,
    _assert_profile,
    _cleanup_attempt,
    _defer_termination_signals,
    _exclusive_host_lock,
    _prepare_output_directory,
    _resource_names,
    _run_owned_mutation,
    _write_new_text,
    _write_nft_rules,
    run_live,
)
from nimbus_network_sovereignty_tripwire.runner import main as tripwire_main  # noqa: E402


def _sha256(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def _assertion(identity: str, expected: object, observed: object) -> dict[str, object]:
    return {
        "id": identity,
        "expected": expected,
        "observed": observed,
        "passed": True,
    }


def _assertions(
    attempts: list[dict[str, object]], runner: dict[str, object]
) -> list[dict[str, object]]:
    controls = [
        attempt["control"]["assertions"]  # type: ignore[index]
        for attempt in attempts
    ]
    profiles = [
        attempt["profile"]["assertions"]  # type: ignore[index]
        for attempt in attempts
    ]
    assertions = [
        _assertion(
            "preflight.named_runner",
            runner["expected_hostname"],
            runner["observed_hostname"],
        ),
        _assertion("preflight.offline_inputs", True, True),
        _assertion("isolation.outer_namespaces", True, [True, True]),
        _assertion(
            "isolation.subject_unprivileged",
            True,
            [control["subject_unprivileged"] for control in controls],
        ),
        _assertion(
            "isolation.peer_forwarding_disabled",
            {"ipv4": 0, "ipv6": 0},
            [
                attempt["setup"]["peer_forwarding"]  # type: ignore[index]
                for attempt in attempts
            ],
        ),
        *[
            _assertion(
                f"control.{name}",
                True,
                [control[name] for control in controls],
            )
            for name in (
                "loopback",
                "private_ipv4",
                "private_ipv6",
                "unenumerated_private_denied",
                "dns_udp",
                "dns_tcp",
                "public_ipv4_denied",
                "public_ipv6_denied",
                "network_trace",
            )
        ],
        _assertion(
            "reset.zero_baseline",
            True,
            [attempt["reset"]["passed"] for attempt in attempts],  # type: ignore[index]
        ),
        *[
            _assertion(
                f"profile.{name}",
                True,
                [profile[name] for profile in profiles],
            )
            for name in ("private_only", "zero_unexpected")
        ],
        _assertion(
            "cleanup.absent",
            True,
            [attempt["cleanup"]["passed"] for attempt in attempts],  # type: ignore[index]
        ),
        _assertion("cleanup.same_identity_reentry", 2, len(attempts)),
        _assertion("evidence.artifacts_authenticated", True, True),
    ]
    assert {row["id"] for row in assertions} == REQUIRED_PASS_ASSERTIONS
    return assertions


def _pass_document(root: Path) -> dict[str, object]:
    runner_id = "nnc47-minicloud"
    names = _resource_names(runner_id)
    resources = {
        "subject_namespace": names.subject_namespace,
        "peer_namespace": names.peer_namespace,
        "subject_interface": names.subject_interface,
        "peer_interface": names.peer_interface,
        "nft_table": names.nft_table,
        "subject_ipv4": names.subject_ipv4,
        "peer_ipv4": names.peer_ipv4,
        "subject_ipv6": names.subject_ipv6,
        "peer_ipv6": names.peer_ipv6,
    }
    exact_counters = {
        "denied_ipv4": 1,
        "denied_ipv6": 1,
        "denied_private": 1,
        "dns_tcp": 1,
        "dns_udp": 1,
        "unexpected": 0,
    }
    zero_counters = {name: 0 for name in exact_counters}
    security = {
        "capamb": 0,
        "capbnd": 0,
        "capeff": 0,
        "capinh": 0,
        "capprm": 0,
        "uids": [65534, 65534, 65534, 65534],
        "gids": [65534, 65534, 65534, 65534],
        "no_new_privs": 1,
    }
    control_probe = {
        "security": security,
        "cap_net_admin": False,
        "subject_unprivileged": True,
        "loopback": True,
        "private_ipv4": True,
        "private_ipv6": True,
        "unenumerated_private_denied": True,
        "dns_udp_attempted": True,
        "dns_tcp_attempted": True,
        "public_ipv4_denied": True,
        "public_ipv6_denied": True,
        "passed": True,
    }
    profile_probe = {
        "security": security,
        "cap_net_admin": False,
        "subject_unprivileged": True,
        "loopback": True,
        "private_ipv4": True,
        "private_ipv6": True,
        "passed": True,
    }
    control_assertions = {
        "subject_unprivileged": True,
        "loopback": True,
        "private_ipv4": True,
        "private_ipv6": True,
        "unenumerated_private_denied": True,
        "dns_udp": True,
        "dns_tcp": True,
        "public_ipv4_denied": True,
        "public_ipv6_denied": True,
        "network_trace": True,
        "no_unclassified_control": True,
    }
    profile_assertions = {
        "subject_unprivileged": True,
        "private_only": True,
        "zero_unexpected": True,
        "zero_dns_capture": True,
        "network_trace_private_only": True,
    }

    def write(relative: str, payload: str) -> Path:
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(payload, encoding="utf-8")
        return path

    tool_paths = {name: f"/usr/bin/{name}" for name in REQUIRED_RUNNER_TOOLS}
    commands: list[dict[str, object]] = []

    def command(
        argv: list[str], *, stdout: str = "", stderr: str = "", exit_code: int = 0
    ) -> None:
        authenticated_argv = [tool_paths.get(token, token) for token in argv]
        index = len(commands) + 1
        stdout_path = write(f"commands/{index:04d}.stdout", stdout)
        stderr_path = write(f"commands/{index:04d}.stderr", stderr)
        commands.append(
            {
                "index": index,
                "argv": authenticated_argv,
                "exit_code": exit_code,
                "process_group": 1000 + index,
                "timed_out": False,
                "interrupted": False,
                "deferred_signals": [],
                "stdout": stdout_path.relative_to(root).as_posix(),
                "stderr": stderr_path.relative_to(root).as_posix(),
            }
        )

    def counter_payload(counters: dict[str, int]) -> str:
        return (
            json.dumps(
                {
                    "nftables": [
                        {
                            "counter": {
                                "family": "inet",
                                "table": names.nft_table,
                                "name": name,
                                "packets": value,
                                "bytes": value * 64,
                            }
                        }
                        for name, value in sorted(counters.items())
                    ]
                },
                sort_keys=True,
            )
            + "\n"
        )

    attempts: list[dict[str, object]] = []
    for attempt_number in (1, 2):
        attempt: dict[str, object] = {
            "attempt": attempt_number,
            "resources": resources,
            "setup": {
                "preexisting_namespaces_absent": True,
                "preexisting_veths_absent": True,
                "peer_forwarding": {"ipv4": 0, "ipv6": 0},
            },
            "control": {
                "probe": control_probe,
                "counters": exact_counters,
                "assertions": control_assertions,
            },
            "reset": {"counters": zero_counters, "passed": True},
            "profile": {
                "probe": profile_probe,
                "counters": zero_counters,
                "assertions": profile_assertions,
            },
            "cleanup": {
                "passed": True,
                "namespaces_absent": True,
                "root_veths_absent": True,
                "errors": [],
                "deferred_signals": [],
            },
        }

        prefix = f"attempt-{attempt_number}"
        write(
            f"{prefix}/topology.txt",
            "\n".join(
                (
                    names.subject_namespace,
                    names.peer_namespace,
                    names.subject_interface,
                    names.peer_interface,
                    names.subject_ipv4,
                    names.peer_ipv4,
                    names.subject_ipv6,
                    names.peer_ipv6,
                )
            )
            + "\n",
        )
        write(
            f"{prefix}/rules.nft",
            "\n".join(
                (
                    f"table inet {names.nft_table} {{",
                    " chain output { type filter hook output priority 0; policy drop;",
                    f"  ip daddr {names.peer_ipv4} udp dport 53 counter name dns_udp",
                    f"  ip daddr {names.peer_ipv4} tcp dport 53 counter name dns_tcp",
                    "  ip daddr 10.253.0.1 counter name denied_private drop",
                    "  meta nfproto ipv4 counter name denied_ipv4 drop",
                    "  meta nfproto ipv6 counter name denied_ipv6 drop",
                    " }",
                    "}",
                )
            )
            + "\n",
        )
        write(
            f"{prefix}/control-dns.txt",
            (
                f"{names.subject_ipv4}.40000 > {names.peer_ipv4}.53: UDP, "
                "nnc47-udp-control.invalid\n"
                f"{names.subject_ipv4}.40001 > {names.peer_ipv4}.53: Flags [S], "
                "nnc47-tcp-control.invalid\n"
            ),
        )
        write(f"{prefix}/profile-dns.txt", "")
        write(
            f"{prefix}/control.strace.1",
            "sin_port=htons(53) "
            + " ".join(
                (
                    "127.0.0.1",
                    names.peer_ipv4,
                    names.peer_ipv6,
                    "10.253.0.1",
                    "192.0.2.1",
                    "2001:db8::1",
                )
            )
            + "\n",
        )
        write(
            f"{prefix}/profile.strace.1",
            " ".join(("127.0.0.1", names.peer_ipv4, names.peer_ipv6)) + "\n",
        )

        command(["ip", "netns", "add", names.subject_namespace])
        command(["ip", "netns", "add", names.peer_namespace])
        command(
            [
                "ip",
                "link",
                "add",
                names.subject_interface,
                "type",
                "veth",
                "peer",
                "name",
                names.peer_interface,
            ]
        )
        command(
            [
                "ip",
                "netns",
                "exec",
                names.peer_namespace,
                "sysctl",
                "-n",
                "net.ipv4.ip_forward",
            ],
            stdout="0\n",
        )
        command(
            [
                "ip",
                "netns",
                "exec",
                names.peer_namespace,
                "sysctl",
                "-n",
                "net.ipv6.conf.all.forwarding",
            ],
            stdout="0\n",
        )
        command(
            [
                "ip",
                "netns",
                "exec",
                names.subject_namespace,
                "nft",
                "-f",
                f"{prefix}/rules.nft",
            ]
        )
        command(
            [
                "ip",
                "netns",
                "exec",
                names.subject_namespace,
                "strace",
                "-ff",
                "setpriv",
                "--bounding-set=-all",
                "--no-new-privs",
                "probe",
                "--mode",
                "control",
                "--dns-ipv4",
                names.peer_ipv4,
            ],
            stdout=json.dumps(control_probe, sort_keys=True) + "\n",
        )
        command(
            [
                "ip",
                "netns",
                "exec",
                names.peer_namespace,
                "tcpdump",
                "port",
                "53",
            ]
        )
        command(
            [
                "ip",
                "netns",
                "exec",
                names.subject_namespace,
                "nft",
                "-j",
                "list",
                "counters",
                "table",
                "inet",
                names.nft_table,
            ],
            stdout=counter_payload(exact_counters),
        )
        command(
            [
                "ip",
                "netns",
                "exec",
                names.subject_namespace,
                "nft",
                "delete",
                "table",
                "inet",
                names.nft_table,
            ]
        )
        command(
            [
                "ip",
                "netns",
                "exec",
                names.subject_namespace,
                "nft",
                "-f",
                f"{prefix}/rules.nft",
            ]
        )
        command(
            [
                "ip",
                "netns",
                "exec",
                names.subject_namespace,
                "nft",
                "-j",
                "list",
                "counters",
                "table",
                "inet",
                names.nft_table,
            ],
            stdout=counter_payload(zero_counters),
        )
        command(
            [
                "ip",
                "netns",
                "exec",
                names.subject_namespace,
                "strace",
                "-ff",
                "setpriv",
                "--bounding-set=-all",
                "--no-new-privs",
                "probe",
                "--mode",
                "profile",
                "--dns-ipv4",
                names.peer_ipv4,
            ],
            stdout=json.dumps(profile_probe, sort_keys=True) + "\n",
        )
        command(
            [
                "ip",
                "netns",
                "exec",
                names.peer_namespace,
                "tcpdump",
                "port",
                "53",
            ]
        )
        command(
            [
                "ip",
                "netns",
                "exec",
                names.subject_namespace,
                "nft",
                "-j",
                "list",
                "counters",
                "table",
                "inet",
                names.nft_table,
            ],
            stdout=counter_payload(zero_counters),
        )
        command(["ip", "netns", "delete", names.subject_namespace])
        command(["ip", "netns", "delete", names.peer_namespace])
        write(
            f"{prefix}/attempt.json",
            json.dumps(attempt, indent=2, sort_keys=True) + "\n",
        )
        attempts.append(attempt)

    write("trace.txt", "network trace\n")
    artifact_paths = sorted(path for path in root.rglob("*") if path.is_file())
    artifacts = [
        {
            "path": path.relative_to(root).as_posix(),
            "size": path.stat().st_size,
            "sha256": _sha256(path.read_bytes()),
        }
        for path in artifact_paths
    ]
    runner: dict[str, object] = {
        "asserted_id": runner_id,
        "expected_hostname": "proof-host",
        "observed_hostname": "proof-host",
        "host_class": "minicloud",
        "provider_kind": "linuxkit",
        "system": "Linux",
        "uid": 0,
        "process_id": 1001,
        "process_start_ticks": 2001,
        "architecture": "x86_64",
        "os_release": "Linux proof-host 6.12.76-linuxkit",
        "boot_id": "11111111-2222-3333-4444-555555555555",
        "kernel": "6.12.76-linuxkit",
        "pid1_command": "/initd",
        "effective_capabilities": "000001ffffffffff",
        "missing_capabilities": [],
        "kvm_access": False,
        "tools": {
            name: {
                "path": path,
                "version": f"{name} 1.0",
            }
            for name, path in tool_paths.items()
        },
    }
    return {
        "schema_version": EVIDENCE_SCHEMA_VERSION,
        "result": {
            "status": "PASS",
            "exit_code": 0,
            "reason": None,
            "phase": "complete",
            "started_at": "2026-07-28T12:00:00+00:00",
            "finished_at": "2026-07-28T12:01:00+00:00",
        },
        "runner": runner,
        "source": {
            "commit": "a" * 40,
            "tree": "b" * 40,
            "dirty": "",
            "harness_sha256": "c" * 64,
            "harness_paths": sorted(REQUIRED_HARNESS_PATHS),
            "harness_files": [
                {
                    "path": path,
                    "size": 1,
                    "sha256": "d" * 64,
                }
                for path in sorted(REQUIRED_HARNESS_PATHS)
            ],
        },
        "inputs": {
            "offline": True,
            "payload_argv": [],
            "prestage_argv": [],
        },
        "attempts": attempts,
        "commands": commands,
        "assertions": _assertions(attempts, runner),
        "artifacts": artifacts,
    }


def _rewrite_attempt_artifact(
    document: dict[str, object], root: Path, attempt_index: int
) -> None:
    relative = f"attempt-{attempt_index}/attempt.json"
    path = root / relative
    attempt = document["attempts"][attempt_index - 1]  # type: ignore[index]
    path.write_text(
        json.dumps(attempt, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    _refresh_artifact(document, root, relative)


def _refresh_artifact(document: dict[str, object], root: Path, relative: str) -> None:
    path = root / relative
    for artifact in document["artifacts"]:  # type: ignore[union-attr]
        if artifact["path"] == relative:
            artifact["size"] = path.stat().st_size
            artifact["sha256"] = _sha256(path.read_bytes())
            return
    raise AssertionError(f"fixture artifact is missing: {relative}")


def _skip_facts(**changes: object) -> PreflightFacts:
    values: dict[str, object] = {
        "system": "Linux",
        "uid": 0,
        "observed_hostname": "proof-host",
        "expected_hostname": "proof-host",
        "host_class": "minicloud",
        "provider_kind": "linuxkit",
        "kvm_access": False,
        "missing_tools": (),
        "unavailable_tool_versions": (),
        "effective_capabilities": 0x1FFFFFFFFFFFFF,
        "missing_capabilities": (),
        "kernel_release": "6.12.76-linuxkit",
        "pid1_command": "/initd",
    }
    values.update(changes)
    return PreflightFacts(**values)  # type: ignore[arg-type]


class EvidenceContractTests(unittest.TestCase):
    def test_canonical_pass_validates(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            validate_evidence(_pass_document(root), evidence_root=root)

    def test_non_linux_pass_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["runner"]["system"] = "Darwin"  # type: ignore[index]
            with self.assertRaisesRegex(EvidenceValidationError, "Linux"):
                validate_evidence(document, evidence_root=root)

    def test_non_root_pass_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["runner"]["uid"] = 501  # type: ignore[index]
            with self.assertRaisesRegex(EvidenceValidationError, "uid 0"):
                validate_evidence(document, evidence_root=root)

    def test_kvm_class_without_kvm_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["runner"]["host_class"] = "kvm"  # type: ignore[index]
            document["runner"]["provider_kind"] = "kvm"  # type: ignore[index]
            with self.assertRaisesRegex(EvidenceValidationError, "KVM"):
                validate_evidence(document, evidence_root=root)

    def test_hostname_mismatch_pass_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["runner"]["observed_hostname"] = "impostor"  # type: ignore[index]
            with self.assertRaisesRegex(EvidenceValidationError, "hostname"):
                validate_evidence(document, evidence_root=root)

    def test_unsafe_runner_identity_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["runner"]["asserted_id"] = "../proof"  # type: ignore[index]
            with self.assertRaisesRegex(EvidenceValidationError, "runner identity"):
                validate_evidence(document, evidence_root=root)

    def test_one_attempt_cannot_claim_cleanup_reentry(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["attempts"] = [{"attempt": 1}]
            with self.assertRaisesRegex(EvidenceValidationError, "two same-identity"):
                validate_evidence(document, evidence_root=root)

    def test_reentry_pair_requires_a_fresh_ordered_process_incarnation(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            predecessor_root = root / "predecessor"
            successor_root = root / "successor"
            predecessor = _pass_document(predecessor_root)
            successor = _pass_document(successor_root)
            successor["runner"]["process_id"] = 1002  # type: ignore[index]
            successor["runner"]["process_start_ticks"] = 3001  # type: ignore[index]
            successor["result"]["started_at"] = "2026-07-28T12:02:00+00:00"  # type: ignore[index]
            successor["result"]["finished_at"] = "2026-07-28T12:03:00+00:00"  # type: ignore[index]
            validate_evidence(predecessor, evidence_root=predecessor_root)
            validate_evidence(successor, evidence_root=successor_root)
            validate_reentry_pair(predecessor, successor)

            same_process = copy.deepcopy(successor)
            same_process["runner"]["process_id"] = 1001  # type: ignore[index]
            same_process["runner"]["process_start_ticks"] = 2001  # type: ignore[index]
            with self.assertRaisesRegex(EvidenceValidationError, "fresh process"):
                validate_reentry_pair(predecessor, same_process)

            overlapping = copy.deepcopy(successor)
            overlapping["result"]["started_at"] = "2026-07-28T12:00:30+00:00"  # type: ignore[index]
            with self.assertRaisesRegex(EvidenceValidationError, "before"):
                validate_reentry_pair(predecessor, overlapping)

    def test_result_intervals_are_complete_and_ordered(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            baseline = _pass_document(root)
            mutations = (
                ("missing start", "started_at", None),
                ("missing finish", "finished_at", None),
                (
                    "inverted",
                    "finished_at",
                    "2026-07-28T11:59:59+00:00",
                ),
            )
            for label, key, value in mutations:
                with self.subTest(label=label):
                    document = copy.deepcopy(baseline)
                    if value is None:
                        document["result"].pop(key)  # type: ignore[union-attr]
                    else:
                        document["result"][key] = value  # type: ignore[index]
                    with self.assertRaisesRegex(
                        EvidenceValidationError,
                        "started_at|finished_at",
                    ):
                        validate_evidence(document, evidence_root=root)

    def test_unavailable_source_commit_or_tree_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            baseline = _pass_document(root)
            for key in ("commit", "tree"):
                with self.subTest(key=key):
                    document = copy.deepcopy(baseline)
                    document["source"][key] = "unavailable: synthetic"  # type: ignore[index]
                    with self.assertRaisesRegex(
                        EvidenceValidationError, f"source {key}"
                    ):
                        validate_evidence(document, evidence_root=root)

    def test_source_identity_is_independently_recomputed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            baseline = _pass_document(root)
            baseline["source"] = source_facts("/usr/bin/git")
            validate_evidence(
                baseline,
                evidence_root=root,
                source_root=REPO_ROOT,
            )
            for key, replacement in (
                ("commit", "0" * 40),
                ("tree", "1" * 40),
                ("dirty", "synthetic dirty claim"),
            ):
                with self.subTest(key=key):
                    document = copy.deepcopy(baseline)
                    document["source"][key] = replacement  # type: ignore[index]
                    with self.assertRaisesRegex(
                        EvidenceValidationError,
                        f"source {key}.*independently observed",
                    ):
                        validate_evidence(
                            document,
                            evidence_root=root,
                            source_root=REPO_ROOT,
                        )

    def test_unavailable_source_dirty_state_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["source"]["dirty"] = "unavailable: timeout"  # type: ignore[index]
            with self.assertRaisesRegex(EvidenceValidationError, "dirty-state"):
                validate_evidence(document, evidence_root=root)

    def test_executable_source_manifest_is_exact_and_includes_entrypoint(self) -> None:
        self.assertEqual(
            {path.relative_to(REPO_ROOT).as_posix() for path in harness_paths()},
            REQUIRED_HARNESS_PATHS,
        )
        self.assertIn(
            "scripts/nimbus_network_sovereignty_tripwire/__main__.py",
            REQUIRED_HARNESS_PATHS,
        )
        self.assertIn(
            "scripts/nimbus_network_sovereignty_tripwire/environment.py",
            REQUIRED_HARNESS_PATHS,
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            baseline = _pass_document(root)
            for key in ("harness_paths", "harness_files"):
                with self.subTest(key=key):
                    document = copy.deepcopy(baseline)
                    document["source"][key] = document["source"][key][1:]  # type: ignore[index]
                    with self.assertRaisesRegex(EvidenceValidationError, "harness"):
                        validate_evidence(document, evidence_root=root)

    def test_pass_requires_runner_capabilities_tools_and_substrate(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            baseline = _pass_document(root)
            mutations = {
                "capability": ("effective_capabilities", "0000000000000000"),
                "missing_capability": (
                    "missing_capabilities",
                    ["CAP_NET_ADMIN"],
                ),
                "provider": ("provider_kind", "kvm"),
                "kernel": ("kernel", "6.12.0-generic"),
                "pid1": ("pid1_command", "/sbin/init"),
            }
            for label, (key, value) in mutations.items():
                with self.subTest(label=label):
                    document = copy.deepcopy(baseline)
                    document["runner"][key] = value  # type: ignore[index]
                    with self.assertRaises(EvidenceValidationError):
                        validate_evidence(document, evidence_root=root)
            document = copy.deepcopy(baseline)
            document["runner"]["tools"].pop("sysctl")  # type: ignore[index]
            with self.assertRaisesRegex(EvidenceValidationError, "tool inventory"):
                validate_evidence(document, evidence_root=root)

    def test_pass_is_derived_from_attempt_and_command_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            baseline = _pass_document(root)
            mutations = (
                ("counter", ("attempts", 0, "control", "counters", "dns_udp"), 2),
                ("forwarding", ("attempts", 0, "setup", "peer_forwarding", "ipv4"), 1),
                ("cleanup", ("attempts", 0, "cleanup", "errors"), ["synthetic"]),
                ("timeout", ("commands", 0, "timed_out"), True),
            )
            for label, path, value in mutations:
                with self.subTest(label=label):
                    document = copy.deepcopy(baseline)
                    target: object = document
                    for component in path[:-1]:
                        target = target[component]  # type: ignore[index]
                    target[path[-1]] = value  # type: ignore[index]
                    with self.assertRaises(EvidenceValidationError):
                        validate_evidence(document, evidence_root=root)

    def test_each_required_assertion_is_independently_mandatory(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            baseline = _pass_document(root)
            for identity in sorted(REQUIRED_PASS_ASSERTIONS):
                with self.subTest(identity=identity):
                    document = copy.deepcopy(baseline)
                    document["assertions"] = [
                        row
                        for row in document["assertions"]  # type: ignore[index]
                        if row["id"] != identity
                    ]
                    with self.assertRaisesRegex(
                        EvidenceValidationError, "missing required assertions"
                    ):
                        validate_evidence(document, evidence_root=root)

    def test_each_failed_required_assertion_rejects_pass(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            baseline = _pass_document(root)
            for identity in sorted(REQUIRED_PASS_ASSERTIONS):
                with self.subTest(identity=identity):
                    document = copy.deepcopy(baseline)
                    for row in document["assertions"]:  # type: ignore[index]
                        if row["id"] == identity:
                            row["passed"] = False
                            row["observed"] = False
                    with self.assertRaisesRegex(
                        EvidenceValidationError, "failed assertions"
                    ):
                        validate_evidence(document, evidence_root=root)

    def test_duplicate_assertion_identity_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["assertions"].append(  # type: ignore[union-attr]
                copy.deepcopy(document["assertions"][0])  # type: ignore[index]
            )
            with self.assertRaisesRegex(EvidenceValidationError, "duplicate assertion"):
                validate_evidence(document, evidence_root=root)

    def test_pass_with_reason_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["result"]["reason"] = "skipped"  # type: ignore[index]
            with self.assertRaisesRegex(EvidenceValidationError, "PASS cannot"):
                validate_evidence(document, evidence_root=root)

    def test_skip_translated_to_zero_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["result"] = {
                "status": "SKIPPED",
                "exit_code": 0,
                "reason": "Darwin",
            }
            document["assertions"] = []
            document["attempts"] = []
            with self.assertRaisesRegex(EvidenceValidationError, "exit 77"):
                validate_evidence(document, evidence_root=root)

    def test_skip_with_passing_assertion_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["result"] = {
                "status": "SKIPPED",
                "exit_code": 77,
                "reason": "missing KVM",
            }
            with self.assertRaisesRegex(
                EvidenceValidationError, "cannot carry passing"
            ):
                validate_evidence(document, evidence_root=root)

    def test_fail_with_zero_or_77_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for exit_code in (0, 77):
                with self.subTest(exit_code=exit_code):
                    document = _pass_document(root)
                    document["result"] = {
                        "status": "FAIL",
                        "exit_code": exit_code,
                        "reason": "synthetic",
                    }
                    document["assertions"] = []
                    document["attempts"] = []
                    with self.assertRaisesRegex(EvidenceValidationError, "FAIL"):
                        validate_evidence(document, evidence_root=root)

    def test_forbidden_install_download_and_pull_commands_are_rejected(self) -> None:
        cases = (
            ["apt-get", "install", "tcpdump"],
            ["curl", "https://example.invalid/tool"],
            ["git", "clone", "https://example.invalid/repo"],
            ["podman", "pull", "busybox:latest"],
            ["sh", "-c", "curl https://example.invalid/tool"],
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for argv in cases:
                with self.subTest(argv=argv):
                    document = _pass_document(root)
                    document["commands"] = [{"argv": argv, "exit_code": 0}]
                    with self.assertRaisesRegex(EvidenceValidationError, "forbidden"):
                        validate_evidence(document, evidence_root=root)

    def test_unmanifested_raw_artifact_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["artifacts"] = [
                artifact
                for artifact in document["artifacts"]  # type: ignore[index]
                if artifact["path"] != "trace.txt"
            ]
            with self.assertRaisesRegex(
                EvidenceValidationError, "artifact census mismatch"
            ):
                validate_evidence(document, evidence_root=root)

    def test_contradictory_passing_assertion_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["assertions"][0]["observed"] = "contradiction"  # type: ignore[index]
            with self.assertRaisesRegex(EvidenceValidationError, "contradicts"):
                validate_evidence(document, evidence_root=root)

    def test_incomplete_phase_assertion_set_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["attempts"][0]["control"]["assertions"].pop("dns_udp")  # type: ignore[index]
            _rewrite_attempt_artifact(document, root, 1)
            with self.assertRaisesRegex(EvidenceValidationError, "control assertions"):
                validate_evidence(document, evidence_root=root)

    def test_subject_capability_summary_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["attempts"][0]["control"]["probe"]["security"]["capbnd"] = 1  # type: ignore[index]
            _rewrite_attempt_artifact(document, root, 1)
            with self.assertRaisesRegex(EvidenceValidationError, "retained authority"):
                validate_evidence(document, evidence_root=root)

    def test_raw_probe_failure_cannot_be_hidden_by_passing_summaries(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            probe = document["attempts"][0]["control"]["probe"]  # type: ignore[index]
            probe["loopback"] = False
            _rewrite_attempt_artifact(document, root, 1)
            control_command = next(
                command
                for command in document["commands"]  # type: ignore[union-attr]
                if "--mode" in command["argv"]
                and command["argv"][command["argv"].index("--mode") + 1] == "control"
            )
            stdout = control_command["stdout"]
            (root / stdout).write_text(  # type: ignore[arg-type]
                json.dumps(probe, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            _refresh_artifact(document, root, stdout)  # type: ignore[arg-type]
            with self.assertRaisesRegex(EvidenceValidationError, "not derived"):
                validate_evidence(document, evidence_root=root)

    def test_failed_required_effect_command_rejects_pass(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["commands"][0]["exit_code"] = 1  # type: ignore[index]
            with self.assertRaisesRegex(EvidenceValidationError, "unexpected exit"):
                validate_evidence(document, evidence_root=root)

    def test_untrusted_effect_tool_path_rejects_pass(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["commands"][0]["argv"][0] = "/tmp/ip"  # type: ignore[index]
            with self.assertRaisesRegex(
                EvidenceValidationError,
                "unauthenticated tool path",
            ):
                validate_evidence(document, evidence_root=root)

    def test_missing_artifact_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            (root / "trace.txt").unlink()
            with self.assertRaisesRegex(EvidenceValidationError, "missing regular"):
                validate_evidence(document, evidence_root=root)

    def test_tampered_artifact_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            (root / "trace.txt").write_text("tampered\n", encoding="utf-8")
            with self.assertRaisesRegex(
                EvidenceValidationError, "size mismatch|digest mismatch"
            ):
                validate_evidence(document, evidence_root=root)

    def test_symlink_artifact_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            target = root / "target.txt"
            target.write_text("network trace\n", encoding="utf-8")
            (root / "trace.txt").unlink()
            (root / "trace.txt").symlink_to(target)
            with self.assertRaisesRegex(EvidenceValidationError, "missing regular"):
                validate_evidence(document, evidence_root=root)

    def test_path_traversal_artifact_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            document = _pass_document(root)
            document["artifacts"][0]["path"] = "../trace.txt"  # type: ignore[index]
            with self.assertRaisesRegex(EvidenceValidationError, "unsafe artifact"):
                validate_evidence(document, evidence_root=root)


class PreflightTests(unittest.TestCase):
    def test_non_linux_is_skipped(self) -> None:
        self.assertEqual(preflight_decision(_skip_facts(system="Darwin"))[0], "SKIPPED")

    def test_non_linux_does_not_mask_invalid_named_runner_identity(self) -> None:
        facts = _skip_facts(system="Darwin", observed_hostname="impostor")
        self.assertEqual(preflight_decision(facts)[0], "FAIL")

    def test_missing_root_is_skipped(self) -> None:
        self.assertEqual(preflight_decision(_skip_facts(uid=501))[0], "SKIPPED")

    def test_missing_required_kvm_is_skipped(self) -> None:
        facts = _skip_facts(host_class="kvm", provider_kind="kvm", kvm_access=False)
        self.assertEqual(preflight_decision(facts)[0], "SKIPPED")

    def test_missing_tool_is_skipped(self) -> None:
        facts = _skip_facts(missing_tools=("strace",))
        status, reason = preflight_decision(facts)
        self.assertEqual(status, "SKIPPED")
        self.assertIn("strace", reason or "")

    def test_each_missing_effective_capability_is_skipped(self) -> None:
        for capability in (
            "CAP_KILL",
            "CAP_SETGID",
            "CAP_SETUID",
            "CAP_SETPCAP",
            "CAP_NET_ADMIN",
            "CAP_NET_RAW",
            "CAP_SYS_ADMIN",
        ):
            with self.subTest(capability=capability):
                facts = _skip_facts(
                    effective_capabilities=0,
                    missing_capabilities=(capability,),
                )
                status, reason = preflight_decision(facts)
                self.assertEqual(status, "SKIPPED")
                self.assertIn(capability, reason or "")

    def test_identity_and_provider_mismatch_are_never_downgraded_to_skip(self) -> None:
        hostname = _skip_facts(
            observed_hostname="impostor",
            uid=501,
            missing_capabilities=("CAP_NET_ADMIN",),
            missing_tools=("nft",),
        )
        self.assertEqual(preflight_decision(hostname)[0], "FAIL")
        provider = _skip_facts(provider_kind="kvm", uid=501)
        self.assertEqual(preflight_decision(provider)[0], "FAIL")

    def test_minicloud_requires_observed_linuxkit_host_substrate(self) -> None:
        for changes in (
            {"kernel_release": "6.12.0-generic"},
            {"pid1_command": "/sbin/init"},
        ):
            with self.subTest(changes=changes):
                status, _ = preflight_decision(_skip_facts(**changes))
                self.assertEqual(status, "SKIPPED")

    def test_mismatched_named_host_is_configuration_failure(self) -> None:
        facts = _skip_facts(observed_hostname="impostor")
        self.assertEqual(preflight_decision(facts)[0], "FAIL")

    def test_admitted_minicloud_does_not_invent_kvm(self) -> None:
        status, _ = preflight_decision(_skip_facts(kvm_access=False))
        self.assertEqual(status, "ADMITTED")

    def test_identity_rejection_executes_no_external_tool(self) -> None:
        config = TripwireConfig(
            runner_id="nnc47-minicloud",
            expected_hostname="expected-host",
            host_class="minicloud",
            provider_kind="linuxkit",
            output_dir=Path("/tmp/unused-nnc47-output"),
            repeat=2,
            command_timeout_seconds=10,
        )
        with (
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment.platform.system",
                return_value="Linux",
            ),
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment.platform.node",
                return_value="impostor",
            ),
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment.os.geteuid",
                return_value=0,
            ),
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment._read_cap_eff",
                return_value=0x1FFFFFFFFFFFFF,
            ),
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment._pid1_command",
                return_value="/initd",
            ),
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment.platform.release",
                return_value="6.12.76-linuxkit",
            ),
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment._trusted_tool_path"
            ) as discover,
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment._tool_version"
            ) as version,
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment.platform.platform"
            ) as unsafe_platform,
        ):
            facts, detail = collect_preflight(config)
        self.assertEqual(preflight_decision(facts)[0], "FAIL")
        self.assertEqual(detail["tools"], {})
        discover.assert_not_called()
        version.assert_not_called()
        unsafe_platform.assert_not_called()

    def test_admitted_runner_records_only_trusted_absolute_tool_paths(self) -> None:
        config = TripwireConfig(
            runner_id="nnc47-minicloud",
            expected_hostname="proof-host",
            host_class="minicloud",
            provider_kind="linuxkit",
            output_dir=Path("/tmp/unused-nnc47-output"),
            repeat=2,
            command_timeout_seconds=10,
        )

        def trusted(name: str) -> str:
            return f"/usr/bin/{name}"

        with (
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment.platform.system",
                return_value="Linux",
            ),
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment.platform.node",
                return_value="proof-host",
            ),
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment.os.geteuid",
                return_value=0,
            ),
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment._read_cap_eff",
                return_value=0x1FFFFFFFFFFFFF,
            ),
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment._pid1_command",
                return_value="/initd",
            ),
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment.platform.release",
                return_value="6.12.76-linuxkit",
            ),
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment._trusted_tool_path",
                side_effect=trusted,
            ),
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment._tool_version",
                side_effect=lambda path: f"{path} 1.0",
            ),
        ):
            facts, detail = collect_preflight(config)
        self.assertEqual(preflight_decision(facts), ("ADMITTED", None))
        self.assertEqual(
            {name: record["path"] for name, record in detail["tools"].items()},
            {name: f"/usr/bin/{name}" for name in REQUIRED_RUNNER_TOOLS},
        )

    def test_root_tool_discovery_rejects_untrusted_search_roots(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            search_root = Path(temporary)
            search_root.chmod(0o777)
            tool = search_root / "ip"
            tool.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            tool.chmod(0o755)
            with (
                mock.patch(
                    "nimbus_network_sovereignty_tripwire.environment.TRUSTED_TOOL_DIRECTORIES",
                    (search_root,),
                ),
                mock.patch(
                    "nimbus_network_sovereignty_tripwire.environment.os.geteuid",
                    return_value=0,
                ),
            ):
                self.assertIsNone(_trusted_tool_path("ip"))

    def test_tool_discovery_returns_the_stable_resolved_executable(self) -> None:
        expected = Path("/usr/bin/python3").resolve(strict=True)
        with (
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment.TRUSTED_TOOL_DIRECTORIES",
                (Path("/usr/bin"),),
            ),
            mock.patch(
                "nimbus_network_sovereignty_tripwire.environment.os.geteuid",
                return_value=501,
            ),
        ):
            self.assertEqual(_trusted_tool_path("python3"), str(expected))

    def test_tool_discovery_rejects_a_symlink_to_the_wrong_binary(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            search_root = Path(temporary)
            (search_root / "ip").symlink_to("/bin/sh")
            with (
                mock.patch(
                    "nimbus_network_sovereignty_tripwire.environment.TRUSTED_TOOL_DIRECTORIES",
                    (search_root,),
                ),
                mock.patch(
                    "nimbus_network_sovereignty_tripwire.environment.os.geteuid",
                    return_value=501,
                ),
            ):
                self.assertIsNone(_trusted_tool_path("ip"))

    def test_cli_rejects_repeat_counts_other_than_two_before_effects(self) -> None:
        with mock.patch(
            "nimbus_network_sovereignty_tripwire.runner.run_live"
        ) as effect:
            with self.assertRaisesRegex(SystemExit, "2"):
                tripwire_main(
                    [
                        "--runner-id",
                        "nnc47-minicloud",
                        "--expected-hostname",
                        "proof-host",
                        "--host-class",
                        "minicloud",
                        "--provider-kind",
                        "linuxkit",
                        "--output-dir",
                        "/tmp/unused-nnc47-output",
                        "--repeat",
                        "3",
                    ]
                )
            effect.assert_not_called()

    def test_skipped_preflight_never_enters_effect_adapter(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary).resolve() / "evidence"
            config = TripwireConfig(
                runner_id="nnc47-darwin",
                expected_hostname="proof-host",
                host_class="minicloud",
                provider_kind="linuxkit",
                output_dir=output,
                repeat=2,
                command_timeout_seconds=10,
            )
            facts = _skip_facts(system="Darwin")
            runner = {
                "asserted_id": config.runner_id,
                "expected_hostname": "proof-host",
                "observed_hostname": "proof-host",
                "host_class": "minicloud",
                "provider_kind": "linuxkit",
                "system": "Darwin",
                "uid": 501,
                "kvm_access": False,
            }
            source = {"commit": "a", "tree": "b", "dirty": "", "harness_sha256": "c"}
            with (
                mock.patch(
                    "nimbus_network_sovereignty_tripwire.isolation.source_facts",
                    return_value=source,
                ),
                mock.patch(
                    "nimbus_network_sovereignty_tripwire.isolation.collect_preflight",
                    return_value=(facts, runner),
                ),
                mock.patch(
                    "nimbus_network_sovereignty_tripwire.isolation._run_attempt"
                ) as effect,
            ):
                self.assertEqual(run_live(config), 77)
                effect.assert_not_called()
            evidence = json.loads(
                (output / "evidence.json").read_text(encoding="utf-8")
            )
            self.assertEqual(evidence["result"]["status"], "SKIPPED")
            self.assertEqual(evidence["result"]["exit_code"], 77)


class DetectorTests(unittest.TestCase):
    def setUp(self) -> None:
        self.names = _resource_names("nnc47-minicloud")
        self.probe = {
            "cap_net_admin": False,
            "subject_unprivileged": True,
            "loopback": True,
            "private_ipv4": True,
            "private_ipv6": True,
            "unenumerated_private_denied": True,
            "dns_udp_attempted": True,
            "dns_tcp_attempted": True,
            "public_ipv4_denied": True,
            "public_ipv6_denied": True,
        }
        self.counters = {
            "dns_udp": 1,
            "dns_tcp": 1,
            "denied_private": 1,
            "denied_ipv4": 1,
            "denied_ipv6": 1,
            "unexpected": 0,
        }
        self.capture = (
            f"{self.names.subject_ipv4}.40000 > {self.names.peer_ipv4}.53: "
            "UDP, nnc47-udp-control.invalid\n"
            f"{self.names.subject_ipv4}.40000 > {self.names.peer_ipv4}.53: Flags [S], "
            "nnc47-tcp-control.invalid\n"
        )
        self.trace = "sin_port=htons(53) " + " ".join(
            (
                "127.0.0.1",
                self.names.peer_ipv4,
                self.names.peer_ipv6,
                "10.253.0.1",
                "192.0.2.1",
                "2001:db8::1",
            )
        )

    def test_complete_positive_controls_pass(self) -> None:
        result = _assert_control(
            self.probe, self.counters, self.capture, self.trace, self.names
        )
        self.assertTrue(all(result.values()))

    def test_each_counter_or_capture_detector_fails_independently(self) -> None:
        mutations = {
            "dns_udp_counter": ("dns_udp", 0, "dns_udp"),
            "dns_tcp_counter": ("dns_tcp", 0, "dns_tcp"),
            "private_counter": ("denied_private", 0, "unenumerated_private_denied"),
            "ipv4_counter": ("denied_ipv4", 0, "public_ipv4_denied"),
            "ipv6_counter": ("denied_ipv6", 0, "public_ipv6_denied"),
            "unclassified_counter": ("unexpected", 1, "no_unclassified_control"),
        }
        for label, (counter, value, assertion) in mutations.items():
            with self.subTest(label=label):
                counters = dict(self.counters)
                counters[counter] = value
                result = _assert_control(
                    self.probe, counters, self.capture, self.trace, self.names
                )
                self.assertFalse(result[assertion])
        for label, capture, assertion in (
            ("udp_capture", self.capture.replace("UDP", "REMOVED"), "dns_udp"),
            (
                "tcp_capture",
                self.capture.replace("Flags [S]", "Flags [.]").replace(
                    "nnc47-tcp-control.invalid", "removed.invalid"
                ),
                "dns_tcp",
            ),
        ):
            with self.subTest(label=label):
                result = _assert_control(
                    self.probe, self.counters, capture, self.trace, self.names
                )
                self.assertFalse(result[assertion])
        for probe_key, assertion in (
            ("dns_udp_attempted", "dns_udp"),
            ("dns_tcp_attempted", "dns_tcp"),
        ):
            with self.subTest(probe_key=probe_key):
                probe = dict(self.probe)
                probe[probe_key] = False
                result = _assert_control(
                    probe, self.counters, self.capture, self.trace, self.names
                )
                self.assertFalse(result[assertion])

    def test_each_probe_control_fails_independently(self) -> None:
        for probe_key, assertion in (
            ("subject_unprivileged", "subject_unprivileged"),
            ("loopback", "loopback"),
            ("private_ipv4", "private_ipv4"),
            ("private_ipv6", "private_ipv6"),
            ("unenumerated_private_denied", "unenumerated_private_denied"),
            ("public_ipv4_denied", "public_ipv4_denied"),
            ("public_ipv6_denied", "public_ipv6_denied"),
        ):
            with self.subTest(probe_key=probe_key):
                probe = dict(self.probe)
                probe[probe_key] = False
                result = _assert_control(
                    probe, self.counters, self.capture, self.trace, self.names
                )
                self.assertFalse(result[assertion])

    def test_duplicate_control_packet_or_query_is_rejected(self) -> None:
        for counter, assertion in (
            ("denied_private", "unenumerated_private_denied"),
            ("denied_ipv4", "public_ipv4_denied"),
            ("denied_ipv6", "public_ipv6_denied"),
            ("dns_udp", "dns_udp"),
            ("dns_tcp", "dns_tcp"),
        ):
            with self.subTest(counter=counter):
                counters = dict(self.counters)
                counters[counter] = 2
                result = _assert_control(
                    self.probe, counters, self.capture, self.trace, self.names
                )
                self.assertFalse(result[assertion])
        duplicated = self.capture + self.capture
        result = _assert_control(
            self.probe, self.counters, duplicated, self.trace, self.names
        )
        self.assertFalse(result["dns_udp"])
        self.assertFalse(result["dns_tcp"])

    def test_each_expected_trace_destination_is_mandatory(self) -> None:
        for address in (
            "127.0.0.1",
            self.names.peer_ipv4,
            self.names.peer_ipv6,
            "10.253.0.1",
            "192.0.2.1",
            "2001:db8::1",
        ):
            with self.subTest(address=address):
                result = _assert_control(
                    self.probe,
                    self.counters,
                    self.capture,
                    self.trace.replace(address, "removed"),
                    self.names,
                )
                self.assertFalse(result["network_trace"])

    def test_reset_profile_rejects_each_residual_counter(self) -> None:
        profile_probe = {
            "cap_net_admin": False,
            "subject_unprivileged": True,
            "loopback": True,
            "private_ipv4": True,
            "private_ipv6": True,
        }
        zero = {name: 0 for name in self.counters}
        profile_trace = " ".join(
            ("127.0.0.1", self.names.peer_ipv4, self.names.peer_ipv6)
        )
        baseline = _assert_profile(profile_probe, zero, "", profile_trace, self.names)
        self.assertTrue(all(baseline.values()))
        for counter in zero:
            with self.subTest(counter=counter):
                counters = dict(zero)
                counters[counter] = 1
                result = _assert_profile(
                    profile_probe, counters, "", profile_trace, self.names
                )
                self.assertFalse(result["zero_unexpected"])

    def test_reset_profile_rejects_dns_or_forbidden_trace(self) -> None:
        profile_probe = {
            "cap_net_admin": False,
            "subject_unprivileged": True,
            "loopback": True,
            "private_ipv4": True,
            "private_ipv6": True,
        }
        zero = {name: 0 for name in self.counters}
        profile_trace = " ".join(
            ("127.0.0.1", self.names.peer_ipv4, self.names.peer_ipv6)
        )
        dns = _assert_profile(
            profile_probe, zero, "unexpected DNS", profile_trace, self.names
        )
        self.assertFalse(dns["zero_dns_capture"])
        for address in ("10.253.0.1", "192.0.2.1", "2001:db8::1"):
            with self.subTest(address=address):
                result = _assert_profile(
                    profile_probe,
                    zero,
                    "",
                    profile_trace + " " + address,
                    self.names,
                )
                self.assertFalse(result["network_trace_private_only"])

    def test_dns_rule_is_scoped_to_the_enumerated_private_peer(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "rules.nft"
            _write_nft_rules(path, self.names)
            rules = path.read_text(encoding="utf-8")
        self.assertIn(f"ip daddr {self.names.peer_ipv4} udp dport 53", rules)
        self.assertIn(f"ip daddr {self.names.peer_ipv4} tcp dport 53", rules)
        self.assertNotRegex(rules, r"(?m)^    udp dport 53")
        self.assertNotRegex(rules, r"(?m)^    tcp dport 53")


class _FakeCompleted:
    def __init__(self, returncode: int, stdout: str = "") -> None:
        self.returncode = returncode
        self.stdout = stdout
        self.stderr = ""


class _CollisionRecorder:
    def __init__(self, names: object) -> None:
        self.names = names
        self.argv: list[list[str]] = []

    def run(
        self, argv: list[str], *, check: bool = True, timeout: int | None = None
    ) -> _FakeCompleted:
        del check, timeout
        self.argv.append(argv)
        if argv == ["ip", "netns", "list"]:
            return _FakeCompleted(
                0,
                f"{self.names.subject_namespace}\n{self.names.peer_namespace}\n",
            )
        if argv[:3] == ["ip", "link", "show"]:
            return _FakeCompleted(0)
        raise AssertionError(f"unexpected mutating cleanup command: {argv}")


class _CleanupFailureRecorder:
    def __init__(self, names: object) -> None:
        self.names = names
        self.argv: list[list[str]] = []

    def run(
        self, argv: list[str], *, check: bool = True, timeout: int | None = None
    ) -> _FakeCompleted:
        del check, timeout
        self.argv.append(argv)
        if argv == ["ip", "netns", "delete", self.names.subject_namespace]:
            raise TripwireProofFailure("synthetic subject cleanup failure")
        if argv == ["ip", "netns", "delete", self.names.peer_namespace]:
            return _FakeCompleted(0)
        if argv == ["ip", "netns", "list"]:
            return _FakeCompleted(0, f"{self.names.subject_namespace}\n")
        if argv[:3] == ["ip", "link", "show"]:
            return _FakeCompleted(1)
        raise AssertionError(f"unexpected cleanup command: {argv}")


class CleanupSafetyTests(unittest.TestCase):
    def test_evidence_output_requires_a_new_non_symlink_directory(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            parent = Path(temporary).resolve()
            existing = parent / "existing"
            existing.mkdir()
            with self.assertRaisesRegex(
                TripwireConfigurationError,
                "must not already exist",
            ):
                _prepare_output_directory(existing)

            target = parent / "target"
            target.mkdir()
            linked = parent / "linked"
            linked.symlink_to(target, target_is_directory=True)
            with self.assertRaisesRegex(
                TripwireConfigurationError,
                "must not already exist",
            ):
                _prepare_output_directory(linked)

            safe_target = parent / "safe-target"
            safe_target.mkdir()
            intermediate = parent / "intermediate"
            intermediate.symlink_to(safe_target, target_is_directory=True)
            with self.assertRaisesRegex(
                TripwireConfigurationError,
                "symlink component",
            ):
                _prepare_output_directory(intermediate / "evidence")

            fresh = parent / "fresh"
            _prepare_output_directory(fresh)
            self.assertTrue(fresh.is_dir())
            self.assertEqual(fresh.stat().st_mode & 0o777, 0o700)

            unsafe_parent = parent / "unsafe-parent"
            unsafe_parent.mkdir(mode=0o777)
            unsafe_parent.chmod(0o777)
            with (
                mock.patch(
                    "nimbus_network_sovereignty_tripwire.workspace.os.geteuid",
                    return_value=0,
                ),
                self.assertRaisesRegex(
                    TripwireConfigurationError,
                    "root-owned and non-writable",
                ),
            ):
                _prepare_output_directory(unsafe_parent / "root-evidence")

    def test_evidence_file_creation_is_exclusive_and_never_follows_symlinks(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            existing = root / "existing.txt"
            _write_new_text(existing, "first\n")
            with self.assertRaises(FileExistsError):
                _write_new_text(existing, "second\n")
            self.assertEqual(existing.read_text(encoding="utf-8"), "first\n")

            target = root / "target.txt"
            target.write_text("protected\n", encoding="utf-8")
            linked = root / "linked.txt"
            linked.symlink_to(target)
            with self.assertRaises(OSError):
                _write_new_text(linked, "overwrite\n")
            self.assertEqual(target.read_text(encoding="utf-8"), "protected\n")

    def test_owned_effect_is_registered_before_evidence_write_failure(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            recorder = CommandRecorder(Path(temporary), timeout_seconds=10)
            owned: set[str] = set()
            with (
                mock.patch(
                    "nimbus_network_sovereignty_tripwire.isolation._write_new_text",
                    side_effect=OSError("synthetic evidence failure"),
                ),
                self.assertRaisesRegex(OSError, "synthetic evidence failure"),
            ):
                _run_owned_mutation(
                    recorder,
                    [sys.executable, "-c", "raise SystemExit(0)"],
                    lambda: owned.add("effect"),
                )
            self.assertEqual(owned, {"effect"})

    def test_command_recorder_substitutes_and_records_authenticated_tools(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            recorder = CommandRecorder(
                Path(temporary),
                timeout_seconds=10,
                tool_paths={"python3": sys.executable},
            )
            recorder.run(["python3", "-c", "raise SystemExit(0)"])
            self.assertEqual(recorder.records[0]["argv"][0], sys.executable)

    def test_preexisting_collision_is_never_deleted_by_cleanup(self) -> None:
        names = _resource_names("nnc47-minicloud")
        recorder = _CollisionRecorder(names)
        result = _cleanup_attempt(
            recorder,  # type: ignore[arg-type]
            names,
            OwnedResources(namespaces=set(), root_interfaces=set()),
            peer=None,
            capture=None,
        )
        self.assertFalse(result["passed"])
        self.assertFalse(
            any("delete" in argv for argv in recorder.argv),
            recorder.argv,
        )

    def test_cleanup_continues_after_error_and_overrides_pass(self) -> None:
        names = _resource_names("nnc47-minicloud")
        recorder = _CleanupFailureRecorder(names)
        result = _cleanup_attempt(
            recorder,  # type: ignore[arg-type]
            names,
            OwnedResources(
                namespaces={names.subject_namespace, names.peer_namespace},
                root_interfaces=set(),
            ),
            peer=None,
            capture=None,
        )
        self.assertFalse(result["passed"])
        self.assertTrue(result["errors"])
        self.assertIn(
            ["ip", "netns", "delete", names.peer_namespace],
            recorder.argv,
        )

    def test_command_timeout_kills_the_complete_owned_process_group(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            recorder = CommandRecorder(Path(temporary), timeout_seconds=1)
            program = (
                "import subprocess,sys,time;"
                "subprocess.Popen([sys.executable,'-c','import time;time.sleep(60)']);"
                "time.sleep(60)"
            )
            with self.assertRaisesRegex(
                TripwireProofFailure, "process group was killed"
            ):
                recorder.run([sys.executable, "-c", program], timeout=1)
            self.assertEqual(len(recorder.records), 1)
            record = recorder.records[0]
            self.assertTrue(record["timed_out"])
            with self.assertRaises(ProcessLookupError):
                os.killpg(record["process_group"], 0)

    def test_command_signal_kills_the_complete_owned_process_group(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            recorder = CommandRecorder(Path(temporary), timeout_seconds=10)
            prior = signal.getsignal(signal.SIGTERM)

            def interrupt(signum: int, _frame: object) -> None:
                raise TripwireInterrupted(f"synthetic {signal.Signals(signum).name}")

            signal.signal(signal.SIGTERM, interrupt)
            timer = threading.Timer(0.1, lambda: os.kill(os.getpid(), signal.SIGTERM))
            try:
                timer.start()
                with self.assertRaisesRegex(TripwireInterrupted, "synthetic"):
                    recorder.run(
                        [
                            sys.executable,
                            "-c",
                            "import subprocess,sys,time;"
                            "subprocess.Popen([sys.executable,'-c',"
                            "'import time;time.sleep(60)']);"
                            "time.sleep(60)",
                        ]
                    )
            finally:
                timer.cancel()
                timer.join(timeout=2)
                signal.signal(signal.SIGTERM, prior)
            record = recorder.records[0]
            self.assertTrue(record["interrupted"])
            with self.assertRaises(ProcessLookupError):
                os.killpg(record["process_group"], 0)

    def test_cleanup_defers_termination_signal_until_absence_checks_finish(
        self,
    ) -> None:
        with _defer_termination_signals() as observed:
            os.kill(os.getpid(), signal.SIGTERM)
            self.assertEqual(observed, [signal.SIGTERM])

    def test_host_lock_rejects_contention_and_allows_fresh_process_reentry(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            lock_path = Path(temporary) / "tripwire.lock"
            program = (
                "import fcntl,sys;"
                "handle=open(sys.argv[1],'a+');"
                "fcntl.flock(handle.fileno(),fcntl.LOCK_EX);"
                "print('READY',flush=True);"
                "sys.stdin.read(1)"
            )
            holder = subprocess.Popen(
                [sys.executable, "-c", program, str(lock_path)],
                text=True,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                start_new_session=True,
            )
            assert holder.stdout is not None
            assert holder.stdin is not None
            try:
                readable, _, _ = select.select([holder.stdout], [], [], 2)
                self.assertEqual(readable, [holder.stdout])
                self.assertEqual(holder.stdout.readline().strip(), "READY")
                with self.assertRaisesRegex(TripwireProofFailure, "host-global lock"):
                    with _exclusive_host_lock(lock_path):
                        self.fail("contended lock was incorrectly admitted")
                holder.stdin.write("x")
                holder.stdin.flush()
                holder.communicate(timeout=5)
            finally:
                if holder.poll() is None:
                    os.killpg(holder.pid, signal.SIGKILL)
                    holder.communicate(timeout=5)
            self.assertEqual(holder.returncode, 0)
            with _exclusive_host_lock(lock_path):
                pass


class WrapperContractTests(unittest.TestCase):
    def test_wrapper_isolated_entry_preserves_skipped_exit_and_evidence(self) -> None:
        result, evidence, python_imported, shell_imported = run_isolated_wrapper(
            REPO_ROOT
        )
        self.assertEqual(result.returncode, 77, result.stderr)
        self.assertEqual(evidence["result"]["status"], "SKIPPED")
        self.assertEqual(evidence["result"]["exit_code"], 77)
        self.assertEqual(evidence["assertions"], [])
        self.assertEqual(evidence["commands"], [])
        self.assertFalse(python_imported)
        self.assertFalse(shell_imported)


if __name__ == "__main__":
    unittest.main(verbosity=2)
