#!/usr/bin/env python3
"""Live privileged NNC4.7 outer-isolation tripwire."""

from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import selectors
import signal
import subprocess
import time
from typing import Any, Callable, Mapping, Sequence

from .evidence import (
    EVIDENCE_SCHEMA_VERSION,
    REQUIRED_PASS_ASSERTIONS,
    EvidenceValidationError,
    artifact_manifest,
    atomic_write_json,
    validate_evidence,
)
from .environment import (
    CONFIG_EXIT,
    FAIL_EXIT,
    SAFE_RUNNER_ID,
    SKIPPED_EXIT,
    TripwireConfig,
    TripwireInterrupted,
    TripwireProofFailure,
    collect_preflight,
    minimal_runner,
    preflight_decision,
    repo_root,
    source_facts,
    source_failure,
)
from .synchronization import _defer_termination_signals, _exclusive_host_lock
from .workspace import _prepare_output_directory, _write_new_text


@dataclass(frozen=True)
class ResourceNames:
    subject_namespace: str
    peer_namespace: str
    subject_interface: str
    peer_interface: str
    nft_table: str
    subject_ipv4: str
    peer_ipv4: str
    subject_ipv6: str
    peer_ipv6: str


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _resource_names(runner_id: str) -> ResourceNames:
    token = hashlib.sha256(runner_id.encode("utf-8")).hexdigest()[:6]
    ula = f"fd42:4e49:4d42:{int(token[:4], 16):x}"
    return ResourceNames(
        subject_namespace=f"nimbus-sov-{token}-subject",
        peer_namespace=f"nimbus-sov-{token}-peer",
        subject_interface=f"nsv{token}s",
        peer_interface=f"nsv{token}p",
        nft_table=f"nimbus_sov_{token}",
        subject_ipv4="10.254.47.2",
        peer_ipv4="10.254.47.1",
        subject_ipv6=f"{ula}::2",
        peer_ipv6=f"{ula}::1",
    )


def _probe_path() -> Path:
    return Path(__file__).resolve().with_name("probe.py")


class CommandRecorder:
    def __init__(
        self,
        output_dir: Path,
        timeout_seconds: int,
        tool_paths: Mapping[str, str] | None = None,
    ) -> None:
        self.output_dir = output_dir / "commands"
        os.mkdir(self.output_dir, 0o700)
        self.timeout_seconds = timeout_seconds
        self.tool_paths = dict(tool_paths or {})
        self.records: list[dict[str, Any]] = []

    def authenticate_argv(self, argv: Sequence[str]) -> list[str]:
        return [self.tool_paths.get(token, token) for token in argv]

    def run(
        self,
        argv: Sequence[str],
        *,
        check: bool = True,
        timeout: int | None = None,
        stdin: str | None = None,
        on_success: Callable[[], None] | None = None,
    ) -> subprocess.CompletedProcess[str]:
        authenticated_argv = self.authenticate_argv(argv)
        index = len(self.records) + 1
        started = utc_now()
        before = time.monotonic()
        process = subprocess.Popen(
            authenticated_argv,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            stdin=subprocess.PIPE if stdin is not None else None,
            start_new_session=True,
        )
        timed_out = False
        interruption: BaseException | None = None
        deferred_signals: list[int] = []
        try:
            stdout, stderr = process.communicate(
                input=stdin,
                timeout=timeout or self.timeout_seconds,
            )
        except subprocess.TimeoutExpired:
            timed_out = True
            os.killpg(process.pid, signal.SIGKILL)
            stdout, stderr = process.communicate(timeout=5)
        except BaseException as error:
            interruption = error
            with _defer_termination_signals() as deferred_signals:
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGKILL)
                stdout, stderr = process.communicate(timeout=5)
        result = subprocess.CompletedProcess(
            authenticated_argv,
            int(process.returncode or 0),
            stdout,
            stderr,
        )
        if (
            result.returncode == 0
            and not timed_out
            and interruption is None
            and on_success is not None
        ):
            on_success()
        stdout_path = self.output_dir / f"{index:04d}.stdout"
        stderr_path = self.output_dir / f"{index:04d}.stderr"
        _write_new_text(stdout_path, result.stdout)
        _write_new_text(stderr_path, result.stderr)
        self.records.append(
            {
                "index": index,
                "argv": authenticated_argv,
                "started_at": started,
                "elapsed_ms": round((time.monotonic() - before) * 1000),
                "exit_code": result.returncode,
                "process_group": process.pid,
                "timed_out": timed_out,
                "interrupted": interruption is not None,
                "deferred_signals": deferred_signals,
                "stdout": stdout_path.relative_to(self.output_dir.parent).as_posix(),
                "stderr": stderr_path.relative_to(self.output_dir.parent).as_posix(),
            }
        )
        if timed_out:
            raise TripwireProofFailure(
                f"command {index} timed out and its process group was killed: "
                + " ".join(authenticated_argv)
            )
        if interruption is not None:
            raise interruption
        if check and result.returncode != 0:
            raise TripwireProofFailure(
                f"command {index} failed with exit {result.returncode}: "
                + " ".join(authenticated_argv)
            )
        return result

    def add_process_record(
        self,
        argv: Sequence[str],
        *,
        started_at: str,
        exit_code: int,
        process_group: int,
        stdout_path: Path,
        stderr_path: Path,
    ) -> None:
        self.records.append(
            {
                "index": len(self.records) + 1,
                "argv": list(argv),
                "started_at": started_at,
                "elapsed_ms": None,
                "exit_code": exit_code,
                "process_group": process_group,
                "timed_out": False,
                "interrupted": False,
                "deferred_signals": [],
                "stdout": stdout_path.relative_to(self.output_dir.parent).as_posix(),
                "stderr": stderr_path.relative_to(self.output_dir.parent).as_posix(),
            }
        )


@dataclass
class ManagedProcess:
    argv: list[str]
    process: subprocess.Popen[str]
    started_at: str
    stdout_path: Path
    stderr_path: Path
    stdout_lines: list[str]
    stderr_lines: list[str]


@dataclass
class OwnedResources:
    namespaces: set[str]
    root_interfaces: set[str]


def _spawn_with_ready(
    recorder: CommandRecorder,
    argv: Sequence[str],
    *,
    ready_stream: str,
    ready_pattern: str,
    label: str,
    timeout_seconds: int = 10,
) -> ManagedProcess:
    authenticated_argv = recorder.authenticate_argv(argv)
    process = subprocess.Popen(
        authenticated_argv,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
        bufsize=1,
    )
    assert process.stdout is not None
    assert process.stderr is not None
    managed = ManagedProcess(
        argv=authenticated_argv,
        process=process,
        started_at=utc_now(),
        stdout_path=recorder.output_dir / f"{label}.stdout",
        stderr_path=recorder.output_dir / f"{label}.stderr",
        stdout_lines=[],
        stderr_lines=[],
    )
    selector = selectors.DefaultSelector()
    selector.register(
        process.stdout,
        selectors.EVENT_READ,
        data=("stdout", managed.stdout_lines),
    )
    selector.register(
        process.stderr,
        selectors.EVENT_READ,
        data=("stderr", managed.stderr_lines),
    )
    deadline = time.monotonic() + timeout_seconds
    ready = False
    failure: BaseException | None = None
    try:
        while time.monotonic() < deadline:
            if process.poll() is not None:
                break
            remaining = max(0.0, deadline - time.monotonic())
            events = selector.select(remaining)
            if not events:
                continue
            for key, _ in events:
                stream_name, lines = key.data
                line = key.fileobj.readline()
                if line:
                    lines.append(line)
                    if stream_name == ready_stream and ready_pattern in line:
                        ready = True
                        break
            if ready:
                break
    except BaseException as error:
        failure = error
    finally:
        selector.close()
    if failure is not None:
        with _defer_termination_signals():
            _stop_process(recorder, managed, signal.SIGTERM)
        raise failure
    if not ready:
        _stop_process(recorder, managed, signal.SIGTERM)
        raise TripwireProofFailure(
            f"{label} did not emit semantic readiness {ready_pattern!r}"
        )
    return managed


def _stop_process(
    recorder: CommandRecorder, managed: ManagedProcess, stop_signal: signal.Signals
) -> int:
    if managed.process.poll() is None:
        os.killpg(managed.process.pid, stop_signal)
    try:
        stdout_tail, stderr_tail = managed.process.communicate(timeout=10)
    except subprocess.TimeoutExpired:
        os.killpg(managed.process.pid, signal.SIGKILL)
        stdout_tail, stderr_tail = managed.process.communicate(timeout=5)
    managed.stdout_lines.append(stdout_tail)
    managed.stderr_lines.append(stderr_tail)
    _write_new_text(managed.stdout_path, "".join(managed.stdout_lines))
    _write_new_text(managed.stderr_path, "".join(managed.stderr_lines))
    exit_code = int(managed.process.returncode or 0)
    recorder.add_process_record(
        managed.argv,
        started_at=managed.started_at,
        exit_code=exit_code,
        process_group=managed.process.pid,
        stdout_path=managed.stdout_path,
        stderr_path=managed.stderr_path,
    )
    return exit_code


def _empty_evidence(
    config: TripwireConfig,
    runner: dict[str, Any],
    source: dict[str, Any],
    *,
    status: str,
    exit_code: int,
    reason: str,
    phase: str,
    started_at: str,
) -> dict[str, Any]:
    return {
        "schema_version": EVIDENCE_SCHEMA_VERSION,
        "result": {
            "status": status,
            "exit_code": exit_code,
            "reason": reason,
            "phase": phase,
            "started_at": started_at,
            "finished_at": utc_now(),
        },
        "runner": runner,
        "source": source,
        "inputs": {
            "runner_id": config.runner_id,
            "expected_hostname": config.expected_hostname,
            "host_class": config.host_class,
            "provider_kind": config.provider_kind,
            "repeat": config.repeat,
            "offline": True,
            "payload_argv": [],
            "prestage_argv": [],
        },
        "attempts": [],
        "commands": [],
        "assertions": [],
        "artifacts": [],
    }


def _write_nft_rules(path: Path, names: ResourceNames) -> None:
    rules = f"""table inet {names.nft_table} {{
  counter dns_udp {{ }}
  counter dns_tcp {{ }}
  counter denied_private {{ }}
  counter denied_ipv4 {{ }}
  counter denied_ipv6 {{ }}
  counter unexpected {{ }}
  chain output {{
    type filter hook output priority filter; policy drop;
    oifname "lo" accept
    ct state established,related accept
    ip6 daddr fe80::/10 meta l4proto ipv6-icmp accept
    ip6 daddr ff02::/16 meta l4proto ipv6-icmp accept
    ip daddr {names.peer_ipv4} udp dport 53 counter name dns_udp accept
    ip daddr {names.peer_ipv4} tcp dport 53 counter name dns_tcp accept
    ip daddr {names.peer_ipv4} accept
    ip6 daddr {names.peer_ipv6} accept
    ip daddr 10.0.0.0/8 counter name denied_private drop
    meta nfproto ipv4 counter name denied_ipv4 drop
    meta nfproto ipv6 counter name denied_ipv6 drop
    counter name unexpected drop
  }}
}}
"""
    _write_new_text(path, rules)


def _netns_exec(namespace: str, *argv: str) -> list[str]:
    return ["ip", "netns", "exec", namespace, *argv]


def _counter_snapshot(
    recorder: CommandRecorder, names: ResourceNames
) -> dict[str, int]:
    result = recorder.run(
        _netns_exec(
            names.subject_namespace,
            "nft",
            "-j",
            "list",
            "counters",
            "table",
            "inet",
            names.nft_table,
        )
    )
    document = json.loads(result.stdout)
    counters: dict[str, int] = {}
    for item in document.get("nftables", []):
        counter = item.get("counter") if isinstance(item, dict) else None
        if isinstance(counter, dict) and counter.get("table") == names.nft_table:
            name = counter.get("name")
            packets = counter.get("packets")
            if isinstance(name, str) and isinstance(packets, int):
                counters[name] = packets
    expected = {
        "dns_udp",
        "dns_tcp",
        "denied_private",
        "denied_ipv4",
        "denied_ipv6",
        "unexpected",
    }
    if set(counters) != expected:
        raise TripwireProofFailure(
            f"nft counter set mismatch: expected {sorted(expected)}, "
            f"observed {sorted(counters)}"
        )
    return counters


def _trace_text(prefix: Path) -> str:
    paths = sorted(prefix.parent.glob(prefix.name + "*"))
    if not paths:
        raise TripwireProofFailure(f"no strace artifacts for {prefix}")
    return "\n".join(path.read_text(encoding="utf-8") for path in paths)


def _start_peer(
    recorder: CommandRecorder, names: ResourceNames, attempt: int
) -> ManagedProcess:
    argv = _netns_exec(
        names.peer_namespace,
        "python3",
        str(_probe_path()),
        "peer-server",
        "--peer-ipv4",
        names.peer_ipv4,
        "--peer-ipv6",
        names.peer_ipv6,
        "--dns-ipv4",
        names.peer_ipv4,
    )
    return _spawn_with_ready(
        recorder,
        argv,
        ready_stream="stdout",
        ready_pattern='"status": "READY"',
        label=f"attempt-{attempt}-peer",
    )


def _start_capture(
    recorder: CommandRecorder,
    names: ResourceNames,
    attempt: int,
    phase: str,
) -> ManagedProcess:
    argv = _netns_exec(
        names.peer_namespace,
        "tcpdump",
        "-l",
        "-n",
        "-n",
        "-v",
        "-v",
        "-s",
        "0",
        "-i",
        names.peer_interface,
        "port",
        "53",
    )
    return _spawn_with_ready(
        recorder,
        argv,
        ready_stream="stderr",
        ready_pattern="listening on",
        label=f"attempt-{attempt}-{phase}-tcpdump",
    )


def _run_probe(
    recorder: CommandRecorder,
    names: ResourceNames,
    *,
    phase: str,
    trace_prefix: Path,
) -> dict[str, Any]:
    argv = _netns_exec(
        names.subject_namespace,
        "strace",
        "-ff",
        "-yy",
        "-ttt",
        "-s",
        "256",
        "-e",
        "trace=%network",
        "-o",
        str(trace_prefix),
        "setpriv",
        "--reuid",
        "65534",
        "--regid",
        "65534",
        "--clear-groups",
        "--no-new-privs",
        "--bounding-set=-all",
        "--inh-caps=-all",
        "--ambient-caps=-all",
        "python3",
        str(_probe_path()),
        "probe",
        "--mode",
        phase,
        "--peer-ipv4",
        names.peer_ipv4,
        "--peer-ipv6",
        names.peer_ipv6,
        "--dns-ipv4",
        names.peer_ipv4,
    )
    result = recorder.run(argv)
    lines = [line for line in result.stdout.splitlines() if line.strip()]
    if len(lines) != 1:
        raise TripwireProofFailure(
            f"{phase} probe emitted {len(lines)} result lines, expected one"
        )
    document = json.loads(lines[0])
    if document.get("passed") is not True:
        raise TripwireProofFailure(f"{phase} probe reported failure: {document}")
    if document.get("subject_unprivileged") is not True:
        raise TripwireProofFailure(
            f"{phase} probe retained identity or capability authority"
        )
    return document


def _assert_control(
    probe: dict[str, Any],
    counters: dict[str, int],
    capture: str,
    trace: str,
    names: ResourceNames,
) -> dict[str, bool]:
    dns_udp_seen = "nnc47-udp-control.invalid" in capture and " UDP" in capture
    dns_tcp_seen = "nnc47-tcp-control.invalid" in capture
    trace_addresses = (
        "127.0.0.1",
        names.peer_ipv4,
        names.peer_ipv6,
        "10.253.0.1",
        "192.0.2.1",
        "2001:db8::1",
    )
    return {
        "subject_unprivileged": probe.get("subject_unprivileged") is True,
        "loopback": probe.get("loopback") is True,
        "private_ipv4": probe.get("private_ipv4") is True,
        "private_ipv6": probe.get("private_ipv6") is True,
        "unenumerated_private_denied": (
            probe.get("unenumerated_private_denied") is True
            and counters["denied_private"] == 1
        ),
        "dns_udp": (
            probe.get("dns_udp_attempted") is True
            and counters["dns_udp"] == 1
            and dns_udp_seen
            and capture.count("nnc47-udp-control.invalid") == 1
        ),
        "dns_tcp": (
            probe.get("dns_tcp_attempted") is True
            and counters["dns_tcp"] == 1
            and dns_tcp_seen
            and capture.count("nnc47-tcp-control.invalid") == 1
        ),
        "public_ipv4_denied": (
            probe.get("public_ipv4_denied") is True and counters["denied_ipv4"] == 1
        ),
        "public_ipv6_denied": (
            probe.get("public_ipv6_denied") is True and counters["denied_ipv6"] == 1
        ),
        "network_trace": (
            all(address in trace for address in trace_addresses)
            and "sin_port=htons(53)" in trace
        ),
        "no_unclassified_control": counters["unexpected"] == 0,
    }


def _assert_profile(
    probe: dict[str, Any],
    counters: dict[str, int],
    capture: str,
    trace: str,
    names: ResourceNames,
) -> dict[str, bool]:
    expected_trace = ("127.0.0.1", names.peer_ipv4, names.peer_ipv6)
    forbidden_trace = ("10.253.0.1", "192.0.2.1", "2001:db8::1")
    return {
        "subject_unprivileged": probe.get("subject_unprivileged") is True,
        "private_only": all(
            probe.get(key) is True
            for key in ("loopback", "private_ipv4", "private_ipv6")
        ),
        "zero_unexpected": all(value == 0 for value in counters.values()),
        "zero_dns_capture": not capture.strip(),
        "network_trace_private_only": (
            all(address in trace for address in expected_trace)
            and all(address not in trace for address in forbidden_trace)
        ),
    }


def _snapshot_topology(
    recorder: CommandRecorder, names: ResourceNames, path: Path
) -> None:
    sections: list[str] = []
    for namespace in (names.subject_namespace, names.peer_namespace):
        for argv in (
            _netns_exec(namespace, "ip", "-details", "link", "show"),
            _netns_exec(namespace, "ip", "-4", "address", "show"),
            _netns_exec(namespace, "ip", "-6", "address", "show"),
            _netns_exec(namespace, "ip", "-4", "route", "show", "table", "all"),
            _netns_exec(namespace, "ip", "-6", "route", "show", "table", "all"),
        ):
            result = recorder.run(argv)
            sections.append("$ " + " ".join(argv) + "\n" + result.stdout)
    _write_new_text(path, "\n".join(sections))


def _run_owned_mutation(
    recorder: CommandRecorder,
    argv: Sequence[str],
    on_success: Callable[[], None],
) -> subprocess.CompletedProcess[str]:
    with _defer_termination_signals() as deferred_signals:
        result = recorder.run(argv, on_success=on_success)
    if deferred_signals:
        raise TripwireInterrupted(
            "termination signal received after owned effect: "
            + ", ".join(str(value) for value in deferred_signals)
        )
    return result


def _setup_attempt(
    recorder: CommandRecorder,
    names: ResourceNames,
    rules_path: Path,
    owned: OwnedResources,
) -> dict[str, Any]:
    netns = recorder.run(["ip", "netns", "list"], check=True).stdout
    if names.subject_namespace in netns or names.peer_namespace in netns:
        raise TripwireProofFailure("run-owned namespace already exists")
    subject_link = recorder.run(
        ["ip", "link", "show", names.subject_interface], check=False
    )
    peer_link = recorder.run(["ip", "link", "show", names.peer_interface], check=False)
    if subject_link.returncode == 0 or peer_link.returncode == 0:
        raise TripwireProofFailure("run-owned veth already exists")

    _run_owned_mutation(
        recorder,
        ["ip", "netns", "add", names.subject_namespace],
        lambda: owned.namespaces.add(names.subject_namespace),
    )
    _run_owned_mutation(
        recorder,
        ["ip", "netns", "add", names.peer_namespace],
        lambda: owned.namespaces.add(names.peer_namespace),
    )
    _run_owned_mutation(
        recorder,
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
        ],
        lambda: owned.root_interfaces.update(
            {names.subject_interface, names.peer_interface}
        ),
    )
    _run_owned_mutation(
        recorder,
        [
            "ip",
            "link",
            "set",
            names.subject_interface,
            "netns",
            names.subject_namespace,
        ],
        lambda: owned.root_interfaces.discard(names.subject_interface),
    )
    _run_owned_mutation(
        recorder,
        [
            "ip",
            "link",
            "set",
            names.peer_interface,
            "netns",
            names.peer_namespace,
        ],
        lambda: owned.root_interfaces.discard(names.peer_interface),
    )
    for namespace, interface, ipv4, ipv6 in (
        (
            names.subject_namespace,
            names.subject_interface,
            names.subject_ipv4,
            names.subject_ipv6,
        ),
        (
            names.peer_namespace,
            names.peer_interface,
            names.peer_ipv4,
            names.peer_ipv6,
        ),
    ):
        recorder.run(["ip", "-n", namespace, "link", "set", "lo", "up"])
        recorder.run(["ip", "-n", namespace, "link", "set", interface, "up"])
        recorder.run(
            ["ip", "-n", namespace, "address", "add", f"{ipv4}/24", "dev", interface]
        )
        recorder.run(
            [
                "ip",
                "-n",
                namespace,
                "-6",
                "address",
                "add",
                f"{ipv6}/64",
                "dev",
                interface,
                "nodad",
            ]
        )
    recorder.run(
        [
            "ip",
            "-n",
            names.subject_namespace,
            "route",
            "add",
            "default",
            "via",
            names.peer_ipv4,
            "dev",
            names.subject_interface,
        ]
    )
    recorder.run(
        [
            "ip",
            "-n",
            names.subject_namespace,
            "-6",
            "route",
            "add",
            "default",
            "via",
            names.peer_ipv6,
            "dev",
            names.subject_interface,
        ]
    )
    recorder.run(
        _netns_exec(names.peer_namespace, "sysctl", "-q", "-w", "net.ipv4.ip_forward=0")
    )
    recorder.run(
        _netns_exec(
            names.peer_namespace,
            "sysctl",
            "-q",
            "-w",
            "net.ipv6.conf.all.forwarding=0",
        )
    )
    forwarding: dict[str, int] = {}
    for setting, family in (
        ("net.ipv4.ip_forward", "ipv4"),
        ("net.ipv6.conf.all.forwarding", "ipv6"),
    ):
        result = recorder.run(
            _netns_exec(names.peer_namespace, "sysctl", "-n", setting)
        )
        try:
            forwarding[family] = int(result.stdout.strip())
        except ValueError as error:
            raise TripwireProofFailure(
                f"peer forwarding read-back for {setting} is not an integer"
            ) from error
    if forwarding != {"ipv4": 0, "ipv6": 0}:
        raise TripwireProofFailure(
            f"peer forwarding remained enabled after configuration: {forwarding}"
        )
    _write_nft_rules(rules_path, names)
    recorder.run(_netns_exec(names.subject_namespace, "nft", "-f", str(rules_path)))
    return {
        "preexisting_namespaces_absent": True,
        "preexisting_veths_absent": True,
        "peer_forwarding": forwarding,
    }


def _cleanup_attempt(
    recorder: CommandRecorder,
    names: ResourceNames,
    owned: OwnedResources,
    *,
    peer: ManagedProcess | None,
    capture: ManagedProcess | None,
) -> dict[str, Any]:
    errors: list[str] = []

    def record_error(label: str, error: BaseException) -> None:
        errors.append(f"{label}: {type(error).__name__}: {error}")

    process_results: dict[str, int | None] = {"peer": None, "capture": None}
    if capture is not None:
        try:
            process_results["capture"] = _stop_process(recorder, capture, signal.SIGINT)
        except BaseException as error:
            record_error("capture stop", error)
    if peer is not None:
        try:
            process_results["peer"] = _stop_process(recorder, peer, signal.SIGTERM)
        except BaseException as error:
            record_error("peer stop", error)
    delete_results: dict[str, int] = {}
    for namespace in (names.subject_namespace, names.peer_namespace):
        if namespace in owned.namespaces:
            try:
                result = recorder.run(["ip", "netns", "delete", namespace], check=False)
                delete_results[namespace] = result.returncode
                if result.returncode == 0:
                    owned.namespaces.discard(namespace)
            except BaseException as error:
                record_error(f"namespace delete {namespace}", error)
    root_delete_results: dict[str, int] = {}
    for interface in sorted(owned.root_interfaces):
        try:
            result = recorder.run(["ip", "link", "delete", interface], check=False)
            root_delete_results[interface] = result.returncode
            if result.returncode == 0:
                owned.root_interfaces.discard(interface)
        except BaseException as error:
            record_error(f"root interface delete {interface}", error)

    netns_after: str | None = None
    subject_link_exit: int | None = None
    peer_link_exit: int | None = None
    try:
        netns_after = recorder.run(["ip", "netns", "list"], check=True).stdout
    except BaseException as error:
        record_error("namespace absence check", error)
    for label, interface in (
        ("subject", names.subject_interface),
        ("peer", names.peer_interface),
    ):
        try:
            result = recorder.run(["ip", "link", "show", interface], check=False)
            if label == "subject":
                subject_link_exit = result.returncode
            else:
                peer_link_exit = result.returncode
        except BaseException as error:
            record_error(f"{label} veth absence check", error)

    namespaces_absent = (
        netns_after is not None
        and names.subject_namespace not in netns_after
        and names.peer_namespace not in netns_after
    )
    root_veths_absent = (
        subject_link_exit is not None
        and peer_link_exit is not None
        and subject_link_exit != 0
        and peer_link_exit != 0
    )
    absent = (
        not errors
        and namespaces_absent
        and root_veths_absent
        and all(status in {0, 1} for status in delete_results.values())
        and all(status in {0, 1} for status in root_delete_results.values())
        and not owned.namespaces
        and not owned.root_interfaces
    )
    return {
        "process_exit_codes": process_results,
        "namespace_delete_exit_codes": delete_results,
        "root_interface_delete_exit_codes": root_delete_results,
        "errors": errors,
        "namespaces_absent": namespaces_absent,
        "root_veths_absent": root_veths_absent,
        "passed": absent,
    }


def _run_attempt(
    recorder: CommandRecorder,
    names: ResourceNames,
    output_dir: Path,
    attempt: int,
) -> dict[str, Any]:
    attempt_dir = output_dir / f"attempt-{attempt}"
    os.mkdir(attempt_dir, 0o700)
    rules_path = attempt_dir / "rules.nft"
    topology_path = attempt_dir / "topology.txt"
    control_capture_text = attempt_dir / "control-dns.txt"
    control_trace = attempt_dir / "control.strace"
    profile_capture_text = attempt_dir / "profile-dns.txt"
    profile_trace = attempt_dir / "profile.strace"

    peer: ManagedProcess | None = None
    capture: ManagedProcess | None = None
    owned = OwnedResources(namespaces=set(), root_interfaces=set())
    cleanup: dict[str, Any] = {"passed": False, "not_entered": True}
    attempt_result: dict[str, Any] = {
        "attempt": attempt,
        "started_at": utc_now(),
        "resources": asdict(names),
    }
    failure: BaseException | None = None
    try:
        setup = _setup_attempt(recorder, names, rules_path, owned)
        attempt_result["setup"] = setup
        _snapshot_topology(recorder, names, topology_path)
        peer = _start_peer(recorder, names, attempt)
        capture = _start_capture(recorder, names, attempt, "control")
        control_probe = _run_probe(
            recorder, names, phase="control", trace_prefix=control_trace
        )
        stopped_capture = capture
        capture_exit = _stop_process(recorder, stopped_capture, signal.SIGINT)
        capture = None
        if capture_exit not in {0, -signal.SIGINT}:
            raise TripwireProofFailure(
                f"control tcpdump exited unexpectedly: {capture_exit}"
            )
        control_capture = stopped_capture.stdout_path.read_text(encoding="utf-8")
        _write_new_text(control_capture_text, control_capture)
        control_counters = _counter_snapshot(recorder, names)
        control_trace_text = _trace_text(control_trace)
        controls = _assert_control(
            control_probe,
            control_counters,
            control_capture,
            control_trace_text,
            names,
        )
        if not all(controls.values()):
            failed = sorted(key for key, value in controls.items() if not value)
            raise TripwireProofFailure(
                "positive-control tripwire assertions failed: " + ", ".join(failed)
            )

        recorder.run(
            _netns_exec(
                names.subject_namespace,
                "nft",
                "delete",
                "table",
                "inet",
                names.nft_table,
            )
        )
        recorder.run(_netns_exec(names.subject_namespace, "nft", "-f", str(rules_path)))
        reset_counters = _counter_snapshot(recorder, names)
        reset_zero = all(value == 0 for value in reset_counters.values())
        if not reset_zero:
            raise TripwireProofFailure(
                f"counter reset left residual evidence: {reset_counters}"
            )

        capture = _start_capture(recorder, names, attempt, "profile")
        profile_probe = _run_probe(
            recorder, names, phase="profile", trace_prefix=profile_trace
        )
        stopped_capture = capture
        capture_exit = _stop_process(recorder, stopped_capture, signal.SIGINT)
        capture = None
        if capture_exit not in {0, -signal.SIGINT}:
            raise TripwireProofFailure(
                f"profile tcpdump exited unexpectedly: {capture_exit}"
            )
        profile_capture = stopped_capture.stdout_path.read_text(encoding="utf-8")
        _write_new_text(profile_capture_text, profile_capture)
        profile_counters = _counter_snapshot(recorder, names)
        profile_trace_text = _trace_text(profile_trace)
        profile = _assert_profile(
            profile_probe,
            profile_counters,
            profile_capture,
            profile_trace_text,
            names,
        )
        if not all(profile.values()):
            failed = sorted(key for key, value in profile.items() if not value)
            raise TripwireProofFailure(
                "post-reset profile assertions failed: " + ", ".join(failed)
            )
        attempt_result.update(
            {
                "control": {
                    "probe": control_probe,
                    "counters": control_counters,
                    "assertions": controls,
                },
                "reset": {
                    "counters": reset_counters,
                    "passed": reset_zero,
                },
                "profile": {
                    "probe": profile_probe,
                    "counters": profile_counters,
                    "assertions": profile,
                },
            }
        )
    except BaseException as error:
        failure = error
    finally:
        with _defer_termination_signals() as deferred_signals:
            cleanup = _cleanup_attempt(
                recorder, names, owned, peer=peer, capture=capture
            )
        cleanup["deferred_signals"] = deferred_signals
        if deferred_signals and failure is None:
            failure = TripwireInterrupted(
                "termination signal received during cleanup: "
                + ", ".join(str(value) for value in deferred_signals)
            )
        attempt_result["cleanup"] = cleanup
        attempt_result["finished_at"] = utc_now()
        _write_new_text(
            attempt_dir / "attempt.json",
            json.dumps(attempt_result, indent=2, sort_keys=True) + "\n",
        )
    if not cleanup.get("passed"):
        cleanup_error = TripwireProofFailure(
            f"attempt {attempt} cleanup did not prove exact absence: {cleanup}"
        )
        if failure is not None:
            raise cleanup_error from failure
        raise cleanup_error
    if failure is not None:
        raise failure
    return attempt_result


def _assertion(
    identity: str, expected: Any, observed: Any, passed: bool
) -> dict[str, Any]:
    return {
        "id": identity,
        "expected": expected,
        "observed": observed,
        "passed": passed,
    }


def _pass_assertions(
    attempts: list[dict[str, Any]], runner: dict[str, Any]
) -> list[dict[str, Any]]:
    controls = [attempt["control"]["assertions"] for attempt in attempts]
    profiles = [attempt["profile"]["assertions"] for attempt in attempts]
    cleanup = [attempt["cleanup"]["passed"] for attempt in attempts]
    assertions = [
        _assertion(
            "preflight.named_runner",
            runner["expected_hostname"],
            runner["observed_hostname"],
            runner["expected_hostname"] == runner["observed_hostname"],
        ),
        _assertion("preflight.offline_inputs", True, True, True),
        _assertion(
            "isolation.outer_namespaces",
            True,
            [
                attempt["setup"]["preexisting_namespaces_absent"]
                and attempt["setup"]["preexisting_veths_absent"]
                for attempt in attempts
            ],
            all(
                attempt["setup"]["preexisting_namespaces_absent"]
                and attempt["setup"]["preexisting_veths_absent"]
                for attempt in attempts
            ),
        ),
        _assertion(
            "isolation.subject_unprivileged",
            True,
            [control["subject_unprivileged"] for control in controls],
            all(control["subject_unprivileged"] for control in controls),
        ),
        _assertion(
            "isolation.peer_forwarding_disabled",
            {"ipv4": 0, "ipv6": 0},
            [attempt["setup"]["peer_forwarding"] for attempt in attempts],
            all(
                attempt["setup"]["peer_forwarding"] == {"ipv4": 0, "ipv6": 0}
                for attempt in attempts
            ),
        ),
        _assertion(
            "control.loopback",
            True,
            [control["loopback"] for control in controls],
            all(control["loopback"] for control in controls),
        ),
        _assertion(
            "control.private_ipv4",
            True,
            [control["private_ipv4"] for control in controls],
            all(control["private_ipv4"] for control in controls),
        ),
        _assertion(
            "control.private_ipv6",
            True,
            [control["private_ipv6"] for control in controls],
            all(control["private_ipv6"] for control in controls),
        ),
        _assertion(
            "control.unenumerated_private_denied",
            True,
            [control["unenumerated_private_denied"] for control in controls],
            all(control["unenumerated_private_denied"] for control in controls),
        ),
        _assertion(
            "control.dns_udp",
            True,
            [control["dns_udp"] for control in controls],
            all(control["dns_udp"] for control in controls),
        ),
        _assertion(
            "control.dns_tcp",
            True,
            [control["dns_tcp"] for control in controls],
            all(control["dns_tcp"] for control in controls),
        ),
        _assertion(
            "control.public_ipv4_denied",
            True,
            [control["public_ipv4_denied"] for control in controls],
            all(control["public_ipv4_denied"] for control in controls),
        ),
        _assertion(
            "control.public_ipv6_denied",
            True,
            [control["public_ipv6_denied"] for control in controls],
            all(control["public_ipv6_denied"] for control in controls),
        ),
        _assertion(
            "control.network_trace",
            True,
            [control["network_trace"] for control in controls],
            all(control["network_trace"] for control in controls),
        ),
        _assertion(
            "reset.zero_baseline",
            True,
            [attempt["reset"]["passed"] for attempt in attempts],
            all(attempt["reset"]["passed"] for attempt in attempts),
        ),
        _assertion(
            "profile.private_only",
            True,
            [profile["private_only"] for profile in profiles],
            all(profile["private_only"] for profile in profiles),
        ),
        _assertion(
            "profile.zero_unexpected",
            True,
            [profile["zero_unexpected"] for profile in profiles],
            all(profile["zero_unexpected"] for profile in profiles),
        ),
        _assertion("cleanup.absent", True, cleanup, all(cleanup)),
        _assertion(
            "cleanup.same_identity_reentry",
            2,
            len(attempts),
            len(attempts) >= 2
            and attempts[1]["setup"]["preexisting_namespaces_absent"]
            and attempts[1]["setup"]["preexisting_veths_absent"],
        ),
        _assertion("evidence.artifacts_authenticated", True, True, True),
    ]
    identities = {item["id"] for item in assertions}
    if identities != REQUIRED_PASS_ASSERTIONS:
        raise TripwireProofFailure(
            "internal assertion contract mismatch: "
            f"missing={sorted(REQUIRED_PASS_ASSERTIONS - identities)} "
            f"extra={sorted(identities - REQUIRED_PASS_ASSERTIONS)}"
        )
    return assertions


def _recursive_files(root: Path) -> list[Path]:
    return [
        path for path in root.rglob("*") if path.is_file() and not path.is_symlink()
    ]


def run_live(config: TripwireConfig) -> int:
    started_at = utc_now()
    _prepare_output_directory(config.output_dir)
    evidence_path = config.output_dir / "evidence.json"

    runner = minimal_runner(config)
    source: dict[str, Any] = {}
    try:
        facts, runner = collect_preflight(config)
    except BaseException as error:
        document = _empty_evidence(
            config,
            runner,
            {},
            status="FAIL",
            exit_code=CONFIG_EXIT,
            reason=f"runner preflight failed: {type(error).__name__}: {error}",
            phase="preflight",
            started_at=started_at,
        )
        validate_evidence(
            document,
            evidence_root=config.output_dir,
            source_root=repo_root(),
        )
        atomic_write_json(evidence_path, document)
        return CONFIG_EXIT
    decision, reason = preflight_decision(facts)
    if decision == "ADMITTED":
        tools = runner.get("tools")
        git_record = tools.get("git") if isinstance(tools, dict) else None
        git_path = git_record.get("path") if isinstance(git_record, dict) else None
        if not isinstance(git_path, str):
            decision = "FAIL"
            reason = "admitted runner lacks an authenticated Git executable"
        else:
            try:
                source = source_facts(git_path)
            except BaseException as error:
                decision = "FAIL"
                reason = f"source preflight failed: {type(error).__name__}: {error}"
            if decision == "ADMITTED":
                source_error = source_failure(source)
                if source_error is not None:
                    decision = "FAIL"
                    reason = source_error
    if decision == "SKIPPED":
        document = _empty_evidence(
            config,
            runner,
            source,
            status="SKIPPED",
            exit_code=SKIPPED_EXIT,
            reason=reason or "preflight unavailable",
            phase="preflight",
            started_at=started_at,
        )
        validate_evidence(
            document,
            evidence_root=config.output_dir,
            source_root=repo_root(),
        )
        atomic_write_json(evidence_path, document)
        return SKIPPED_EXIT
    if decision == "FAIL":
        document = _empty_evidence(
            config,
            runner,
            source,
            status="FAIL",
            exit_code=CONFIG_EXIT,
            reason=reason or "invalid runner identity",
            phase="preflight",
            started_at=started_at,
        )
        validate_evidence(
            document,
            evidence_root=config.output_dir,
            source_root=repo_root(),
        )
        atomic_write_json(evidence_path, document)
        return CONFIG_EXIT

    tool_records = runner["tools"]
    tool_paths = {name: record["path"] for name, record in tool_records.items()}
    recorder = CommandRecorder(
        config.output_dir,
        config.command_timeout_seconds,
        tool_paths,
    )
    names = _resource_names(config.runner_id)
    attempts: list[dict[str, Any]] = []
    lock_path = Path("/run/lock/nimbus-network-sovereignty-tripwire.lock")
    failure: BaseException | None = None
    previous_handlers: dict[signal.Signals, Any] = {}

    def interrupt(signum: int, _frame: Any) -> None:
        raise TripwireInterrupted(
            f"received terminating signal {signal.Signals(signum).name}"
        )

    for signum in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        previous_handlers[signum] = signal.getsignal(signum)
        signal.signal(signum, interrupt)
    try:
        with _exclusive_host_lock(lock_path):
            for attempt in range(1, config.repeat + 1):
                try:
                    attempts.append(
                        _run_attempt(recorder, names, config.output_dir, attempt)
                    )
                except BaseException as error:
                    failure = error
                    break
    except BaseException as error:
        failure = error
    finally:
        for signum, handler in previous_handlers.items():
            signal.signal(signum, handler)

    inputs = {
        "runner_id": config.runner_id,
        "expected_hostname": config.expected_hostname,
        "host_class": config.host_class,
        "provider_kind": config.provider_kind,
        "repeat": config.repeat,
        "offline": True,
        "payload_argv": [],
        "prestage_argv": [],
        "private_peers": [names.peer_ipv4, names.peer_ipv6],
        "documentation_destinations": [
            "10.253.0.1",
            "192.0.2.1",
            "2001:db8::1",
        ],
    }
    if failure is not None:
        document = {
            "schema_version": EVIDENCE_SCHEMA_VERSION,
            "result": {
                "status": "FAIL",
                "exit_code": FAIL_EXIT,
                "reason": f"{type(failure).__name__}: {failure}",
                "phase": f"attempt-{len(attempts) + 1}",
                "started_at": started_at,
                "finished_at": utc_now(),
            },
            "runner": runner,
            "source": source,
            "inputs": inputs,
            "attempts": attempts,
            "commands": recorder.records,
            "assertions": [],
            "artifacts": [],
        }
        document["artifacts"] = artifact_manifest(
            config.output_dir,
            _recursive_files(config.output_dir),
            exclude=[evidence_path],
        )
        validate_evidence(
            document,
            evidence_root=config.output_dir,
            source_root=repo_root(),
        )
        atomic_write_json(evidence_path, document)
        return FAIL_EXIT

    document = {
        "schema_version": EVIDENCE_SCHEMA_VERSION,
        "result": {
            "status": "PASS",
            "exit_code": 0,
            "reason": None,
            "phase": "complete",
            "started_at": started_at,
            "finished_at": utc_now(),
        },
        "runner": runner,
        "source": source,
        "inputs": inputs,
        "attempts": attempts,
        "commands": recorder.records,
        "assertions": _pass_assertions(attempts, runner),
        "artifacts": [],
    }
    document["artifacts"] = artifact_manifest(
        config.output_dir,
        _recursive_files(config.output_dir),
        exclude=[evidence_path],
    )
    try:
        validate_evidence(
            document,
            evidence_root=config.output_dir,
            source_root=repo_root(),
        )
    except EvidenceValidationError as error:
        document["result"] = {
            "status": "FAIL",
            "exit_code": FAIL_EXIT,
            "reason": f"candidate PASS evidence rejected: {error}",
            "phase": "evidence-validation",
            "started_at": started_at,
            "finished_at": utc_now(),
        }
        document["assertions"] = []
        validate_evidence(
            document,
            evidence_root=config.output_dir,
            source_root=repo_root(),
        )
        atomic_write_json(evidence_path, document)
        return FAIL_EXIT
    atomic_write_json(evidence_path, document)
    return 0


def _absolute_output(value: str) -> Path:
    path = Path(value)
    if not path.is_absolute():
        raise argparse.ArgumentTypeError("output directory must be absolute")
    return path


def _runner_id(value: str) -> str:
    if not SAFE_RUNNER_ID.fullmatch(value):
        raise argparse.ArgumentTypeError("runner id must match [a-z][a-z0-9-]{2,62}")
    return value


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="NNC4.7 privileged local-sovereignty tripwire"
    )
    parser.add_argument("--runner-id", required=True, type=_runner_id)
    parser.add_argument("--expected-hostname", required=True)
    parser.add_argument("--host-class", choices=("kvm", "minicloud"), required=True)
    parser.add_argument("--provider-kind", choices=("kvm", "linuxkit"), required=True)
    parser.add_argument("--output-dir", required=True, type=_absolute_output)
    parser.add_argument("--repeat", type=int, default=2)
    parser.add_argument("--command-timeout-seconds", type=int, default=30)
    return parser
