#!/usr/bin/env python3
"""Deterministic mutation tests for the NNC9.2 lifecycle evidence contract."""

from __future__ import annotations

import argparse
import copy
from datetime import datetime, timedelta, timezone
import json
from pathlib import Path
import tempfile
from typing import Any, Callable
from unittest import mock
import sys

REPO_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO_ROOT / "scripts"))

from nimbus_network_sovereignty_tripwire.evidence import (  # noqa: E402
    REQUIRED_CONTROL_ASSERTIONS,
    artifact_manifest,
)
from nimbus_network_sovereignty_tripwire.integrity import sha256_file  # noqa: E402
from nimbus_network_sovereignty_tripwire.isolation import _resource_names  # noqa: E402
from nimbus_network_sovereignty_tripwire.lifecycle import (  # noqa: E402
    CONTROL_QUIESCENCE_SECONDS,
    _krun_process_pair_exact,
    _netavark_scaffold_evidence,
    _product_argv,
    _profile_capture_text,
    _select_run_processes,
    _terminal_compose_projection_exact,
    _terminal_krun_manifest_evidence,
    _terminal_provider_journals,
    _trace_addresses,
    _write_lifecycle_rules,
)
from nimbus_network_sovereignty_tripwire.lifecycle_evidence import (  # noqa: E402
    REQUIRED_TRANSITIONS,
    SCHEMA_VERSION,
    LifecycleEvidenceError,
    derive_assertions,
    teardown_order_exact,
    validate_evidence,
)
from nimbus_network_sovereignty_tripwire import lifecycle_probe  # noqa: E402


def _assert_foreground_trace_lifetime() -> dict[str, Any]:
    argv = _product_argv(
        _resource_names("nnc92-trace-lifetime"),
        Path("/proof/foreground.strace"),
        Path("/proof/nimbus"),
        Path("/proof/durable"),
        Path("/proof/fixture"),
        "up",
    )
    strace_index = argv.index("strace")
    if argv[strace_index + 1] != "-DDD":
        raise AssertionError(
            "foreground tracing must isolate the tracer lifetime from the Compose owner"
        )
    return {"name": "foreground-tracer-lifetime", "passed": True}


def _assert_exact_krun_process_census() -> dict[str, Any]:
    durable_root = Path("/proof/run56/attempt-1/durable")
    sandbox_id = "sandbox-run56"
    historical = [
        {
            "pid": 10,
            "ppid": 1,
            "comm": "conmon",
            "argv": [
                "/usr/bin/conmon",
                "-c",
                "sandbox-run53",
                "-u",
                "sandbox-run53",
                "-r",
                "/usr/libexec/nimbus/crun",
                "-b",
                "/proof/run53/bundle",
            ],
        },
        {
            "pid": 11,
            "ppid": 10,
            "comm": "libkrun VM",
            "argv": ["[libcrun:krun]", "/bin/busybox"],
        },
    ]
    current = [
        {
            "pid": 20,
            "ppid": 1,
            "comm": "conmon",
            "argv": [
                "/usr/bin/conmon",
                "-c",
                sandbox_id,
                "-u",
                sandbox_id,
                "-r",
                "/usr/libexec/nimbus/crun",
                "-b",
                str(durable_root / "control/backends/krun/bundle"),
            ],
        },
        {
            "pid": 21,
            "ppid": 20,
            "comm": "libkrun VM",
            "argv": ["[libcrun:krun]", "/bin/busybox"],
        },
    ]
    selected = _select_run_processes(historical + current, durable_root, sandbox_id)
    if [row["pid"] for row in selected] != [20, 21]:
        raise AssertionError("process census must exclude historical crun owners")
    if not _krun_process_pair_exact(durable_root, sandbox_id, selected):
        raise AssertionError("exact conmon-to-libkrun-VM pair must authenticate")
    duplicate = selected + [
        {**current[0], "pid": 30},
        {**current[1], "pid": 31, "ppid": 30},
    ]
    if _krun_process_pair_exact(durable_root, sandbox_id, duplicate):
        raise AssertionError("duplicate exact runtime owners must fail closed")
    crossed = copy.deepcopy(selected)
    crossed[0]["argv"][6] = "/usr/bin/crun"
    if _krun_process_pair_exact(durable_root, sandbox_id, crossed):
        raise AssertionError("stock crun substitution must fail closed")
    alternate = selected + [
        {
            **current[0],
            "pid": 40,
            "argv": [
                "/usr/bin/conmon",
                "-c",
                sandbox_id,
                "-u",
                sandbox_id,
                "-r",
                "/opt/crossed/crun",
                "-b",
                str(durable_root / "control/backends/krun/crossed-bundle"),
            ],
        },
        {**current[1], "pid": 41, "ppid": 40},
    ]
    if _krun_process_pair_exact(durable_root, sandbox_id, alternate):
        raise AssertionError("alternate same-sandbox runtime owner must fail closed")
    return {"name": "exact-krun-process-census", "passed": True}


def _assert_terminal_provider_journal_contract() -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix="nnc92-provider-journals-") as temporary:
        root = Path(temporary)
        if _terminal_provider_journals(root):
            raise AssertionError("missing provider journals were accepted")
        path = root / ".nimbus-provider-command-attempts/command/attempt.json"
        path.parent.mkdir(parents=True)
        for kind in ("succeeded", "absent"):
            path.write_text(
                json.dumps({"observation": {"kind": kind}}), encoding="utf-8"
            )
            if not _terminal_provider_journals(root):
                raise AssertionError(f"resolved provider journal {kind} was rejected")
        for kind in (
            None,
            "unknown",
            "definite_failure",
            "retry_authorized",
            "claimed",
            "in_progress",
            "ambiguous",
        ):
            observation = {} if kind is None else {"kind": kind}
            path.write_text(
                json.dumps({"observation": observation}), encoding="utf-8"
            )
            if _terminal_provider_journals(root):
                raise AssertionError(f"unresolved provider journal {kind!r} was accepted")
    return {"name": "terminal-provider-journal-contract", "passed": True}


def _assert_unreachable_probe_contract() -> dict[str, Any]:
    arguments = argparse.Namespace(
        host="127.0.0.1",
        port=15992,
        path="/",
        timeout_seconds=0.002,
        connect_timeout=0.001,
        interval_seconds=0.0,
    )
    with mock.patch.object(
        lifecycle_probe,
        "_http_request",
        side_effect=RuntimeError("malformed response after connect"),
    ):
        try:
            lifecycle_probe._expect_unreachable(arguments)
        except RuntimeError as error:
            if "malformed response" not in str(error):
                raise
        else:
            raise AssertionError("malformed HTTP after connect was accepted as unreachable")
    with mock.patch.object(
        lifecycle_probe,
        "_http_request",
        side_effect=lifecycle_probe.HttpConnectionFailure("connection refused"),
    ):
        result = lifecycle_probe._expect_unreachable(arguments)
    if result.get("passed") is not True or result.get("attempts", 0) < 1:
        raise AssertionError("connection refusal did not prove endpoint unreachability")
    with mock.patch.object(
        lifecycle_probe,
        "_http_request",
        side_effect=lifecycle_probe.socket.timeout("response timed out after connect"),
    ):
        try:
            lifecycle_probe._expect_unreachable(arguments)
        except lifecycle_probe.socket.timeout:
            pass
        else:
            raise AssertionError("post-connect response timeout was accepted as unreachable")
    return {"name": "unreachable-probe-contract", "passed": True}


def _assert_terminal_cleanup_manifest_contract() -> dict[str, Any]:
    terminal = {
        "handle": {
            "tenant_id": "tenant-sovereign",
            "id": "sandbox-sovereign",
            "backend": "krun",
            "status": "stopped",
            "published_endpoints": [],
        },
        "status": "stopped",
        "shutdown_requested": True,
        "launch_authority": {"phase": "released"},
        "launch_artifact": None,
        "creator_handoff": {"phase": "quiesced", "proof": {"kind": "dead_contained"}},
        "provider_failure_cleanup": {"phase": "inactive"},
        "network_teardown": {"detachPhase": "detached", "releasePhase": "released"},
        "execution_teardown": {"stop": {"phase": "execution_stopped"}},
    }
    mutations = {
        "partial-status": lambda value: value.update(status="stopping"),
        "crossed-identity": lambda value: value["handle"].update(id="sandbox-crossed"),
        "retained-artifact": lambda value: value.update(launch_artifact={"kind": "rootfs"}),
        "live-creator": lambda value: value["creator_handoff"].update(phase="adopted"),
    }
    with tempfile.TemporaryDirectory(prefix="nnc92-terminal-manifest-") as temporary:
        path = Path(temporary) / "manifest.json"
        path.write_text(json.dumps(terminal), encoding="utf-8")
        if not _terminal_krun_manifest_evidence(
            path, "tenant-sovereign", "sandbox-sovereign"
        )["passed"]:
            raise AssertionError("exact terminal Krun manifest must pass")
        for name, mutate in mutations.items():
            candidate = copy.deepcopy(terminal)
            mutate(candidate)
            path.write_text(json.dumps(candidate), encoding="utf-8")
            if _terminal_krun_manifest_evidence(
                path, "tenant-sovereign", "sandbox-sovereign"
            )["passed"]:
                raise AssertionError(f"{name} terminal manifest mutation was accepted")
    return {"name": "terminal-cleanup-manifest-contract", "passed": True}


def _assert_terminal_compose_projection_contract() -> dict[str, Any]:
    projection = [
        {
            "sandbox_id": "sandbox-sovereign",
            "tenant_id": "tenant-sovereign",
            "service_name": "api",
            "status": "stopped",
            "published_endpoints": [],
            "last_exit_code": None,
            "shutdown_requested": True,
        }
    ]
    arguments = {
        "tenant_id": "tenant-sovereign",
        "sandbox_id": "sandbox-sovereign",
        "service_name": "api",
    }
    if not _terminal_compose_projection_exact(projection, **arguments):
        raise AssertionError("exact terminal Compose projection must pass")
    mutations = {
        "missing": [],
        "duplicate": projection + copy.deepcopy(projection),
        "crossed-identity": [{**projection[0], "sandbox_id": "sandbox-crossed"}],
        "active-status": [{**projection[0], "status": "ready"}],
        "published-endpoint": [
            {**projection[0], "published_endpoints": [{"id": "endpoint-live"}]}
        ],
        "restartable": [{**projection[0], "shutdown_requested": False}],
    }
    for name, candidate in mutations.items():
        if _terminal_compose_projection_exact(candidate, **arguments):
            raise AssertionError(f"{name} terminal Compose projection was accepted")
    return {"name": "terminal-compose-projection-contract", "passed": True}


def _assert_netavark_cleanup_scaffold_contract() -> dict[str, Any]:
    global_chains = (
        "INPUT",
        "FORWARD",
        "POSTROUTING",
        "PREROUTING",
        "OUTPUT",
        "NETAVARK-HOSTPORT-DNAT",
        "NETAVARK-HOSTPORT-SETMARK",
        "NETAVARK-ISOLATION-1",
        "NETAVARK-ISOLATION-2",
        "NETAVARK-ISOLATION-3",
    )
    global_rule_counts = {
        "FORWARD": 2,
        "POSTROUTING": 1,
        "PREROUTING": 1,
        "OUTPUT": 1,
        "NETAVARK-HOSTPORT-SETMARK": 1,
        "NETAVARK-ISOLATION-3": 1,
    }
    document: dict[str, Any] = {
        "nftables": [
            {"metainfo": {"version": "1.1.3"}},
            {"table": {"family": "inet", "name": "nimbus-proof", "handle": 3}},
            {
                "counter": {
                    "family": "inet",
                    "table": "nimbus-proof",
                    "name": "denied_ipv4",
                }
            },
            {
                "rule": {
                    "family": "inet",
                    "table": "nimbus-proof",
                    "chain": "output",
                    "expr": [
                        {"match": {"right": {"prefix": {"addr": "10.0.0.0", "len": 24}}}},
                        {"match": {"right": 15992}},
                    ],
                }
            },
            {"table": {"family": "inet", "name": "netavark", "handle": 4}},
            *[
                {
                    "chain": {
                        "family": "inet",
                        "table": "netavark",
                        "name": chain,
                        "handle": index + 10,
                    }
                }
                for index, chain in enumerate(global_chains)
            ],
            *[
                {
                    "rule": {
                        "family": "inet",
                        "table": "netavark",
                        "chain": chain,
                        "expr": [{"counter": None}],
                        "handle": index + 30,
                    }
                }
                for index, chain in enumerate(
                    chain
                    for chain, count in global_rule_counts.items()
                    for _ in range(count)
                )
            ],
        ]
    }
    network_config = {
        "network_interface": "nb-0",
        "network_subnet": "10.0.0.0/24",
        "network_id": "00000000000000000000000000000000",
    }

    def accepted(candidate: dict[str, Any]) -> bool:
        return _netavark_scaffold_evidence(
            candidate,
            network_config,
            published_port=15992,
            guest_port=8080,
        )["passed"]

    if not accepted(document):
        raise AssertionError("exact provider-global Netavark scaffold must pass")
    mutations = {
        "dynamic-chain": {
            "chain": {
                "family": "inet",
                "table": "netavark",
                "name": "nv_00000000_10_0_0_0_nm24",
            }
        },
        "bridge-rule": {
            "rule": {
                "family": "inet",
                "table": "netavark",
                "chain": "FORWARD",
                "expr": [{"match": {"right": "nb-0"}}],
            }
        },
        "subnet-rule": {
            "rule": {
                "family": "inet",
                "table": "netavark",
                "chain": "POSTROUTING",
                "expr": [{"match": {"right": "10.0.0.2"}}],
            }
        },
        "host-port-rule": {
            "rule": {
                "family": "inet",
                "table": "netavark",
                "chain": "NETAVARK-HOSTPORT-DNAT",
                "expr": [{"match": {"right": 15992}}],
            }
        },
        "unknown-executable-object": {
            "set": {"family": "inet", "table": "netavark", "name": "run-owned"}
        },
    }
    for name, mutation in mutations.items():
        candidate = copy.deepcopy(document)
        candidate["nftables"].append(mutation)
        if accepted(candidate):
            raise AssertionError(f"{name} Netavark cleanup residue was accepted")
    missing_rule = copy.deepcopy(document)
    missing_rule["nftables"].pop()
    if accepted(missing_rule):
        raise AssertionError("incomplete provider-global Netavark scaffold was accepted")
    return {"name": "netavark-cleanup-scaffold-contract", "passed": True}


def _assert_profile_evidence_scope() -> dict[str, Any]:
    names = _resource_names("nnc92-profile-evidence-scope")
    with tempfile.TemporaryDirectory(prefix="nnc92-profile-scope-") as temporary:
        root = Path(temporary)
        (root / "control.strace.1").write_text(
            'connect inet_addr("192.0.2.1")\n'
            'connect inet_pton(AF_INET6, "2001:db8::1")\n',
            encoding="utf-8",
        )
        (root / "first-owner.strace.2").write_text(
            'bind inet_pton(AF_INET6, "::ffff:10.0.0.2")\n'
            f'connect inet_addr("{names.peer_ipv4}")\n',
            encoding="utf-8",
        )
        addresses, forbidden = _trace_addresses(root, names)
        rules_path = root / "lifecycle-rules.nft"
        _write_lifecycle_rules(rules_path, names)
        rules = rules_path.read_text(encoding="utf-8")
    if addresses != ["10.0.0.2", names.peer_ipv4] or forbidden:
        raise AssertionError("profile trace scope crossed controls or IPv4-mapped peers")
    if _profile_capture_text("\n") != "":
        raise AssertionError("tcpdump signal-only newline must normalize to empty evidence")
    if _profile_capture_text("dns.invalid\n") != "dns.invalid":
        raise AssertionError("nonempty DNS evidence must remain observable")
    if CONTROL_QUIESCENCE_SECONDS < 2.0:
        raise AssertionError("control retransmission quiescence window is too short")
    igmp_allow = "ip daddr 224.0.0.0/24 ip protocol igmp accept"
    ipv4_deny = "meta nfproto ipv4 counter name denied_ipv4 drop"
    if rules.count(igmp_allow) != 1 or rules.index(igmp_allow) > rules.index(ipv4_deny):
        raise AssertionError("link-local IGMP must be allowed before the IPv4 deny counter")
    if "ip daddr 224.0.0.0/4 accept" in rules:
        raise AssertionError("the profile must not allow broad IPv4 multicast")
    return {"name": "profile-evidence-scope", "passed": True}


def _write(path: Path, payload: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(payload)


def _manifest(attempt_id: str) -> dict[str, Any]:
    return {
        "handle": {
            "tenant_id": "tenant-sovereign",
            "id": "sandbox-sovereign",
            "name": "api",
            "backend": "krun",
            "status": "ready",
            "published_endpoints": [{"id": "endpoint-sovereign"}],
        },
        "execution_attempt_id": attempt_id,
        "provision_network_plan": {
            "network_plan": {
                "generation": 1,
                "requirements": {
                    "sovereignty": {
                        "maximum_control_plane_locality": "local_only",
                        "allowed_external_dependencies": [],
                        "offline_restart_required": True,
                    }
                },
            }
        },
    }


def _saga(
    manifest: dict[str, Any],
    *,
    restart_epoch: int,
    automatic_restart_count: int,
) -> dict[str, Any]:
    handle = manifest["handle"]
    attempt_id = manifest["execution_attempt_id"]
    execution = {
        "workloadUid": "workload-sovereign",
        "nodeIdentity": "node-sovereign",
        "executionId": handle["id"],
        "restartEpoch": str(restart_epoch),
        "attemptId": attempt_id,
        "generation": "1",
        "desiredDigest": "desired-sovereign",
    }
    publication = {
        "endpoints": ["endpoint-sovereign"],
        "network": {
            "planId": "plan-sovereign",
            "generation": "1",
            "digest": "network-sovereign",
        },
        "execution": copy.deepcopy(execution),
    }
    return {
        "sagaId": "saga-sovereign",
        "tenantId": handle["tenant_id"],
        "workloadId": handle["name"],
        "desiredState": "running",
        "desiredGeneration": "1",
        "desiredDigest": "desired-sovereign",
        "phase": "observed",
        "phaseDetail": {
            "kind": "provision",
            "value": {
                "references": {
                    "execution": execution,
                    "publication": publication,
                },
                "observations": [
                    {"kind": kind}
                    for kind in (
                        "network_reserved",
                        "execution_prepared",
                        "network_attached",
                        "execution_activated",
                        "ready",
                        "publication_present",
                        "publication_observed",
                    )
                ],
            },
        },
        "restartState": {
            "currentExecutionAttemptId": attempt_id,
            "completedRestartEpoch": str(restart_epoch),
            "completedAutomaticRestartCount": automatic_restart_count,
            "active": None,
        },
        "provisionDisposition": {"kind": "ready"},
    }


def _teardown_history(first_saga: dict[str, Any]) -> dict[str, Any]:
    references = first_saga["phaseDetail"]["value"]["references"]
    publication = references["publication"]
    execution = references["execution"]
    network = {
        "attachmentId": "netatt-sovereign",
        "planId": "plan-sovereign",
        "generation": "1",
    }
    observations = [
        {
            "kind": "publication_absent",
            "reference": copy.deepcopy(publication),
            "evidence": "evidence-publication",
        },
        {
            "kind": "execution_drained",
            "reference": copy.deepcopy(execution),
            "evidence": "evidence-drain",
        },
        {
            "kind": "execution_stopped",
            "reference": copy.deepcopy(execution),
            "evidence": "evidence-stop",
        },
        {
            "kind": "network_detached",
            "reference": copy.deepcopy(network),
            "evidence": "evidence-detach",
        },
        {
            "kind": "network_released",
            "reference": copy.deepcopy(network),
            "evidence": "evidence-release",
        },
    ]
    phases = (
        (10, "withdrawal_committed", 0),
        (11, "withdrawn", 1),
        (12, "drained", 2),
        (13, "workload_stopped", 3),
        (14, "network_detached", 4),
        (15, "network_released", 5),
    )
    return {
        "saga_id": first_saga["sagaId"],
        "tenant_id": first_saga["tenantId"],
        "workload_id": first_saga["workloadId"],
        "entries": [
            {
                "commit_sequence": sequence,
                "saga_revision": str(sequence),
                "phase": phase,
                "terminal_observations": copy.deepcopy(observations[:count]),
            }
            for sequence, phase, count in phases
        ],
    }


def _attempt(index: int, started: datetime) -> dict[str, Any]:
    first = _manifest(f"attempt-{index}-first")
    restarted = _manifest(f"attempt-{index}-restart")
    first_saga = _saga(
        first,
        restart_epoch=0,
        automatic_restart_count=0,
    )
    restart_saga = _saga(
        restarted,
        restart_epoch=1,
        automatic_restart_count=1,
    )
    return {
        "attempt": index,
        "started_at": started.isoformat(),
        "finished_at": (started + timedelta(seconds=1)).isoformat(),
        "durable_root": f"/proof/attempt-{index}",
        "transitions": list(REQUIRED_TRANSITIONS),
        "namespace_view": {
            "host": "cgroup2fs",
            "subject": "cgroup2fs",
            "passed": True,
        },
        "control": {
            "passed": True,
            "assertions": {name: True for name in REQUIRED_CONTROL_ASSERTIONS},
            "counters": {
                "denied_ipv4": 1,
                "denied_ipv6": 1,
                "denied_private": 1,
                "dns_tcp": 1,
                "dns_udp": 1,
                "unexpected": 0,
            },
        },
        "reset": {
            "passed": True,
            "counters": {
                "denied_ipv4": 0,
                "denied_ipv6": 0,
                "denied_private": 0,
                "dns_tcp": 0,
                "dns_udp": 0,
                "unexpected": 0,
            },
        },
        "lifecycle": {
            "provider_exact": True,
            "readiness_exact": True,
            "first_http": {"passed": True},
            "second_http": {"passed": True},
            "fresh_http": {"passed": True},
            "retired_http": {"passed": True},
            "first_manifest": first,
            "restart_manifest": restarted,
            "fresh_manifest": copy.deepcopy(restarted),
            "first_saga": first_saga,
            "restart_saga": restart_saga,
            "restart_count": 1,
            "fresh_owner_exact": True,
            "retirement_exit": 0,
            "terminal_saga_history": _teardown_history(restart_saga),
            "terminal_projection_exact": True,
        },
        "profile": {
            "counters": {
                "denied_ipv4": 0,
                "denied_ipv6": 0,
                "denied_private": 0,
                "dns_tcp": 0,
                "dns_udp": 0,
                "unexpected": 0,
            },
            "dns_capture": "",
            "forbidden_trace_addresses": [],
        },
        "cleanup": {"passed": True},
    }


def _pass_document(root: Path) -> dict[str, Any]:
    candidate = root / "candidate" / "nimbus"
    _write(candidate, b"authenticated-candidate")
    fixture = root / "fixture"
    fixture_payloads = {
        "Dockerfile": b"FROM scratch\n",
        "compose.yaml": b"services: {}\n",
        "lifecycle.sh": b"#!/bin/sh\nexit 0\n",
        "rootfs/bin/busybox": b"busybox",
    }
    fixture_files = []
    for relative, payload in fixture_payloads.items():
        path = fixture / relative
        _write(path, payload)
        mode = 0o555 if relative in {"lifecycle.sh", "rootfs/bin/busybox"} else 0o444
        path.chmod(mode)
        fixture_files.append(
            {
                "path": relative,
                "size": path.stat().st_size,
                "sha256": sha256_file(path),
                "mode": mode,
            }
        )
    stdout = root / "commands/0001.stdout"
    stderr = root / "commands/0001.stderr"
    _write(stdout, b"ok\n")
    _write(stderr, b"")
    started = datetime(2026, 8, 14, tzinfo=timezone.utc)
    attempts = [
        _attempt(1, started),
        _attempt(2, started + timedelta(seconds=2)),
    ]
    fixture_by_path = {row["path"]: row["sha256"] for row in fixture_files}
    document: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "result": {
            "status": "PASS",
            "exit_code": 0,
            "reason": None,
            "phase": "complete",
            "started_at": started.isoformat(),
            "finished_at": (started + timedelta(seconds=4)).isoformat(),
        },
        "runner": {
            "admitted": True,
            "uid": 0,
            "kvm_access": True,
            "missing_tools": [],
        },
        "source": {
            "commit": "0" * 40,
            "tree": "1" * 40,
            "harness_sha256": "2" * 64,
        },
        "inputs": {
            "repeat": 2,
            "offline_after_admission": True,
            "nimbus_path": str(candidate),
            "nimbus_size": candidate.stat().st_size,
            "nimbus_sha256": sha256_file(candidate),
            "fixture_root": str(fixture),
            "fixture_files": fixture_files,
            "busybox_sha256": fixture_by_path["rootfs/bin/busybox"],
            "compose_sha256": fixture_by_path["compose.yaml"],
            "dockerfile_sha256": fixture_by_path["Dockerfile"],
            "lifecycle_sha256": fixture_by_path["lifecycle.sh"],
            "fixture_executable": True,
        },
        "attempts": attempts,
        "commands": [
            {
                "index": 1,
                "argv": ["/proof/nimbus", "compose", "up"],
                "exit_code": 0,
                "critical": True,
                "accepted_exit_codes": [0],
                "timed_out": False,
                "interrupted": False,
                "stdout": stdout.relative_to(root).as_posix(),
                "stderr": stderr.relative_to(root).as_posix(),
            }
        ],
        "self_tests": {"passed": 8, "required": 8},
        "assertions": [],
        "artifacts": [],
    }
    document["assertions"] = derive_assertions(document)
    document["artifacts"] = artifact_manifest(
        root,
        [path for path in root.rglob("*") if path.is_file()],
    )
    validate_evidence(document, evidence_root=root)
    return document


def _must_reject(
    name: str,
    mutate: Callable[[dict[str, Any], Path], None],
) -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix=f"nnc92-{name}-") as temporary:
        root = Path(temporary)
        document = _pass_document(root)
        mutate(document, root)
        try:
            validate_evidence(document, evidence_root=root)
        except LifecycleEvidenceError as error:
            return {"name": name, "passed": True, "reason": str(error)}
        raise AssertionError(f"{name} mutation was accepted")


def _assert_k4_detailed_evidence_contract() -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix="nnc92-k4-details-") as temporary:
        root = Path(temporary)
        document = _pass_document(root / "control")
        document["attempts"][0]["control"]["assertions"][
            "public_ipv4_denied"
        ] = False
        if next(row for row in derive_assertions(document) if row["id"] == "K4")[
            "passed"
        ]:
            raise AssertionError("K4 accepted a failed detailed control assertion")

        document = _pass_document(root / "reset")
        document["attempts"][0]["reset"]["counters"]["unexpected"] = 1
        if next(row for row in derive_assertions(document) if row["id"] == "K4")[
            "passed"
        ]:
            raise AssertionError("K4 accepted a nonzero detailed reset counter")
    return {"name": "k4-detailed-evidence-contract", "passed": True}


def _assert_k10_durable_order_contract() -> dict[str, Any]:
    with tempfile.TemporaryDirectory(prefix="nnc92-k10-order-") as temporary:
        root = Path(temporary)
        document = _pass_document(root)
        attempt = document["attempts"][0]
        history = attempt["lifecycle"]["terminal_saga_history"]
        source_saga = attempt["lifecycle"]["restart_saga"]
        if not teardown_order_exact(history, source_saga):
            raise AssertionError("exact durable teardown order was rejected")
        if teardown_order_exact(history, attempt["lifecycle"]["first_saga"]):
            raise AssertionError(
                "terminal teardown was accepted against the superseded execution attempt"
            )
        retracted_document = copy.deepcopy(document)
        retracted_attempt = retracted_document["attempts"][0]
        retracted = retracted_attempt["lifecycle"]["terminal_saga_history"]
        entries = retracted["entries"]
        publication_index = next(
            (
                index
                for index, entry in enumerate(entries[:-1])
                if entry["terminal_observations"]
            ),
            None,
        )
        if publication_index is None:
            raise AssertionError("fixture has no durable publication prefix to retract")
        entries[publication_index + 1]["terminal_observations"] = []
        if teardown_order_exact(retracted, source_saga):
            raise AssertionError("durable teardown history accepted a retracted prefix")
        if next(
            row
            for row in derive_assertions(retracted_document)
            if row["id"] == "K10"
        )["passed"]:
            raise AssertionError("K10 accepted publication prefix retraction")
        crossed = copy.deepcopy(history)
        observations = crossed["entries"][-1]["terminal_observations"]
        observations[0], observations[2] = observations[2], observations[0]
        attempt["lifecycle"]["terminal_saga_history"] = crossed
        if next(row for row in derive_assertions(document) if row["id"] == "K10")[
            "passed"
        ]:
            raise AssertionError("K10 accepted execution stop before publication absence")
    return {"name": "k10-durable-order-contract", "passed": True}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="NNC9.2 lifecycle mutation tests")
    parser.add_argument("--json-output", type=Path)
    args = parser.parse_args(argv)
    invariants = [
        _assert_foreground_trace_lifetime(),
        _assert_exact_krun_process_census(),
        _assert_terminal_provider_journal_contract(),
        _assert_unreachable_probe_contract(),
        _assert_terminal_cleanup_manifest_contract(),
        _assert_terminal_compose_projection_contract(),
        _assert_netavark_cleanup_scaffold_contract(),
        _assert_profile_evidence_scope(),
        _assert_k4_detailed_evidence_contract(),
        _assert_k10_durable_order_contract(),
    ]

    mutations: list[tuple[str, Callable[[dict[str, Any], Path], None]]] = [
        (
            "admission-failure",
            lambda document, _root: document["runner"].update(admitted=False),
        ),
        (
            "payload-failure",
            lambda document, _root: document["commands"][0].update(exit_code=1),
        ),
        (
            "timeout",
            lambda document, _root: document["commands"][0].update(timed_out=True),
        ),
        (
            "nonzero-counter-or-dns",
            lambda document, _root: document["attempts"][0]["profile"].update(
                dns_capture="unexpected.invalid"
            ),
        ),
        (
            "altered-input",
            lambda document, root: (root / "candidate/nimbus").write_bytes(b"altered"),
        ),
        (
            "missing-transition",
            lambda document, _root: document["attempts"][0]["transitions"].pop(),
        ),
        (
            "incomplete-cleanup",
            lambda document, _root: document["attempts"][0]["cleanup"].update(
                passed=False
            ),
        ),
        (
            "pass-evidence-mutation",
            lambda document, _root: document["assertions"][0].update(observed=False),
        ),
    ]
    cases = [_must_reject(name, mutation) for name, mutation in mutations]
    result = {
        "required": len(mutations),
        "passed": len(cases),
        "cases": cases,
        "invariants": invariants,
    }
    payload = json.dumps(result, indent=2, sort_keys=True) + "\n"
    if args.json_output is not None:
        args.json_output.parent.mkdir(parents=True, exist_ok=True)
        args.json_output.write_text(payload, encoding="utf-8")
    print(payload, end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
