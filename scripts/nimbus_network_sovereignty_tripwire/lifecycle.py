#!/usr/bin/env python3
"""Privileged NNC9.2 offline sovereign-lifecycle adapter."""

from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass
import ipaddress
import json
import os
from pathlib import Path
import re
import shutil
import signal
import sqlite3
import stat
import subprocess
import time
from typing import Any, Sequence

from .environment import (
    FAIL_EXIT,
    SKIPPED_EXIT,
    TripwireConfig,
    TripwireProofFailure,
    collect_preflight,
    preflight_decision,
    repo_root,
)
from .evidence import artifact_manifest, atomic_write_json
from .integrity import harness_source_digest, harness_source_manifest, sha256_file
from .isolation import (
    CommandRecorder,
    ManagedProcess,
    OwnedResources,
    ResourceNames,
    _assert_control,
    _cleanup_attempt,
    _counter_snapshot,
    _defer_termination_signals,
    _exclusive_host_lock,
    _netns_exec,
    _prepare_output_directory,
    _resource_names,
    _setup_attempt,
    _snapshot_topology,
    _start_capture,
    _start_peer,
    _stop_process,
    _write_new_text,
    utc_now,
)
from .lifecycle_evidence import (
    REQUIRED_TRANSITIONS,
    SCHEMA_VERSION,
    LifecycleEvidenceError,
    derive_assertions,
    teardown_order_exact,
    validate_evidence,
)

LIFECYCLE_HARNESS_PATHS = frozenset(
    {
        "scripts/nimbus-network-sovereign-lifecycle.sh",
        "scripts/nimbus-network-control-plane/sovereign-lifecycle-self-tests.py",
        "scripts/nimbus-network-control-plane/sovereign-lifecycle-self-tests.sh",
        "scripts/nimbus_network_sovereignty_tripwire/__init__.py",
        "scripts/nimbus_network_sovereignty_tripwire/environment.py",
        "scripts/nimbus_network_sovereignty_tripwire/evidence.py",
        "scripts/nimbus_network_sovereignty_tripwire/integrity.py",
        "scripts/nimbus_network_sovereignty_tripwire/isolation.py",
        "scripts/nimbus_network_sovereignty_tripwire/lifecycle.py",
        "scripts/nimbus_network_sovereignty_tripwire/lifecycle_evidence.py",
        "scripts/nimbus_network_sovereignty_tripwire/lifecycle_probe.py",
        "scripts/nimbus_network_sovereignty_tripwire/probe.py",
        "scripts/nimbus_network_sovereignty_tripwire/synchronization.py",
        "scripts/nimbus_network_sovereignty_tripwire/workspace.py",
    }
)
FIXTURE_FILES = (
    "Dockerfile",
    "compose.yaml",
    "lifecycle.sh",
    "rootfs/bin/busybox",
    "rootfs/lib/ld-linux-x86-64.so.2",
    "rootfs/lib/libc.so.6",
    "rootfs/lib/libm.so.6",
    "rootfs/lib/libresolv.so.2",
)
LIFECYCLE_TOOL_PATHS = {
    "buildah": Path("/usr/bin/buildah"),
    "conmon": Path("/usr/bin/conmon"),
    "env": Path("/usr/bin/env"),
    "nsenter": Path("/usr/bin/nsenter"),
    "nimbus-crun": Path("/usr/libexec/nimbus/crun"),
    "netavark": Path("/usr/lib/podman/netavark"),
    "aardvark-dns": Path("/usr/lib/podman/aardvark-dns"),
    "stat": Path("/usr/bin/stat"),
}
EXPECTED_PROVISION_OBSERVATIONS = (
    "network_reserved",
    "execution_prepared",
    "network_attached",
    "execution_activated",
    "ready",
    "publication_present",
    "publication_observed",
)
PROVIDER_NETWORK = ipaddress.ip_network("10.0.0.0/24")
PUBLISHED_PORT = 15992
CONTROL_QUIESCENCE_SECONDS = 2.0
SAFE_RUNNER_ID = re.compile(r"^[a-z][a-z0-9-]{2,62}$")
NETAVARK_GLOBAL_CHAINS = frozenset(
    {
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
    }
)
NETAVARK_GLOBAL_RULE_COUNTS = {
    "FORWARD": 2,
    "POSTROUTING": 1,
    "PREROUTING": 1,
    "OUTPUT": 1,
    "NETAVARK-HOSTPORT-SETMARK": 1,
    "NETAVARK-ISOLATION-3": 1,
}


@dataclass(frozen=True)
class LifecycleConfig:
    runner_id: str
    expected_hostname: str
    output_dir: Path
    nimbus_bin: Path
    expected_nimbus_sha256: str
    fixture_dir: Path
    repeat: int
    command_timeout_seconds: int
    lifecycle_timeout_seconds: int


@dataclass
class BackgroundCommand:
    argv: list[str]
    process: subprocess.Popen[bytes]
    started_at: str
    stdout_path: Path
    stderr_path: Path
    stdout_handle: Any
    stderr_handle: Any


def _recursive_files(root: Path) -> list[Path]:
    return [path for path in root.rglob("*") if path.is_file() and not path.is_symlink()]


def _failure_artifact_manifest(root: Path, evidence_path: Path) -> list[dict[str, Any]]:
    """Preserve failed-run metadata when a retained provider handle is unreadable."""

    rows: list[dict[str, Any]] = []
    for path in sorted(_recursive_files(root)):
        if path == evidence_path:
            continue
        try:
            rows.extend(artifact_manifest(root, [path]))
        except OSError as error:
            rows.append(
                {
                    "path": path.relative_to(root).as_posix(),
                    "size": path.stat().st_size,
                    "sha256": None,
                    "read_error": f"{type(error).__name__}: {error}",
                }
            )
    return rows


def _safe_absolute(value: str) -> Path:
    path = Path(value)
    if not path.is_absolute():
        raise argparse.ArgumentTypeError("path must be absolute")
    return path


def _runner_id(value: str) -> str:
    if SAFE_RUNNER_ID.fullmatch(value) is None:
        raise argparse.ArgumentTypeError("runner id must match [a-z][a-z0-9-]{2,62}")
    return value


def _sha256(value: str) -> str:
    if re.fullmatch(r"[0-9a-f]{64}", value) is None:
        raise argparse.ArgumentTypeError("expected SHA-256 must be 64 lowercase hex digits")
    return value


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="NNC9.2 complete offline sovereign lifecycle"
    )
    parser.add_argument("--runner-id", required=True, type=_runner_id)
    parser.add_argument("--expected-hostname", required=True)
    parser.add_argument("--output-dir", required=True, type=_safe_absolute)
    parser.add_argument("--nimbus-bin", required=True, type=_safe_absolute)
    parser.add_argument("--expected-nimbus-sha256", required=True, type=_sha256)
    parser.add_argument("--fixture-dir", required=True, type=_safe_absolute)
    parser.add_argument("--repeat", type=int, default=2)
    parser.add_argument("--command-timeout-seconds", type=int, default=120)
    parser.add_argument("--lifecycle-timeout-seconds", type=int, default=300)
    return parser


def _trusted_regular(path: Path, *, executable: bool) -> bool:
    try:
        metadata = path.lstat()
    except OSError:
        return False
    if path.is_symlink() or not stat.S_ISREG(metadata.st_mode) or metadata.st_uid != 0:
        return False
    if metadata.st_mode & (stat.S_IWGRP | stat.S_IWOTH):
        return False
    return not executable or os.access(path, os.X_OK)


def _trusted_directory(path: Path) -> bool:
    try:
        metadata = path.lstat()
    except OSError:
        return False
    return (
        not path.is_symlink()
        and stat.S_ISDIR(metadata.st_mode)
        and metadata.st_uid == 0
        and not metadata.st_mode & (stat.S_IWGRP | stat.S_IWOTH)
    )


def _tool_version(path: Path) -> str:
    for flag in ("--version", "-V"):
        try:
            result = subprocess.run(
                [str(path), flag],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                timeout=10,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired):
            continue
        if result.returncode == 0 and result.stdout.strip():
            return result.stdout.strip().splitlines()[0]
    return "version unavailable"


def _git_output(git_path: str, *argv: str) -> str:
    result = subprocess.run(
        [git_path, "-c", f"safe.directory={repo_root()}", *argv],
        cwd=repo_root(),
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=60,
        check=False,
    )
    if result.returncode != 0:
        raise TripwireProofFailure(
            "source identity command failed: " + result.stderr.strip()
        )
    return result.stdout.strip()


def _source_facts(git_path: str) -> dict[str, Any]:
    missing = [
        relative
        for relative in sorted(LIFECYCLE_HARNESS_PATHS)
        if not (repo_root() / relative).is_file()
    ]
    if missing:
        raise TripwireProofFailure("lifecycle harness is incomplete: " + ", ".join(missing))
    return {
        "commit": _git_output(git_path, "rev-parse", "HEAD"),
        "tree": _git_output(git_path, "rev-parse", "HEAD^{tree}"),
        "dirty": _git_output(git_path, "status", "--short", "--untracked-files=all"),
        "harness_sha256": harness_source_digest(repo_root(), LIFECYCLE_HARNESS_PATHS),
        "harness_paths": sorted(LIFECYCLE_HARNESS_PATHS),
        "harness_files": harness_source_manifest(repo_root(), LIFECYCLE_HARNESS_PATHS),
    }


def _copy_durable(source: Path, destination: Path, mode: int) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    temporary = destination.with_name(f".{destination.name}.{os.getpid()}.tmp")
    with source.open("rb") as reader, temporary.open("xb") as writer:
        shutil.copyfileobj(reader, writer, length=1024 * 1024)
        writer.flush()
        os.fsync(writer.fileno())
    os.chmod(temporary, mode)
    os.replace(temporary, destination)
    directory = os.open(destination.parent, os.O_RDONLY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


def _stage_candidate(config: LifecycleConfig) -> Path:
    observed = sha256_file(config.nimbus_bin)
    if observed != config.expected_nimbus_sha256:
        raise TripwireProofFailure(
            f"candidate hash mismatch: expected {config.expected_nimbus_sha256}, observed {observed}"
        )
    base = Path("/var/lib/nimbus-nnc92-candidates")
    base.mkdir(mode=0o700, exist_ok=True)
    if not _trusted_directory(base):
        raise TripwireProofFailure("candidate staging base is not root-exclusive")
    root = base / observed
    root.mkdir(mode=0o700, exist_ok=True)
    os.chmod(root, 0o700)
    if not _trusted_directory(root):
        raise TripwireProofFailure("candidate staging directory is not root-exclusive")
    staged = root / "nimbus"
    if not staged.exists():
        _copy_durable(config.nimbus_bin, staged, 0o500)
    if not _trusted_regular(staged, executable=True) or sha256_file(staged) != observed:
        raise TripwireProofFailure("root-owned staged candidate failed authentication")
    return staged


def _stage_control_probe() -> Path:
    source = Path(__file__).resolve().with_name("probe.py")
    digest = sha256_file(source)
    base = Path("/var/lib/nimbus-nnc92-probes")
    base.mkdir(mode=0o555, exist_ok=True)
    os.chmod(base, 0o555)
    if not _trusted_directory(base):
        raise TripwireProofFailure("control-probe staging base is not root-owned")
    staged = base / f"{digest}.py"
    if not staged.exists():
        _copy_durable(source, staged, 0o444)
    if not _trusted_regular(staged, executable=False) or sha256_file(staged) != digest:
        raise TripwireProofFailure("staged unprivileged control probe failed authentication")
    return staged


def _stage_fixture(config: LifecycleConfig) -> tuple[Path, list[dict[str, Any]]]:
    stage = config.output_dir / "staged-fixture"
    stage.mkdir(mode=0o700)
    manifest: list[dict[str, Any]] = []
    for relative in FIXTURE_FILES:
        source = config.fixture_dir / relative
        if not source.is_file():
            raise TripwireProofFailure(f"fixture input is missing: {source}")
        destination = stage / relative
        executable = bool(stat.S_IMODE(source.stat().st_mode) & 0o111)
        expected_mode = 0o555 if executable else 0o444
        _copy_durable(source, destination, expected_mode)
        observed_mode = stat.S_IMODE(destination.stat().st_mode)
        if observed_mode != expected_mode:
            raise TripwireProofFailure(
                f"staged fixture mode mismatch: {relative} "
                f"expected={expected_mode:o} observed={observed_mode:o}"
            )
        manifest.append(
            {
                "path": relative,
                "size": destination.stat().st_size,
                "sha256": sha256_file(destination),
                "mode": observed_mode,
            }
        )
    return stage, manifest


def _run_fixture_smoke(
    recorder: CommandRecorder,
    fixture: Path,
) -> dict[str, Any]:
    rootfs = fixture / "rootfs"
    result = _run(
        recorder,
        [
            str(rootfs / "lib/ld-linux-x86-64.so.2"),
            "--library-path",
            str(rootfs / "lib"),
            str(rootfs / "bin/busybox"),
            "true",
        ],
    )
    return {"passed": result.returncode == 0, "exit_code": result.returncode}


def _run_self_tests(
    config: LifecycleConfig,
    recorder: CommandRecorder,
    admitted_source: dict[str, Any],
) -> dict[str, Any]:
    destination = config.output_dir / "self-tests.json"
    script = repo_root() / "scripts/nimbus-network-control-plane/sovereign-lifecycle-self-tests.py"
    result = _run(
        recorder,
        [
            "env",
            "PYTHONDONTWRITEBYTECODE=1",
            "python3",
            str(script),
            "--json-output",
            str(destination),
        ],
        timeout=config.command_timeout_seconds,
    )
    try:
        document = json.loads(destination.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise TripwireProofFailure(f"self-test evidence is unavailable: {error}") from error
    if document.get("passed") != document.get("required") or document.get("required") != 8:
        raise TripwireProofFailure("lifecycle self-tests did not pass the exact 8-case contract")
    if json.loads(result.stdout) != document:
        raise TripwireProofFailure("self-test stdout crossed its durable evidence")
    current_source = _source_facts(admitted_source["git_path"])
    current_source.pop("git_path", None)
    expected_source = dict(admitted_source)
    expected_source.pop("git_path", None)
    if current_source != expected_source:
        raise TripwireProofFailure("harness source changed during admission self-tests")
    document["sha256"] = sha256_file(destination)
    return document


def _write_lifecycle_rules(path: Path, names: ResourceNames) -> None:
    rules = f"""table inet {names.nft_table} {{
  counter dns_udp {{ }}
  counter dns_tcp {{ }}
  counter denied_private {{ }}
  counter denied_ipv4 {{ }}
  counter denied_ipv6 {{ }}
  counter unexpected {{ }}
  chain output {{
    type filter hook output priority filter; policy drop;
    oifname \"lo\" accept
    ct state established,related accept
    ip6 daddr fe80::/10 meta l4proto ipv6-icmp accept
    ip6 daddr ff02::/16 meta l4proto ipv6-icmp accept
    ip daddr {names.peer_ipv4} udp dport 53 counter name dns_udp accept
    ip daddr {names.peer_ipv4} tcp dport 53 counter name dns_tcp accept
    ip daddr {names.peer_ipv4} accept
    ip6 daddr {names.peer_ipv6} accept
    ip daddr {PROVIDER_NETWORK} accept
    ip daddr 224.0.0.0/24 ip protocol igmp accept
    ip daddr 10.0.0.0/8 counter name denied_private drop
    meta nfproto ipv4 counter name denied_ipv4 drop
    meta nfproto ipv6 counter name denied_ipv6 drop
    counter name unexpected drop
  }}
  chain forward {{
    type filter hook forward priority filter; policy drop;
    ct state established,related accept
    ip daddr {names.peer_ipv4} accept
    ip6 daddr {names.peer_ipv6} accept
    ip saddr {PROVIDER_NETWORK} ip daddr {PROVIDER_NETWORK} accept
    ip daddr 10.0.0.0/8 counter name denied_private drop
    meta nfproto ipv4 counter name denied_ipv4 drop
    meta nfproto ipv6 counter name denied_ipv6 drop
    counter name unexpected drop
  }}
}}
"""
    _write_new_text(path, rules)


def _mark_latest(
    recorder: CommandRecorder,
    *,
    critical: bool,
    accepted_exit_codes: Sequence[int] = (0,),
) -> None:
    record = recorder.records[-1]
    record["critical"] = critical
    record["accepted_exit_codes"] = list(accepted_exit_codes)


def _run(
    recorder: CommandRecorder,
    argv: Sequence[str],
    *,
    check: bool = True,
    critical: bool = True,
    accepted_exit_codes: Sequence[int] = (0,),
    timeout: int | None = None,
) -> subprocess.CompletedProcess[str]:
    result = recorder.run(argv, check=check, timeout=timeout)
    _mark_latest(
        recorder,
        critical=critical,
        accepted_exit_codes=accepted_exit_codes,
    )
    return result


def _spawn(
    recorder: CommandRecorder,
    argv: Sequence[str],
    *,
    stdout_path: Path,
    stderr_path: Path,
) -> BackgroundCommand:
    authenticated = recorder.authenticate_argv(argv)
    stdout_handle = stdout_path.open("xb")
    stderr_handle = stderr_path.open("xb")
    try:
        process = subprocess.Popen(
            authenticated,
            stdin=subprocess.DEVNULL,
            stdout=stdout_handle,
            stderr=stderr_handle,
            start_new_session=True,
        )
    except BaseException:
        stdout_handle.close()
        stderr_handle.close()
        raise
    return BackgroundCommand(
        argv=authenticated,
        process=process,
        started_at=utc_now(),
        stdout_path=stdout_path,
        stderr_path=stderr_path,
        stdout_handle=stdout_handle,
        stderr_handle=stderr_handle,
    )


def _finish_background(
    recorder: CommandRecorder,
    command: BackgroundCommand,
    *,
    stop_signal: signal.Signals | None,
    timeout_seconds: int,
    accepted_exit_codes: Sequence[int],
) -> int:
    if stop_signal is not None and command.process.poll() is None:
        os.killpg(command.process.pid, stop_signal)
    timed_out = False
    try:
        exit_code = command.process.wait(timeout=timeout_seconds)
    except subprocess.TimeoutExpired:
        timed_out = True
        os.killpg(command.process.pid, signal.SIGKILL)
        exit_code = command.process.wait(timeout=10)
    finally:
        command.stdout_handle.close()
        command.stderr_handle.close()
    recorder.add_process_record(
        command.argv,
        started_at=command.started_at,
        exit_code=exit_code,
        process_group=command.process.pid,
        stdout_path=command.stdout_path,
        stderr_path=command.stderr_path,
    )
    record = recorder.records[-1]
    record["critical"] = True
    record["accepted_exit_codes"] = list(accepted_exit_codes)
    record["timed_out"] = timed_out
    if timed_out:
        raise TripwireProofFailure("background lifecycle command timed out during stop")
    if exit_code not in accepted_exit_codes:
        raise TripwireProofFailure(
            f"background lifecycle command exited {exit_code}, expected {list(accepted_exit_codes)}"
        )
    return exit_code


def _network_namespace_exec(names: ResourceNames, *command: str) -> list[str]:
    """Enter only the subject network namespace and preserve host mount state."""

    return [
        "nsenter",
        f"--net=/run/netns/{names.subject_namespace}",
        "--",
        *command,
    ]


def _product_argv(
    names: ResourceNames,
    trace_prefix: Path,
    candidate: Path,
    durable_root: Path,
    fixture: Path,
    *command: str,
) -> list[str]:
    return _network_namespace_exec(
        names,
        "strace",
        "-DDD",
        "-ff",
        "-yy",
        "-ttt",
        "-s",
        "256",
        "-e",
        "trace=%network",
        "-o",
        str(trace_prefix),
        "env",
        f"NIMBUS_DATA_DIR={durable_root / 'data'}",
        f"NIMBUS_CONTROL_DATA_DIR={durable_root / 'control'}",
        "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        str(candidate),
        "compose",
        "--network-state-dir",
        str(durable_root / "network"),
        *command,
        "--file",
        str(fixture / "compose.yaml"),
    )


def _cgroup_namespace_view(
    recorder: CommandRecorder,
    names: ResourceNames,
) -> dict[str, Any]:
    host = _run(
        recorder,
        ["stat", "-f", "-c", "%T", "/sys/fs/cgroup"],
    ).stdout.strip()
    subject = _run(
        recorder,
        _network_namespace_exec(
            names,
            "stat",
            "-f",
            "-c",
            "%T",
            "/sys/fs/cgroup",
        ),
    ).stdout.strip()
    passed = host in {"cgroup", "cgroup2fs"} and subject == host
    if not passed:
        raise TripwireProofFailure(
            "network namespace entry changed the cgroup filesystem view: "
            f"host={host!r} subject={subject!r}"
        )
    return {"host": host, "subject": subject, "passed": True}


def _probe_argv(
    names: ResourceNames,
    trace_prefix: Path,
    *command: str,
) -> list[str]:
    return _netns_exec(
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
        "python3",
        str(Path(__file__).resolve().with_name("lifecycle_probe.py")),
        *command,
    )


def _run_control_probe(
    recorder: CommandRecorder,
    names: ResourceNames,
    staged_probe: Path,
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
        str(staged_probe),
        "probe",
        "--mode",
        "control",
        "--peer-ipv4",
        names.peer_ipv4,
        "--peer-ipv6",
        names.peer_ipv6,
        "--dns-ipv4",
        names.peer_ipv4,
    )
    result = _run(recorder, argv)
    lines = [line for line in result.stdout.splitlines() if line.strip()]
    if len(lines) != 1:
        raise TripwireProofFailure(
            f"control probe emitted {len(lines)} result lines, expected one"
        )
    try:
        document = json.loads(lines[0])
    except json.JSONDecodeError as error:
        raise TripwireProofFailure("control probe emitted invalid JSON") from error
    if document.get("passed") is not True or document.get("subject_unprivileged") is not True:
        raise TripwireProofFailure(f"control probe retained authority or failed: {document}")
    return document


def _json_stdout(result: subprocess.CompletedProcess[str], label: str) -> dict[str, Any]:
    lines = [line for line in result.stdout.splitlines() if line.strip()]
    if len(lines) != 1:
        raise TripwireProofFailure(f"{label} emitted {len(lines)} JSON lines")
    try:
        value = json.loads(lines[0])
    except json.JSONDecodeError as error:
        raise TripwireProofFailure(f"{label} emitted invalid JSON") from error
    if not isinstance(value, dict) or value.get("passed") is not True:
        raise TripwireProofFailure(f"{label} failed: {value}")
    return value


def _terminal_compose_projection_exact(
    value: Any,
    *,
    tenant_id: str,
    sandbox_id: str,
    service_name: str,
) -> bool:
    """Authenticate the one retained, terminal Compose observation."""

    if not isinstance(value, list) or len(value) != 1 or not isinstance(value[0], dict):
        return False
    row = value[0]
    last_exit_code = row.get("last_exit_code")
    return (
        set(row)
        == {
            "sandbox_id",
            "tenant_id",
            "service_name",
            "status",
            "published_endpoints",
            "last_exit_code",
            "shutdown_requested",
        }
        and row.get("sandbox_id") == sandbox_id
        and row.get("tenant_id") == tenant_id
        and row.get("service_name") == service_name
        and row.get("status") == "stopped"
        and row.get("published_endpoints") == []
        and row.get("shutdown_requested") is True
        and (
            last_exit_code is None
            or (isinstance(last_exit_code, int) and not isinstance(last_exit_code, bool))
        )
    )


def _wait_for_text(path: Path, text: str, timeout_seconds: int) -> None:
    deadline = time.monotonic() + timeout_seconds
    while time.monotonic() < deadline:
        try:
            content = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            content = ""
        if text in content:
            return
        time.sleep(0.2)
    raise TripwireProofFailure(f"{path.name} did not report {text!r}")


def _find_manifest(durable_root: Path) -> Path:
    paths = sorted(
        durable_root.glob(
            "control/services/projects/*/backends/krun/state/tenants/*/sandboxes/*/state/containers/*/manifest.json"
        )
    )
    if len(paths) != 1:
        raise TripwireProofFailure(f"expected one Krun manifest, observed {len(paths)}")
    return paths[0]


def _wait_manifest(
    durable_root: Path,
    *,
    prior_attempt: str | None,
    timeout_seconds: int,
) -> tuple[Path, dict[str, Any]]:
    deadline = time.monotonic() + timeout_seconds
    last: str = "manifest absent"
    while time.monotonic() < deadline:
        try:
            path = _find_manifest(durable_root)
            manifest = json.loads(path.read_text(encoding="utf-8"))
            attempt = manifest.get("execution_attempt_id")
            handle = manifest.get("handle", {})
            if (
                isinstance(attempt, str)
                and attempt != prior_attempt
                and isinstance(handle.get("id"), str)
                and handle.get("backend") == "krun"
                and isinstance(manifest.get("provision_network_plan"), dict)
            ):
                return path, manifest
            last = (
                f"attempt={attempt!r} id={handle.get('id')!r} "
                f"backend={handle.get('backend')!r}"
            )
        except (OSError, json.JSONDecodeError, TripwireProofFailure) as error:
            last = str(error)
        time.sleep(0.2)
    raise TripwireProofFailure(f"manifest did not publish exact provider identity: {last}")


def _wait_restart_manifest(
    durable_root: Path,
    *,
    prior_attempt: str,
    sandbox_id: str,
    timeout_seconds: int,
) -> tuple[Path, dict[str, Any]]:
    """Wait for the next execution identity without treating provider state as projection."""

    deadline = time.monotonic() + timeout_seconds
    last: str = "manifest absent"
    while time.monotonic() < deadline:
        try:
            path = _find_manifest(durable_root)
            manifest = json.loads(path.read_text(encoding="utf-8"))
            attempt = manifest.get("execution_attempt_id")
            handle = manifest.get("handle", {})
            if (
                isinstance(attempt, str)
                and attempt != prior_attempt
                and handle.get("id") == sandbox_id
                and handle.get("backend") == "krun"
            ):
                return path, manifest
            last = (
                f"attempt={attempt!r} id={handle.get('id')!r} "
                f"backend={handle.get('backend')!r}"
            )
        except (OSError, json.JSONDecodeError, TripwireProofFailure) as error:
            last = str(error)
        time.sleep(0.2)
    raise TripwireProofFailure(f"manifest did not publish the next exact attempt: {last}")


def _manifest_rootfs(durable_root: Path, manifest: dict[str, Any]) -> Path:
    root = manifest.get("spec", {}).get("root", {})
    raw_path = root.get("rootfs")
    if root.get("kind") != "rootfs" or not isinstance(raw_path, str):
        raise TripwireProofFailure("Krun manifest omitted its exact rootfs")
    rootfs = Path(raw_path)
    if not rootfs.is_absolute():
        raise TripwireProofFailure("Krun manifest rootfs is not absolute")
    try:
        durable = durable_root.resolve(strict=True)
        resolved = rootfs.resolve(strict=True)
    except OSError as error:
        raise TripwireProofFailure(f"Krun manifest rootfs is unavailable: {error}") from error
    if resolved == durable or not resolved.is_relative_to(durable):
        raise TripwireProofFailure("Krun manifest rootfs escapes the attempt durability root")
    return resolved


def _read_workload_saga(
    durable_root: Path,
    *,
    tenant_id: str,
    workload_id: str,
) -> dict[str, Any]:
    database = durable_root / "data" / "_nimbus.sqlite3"
    if not database.is_file() or database.is_symlink():
        raise TripwireProofFailure("workload saga database is absent or not a regular file")
    try:
        with sqlite3.connect(
            f"file:{database}?mode=ro",
            uri=True,
            timeout=5,
        ) as connection:
            rows = connection.execute("SELECT data_json FROM documents").fetchall()
    except sqlite3.Error as error:
        raise TripwireProofFailure(f"could not read durable workload saga: {error}") from error
    matches: list[dict[str, Any]] = []
    for (raw_document,) in rows:
        try:
            document = json.loads(raw_document)
        except (TypeError, json.JSONDecodeError):
            continue
        if (
            isinstance(document, dict)
            and isinstance(document.get("sagaId"), str)
            and document.get("tenantId") == tenant_id
            and document.get("workloadId") == workload_id
        ):
            matches.append(document)
    if len(matches) != 1:
        raise TripwireProofFailure(
            f"expected one exact workload saga for {tenant_id}/{workload_id}, "
            f"observed {len(matches)}"
        )
    return matches[0]


def _read_workload_teardown_history(
    durable_root: Path,
    *,
    tenant_id: str,
    workload_id: str,
    saga_id: str,
) -> dict[str, Any]:
    """Read the exact durable teardown prefix from document history."""

    database = durable_root / "data" / "_nimbus.sqlite3"
    if not database.is_file() or database.is_symlink():
        raise TripwireProofFailure("workload saga history database is unavailable")
    try:
        with sqlite3.connect(
            f"file:{database}?mode=ro",
            uri=True,
            timeout=5,
        ) as connection:
            rows = connection.execute(
                "SELECT commit_sequence, data_json "
                "FROM document_versions ORDER BY commit_sequence"
            ).fetchall()
    except sqlite3.Error as error:
        raise TripwireProofFailure(
            f"could not read durable workload saga history: {error}"
        ) from error
    entries: list[dict[str, Any]] = []
    for commit_sequence, raw_document in rows:
        try:
            document = json.loads(raw_document)
        except (TypeError, json.JSONDecodeError):
            continue
        if (
            not isinstance(document, dict)
            or document.get("sagaId") != saga_id
            or document.get("tenantId") != tenant_id
            or document.get("workloadId") != workload_id
        ):
            continue
        detail = document.get("phaseDetail")
        if not isinstance(detail, dict) or detail.get("kind") != "teardown":
            continue
        value = detail.get("value")
        entries.append(
            {
                "commit_sequence": commit_sequence,
                "saga_revision": document.get("sagaRevision"),
                "phase": document.get("phase"),
                "terminal_observations": (
                    value.get("terminalObservations")
                    if isinstance(value, dict)
                    else None
                ),
            }
        )
    return {
        "saga_id": saga_id,
        "tenant_id": tenant_id,
        "workload_id": workload_id,
        "entries": entries,
    }


def _observed_saga_exact(
    saga: dict[str, Any],
    manifest: dict[str, Any],
    *,
    restart_epoch: int,
    automatic_restart_count: int,
) -> bool:
    handle = manifest.get("handle", {})
    detail = saga.get("phaseDetail", {})
    value = detail.get("value", {}) if detail.get("kind") == "provision" else {}
    references = value.get("references", {})
    execution = references.get("execution", {})
    publication = references.get("publication", {})
    observations = value.get("observations", [])
    restart = saga.get("restartState", {})
    desired_generation = saga.get("desiredGeneration")
    plan_generation = (
        manifest.get("provision_network_plan", {})
        .get("network_plan", {})
        .get("generation")
    )
    return (
        saga.get("tenantId") == handle.get("tenant_id")
        and saga.get("workloadId") == handle.get("name")
        and saga.get("desiredState") == "running"
        and saga.get("phase") == "observed"
        and saga.get("provisionDisposition", {}).get("kind") == "ready"
        and [row.get("kind") for row in observations]
        == list(EXPECTED_PROVISION_OBSERVATIONS)
        and execution.get("executionId") == handle.get("id")
        and execution.get("attemptId") == manifest.get("execution_attempt_id")
        and execution.get("restartEpoch") == str(restart_epoch)
        and execution.get("generation") == desired_generation
        and str(plan_generation) == desired_generation
        and publication.get("execution") == execution
        and len(publication.get("endpoints", [])) == 1
        and isinstance(publication.get("endpoints", [None])[0], str)
        and restart.get("currentExecutionAttemptId") == execution.get("attemptId")
        and restart.get("completedRestartEpoch") == str(restart_epoch)
        and restart.get("completedAutomaticRestartCount") == automatic_restart_count
        and restart.get("active") is None
    )


def _wait_observed_saga(
    durable_root: Path,
    manifest: dict[str, Any],
    *,
    restart_epoch: int,
    automatic_restart_count: int,
    timeout_seconds: int,
) -> dict[str, Any]:
    deadline = time.monotonic() + timeout_seconds
    handle = manifest.get("handle", {})
    last = "workload saga unavailable"
    while time.monotonic() < deadline:
        try:
            saga = _read_workload_saga(
                durable_root,
                tenant_id=handle["tenant_id"],
                workload_id=handle["name"],
            )
            if _observed_saga_exact(
                saga,
                manifest,
                restart_epoch=restart_epoch,
                automatic_restart_count=automatic_restart_count,
            ):
                return saga
            active = saga.get("restartState", {}).get("active")
            observations = (
                saga.get("phaseDetail", {})
                .get("value", {})
                .get("observations", [])
            )
            last = (
                f"phase={saga.get('phase')!r} revision={saga.get('revision')!r} "
                f"restart_active_phase={active.get('phase') if isinstance(active, dict) else None!r} "
                f"observations={[row.get('kind') for row in observations]!r}"
            )
        except (KeyError, TripwireProofFailure) as error:
            last = str(error)
        time.sleep(0.2)
    raise TripwireProofFailure(
        "workload saga did not reach exact observed readiness: " + last
    )


def _write_json(path: Path, value: Any) -> None:
    _write_new_text(path, json.dumps(value, indent=2, sort_keys=True) + "\n")


def _select_run_processes(
    processes: list[dict[str, Any]],
    durable_root: Path,
    sandbox_id: str,
) -> list[dict[str, Any]]:
    """Select exact run owners plus their process descendants."""

    durable_needle = str(durable_root)
    selected = {
        row["pid"]
        for row in processes
        if durable_needle in " ".join(row["argv"])
        or sandbox_id in " ".join(row["argv"])
    }
    changed = True
    while changed:
        before = len(selected)
        selected.update(
            row["pid"] for row in processes if row["ppid"] in selected
        )
        changed = len(selected) != before
    return sorted(
        (row for row in processes if row["pid"] in selected),
        key=lambda row: row["pid"],
    )


def _process_snapshot(durable_root: Path, sandbox_id: str) -> list[dict[str, Any]]:
    processes: list[dict[str, Any]] = []
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            raw = (entry / "cmdline").read_bytes()
            status = (entry / "status").read_text(encoding="utf-8")
            comm = (entry / "comm").read_text(encoding="utf-8").strip()
        except OSError:
            continue
        argv = [value.decode("utf-8", errors="replace") for value in raw.split(b"\0") if value]
        ppid = next(
            (
                int(line.split(":", 1)[1].strip())
                for line in status.splitlines()
                if line.startswith("PPid:")
            ),
            None,
        )
        if ppid is None:
            continue
        processes.append(
            {
                "pid": int(entry.name),
                "ppid": ppid,
                "comm": comm,
                "argv": argv,
            }
        )
    return _select_run_processes(processes, durable_root, sandbox_id)


def _argv_option(argv: list[str], option: str) -> str | None:
    try:
        index = argv.index(option)
        return argv[index + 1]
    except (ValueError, IndexError):
        return None


def _krun_process_pair_exact(
    durable_root: Path,
    sandbox_id: str,
    processes: list[dict[str, Any]],
) -> bool:
    conmons = []
    for process in processes:
        argv = process.get("argv", [])
        bundle = _argv_option(argv, "-b")
        same_run = (
            _argv_option(argv, "-c") == sandbox_id
            or _argv_option(argv, "-u") == sandbox_id
            or (
                bundle is not None
                and Path(bundle).is_absolute()
                and Path(bundle).is_relative_to(durable_root)
            )
        )
        conmon_executable = (
            process.get("comm") == "conmon"
            or (argv and Path(argv[0]).name == "conmon")
        )
        if same_run and conmon_executable:
            conmons.append(process)
    if len(conmons) != 1:
        return False
    conmon = conmons[0]
    conmon_argv = conmon.get("argv", [])
    bundle = _argv_option(conmon_argv, "-b")
    if not (
        conmon_argv[:1] == ["/usr/bin/conmon"]
        and _argv_option(conmon_argv, "-c") == sandbox_id
        and _argv_option(conmon_argv, "-u") == sandbox_id
        and _argv_option(conmon_argv, "-r") == "/usr/libexec/nimbus/crun"
        and bundle is not None
        and Path(bundle).is_absolute()
        and Path(bundle).is_relative_to(durable_root)
    ):
        return False
    conmon_pid = conmon["pid"]
    runtime_processes = [
        process
        for process in processes
        if process.get("comm") == "libkrun VM"
        or process.get("argv", [])[:1] == ["[libcrun:krun]"]
    ]
    crossed_runtime = any(
        value == "/usr/bin/crun"
        for process in processes
        for value in process.get("argv", [])
    )
    return (
        len(runtime_processes) == 1
        and runtime_processes[0].get("ppid") == conmon_pid
        and runtime_processes[0].get("comm") == "libkrun VM"
        and runtime_processes[0].get("argv", [])[:1] == ["[libcrun:krun]"]
        and not crossed_runtime
    )


def _provider_exact(manifest: dict[str, Any], processes: list[dict[str, Any]]) -> bool:
    handle = manifest.get("handle", {})
    plan = manifest.get("provision_network_plan", {}).get("network_plan", {})
    sovereignty = plan.get("requirements", {}).get("sovereignty", {})
    network_state_root = manifest.get("network_layout", {}).get("network_state_root")
    durable_root = (
        Path(network_state_root).parent
        if isinstance(network_state_root, str) and Path(network_state_root).is_absolute()
        else None
    )
    return (
        handle.get("backend") == "krun"
        and sovereignty.get("maximum_control_plane_locality") == "local_only"
        and sovereignty.get("allowed_external_dependencies") == []
        and sovereignty.get("offline_restart_required") is True
        and durable_root is not None
        and _krun_process_pair_exact(durable_root, handle.get("id"), processes)
    )


def _trace_addresses(attempt_dir: Path, names: ResourceNames) -> tuple[list[str], list[str]]:
    addresses: set[str] = set()
    invalid_addresses: set[str] = set()
    patterns = (
        re.compile(r'inet_addr\("([^"\\]+)"\)'),
        re.compile(r'inet_pton\(AF_INET6, "([^"\\]+)"'),
    )
    for path in sorted(attempt_dir.glob("*.strace*")):
        if path.name.startswith("control.strace"):
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        for pattern in patterns:
            for raw in pattern.findall(text):
                try:
                    address = ipaddress.ip_address(raw)
                except ValueError:
                    invalid_addresses.add(raw)
                    continue
                addresses.add(str(getattr(address, "ipv4_mapped", None) or address))
    allowed_networks = (
        ipaddress.ip_network("127.0.0.0/8"),
        ipaddress.ip_network("::1/128"),
        PROVIDER_NETWORK,
        ipaddress.ip_network(f"{names.peer_ipv4}/32"),
        ipaddress.ip_network(f"{names.peer_ipv6}/128"),
        ipaddress.ip_network("169.254.0.0/16"),
        ipaddress.ip_network("fe80::/10"),
        ipaddress.ip_network("ff00::/8"),
    )
    forbidden = sorted(invalid_addresses)
    for raw in sorted(addresses):
        address = ipaddress.ip_address(raw)
        if address.is_unspecified or any(address in network for network in allowed_networks):
            continue
        forbidden.append(raw)
    return sorted(addresses), forbidden


def _profile_capture_text(payload: str) -> str:
    """Normalize tcpdump's signal-only trailing newline to empty evidence."""

    return payload.strip()


def _terminal_provider_journals(durable_root: Path) -> bool:
    paths = list(durable_root.rglob(".nimbus-provider-command-attempts/**/*.json"))
    if not paths:
        return False
    for path in paths:
        try:
            kind = json.loads(path.read_text(encoding="utf-8")).get("observation", {}).get("kind")
        except (OSError, json.JSONDecodeError):
            return False
        if kind not in {"succeeded", "absent"}:
            return False
    return True


def _terminal_krun_manifest_evidence(
    path: Path,
    tenant_id: str,
    sandbox_id: str,
) -> dict[str, Any]:
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        return {"path": str(path), "passed": False, "error": str(error)}
    handle = manifest.get("handle", {})
    creator = manifest.get("creator_handoff", {})
    cleanup = manifest.get("provider_failure_cleanup", {})
    network = manifest.get("network_teardown", {})
    stop = manifest.get("execution_teardown", {}).get("stop", {})
    exact = (
        handle.get("tenant_id") == tenant_id
        and handle.get("id") == sandbox_id
        and handle.get("backend") == "krun"
        and handle.get("status") == "stopped"
        and handle.get("published_endpoints") == []
        and manifest.get("status") == "stopped"
        and manifest.get("shutdown_requested") is True
        and manifest.get("launch_authority") == {"phase": "released"}
        and manifest.get("launch_artifact") is None
        and creator.get("phase") == "quiesced"
        and isinstance(creator.get("proof"), dict)
        and cleanup == {"phase": "inactive"}
        and network.get("detachPhase") == "detached"
        and network.get("releasePhase") == "released"
        and stop.get("phase") == "execution_stopped"
    )
    return {
        "path": str(path),
        "tenant_id": handle.get("tenant_id"),
        "sandbox_id": handle.get("id"),
        "status": manifest.get("status"),
        "launch_authority": manifest.get("launch_authority"),
        "launch_artifact_retained": manifest.get("launch_artifact") is not None,
        "creator_phase": creator.get("phase"),
        "provider_failure_cleanup": cleanup,
        "detach_phase": network.get("detachPhase"),
        "release_phase": network.get("releasePhase"),
        "passed": exact,
    }


def _netavark_scaffold_evidence(
    document: dict[str, Any],
    network_config: dict[str, Any],
    *,
    published_port: int,
    guest_port: int,
) -> dict[str, Any]:
    """Prove that Netavark retained only its provider-global nft scaffold."""

    network_interface = network_config.get("network_interface")
    network_subnet_value = network_config.get("network_subnet")
    network_id = network_config.get("network_id")
    try:
        network_subnet = ipaddress.ip_network(network_subnet_value, strict=True)
    except (TypeError, ValueError):
        network_subnet = None

    object_kinds: list[str] = []
    tables: list[dict[str, Any]] = []
    chains: list[dict[str, Any]] = []
    rules: list[dict[str, Any]] = []
    relevant_items: list[dict[str, Any]] = []
    malformed_objects = 0
    for item in document.get("nftables", []):
        if not isinstance(item, dict) or len(item) != 1:
            malformed_objects += 1
            continue
        kind, payload = next(iter(item.items()))
        if kind == "metainfo":
            object_kinds.append(kind)
            continue
        if not isinstance(payload, dict):
            malformed_objects += 1
            continue
        if kind == "table":
            relevant = payload.get("family") == "inet" and payload.get("name") == "netavark"
        else:
            relevant = payload.get("family") == "inet" and payload.get("table") == "netavark"
        if not relevant:
            continue
        object_kinds.append(kind)
        relevant_items.append(item)
        if kind == "table":
            tables.append(payload)
        elif kind == "chain":
            chains.append(payload)
        elif kind == "rule":
            rules.append(payload)

    chain_names = [chain.get("name") for chain in chains]
    rule_counts: dict[str, int] = {}
    for rule in rules:
        chain = rule.get("chain")
        if isinstance(chain, str):
            rule_counts[chain] = rule_counts.get(chain, 0) + 1

    run_owned_markers: list[str] = []

    def inspect(value: Any, path: str) -> None:
        if isinstance(value, dict):
            for key, child in value.items():
                inspect(child, f"{path}.{key}")
            return
        if isinstance(value, list):
            for index, child in enumerate(value):
                inspect(child, f"{path}[{index}]")
            return
        if isinstance(value, str):
            if value.startswith("nv_"):
                run_owned_markers.append(f"{path}=dynamic-chain:{value}")
            if isinstance(network_interface, str) and value == network_interface:
                run_owned_markers.append(f"{path}=network-interface:{value}")
            if isinstance(network_id, str) and network_id and network_id[:8] in value:
                run_owned_markers.append(f"{path}=network-id:{value}")
            try:
                address = ipaddress.ip_address(value)
            except ValueError:
                address = None
            if network_subnet is not None and address is not None and address in network_subnet:
                run_owned_markers.append(f"{path}=network-address:{value}")
            try:
                candidate_subnet = ipaddress.ip_network(value, strict=False)
            except ValueError:
                candidate_subnet = None
            if (
                network_subnet is not None
                and candidate_subnet is not None
                and candidate_subnet.overlaps(network_subnet)
            ):
                run_owned_markers.append(f"{path}=network-subnet:{value}")
            return
        if (
            isinstance(value, int)
            and not isinstance(value, bool)
            and value in {published_port, guest_port}
        ):
            run_owned_markers.append(f"{path}=published-port:{value}")

    inspect({"nftables": relevant_items}, "nftables")
    unexpected_kinds = sorted(
        kind for kind in set(object_kinds) if kind not in {"metainfo", "table", "chain", "rule"}
    )
    exact_table = (
        len(tables) == 1
        and tables[0].get("family") == "inet"
        and tables[0].get("name") == "netavark"
    )
    exact_chains = (
        len(chain_names) == len(NETAVARK_GLOBAL_CHAINS)
        and set(chain_names) == NETAVARK_GLOBAL_CHAINS
        and all(
            chain.get("family") == "inet" and chain.get("table") == "netavark"
            for chain in chains
        )
    )
    exact_rules = (
        rule_counts == NETAVARK_GLOBAL_RULE_COUNTS
        and all(
            rule.get("family") == "inet"
            and rule.get("table") == "netavark"
            and rule.get("chain") in NETAVARK_GLOBAL_CHAINS
            for rule in rules
        )
    )
    passed = (
        isinstance(document.get("nftables"), list)
        and malformed_objects == 0
        and not unexpected_kinds
        and exact_table
        and exact_chains
        and exact_rules
        and not run_owned_markers
    )
    return {
        "table_count": len(tables),
        "chain_names": sorted(name for name in chain_names if isinstance(name, str)),
        "rule_counts": dict(sorted(rule_counts.items())),
        "malformed_objects": malformed_objects,
        "unexpected_object_kinds": unexpected_kinds,
        "run_owned_markers": sorted(set(run_owned_markers)),
        "passed": passed,
    }


def _product_cleanup(
    recorder: CommandRecorder,
    names: ResourceNames,
    durable_root: Path,
    tenant_id: str,
    sandbox_id: str,
) -> dict[str, Any]:
    link = _run(
        recorder,
        ["ip", "-n", names.subject_namespace, "link", "show"],
        critical=True,
    )
    tables = _run(
        recorder,
        _netns_exec(names.subject_namespace, "nft", "-j", "list", "tables"),
        critical=True,
    )
    processes = _process_snapshot(durable_root, sandbox_id)
    manifests = list(durable_root.rglob("manifest.json"))
    terminal_manifest = (
        _terminal_krun_manifest_evidence(manifests[0], tenant_id, sandbox_id)
        if len(manifests) == 1
        else {"passed": False, "observed_count": len(manifests)}
    )
    manifest = (
        json.loads(manifests[0].read_text(encoding="utf-8"))
        if len(manifests) == 1
        else {}
    )
    nft_ruleset = _run(
        recorder,
        _netns_exec(
            names.subject_namespace,
            "nft",
            "-j",
            "list",
            "ruleset",
        ),
        critical=True,
    )
    netavark_scaffold = _netavark_scaffold_evidence(
        json.loads(nft_ruleset.stdout),
        manifest.get("network_config", {}),
        published_port=PUBLISHED_PORT,
        guest_port=8080,
    )
    status_files = list(durable_root.rglob("networks/containers/*/status.json"))
    netns_files = [
        path
        for path in durable_root.rglob("networks/netns/*")
        if path.is_file() or path.is_symlink()
    ]
    nft_document = json.loads(tables.stdout)
    table_names = sorted(
        item["table"]["name"]
        for item in nft_document.get("nftables", [])
        if isinstance(item, dict) and isinstance(item.get("table"), dict)
    )
    passed = (
        "nb-0" not in link.stdout
        and "veth" not in link.stdout
        and table_names == ["netavark", names.nft_table]
        and netavark_scaffold["passed"]
        and not processes
        and terminal_manifest["passed"]
        and not status_files
        and not netns_files
        and _terminal_provider_journals(durable_root)
    )
    return {
        "links": link.stdout,
        "nft_tables": table_names,
        "netavark_scaffold": netavark_scaffold,
        "processes": processes,
        "manifest_count": len(manifests),
        "terminal_manifest": terminal_manifest,
        "status_count": len(status_files),
        "netns_count": len(netns_files),
        "provider_journals_terminal": _terminal_provider_journals(durable_root),
        "passed": passed,
    }


def _run_attempt(
    recorder: CommandRecorder,
    names: ResourceNames,
    output_dir: Path,
    candidate: Path,
    fixture: Path,
    staged_probe: Path,
    attempt_number: int,
    lifecycle_timeout: int,
) -> dict[str, Any]:
    attempt_dir = output_dir / f"attempt-{attempt_number}"
    attempt_dir.mkdir(mode=0o700)
    durable_root = attempt_dir / "durable"
    for child in ("data", "control", "network"):
        (durable_root / child).mkdir(parents=True, mode=0o700)
    old_rules = attempt_dir / "initial-rules.nft"
    lifecycle_rules = attempt_dir / "lifecycle-rules.nft"
    topology = attempt_dir / "topology.txt"
    control_dns_path = attempt_dir / "control-dns.txt"
    profile_dns_path = attempt_dir / "profile-dns.txt"
    control_trace = attempt_dir / "control.strace"
    transitions: list[str] = []
    peer: ManagedProcess | None = None
    capture: ManagedProcess | None = None
    first_owner: BackgroundCommand | None = None
    fresh_owner: BackgroundCommand | None = None
    owned = OwnedResources(namespaces=set(), root_interfaces=set())
    failure: BaseException | None = None
    outer_cleanup: dict[str, Any] = {"passed": False, "not_entered": True}
    result: dict[str, Any] = {
        "attempt": attempt_number,
        "started_at": utc_now(),
        "durable_root": str(durable_root),
        "resources": asdict(names),
        "transitions": transitions,
    }
    try:
        setup = _setup_attempt(recorder, names, old_rules, owned)
        _write_lifecycle_rules(lifecycle_rules, names)
        _run(
            recorder,
            _netns_exec(names.subject_namespace, "nft", "delete", "table", "inet", names.nft_table),
        )
        _run(recorder, _netns_exec(names.subject_namespace, "nft", "-f", str(lifecycle_rules)))
        result["setup"] = setup
        _snapshot_topology(recorder, names, topology)
        result["namespace_view"] = _cgroup_namespace_view(recorder, names)

        config_result = _run(
            recorder,
            _product_argv(
                names,
                attempt_dir / "config.strace",
                candidate,
                durable_root,
                fixture,
                "config",
            ),
            timeout=lifecycle_timeout,
        )
        config_exact = (
            "backend: krun" in config_result.stdout
            and "kind: build" in config_result.stdout
            and "host_address: 127.0.0.1" in config_result.stdout
            and "host_port: 15992" in config_result.stdout
            and "guest_port: 8080" in config_result.stdout
            and "requested: on-failure:1" in config_result.stdout
        )
        if not config_exact:
            raise TripwireProofFailure("Compose config did not select the exact Krun service")
        transitions.append("config-admitted")

        peer = _start_peer(recorder, names, attempt_number)
        capture = _start_capture(recorder, names, attempt_number, "control")
        control_probe = _run_control_probe(
            recorder, names, staged_probe, control_trace
        )
        stopped_capture = capture
        capture_exit = _stop_process(recorder, stopped_capture, signal.SIGINT)
        _mark_latest(recorder, critical=True, accepted_exit_codes=(0, -signal.SIGINT))
        capture = None
        if capture_exit not in {0, -signal.SIGINT}:
            raise TripwireProofFailure(f"control tcpdump exited {capture_exit}")
        control_dns = stopped_capture.stdout_path.read_text(encoding="utf-8")
        _write_new_text(control_dns_path, control_dns)
        control_counters = _counter_snapshot(recorder, names)
        control_trace_text = "\n".join(
            path.read_text(encoding="utf-8", errors="replace")
            for path in sorted(attempt_dir.glob("control.strace*"))
        )
        control_assertions = _assert_control(
            control_probe, control_counters, control_dns, control_trace_text, names
        )
        control_passed = all(control_assertions.values())
        if not control_passed:
            raise TripwireProofFailure("positive controls failed")
        result["control"] = {
            "probe": control_probe,
            "counters": control_counters,
            "assertions": control_assertions,
            "passed": True,
        }
        transitions.append("positive-controls-passed")

        # The negative TCP controls use nonblocking connects. Let their closed
        # sockets leave the kernel retransmission queue before the measured
        # lifecycle receives a fresh ruleset and zeroed counters.
        time.sleep(CONTROL_QUIESCENCE_SECONDS)
        _run(
            recorder,
            _netns_exec(names.subject_namespace, "nft", "delete", "table", "inet", names.nft_table),
        )
        _run(recorder, _netns_exec(names.subject_namespace, "nft", "-f", str(lifecycle_rules)))
        reset_counters = _counter_snapshot(recorder, names)
        reset_passed = all(value == 0 for value in reset_counters.values())
        if not reset_passed:
            raise TripwireProofFailure(f"counter reset failed: {reset_counters}")
        result["reset"] = {"counters": reset_counters, "passed": True}
        transitions.append("counter-reset")

        capture = _start_capture(recorder, names, attempt_number, "lifecycle")
        first_owner = _spawn(
            recorder,
            _product_argv(
                names,
                attempt_dir / "first-owner.strace",
                candidate,
                durable_root,
                fixture,
                "up",
            ),
            stdout_path=attempt_dir / "first-owner.stdout",
            stderr_path=attempt_dir / "first-owner.stderr",
        )
        transitions.append("first-owner-started")
        _wait_for_text(
            first_owner.stdout_path, "status ready", lifecycle_timeout
        )
        first_manifest_path, first_manifest = _wait_manifest(
            durable_root, prior_attempt=None, timeout_seconds=lifecycle_timeout
        )
        first_saga = _wait_observed_saga(
            durable_root,
            first_manifest,
            restart_epoch=0,
            automatic_restart_count=0,
            timeout_seconds=lifecycle_timeout,
        )
        _write_json(attempt_dir / "first-saga.json", first_saga)
        transitions.append("first-ready")
        first_inspect = _run(
            recorder,
            _product_argv(
                names,
                attempt_dir / "first-inspect.strace",
                candidate,
                durable_root,
                fixture,
                "inspect",
                "api",
                "--format",
                "json",
            ),
            timeout=lifecycle_timeout,
        )
        first_inspect_document = json.loads(first_inspect.stdout)
        transitions.append("first-inspected")
        first_processes = _process_snapshot(
            durable_root, first_manifest["handle"]["id"]
        )
        _write_json(attempt_dir / "first-processes.json", first_processes)
        first_http = _json_stdout(
            _run(
                recorder,
                _probe_argv(
                    names,
                    attempt_dir / "first-http.strace",
                    "wait-http",
                    "--port",
                    str(PUBLISHED_PORT),
                    "--path",
                    "/nnc92-state",
                    "--expect-body",
                    "first-ready",
                    "--timeout-seconds",
                    str(lifecycle_timeout),
                ),
                timeout=lifecycle_timeout,
            ),
            "first HTTP",
        )
        transitions.append("first-served")
        exit_trigger = _manifest_rootfs(durable_root, first_manifest) / "nnc92-exit-now"
        if exit_trigger.exists():
            raise TripwireProofFailure("first-attempt exit trigger already exists")
        _run(recorder, ["touch", "--", str(exit_trigger)], critical=True)
        transitions.append("first-exit-triggered")
        restart_manifest_path, restart_manifest = _wait_restart_manifest(
            durable_root,
            prior_attempt=first_manifest["execution_attempt_id"],
            sandbox_id=first_manifest["handle"]["id"],
            timeout_seconds=lifecycle_timeout,
        )
        transitions.append("restart-observed")
        second_http = _json_stdout(
            _run(
                recorder,
                _probe_argv(
                    names,
                    attempt_dir / "second-http.strace",
                    "wait-http",
                    "--port",
                    str(PUBLISHED_PORT),
                    "--path",
                    "/nnc92-state",
                    "--expect-body",
                    "restarted-ready",
                    "--timeout-seconds",
                    str(lifecycle_timeout),
                ),
                timeout=lifecycle_timeout,
            ),
            "second HTTP",
        )
        transitions.append("second-served")
        restart_saga = _wait_observed_saga(
            durable_root,
            restart_manifest,
            restart_epoch=1,
            automatic_restart_count=1,
            timeout_seconds=lifecycle_timeout,
        )
        _write_json(attempt_dir / "restart-saga.json", restart_saga)
        restart_inspect_result = _run(
            recorder,
            _product_argv(
                names,
                attempt_dir / "restart-inspect.strace",
                candidate,
                durable_root,
                fixture,
                "inspect",
                "api",
                "--format",
                "json",
            ),
            timeout=lifecycle_timeout,
        )
        restart_inspect = json.loads(restart_inspect_result.stdout)
        transitions.append("restart-inspected")
        _write_json(attempt_dir / "first-manifest.json", first_manifest)
        _write_json(attempt_dir / "restart-manifest.json", restart_manifest)

        _finish_background(
            recorder,
            first_owner,
            stop_signal=signal.SIGINT,
            timeout_seconds=30,
            accepted_exit_codes=(0, 130, -signal.SIGINT),
        )
        first_owner = None
        transitions.append("first-owner-stopped")

        fresh_owner = _spawn(
            recorder,
            _product_argv(
                names,
                attempt_dir / "fresh-owner.strace",
                candidate,
                durable_root,
                fixture,
                "up",
            ),
            stdout_path=attempt_dir / "fresh-owner.stdout",
            stderr_path=attempt_dir / "fresh-owner.stderr",
        )
        transitions.append("fresh-owner-started")
        fresh_http = _json_stdout(
            _run(
                recorder,
                _probe_argv(
                    names,
                    attempt_dir / "fresh-http.strace",
                    "wait-http",
                    "--port",
                    str(PUBLISHED_PORT),
                    "--path",
                    "/nnc92-state",
                    "--expect-body",
                    "restarted-ready",
                    "--timeout-seconds",
                    str(lifecycle_timeout),
                ),
                timeout=lifecycle_timeout,
            ),
            "fresh-owner HTTP",
        )
        _wait_for_text(
            fresh_owner.stdout_path, "already_running", min(30, lifecycle_timeout)
        )
        fresh_manifest = json.loads(_find_manifest(durable_root).read_text(encoding="utf-8"))
        _write_json(attempt_dir / "fresh-manifest.json", fresh_manifest)
        fresh_processes = _process_snapshot(
            durable_root, fresh_manifest["handle"]["id"]
        )
        _write_json(attempt_dir / "fresh-processes.json", fresh_processes)
        fresh_owner_exact = (
            fresh_manifest.get("execution_attempt_id")
            == restart_manifest.get("execution_attempt_id")
            and _krun_process_pair_exact(
                durable_root,
                fresh_manifest["handle"]["id"],
                fresh_processes,
            )
        )
        if not fresh_owner_exact:
            raise TripwireProofFailure("fresh owner duplicated or crossed the running attempt")
        transitions.append("fresh-owner-reconciled")
        _finish_background(
            recorder,
            fresh_owner,
            stop_signal=signal.SIGINT,
            timeout_seconds=30,
            accepted_exit_codes=(0, 130, -signal.SIGINT),
        )
        fresh_owner = None
        transitions.append("fresh-owner-stopped")

        transitions.append("withdrawal-started")
        down = _run(
            recorder,
            _product_argv(
                names,
                attempt_dir / "retirement.strace",
                candidate,
                durable_root,
                fixture,
                "down",
            ),
            timeout=lifecycle_timeout,
        )
        if "Compose down completed" not in down.stdout:
            raise TripwireProofFailure("Compose down omitted terminal recorded evidence")
        terminal_saga_history = _read_workload_teardown_history(
            durable_root,
            tenant_id=first_manifest["handle"]["tenant_id"],
            workload_id=first_manifest["handle"]["name"],
            saga_id=first_saga["sagaId"],
        )
        if not teardown_order_exact(terminal_saga_history, restart_saga):
            raise TripwireProofFailure(
                "durable teardown did not withdraw publication before execution stop"
            )
        _write_json(attempt_dir / "terminal-saga-history.json", terminal_saga_history)
        transitions.append("retirement-recorded")
        retired_http = _json_stdout(
            _run(
                recorder,
                _probe_argv(
                    names,
                    attempt_dir / "retired-http.strace",
                    "expect-unreachable",
                    "--port",
                    str(PUBLISHED_PORT),
                    "--timeout-seconds",
                    "3",
                ),
                timeout=10,
            ),
            "retired HTTP",
        )
        ps_result = _run(
            recorder,
            _product_argv(
                names,
                attempt_dir / "post-retirement-ps.strace",
                candidate,
                durable_root,
                fixture,
                "ps",
                "--format",
                "json",
            ),
            timeout=lifecycle_timeout,
        )
        terminal_projection_exact = _terminal_compose_projection_exact(
            json.loads(ps_result.stdout),
            tenant_id=first_manifest["handle"]["tenant_id"],
            sandbox_id=first_manifest["handle"]["id"],
            service_name=first_manifest["handle"]["name"],
        )
        if not terminal_projection_exact:
            raise TripwireProofFailure(
                "Compose projection did not retain exactly one terminal observation"
            )
        transitions.append("retired-endpoint-unreachable")
        product_cleanup = _product_cleanup(
            recorder,
            names,
            durable_root,
            first_manifest["handle"]["tenant_id"],
            first_manifest["handle"]["id"],
        )
        if not product_cleanup["passed"]:
            raise TripwireProofFailure(f"product cleanup is incomplete: {product_cleanup}")
        transitions.append("cleanup-proven")

        stopped_capture = capture
        capture_exit = _stop_process(recorder, stopped_capture, signal.SIGINT)
        _mark_latest(recorder, critical=True, accepted_exit_codes=(0, -signal.SIGINT))
        capture = None
        if capture_exit not in {0, -signal.SIGINT}:
            raise TripwireProofFailure(f"lifecycle tcpdump exited {capture_exit}")
        profile_dns = _profile_capture_text(
            stopped_capture.stdout_path.read_text(encoding="utf-8")
        )
        _write_new_text(profile_dns_path, profile_dns)
        profile_counters = _counter_snapshot(recorder, names)
        trace_addresses, forbidden_addresses = _trace_addresses(attempt_dir, names)

        result["lifecycle"] = {
            "config_exact": config_exact,
            "provider_exact": _provider_exact(first_manifest, first_processes),
            "readiness_exact": _observed_saga_exact(
                first_saga,
                first_manifest,
                restart_epoch=0,
                automatic_restart_count=0,
            ),
            "first_http": first_http,
            "first_inspect": first_inspect_document,
            "first_saga": first_saga,
            "second_http": second_http,
            "restart_inspect": restart_inspect,
            "restart_saga": restart_saga,
            "fresh_http": fresh_http,
            "retired_http": retired_http,
            "first_manifest": first_manifest,
            "restart_manifest": restart_manifest,
            "fresh_manifest": fresh_manifest,
            "restart_count": 1,
            "fresh_owner_exact": fresh_owner_exact,
            "retirement_exit": down.returncode,
            "terminal_saga_history": terminal_saga_history,
            "terminal_projection_exact": terminal_projection_exact,
            "product_cleanup": product_cleanup,
        }
        result["profile"] = {
            "counters": profile_counters,
            "dns_capture": profile_dns,
            "trace_addresses": trace_addresses,
            "forbidden_trace_addresses": forbidden_addresses,
        }
        if not _provider_exact(first_manifest, first_processes):
            raise TripwireProofFailure("provider selection evidence is not exact")
        if profile_dns or any(profile_counters.values()) or forbidden_addresses:
            raise TripwireProofFailure(
                "offline lifecycle emitted network evidence: "
                f"dns={bool(profile_dns)} counters={profile_counters} "
                f"forbidden={forbidden_addresses}"
            )
        if transitions != list(REQUIRED_TRANSITIONS):
            raise TripwireProofFailure(f"lifecycle transitions are incomplete: {transitions}")
    except BaseException as error:
        failure = error
    finally:
        for command in (fresh_owner, first_owner):
            if command is None:
                continue
            try:
                _finish_background(
                    recorder,
                    command,
                    stop_signal=signal.SIGKILL,
                    timeout_seconds=10,
                    accepted_exit_codes=(-signal.SIGKILL,),
                )
            except BaseException as cleanup_error:
                if failure is None:
                    failure = cleanup_error
        with _defer_termination_signals() as deferred_signals:
            outer_cleanup = _cleanup_attempt(
                recorder, names, owned, peer=peer, capture=capture
            )
        outer_cleanup["deferred_signals"] = deferred_signals
        result["cleanup"] = {
            "outer": outer_cleanup,
            "passed": (
                outer_cleanup.get("passed") is True
                and result.get("lifecycle", {}).get("product_cleanup", {}).get("passed") is True
            ),
        }
        result["finished_at"] = utc_now()
        _write_json(attempt_dir / "attempt.json", result)
    if not outer_cleanup.get("passed"):
        raise TripwireProofFailure(f"outer isolation cleanup is incomplete: {outer_cleanup}")
    if failure is not None:
        raise failure
    return result


def _empty_document(
    config: LifecycleConfig,
    *,
    status: str,
    exit_code: int,
    reason: str,
    phase: str,
    started_at: str,
) -> dict[str, Any]:
    return {
        "schema_version": SCHEMA_VERSION,
        "result": {
            "status": status,
            "exit_code": exit_code,
            "reason": reason,
            "phase": phase,
            "started_at": started_at,
            "finished_at": utc_now(),
        },
        "runner": {},
        "source": {},
        "inputs": {
            "runner_id": config.runner_id,
            "repeat": config.repeat,
            "offline_after_admission": True,
        },
        "attempts": [],
        "commands": [],
        "self_tests": {},
        "assertions": [],
        "artifacts": [],
    }


def run_live(config: LifecycleConfig) -> int:
    started_at = utc_now()
    _prepare_output_directory(config.output_dir)
    evidence_path = config.output_dir / "evidence.json"
    document = _empty_document(
        config,
        status="FAIL",
        exit_code=FAIL_EXIT,
        reason="preflight did not complete",
        phase="preflight",
        started_at=started_at,
    )
    try:
        tripwire_config = TripwireConfig(
            runner_id=config.runner_id,
            expected_hostname=config.expected_hostname,
            host_class="kvm",
            provider_kind="kvm",
            output_dir=config.output_dir,
            repeat=config.repeat,
            command_timeout_seconds=config.command_timeout_seconds,
        )
        facts, runner = collect_preflight(tripwire_config)
        decision, reason = preflight_decision(facts)
        if decision == "SKIPPED":
            document["result"].update(
                status="SKIPPED",
                exit_code=SKIPPED_EXIT,
                reason=reason,
                phase="preflight",
                finished_at=utc_now(),
            )
            document["runner"] = runner
            atomic_write_json(evidence_path, document)
            return SKIPPED_EXIT
        if decision != "ADMITTED":
            raise TripwireProofFailure(reason or "runner admission failed")
        missing_lifecycle_tools = [
            name
            for name, path in LIFECYCLE_TOOL_PATHS.items()
            if not _trusted_regular(path, executable=True)
        ]
        runner["lifecycle_tools"] = {
            name: {
                "path": str(path),
                "version": _tool_version(path),
                "sha256": sha256_file(path),
            }
            for name, path in LIFECYCLE_TOOL_PATHS.items()
            if name not in missing_lifecycle_tools
        }
        runner["missing_tools"] = missing_lifecycle_tools
        runner["admitted"] = not missing_lifecycle_tools
        if missing_lifecycle_tools:
            raise TripwireProofFailure(
                "required lifecycle tools are unavailable: " + ", ".join(missing_lifecycle_tools)
            )
        git_path = runner["tools"]["git"]["path"]
        source = _source_facts(git_path)
        source["git_path"] = git_path
        candidate = _stage_candidate(config)
        staged_probe = _stage_control_probe()
        fixture, fixture_manifest = _stage_fixture(config)
        tool_paths = {
            name: record["path"] for name, record in runner["tools"].items()
        }
        tool_paths.update(
            {name: str(path) for name, path in LIFECYCLE_TOOL_PATHS.items()}
        )
        recorder = CommandRecorder(
            config.output_dir, config.command_timeout_seconds, tool_paths
        )
        fixture_smoke = _run_fixture_smoke(recorder, fixture)
        self_tests = _run_self_tests(config, recorder, source)
        source.pop("git_path", None)
        names = _resource_names(config.runner_id)
        attempts: list[dict[str, Any]] = []
        lock = Path("/run/lock/nimbus-network-sovereignty-tripwire.lock")
        with _exclusive_host_lock(lock):
            for attempt_number in range(1, config.repeat + 1):
                attempts.append(
                    _run_attempt(
                        recorder,
                        names,
                        config.output_dir,
                        candidate,
                        fixture,
                        staged_probe,
                        attempt_number,
                        config.lifecycle_timeout_seconds,
                    )
                )
        fixture_by_path = {row["path"]: row for row in fixture_manifest}
        document = {
            "schema_version": SCHEMA_VERSION,
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
            "inputs": {
                "runner_id": config.runner_id,
                "expected_hostname": config.expected_hostname,
                "repeat": config.repeat,
                "offline_after_admission": True,
                "nimbus_path": str(candidate),
                "nimbus_size": candidate.stat().st_size,
                "nimbus_sha256": sha256_file(candidate),
                "fixture_root": str(fixture),
                "busybox_sha256": fixture_by_path["rootfs/bin/busybox"]["sha256"],
                "compose_sha256": fixture_by_path["compose.yaml"]["sha256"],
                "dockerfile_sha256": fixture_by_path["Dockerfile"]["sha256"],
                "lifecycle_sha256": fixture_by_path["lifecycle.sh"]["sha256"],
                "fixture_files": fixture_manifest,
                "fixture_executable": fixture_smoke["passed"],
                "private_provider_networks": [str(PROVIDER_NETWORK)],
                "published_endpoint": f"127.0.0.1:{PUBLISHED_PORT}",
            },
            "attempts": attempts,
            "commands": recorder.records,
            "self_tests": self_tests,
            "assertions": [],
            "artifacts": [],
        }
        document["assertions"] = derive_assertions(document)
        document["artifacts"] = artifact_manifest(
            config.output_dir,
            _recursive_files(config.output_dir),
            exclude=[evidence_path],
        )
        validate_evidence(document, evidence_root=config.output_dir)
        atomic_write_json(evidence_path, document)
        return 0
    except BaseException as error:
        document["result"] = {
            "status": "FAIL",
            "exit_code": FAIL_EXIT,
            "reason": f"{type(error).__name__}: {error}",
            "phase": "lifecycle",
            "started_at": started_at,
            "finished_at": utc_now(),
        }
        if "recorder" in locals():
            document["commands"] = recorder.records
        if "runner" in locals():
            document["runner"] = runner
        if "source" in locals():
            document["source"] = source
        if "attempts" in locals():
            document["attempts"] = attempts
        document["artifacts"] = _failure_artifact_manifest(
            config.output_dir, evidence_path
        )
        try:
            validate_evidence(document, evidence_root=config.output_dir)
        except LifecycleEvidenceError:
            pass
        atomic_write_json(evidence_path, document)
        return FAIL_EXIT


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    if args.repeat != 2:
        parser.error("--repeat must be exactly 2")
    if not 5 <= args.command_timeout_seconds <= 300:
        parser.error("--command-timeout-seconds must be between 5 and 300")
    if not 30 <= args.lifecycle_timeout_seconds <= 600:
        parser.error("--lifecycle-timeout-seconds must be between 30 and 600")
    config = LifecycleConfig(
        runner_id=args.runner_id,
        expected_hostname=args.expected_hostname,
        output_dir=args.output_dir,
        nimbus_bin=args.nimbus_bin,
        expected_nimbus_sha256=args.expected_nimbus_sha256,
        fixture_dir=args.fixture_dir,
        repeat=args.repeat,
        command_timeout_seconds=args.command_timeout_seconds,
        lifecycle_timeout_seconds=args.lifecycle_timeout_seconds,
    )
    return run_live(config)


if __name__ == "__main__":
    raise SystemExit(main())
