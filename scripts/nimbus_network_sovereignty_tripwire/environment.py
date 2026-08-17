#!/usr/bin/env python3
"""Fail-closed runner and source admission for the NNC4.7 tripwire."""

from __future__ import annotations

from dataclasses import dataclass, replace
import os
from pathlib import Path
import platform
import re
import stat
import subprocess
from typing import Any

from .evidence import REQUIRED_HARNESS_PATHS
from .integrity import harness_source_digest, harness_source_manifest

SKIPPED_EXIT = 77
CONFIG_EXIT = 64
FAIL_EXIT = 1
SAFE_RUNNER_ID = re.compile(r"^[a-z][a-z0-9-]{2,62}$")
REQUIRED_TOOLS = (
    "git",
    "hostname",
    "ip",
    "nft",
    "python3",
    "setpriv",
    "strace",
    "sysctl",
    "tcpdump",
    "uname",
)
REQUIRED_RUNNER_CAPABILITIES = {
    "CAP_KILL": 5,
    "CAP_SETGID": 6,
    "CAP_SETUID": 7,
    "CAP_SETPCAP": 8,
    "CAP_NET_ADMIN": 12,
    "CAP_NET_RAW": 13,
    "CAP_SYS_ADMIN": 21,
}
TRUSTED_TOOL_DIRECTORIES = (
    Path("/usr/local/sbin"),
    Path("/usr/local/bin"),
    Path("/usr/sbin"),
    Path("/usr/bin"),
    Path("/sbin"),
    Path("/bin"),
)


class TripwireError(RuntimeError):
    exit_code = FAIL_EXIT


class TripwireSkipped(TripwireError):
    exit_code = SKIPPED_EXIT


class TripwireConfigurationError(TripwireError):
    exit_code = CONFIG_EXIT


class TripwireProofFailure(TripwireError):
    exit_code = FAIL_EXIT


class TripwireInterrupted(TripwireProofFailure):
    """Raised by a terminating signal so attempt cleanup still runs."""


@dataclass(frozen=True)
class PreflightFacts:
    system: str
    uid: int
    observed_hostname: str
    expected_hostname: str
    host_class: str
    provider_kind: str
    kvm_access: bool
    missing_tools: tuple[str, ...]
    unavailable_tool_versions: tuple[str, ...]
    effective_capabilities: int
    missing_capabilities: tuple[str, ...]
    kernel_release: str
    pid1_command: str


@dataclass(frozen=True)
class TripwireConfig:
    runner_id: str
    expected_hostname: str
    host_class: str
    provider_kind: str
    output_dir: Path
    repeat: int
    command_timeout_seconds: int


def preflight_decision(facts: PreflightFacts) -> tuple[str, str | None]:
    """Pure fail-closed environment classifier used by live and fake tests."""

    if facts.observed_hostname != facts.expected_hostname:
        return (
            "FAIL",
            "runner hostname mismatch: "
            f"expected {facts.expected_hostname!r}, observed {facts.observed_hostname!r}",
        )
    if facts.host_class == "kvm" and facts.provider_kind != "kvm":
        return "FAIL", "KVM host class requires provider kind kvm"
    if facts.host_class == "minicloud" and facts.provider_kind != "linuxkit":
        return "FAIL", "minicloud host class requires provider kind linuxkit"
    if facts.system != "Linux":
        return "SKIPPED", f"unsupported operating system: {facts.system}"
    if facts.uid != 0:
        return "SKIPPED", f"privileged uid 0 is required; observed uid {facts.uid}"
    if facts.missing_capabilities:
        return "SKIPPED", "missing required effective capabilities: " + ", ".join(
            facts.missing_capabilities
        )
    if facts.host_class == "kvm" and not facts.kvm_access:
        return "SKIPPED", "KVM-class runner lacks readable/writable /dev/kvm"
    if facts.host_class == "minicloud":
        if "linuxkit" not in facts.kernel_release.lower():
            return (
                "SKIPPED",
                "LinuxKit minicloud substrate is unavailable: "
                f"kernel release {facts.kernel_release!r}",
            )
        if facts.pid1_command != "/initd":
            return (
                "SKIPPED",
                "LinuxKit host PID namespace is unavailable: "
                f"PID 1 command {facts.pid1_command!r}",
            )
    if facts.missing_tools:
        return "SKIPPED", "missing preinstalled tools: " + ", ".join(
            facts.missing_tools
        )
    if facts.unavailable_tool_versions:
        return "SKIPPED", "tool version evidence unavailable: " + ", ".join(
            facts.unavailable_tool_versions
        )
    return "ADMITTED", None


def repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def harness_paths() -> tuple[Path, ...]:
    return tuple(repo_root() / relative for relative in sorted(REQUIRED_HARNESS_PATHS))


def _git_output(git_path: str, *args: str) -> str:
    try:
        result = subprocess.run(
            [git_path, "-c", f"safe.directory={repo_root()}", *args],
            cwd=repo_root(),
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=60,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        return f"unavailable: {type(error).__name__}: {error}"
    if result.returncode != 0:
        return f"unavailable: {result.stderr.strip()}"
    return result.stdout.strip()


def source_facts(git_path: str) -> dict[str, Any]:
    paths = harness_paths()
    missing = [str(path) for path in paths if not path.is_file()]
    if missing:
        raise TripwireConfigurationError(
            "harness source is incomplete: " + ", ".join(missing)
        )
    relative_paths = [
        path.relative_to(repo_root()).as_posix() for path in sorted(paths)
    ]
    return {
        "commit": _git_output(git_path, "rev-parse", "HEAD"),
        "tree": _git_output(git_path, "rev-parse", "HEAD^{tree}"),
        "dirty": _git_output(git_path, "status", "--short"),
        "harness_sha256": harness_source_digest(repo_root(), relative_paths),
        "harness_paths": relative_paths,
        "harness_files": harness_source_manifest(repo_root(), relative_paths),
    }


def source_failure(source: dict[str, Any]) -> str | None:
    for key, pattern in (
        ("commit", r"[0-9a-f]{40}"),
        ("tree", r"[0-9a-f]{40}"),
        ("harness_sha256", r"[0-9a-f]{64}"),
    ):
        value = source.get(key)
        if not isinstance(value, str) or re.fullmatch(pattern, value) is None:
            return f"source {key} is unavailable or malformed: {value!r}"
    dirty = source.get("dirty")
    if not isinstance(dirty, str) or dirty.startswith("unavailable:"):
        return f"source dirty state is unavailable: {dirty!r}"
    paths = source.get("harness_paths")
    if (
        not isinstance(paths, list)
        or set(paths) != REQUIRED_HARNESS_PATHS
        or len(paths) != len(REQUIRED_HARNESS_PATHS)
    ):
        return "source harness path set is unavailable or incomplete"
    return None


def _tool_version(path: str) -> str:
    candidates = ([path, "--version"], [path, "-V"], [path, "-Version"])
    for argv in candidates:
        try:
            result = subprocess.run(
                argv,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                timeout=5,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired):
            continue
        output = result.stdout.strip()
        if result.returncode == 0 and output:
            return output.splitlines()[0]
    return "version unavailable"


def _trusted_tool_path(name: str) -> str | None:
    for directory in TRUSTED_TOOL_DIRECTORIES:
        candidate = directory / name
        if not candidate.exists():
            continue
        try:
            candidate_metadata = candidate.lstat()
            resolved = candidate.resolve(strict=True)
            metadata = resolved.stat()
        except OSError:
            return None
        if (
            not resolved.is_absolute()
            or not stat.S_ISREG(metadata.st_mode)
            or not os.access(resolved, os.X_OK)
            or (
                resolved.name != name
                and not (name == "python3" and resolved.name.startswith("python3."))
            )
        ):
            return None
        if os.geteuid() == 0:
            for checked in (candidate, resolved):
                current = checked.parent
                while True:
                    try:
                        parent_metadata = current.stat()
                    except OSError:
                        return None
                    if parent_metadata.st_uid != 0 or parent_metadata.st_mode & (
                        stat.S_IWGRP | stat.S_IWOTH
                    ):
                        return None
                    if current == current.parent:
                        break
                    current = current.parent
            if (
                candidate_metadata.st_uid != 0
                or (
                    not stat.S_ISLNK(candidate_metadata.st_mode)
                    and candidate_metadata.st_mode & (stat.S_IWGRP | stat.S_IWOTH)
                )
                or metadata.st_uid != 0
                or metadata.st_mode & (stat.S_IWGRP | stat.S_IWOTH)
            ):
                return None
        return str(resolved)
    return None


def _kvm_access() -> bool:
    try:
        descriptor = os.open("/dev/kvm", os.O_RDWR)
    except OSError:
        return False
    os.close(descriptor)
    return True


def _read_cap_eff() -> int:
    try:
        lines = Path("/proc/self/status").read_text(encoding="utf-8").splitlines()
    except OSError:
        return 0
    for line in lines:
        if line.startswith("CapEff:"):
            return int(line.split()[1], 16)
    return 0


def _pid1_command() -> str:
    try:
        payload = Path("/proc/1/cmdline").read_bytes()
    except OSError:
        return "unavailable"
    values = [value.decode("utf-8", errors="replace") for value in payload.split(b"\0")]
    return values[0] if values and values[0] else "unavailable"


def _process_start_ticks() -> int:
    try:
        payload = Path("/proc/self/stat").read_text(encoding="utf-8")
        fields = payload[payload.rfind(")") + 2 :].split()
        return int(fields[19])
    except (OSError, ValueError, IndexError):
        return -1


def _read_optional_text(path: Path) -> str:
    try:
        return path.read_text(encoding="utf-8").strip()
    except OSError as error:
        return f"unavailable: {type(error).__name__}: {error}"


def collect_preflight(config: TripwireConfig) -> tuple[PreflightFacts, dict[str, Any]]:
    cap_eff = _read_cap_eff()
    missing_capabilities = tuple(
        name
        for name, bit in REQUIRED_RUNNER_CAPABILITIES.items()
        if not cap_eff & (1 << bit)
    )
    kernel_release = platform.release()
    pid1_command = _pid1_command()
    facts = PreflightFacts(
        system=platform.system(),
        uid=os.geteuid(),
        observed_hostname=platform.node(),
        expected_hostname=config.expected_hostname,
        host_class=config.host_class,
        provider_kind=config.provider_kind,
        kvm_access=_kvm_access(),
        missing_tools=(),
        unavailable_tool_versions=(),
        effective_capabilities=cap_eff,
        missing_capabilities=missing_capabilities,
        kernel_release=kernel_release,
        pid1_command=pid1_command,
    )
    detail: dict[str, Any] = {
        "asserted_id": config.runner_id,
        "expected_hostname": config.expected_hostname,
        "observed_hostname": facts.observed_hostname,
        "host_class": config.host_class,
        "provider_kind": config.provider_kind,
        "system": facts.system,
        "os_release": " ".join(
            (
                facts.system,
                kernel_release,
                platform.version(),
                platform.machine(),
            )
        ),
        "kernel": kernel_release,
        "architecture": platform.machine(),
        "uid": facts.uid,
        "process_id": os.getpid(),
        "process_start_ticks": _process_start_ticks(),
        "effective_capabilities": f"{cap_eff:016x}",
        "required_capabilities": sorted(REQUIRED_RUNNER_CAPABILITIES),
        "missing_capabilities": list(missing_capabilities),
        "kvm_access": facts.kvm_access,
        "pid1_command": pid1_command,
        "boot_id": _read_optional_text(Path("/proc/sys/kernel/random/boot_id")),
        "tools": {},
    }
    if preflight_decision(facts)[0] != "ADMITTED":
        return facts, detail

    tools = {tool: _trusted_tool_path(tool) for tool in REQUIRED_TOOLS}
    missing = tuple(sorted(tool for tool, path in tools.items() if path is None))
    versions = {
        name: _tool_version(path) if path else None
        for name, path in sorted(tools.items())
    }
    unavailable_versions = tuple(
        sorted(
            name
            for name, version in versions.items()
            if version == "version unavailable"
        )
    )
    facts = replace(
        facts,
        missing_tools=missing,
        unavailable_tool_versions=unavailable_versions,
    )
    detail["tools"] = {
        name: {
            "path": path,
            "version": versions[name],
        }
        for name, path in sorted(tools.items())
    }
    return facts, detail


def minimal_runner(config: TripwireConfig) -> dict[str, Any]:
    return {
        "asserted_id": config.runner_id,
        "expected_hostname": config.expected_hostname,
        "observed_hostname": platform.node(),
        "host_class": config.host_class,
        "provider_kind": config.provider_kind,
        "system": platform.system(),
        "uid": os.geteuid(),
        "process_id": os.getpid(),
        "process_start_ticks": _process_start_ticks(),
        "kvm_access": False,
    }
