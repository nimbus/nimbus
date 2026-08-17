#!/usr/bin/env python3
"""Versioned evidence contract for the NNC4.7 sovereignty tripwire."""

from __future__ import annotations

from datetime import datetime
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
from typing import Any, Iterable

from .integrity import harness_source_digest, harness_source_manifest, sha256_file

EVIDENCE_SCHEMA_VERSION = 1
REQUIRED_HARNESS_PATHS = frozenset(
    {
        "scripts/nimbus-network-sovereignty-tripwire.sh",
        "scripts/nimbus_network_sovereignty_tripwire/__init__.py",
        "scripts/nimbus_network_sovereignty_tripwire/__main__.py",
        "scripts/nimbus_network_sovereignty_tripwire/environment.py",
        "scripts/nimbus_network_sovereignty_tripwire/evidence.py",
        "scripts/nimbus_network_sovereignty_tripwire/integrity.py",
        "scripts/nimbus_network_sovereignty_tripwire/isolation.py",
        "scripts/nimbus_network_sovereignty_tripwire/probe.py",
        "scripts/nimbus_network_sovereignty_tripwire/runner.py",
        "scripts/nimbus_network_sovereignty_tripwire/synchronization.py",
        "scripts/nimbus_network_sovereignty_tripwire/workspace.py",
    }
)
REQUIRED_RUNNER_TOOLS = frozenset(
    {
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
    }
)
REQUIRED_RUNNER_CAPABILITY_BITS = (5, 6, 7, 8, 12, 13, 21)
REQUIRED_CONTROL_ASSERTIONS = frozenset(
    {
        "subject_unprivileged",
        "loopback",
        "private_ipv4",
        "private_ipv6",
        "unenumerated_private_denied",
        "dns_udp",
        "dns_tcp",
        "public_ipv4_denied",
        "public_ipv6_denied",
        "network_trace",
        "no_unclassified_control",
    }
)
REQUIRED_PROFILE_ASSERTIONS = frozenset(
    {
        "subject_unprivileged",
        "private_only",
        "zero_unexpected",
        "zero_dns_capture",
        "network_trace_private_only",
    }
)
REQUIRED_PROBE_SECURITY_FIELDS = frozenset(
    {
        "capamb",
        "capbnd",
        "capeff",
        "capinh",
        "capprm",
        "uids",
        "gids",
        "no_new_privs",
    }
)

REQUIRED_PASS_ASSERTIONS = frozenset(
    {
        "preflight.named_runner",
        "preflight.offline_inputs",
        "isolation.outer_namespaces",
        "isolation.subject_unprivileged",
        "isolation.peer_forwarding_disabled",
        "control.loopback",
        "control.private_ipv4",
        "control.private_ipv6",
        "control.unenumerated_private_denied",
        "control.dns_udp",
        "control.dns_tcp",
        "control.public_ipv4_denied",
        "control.public_ipv6_denied",
        "control.network_trace",
        "reset.zero_baseline",
        "profile.private_only",
        "profile.zero_unexpected",
        "cleanup.absent",
        "cleanup.same_identity_reentry",
        "evidence.artifacts_authenticated",
    }
)

_STATUS_EXITS = {"PASS": 0, "SKIPPED": 77}
_SAFE_ID = re.compile(r"^[a-z][a-z0-9-]{2,62}$")
_FORBIDDEN_EFFECT_WORDS = frozenset(
    {
        "apt",
        "apt-get",
        "brew",
        "curl",
        "dnf",
        "git-clone",
        "git-fetch",
        "npm-install",
        "podman-pull",
        "docker-pull",
        "buildah-pull",
        "bash",
        "cmd",
        "dash",
        "fish",
        "registry-pull",
        "rpm-ostree-install",
        "powershell",
        "pwsh",
        "sh",
        "wget",
        "yum",
        "zsh",
    }
)


class EvidenceValidationError(ValueError):
    """Raised when an evidence document could produce a false sovereignty claim."""


def atomic_write_json(path: Path, document: dict[str, Any]) -> None:
    """Write JSON durably without exposing a partially-written result."""

    path.parent.mkdir(parents=True, exist_ok=True)
    payload = (
        json.dumps(document, indent=2, sort_keys=True, ensure_ascii=True) + "\n"
    ).encode("utf-8")
    descriptor, temporary_name = tempfile.mkstemp(
        dir=path.parent, prefix=f".{path.name}.", suffix=".tmp"
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        directory_descriptor = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory_descriptor)
        finally:
            os.close(directory_descriptor)
    finally:
        temporary.unlink(missing_ok=True)


def artifact_manifest(
    root: Path, paths: Iterable[Path], *, exclude: Iterable[Path] = ()
) -> list[dict[str, Any]]:
    excluded = {path.resolve() for path in exclude}
    manifest: list[dict[str, Any]] = []
    for path in sorted({path.resolve() for path in paths}):
        if path in excluded or not path.is_file():
            continue
        try:
            relative = path.relative_to(root.resolve())
        except ValueError as error:
            raise EvidenceValidationError(
                f"artifact escapes evidence root: {path}"
            ) from error
        manifest.append(
            {
                "path": relative.as_posix(),
                "size": path.stat().st_size,
                "sha256": sha256_file(path),
            }
        )
    return manifest


def _require_mapping(document: dict[str, Any], key: str) -> dict[str, Any]:
    value = document.get(key)
    if not isinstance(value, dict):
        raise EvidenceValidationError(f"{key} must be an object")
    return value


def _require_list(document: dict[str, Any], key: str) -> list[Any]:
    value = document.get(key)
    if not isinstance(value, list):
        raise EvidenceValidationError(f"{key} must be an array")
    return value


def _flatten_command_tokens(document: dict[str, Any]) -> list[str]:
    tokens: list[str] = []
    inputs = document.get("inputs")
    if isinstance(inputs, dict):
        for key in ("payload_argv", "prestage_argv"):
            value = inputs.get(key)
            if isinstance(value, list):
                tokens.extend(str(token) for token in value)
    for command in document.get("commands", []):
        if not isinstance(command, dict):
            continue
        argv = command.get("argv")
        if isinstance(argv, list):
            tokens.extend(str(token) for token in argv)
    return tokens


def _normalized_effect_tokens(tokens: Iterable[str]) -> set[str]:
    normalized: set[str] = set()
    prior = ""
    for raw in tokens:
        token = Path(raw).name.lower()
        if token in {"install", "pull", "clone", "fetch"} and prior:
            normalized.add(f"{prior}-{token}")
        normalized.add(token)
        prior = token
    return normalized


def _validate_offline_commands(document: dict[str, Any]) -> None:
    observed = _normalized_effect_tokens(_flatten_command_tokens(document))
    forbidden = sorted(observed & _FORBIDDEN_EFFECT_WORDS)
    if forbidden:
        raise EvidenceValidationError(
            "proof contains a forbidden install/download/network command: "
            + ", ".join(forbidden)
        )


def _validate_artifacts(document: dict[str, Any], evidence_root: Path | None) -> None:
    artifacts = _require_list(document, "artifacts")
    seen: set[str] = set()
    if document["result"]["status"] == "PASS" and not artifacts:
        raise EvidenceValidationError("PASS evidence requires authenticated artifacts")
    for index, artifact in enumerate(artifacts):
        if not isinstance(artifact, dict):
            raise EvidenceValidationError(f"artifacts[{index}] must be an object")
        relative = artifact.get("path")
        expected_size = artifact.get("size")
        expected_hash = artifact.get("sha256")
        if (
            not isinstance(relative, str)
            or not relative
            or relative.startswith("/")
            or ".." in Path(relative).parts
        ):
            raise EvidenceValidationError(f"unsafe artifact path: {relative!r}")
        if relative in seen:
            raise EvidenceValidationError(f"duplicate artifact path: {relative}")
        seen.add(relative)
        if not isinstance(expected_size, int) or expected_size < 0:
            raise EvidenceValidationError(f"invalid artifact size: {relative}")
        if not isinstance(expected_hash, str) or not re.fullmatch(
            r"[0-9a-f]{64}", expected_hash
        ):
            raise EvidenceValidationError(f"invalid artifact digest: {relative}")
        if evidence_root is None:
            continue
        path = evidence_root / relative
        if path.is_symlink() or not path.is_file():
            raise EvidenceValidationError(f"missing regular artifact: {relative}")
        if path.stat().st_size != expected_size:
            raise EvidenceValidationError(f"artifact size mismatch: {relative}")
        if sha256_file(path) != expected_hash:
            raise EvidenceValidationError(f"artifact digest mismatch: {relative}")
    if evidence_root is not None and document["result"]["status"] == "PASS":
        actual: set[str] = set()
        for path in evidence_root.rglob("*"):
            relative = path.relative_to(evidence_root).as_posix()
            if relative == "evidence.json":
                continue
            if path.is_symlink():
                raise EvidenceValidationError(
                    f"evidence tree contains a symbolic link: {relative}"
                )
            if path.is_file():
                actual.add(relative)
        if actual != seen:
            missing = sorted(seen - actual)
            unauthenticated = sorted(actual - seen)
            raise EvidenceValidationError(
                "artifact census mismatch: "
                f"missing={missing} unauthenticated={unauthenticated}"
            )


def _validate_assertions(document: dict[str, Any]) -> None:
    assertions = _require_list(document, "assertions")
    identities: set[str] = set()
    passed_identities: set[str] = set()
    for index, assertion in enumerate(assertions):
        if not isinstance(assertion, dict):
            raise EvidenceValidationError(f"assertions[{index}] must be an object")
        identity = assertion.get("id")
        if not isinstance(identity, str) or not identity:
            raise EvidenceValidationError(f"assertions[{index}] has no stable id")
        if identity in identities:
            raise EvidenceValidationError(f"duplicate assertion id: {identity}")
        identities.add(identity)
        if assertion.get("passed") is True:
            expected = assertion.get("expected")
            observed = assertion.get("observed")
            consistent = (
                all(value == expected for value in observed)
                if isinstance(observed, list)
                else observed == expected
            )
            if not consistent:
                raise EvidenceValidationError(
                    f"assertion {identity} contradicts its expected/observed evidence"
                )
            passed_identities.add(identity)
        elif assertion.get("passed") is not False:
            raise EvidenceValidationError(
                f"assertion {identity} must carry a boolean result"
            )

    status = document["result"]["status"]
    if status == "PASS":
        missing = sorted(REQUIRED_PASS_ASSERTIONS - identities)
        extra = sorted(identities - REQUIRED_PASS_ASSERTIONS)
        if missing:
            raise EvidenceValidationError(
                "PASS evidence is missing required assertions: " + ", ".join(missing)
            )
        if extra:
            raise EvidenceValidationError(
                "PASS evidence contains unversioned assertions: " + ", ".join(extra)
            )
        failed = sorted(identities - passed_identities)
        if failed:
            raise EvidenceValidationError(
                "PASS evidence contains failed assertions: " + ", ".join(failed)
            )
    elif passed_identities:
        raise EvidenceValidationError(
            f"{status} evidence cannot carry passing sovereignty assertions"
        )


def _validate_runner(document: dict[str, Any], runner: dict[str, Any]) -> None:
    if document["result"]["status"] != "PASS":
        return
    if runner.get("system") != "Linux":
        raise EvidenceValidationError("PASS requires an observed Linux runner")
    if runner.get("uid") != 0:
        raise EvidenceValidationError("PASS requires privileged uid 0 evidence")
    if runner.get("expected_hostname") != runner.get("observed_hostname"):
        raise EvidenceValidationError("PASS runner hostname is not exact")
    for key in ("architecture", "os_release"):
        value = runner.get(key)
        if not isinstance(value, str) or not value.strip():
            raise EvidenceValidationError(
                f"PASS requires exact runner {key.replace('_', ' ')} evidence"
            )
    boot_id = runner.get("boot_id")
    if (
        not isinstance(boot_id, str)
        or re.fullmatch(
            r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
            boot_id,
        )
        is None
    ):
        raise EvidenceValidationError("PASS requires an exact runner boot identity")
    if (
        not isinstance(runner.get("process_id"), int)
        or runner["process_id"] <= 1
        or not isinstance(runner.get("process_start_ticks"), int)
        or runner["process_start_ticks"] < 0
    ):
        raise EvidenceValidationError("PASS requires an exact runner process identity")

    host_class = runner.get("host_class")
    provider_kind = runner.get("provider_kind")
    if host_class == "kvm":
        if provider_kind != "kvm" or runner.get("kvm_access") is not True:
            raise EvidenceValidationError(
                "KVM-class PASS requires KVM provider kind and exact KVM access"
            )
    elif host_class == "minicloud":
        if provider_kind != "linuxkit":
            raise EvidenceValidationError(
                "minicloud PASS requires an observed LinuxKit provider"
            )
        kernel = runner.get("kernel")
        if not isinstance(kernel, str) or "linuxkit" not in kernel.lower():
            raise EvidenceValidationError(
                "minicloud PASS requires a LinuxKit kernel observation"
            )
        if runner.get("pid1_command") != "/initd":
            raise EvidenceValidationError(
                "minicloud PASS requires the LinuxKit host PID namespace"
            )
    else:
        raise EvidenceValidationError(f"invalid PASS host class: {host_class!r}")

    cap_eff = runner.get("effective_capabilities")
    if not isinstance(cap_eff, str) or re.fullmatch(r"[0-9a-f]{16}", cap_eff) is None:
        raise EvidenceValidationError(
            "PASS requires exact effective-capability evidence"
        )
    capability_bits = int(cap_eff, 16)
    if any(not capability_bits & (1 << bit) for bit in REQUIRED_RUNNER_CAPABILITY_BITS):
        raise EvidenceValidationError(
            "PASS runner lacks a required effective capability"
        )
    if runner.get("missing_capabilities") != []:
        raise EvidenceValidationError(
            "PASS runner reports missing effective capabilities"
        )

    tools = runner.get("tools")
    if not isinstance(tools, dict) or set(tools) != REQUIRED_RUNNER_TOOLS:
        raise EvidenceValidationError("PASS runner tool inventory is incomplete")
    for name, record in tools.items():
        if not isinstance(record, dict):
            raise EvidenceValidationError(f"runner tool {name} must be an object")
        path = record.get("path")
        version = record.get("version")
        if not isinstance(path, str) or not path.startswith("/"):
            raise EvidenceValidationError(f"runner tool {name} lacks an absolute path")
        if (
            not isinstance(version, str)
            or not version.strip()
            or version == "version unavailable"
        ):
            raise EvidenceValidationError(
                f"runner tool {name} lacks exact version evidence"
            )


def _validate_source(
    document: dict[str, Any],
    source: dict[str, Any],
    source_root: Path | None,
) -> None:
    if document["result"]["status"] != "PASS":
        return
    for key in ("commit", "tree"):
        value = source.get(key)
        if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{40}", value):
            raise EvidenceValidationError(f"PASS requires an exact 40-hex source {key}")
    dirty_state = source.get("dirty")
    if not isinstance(dirty_state, str) or dirty_state.startswith("unavailable:"):
        raise EvidenceValidationError(
            "PASS requires an exact source dirty-state record"
        )

    paths = source.get("harness_paths")
    if (
        not isinstance(paths, list)
        or any(not isinstance(path, str) for path in paths)
        or set(paths) != REQUIRED_HARNESS_PATHS
        or len(paths) != len(REQUIRED_HARNESS_PATHS)
    ):
        raise EvidenceValidationError(
            "PASS source does not name the exact executable harness path set"
        )
    manifest = source.get("harness_files")
    if not isinstance(manifest, list) or len(manifest) != len(REQUIRED_HARNESS_PATHS):
        raise EvidenceValidationError(
            "PASS source lacks the exact executable harness manifest"
        )
    manifest_paths: set[str] = set()
    for row in manifest:
        if not isinstance(row, dict):
            raise EvidenceValidationError(
                "source harness manifest row is not an object"
            )
        path = row.get("path")
        size = row.get("size")
        digest = row.get("sha256")
        if (
            not isinstance(path, str)
            or path in manifest_paths
            or path not in REQUIRED_HARNESS_PATHS
        ):
            raise EvidenceValidationError(f"invalid source harness path: {path!r}")
        manifest_paths.add(path)
        if not isinstance(size, int) or size < 0:
            raise EvidenceValidationError(f"invalid source harness size: {path}")
        if not isinstance(digest, str) or re.fullmatch(r"[0-9a-f]{64}", digest) is None:
            raise EvidenceValidationError(f"invalid source harness digest: {path}")
    if manifest_paths != REQUIRED_HARNESS_PATHS:
        raise EvidenceValidationError("source harness manifest is incomplete")

    harness_digest = source.get("harness_sha256")
    if (
        not isinstance(harness_digest, str)
        or re.fullmatch(r"[0-9a-f]{64}", harness_digest) is None
    ):
        raise EvidenceValidationError("PASS requires an exact harness source digest")
    if source_root is None:
        return
    expected_manifest = harness_source_manifest(source_root, paths)
    if manifest != expected_manifest:
        raise EvidenceValidationError(
            "source harness manifest does not match the executable files"
        )
    if harness_digest != harness_source_digest(source_root, paths):
        raise EvidenceValidationError(
            "source harness aggregate digest does not match the executable files"
        )
    git_path = Path("/usr/bin/git")
    if not git_path.is_file() or not os.access(git_path, os.X_OK):
        raise EvidenceValidationError(
            "source validator lacks the fixed trusted Git executable"
        )

    def git_output(*args: str) -> str:
        try:
            result = subprocess.run(
                [
                    str(git_path),
                    "-c",
                    f"safe.directory={source_root}",
                    *args,
                ],
                cwd=source_root,
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=60,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as error:
            raise EvidenceValidationError(
                f"source validator could not run Git: {error}"
            ) from error
        if result.returncode != 0:
            raise EvidenceValidationError(
                "source validator Git command failed: " + result.stderr.strip()
            )
        return result.stdout.strip()

    expected_source = {
        "commit": git_output("rev-parse", "HEAD"),
        "tree": git_output("rev-parse", "HEAD^{tree}"),
        "dirty": git_output("status", "--short"),
    }
    for key, expected in expected_source.items():
        if source.get(key) != expected:
            raise EvidenceValidationError(
                f"source {key} does not match the independently observed repository"
            )


def _read_authenticated_text(
    evidence_root: Path,
    relative: str,
    artifact_paths: set[str],
) -> str:
    if relative not in artifact_paths:
        raise EvidenceValidationError(
            f"required artifact is unauthenticated: {relative}"
        )
    path = evidence_root / relative
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        raise EvidenceValidationError(
            f"required artifact is unreadable: {relative}"
        ) from error


def _command_token_matches(observed: Any, expected: str) -> bool:
    if not isinstance(observed, str):
        return False
    if expected in REQUIRED_RUNNER_TOOLS:
        observed_name = Path(observed).name
        return observed_name == expected or (
            expected == "python3" and observed_name.startswith("python3.")
        )
    return observed == expected


def _command_contains(argv: list[Any], *tokens: str) -> bool:
    if len(tokens) > len(argv):
        return False
    return any(
        all(
            _command_token_matches(observed, expected)
            for observed, expected in zip(
                argv[index : index + len(tokens)],
                tokens,
                strict=True,
            )
        )
        for index in range(len(argv) - len(tokens) + 1)
    )


def _command_equals(argv: list[Any], *tokens: str) -> bool:
    return len(argv) == len(tokens) and _command_contains(argv, *tokens)


def _parse_nft_counters(payload: str, table: str) -> dict[str, int]:
    try:
        document = json.loads(payload)
    except json.JSONDecodeError as error:
        raise EvidenceValidationError(
            "nft counter artifact is not valid JSON"
        ) from error
    counters: dict[str, int] = {}
    for item in document.get("nftables", []):
        counter = item.get("counter") if isinstance(item, dict) else None
        if isinstance(counter, dict) and counter.get("table") == table:
            name = counter.get("name")
            packets = counter.get("packets")
            if isinstance(name, str) and isinstance(packets, int):
                counters[name] = packets
    return counters


def _parse_probe(payload: str, phase: str) -> dict[str, Any]:
    lines = [line for line in payload.splitlines() if line.strip()]
    if len(lines) != 1:
        raise EvidenceValidationError(
            f"{phase} probe artifact must contain exactly one JSON result"
        )
    try:
        document = json.loads(lines[0])
    except json.JSONDecodeError as error:
        raise EvidenceValidationError(
            f"{phase} probe artifact is not valid JSON"
        ) from error
    if not isinstance(document, dict):
        raise EvidenceValidationError(f"{phase} probe artifact is not an object")
    return document


def _derive_phase_assertions(
    attempt: dict[str, Any],
    *,
    control_dns: str,
    profile_dns: str,
    control_trace: str,
    profile_trace: str,
    resources: dict[str, Any],
) -> tuple[dict[str, bool], dict[str, bool]]:
    control = _require_mapping(attempt, "control")
    profile = _require_mapping(attempt, "profile")
    control_probe = _require_mapping(control, "probe")
    profile_probe = _require_mapping(profile, "probe")
    control_counters = _require_mapping(control, "counters")
    profile_counters = _require_mapping(profile, "counters")
    control_trace_addresses = (
        "127.0.0.1",
        resources["peer_ipv4"],
        resources["peer_ipv6"],
        "10.253.0.1",
        "192.0.2.1",
        "2001:db8::1",
    )
    profile_trace_addresses = (
        "127.0.0.1",
        resources["peer_ipv4"],
        resources["peer_ipv6"],
    )
    forbidden_profile_addresses = ("10.253.0.1", "192.0.2.1", "2001:db8::1")
    return (
        {
            "subject_unprivileged": control_probe.get("subject_unprivileged") is True,
            "loopback": control_probe.get("loopback") is True,
            "private_ipv4": control_probe.get("private_ipv4") is True,
            "private_ipv6": control_probe.get("private_ipv6") is True,
            "unenumerated_private_denied": (
                control_probe.get("unenumerated_private_denied") is True
                and control_counters.get("denied_private") == 1
            ),
            "dns_udp": (
                control_probe.get("dns_udp_attempted") is True
                and control_counters.get("dns_udp") == 1
                and " UDP" in control_dns
                and control_dns.count("nnc47-udp-control.invalid") == 1
            ),
            "dns_tcp": (
                control_probe.get("dns_tcp_attempted") is True
                and control_counters.get("dns_tcp") == 1
                and control_dns.count("nnc47-tcp-control.invalid") == 1
            ),
            "public_ipv4_denied": (
                control_probe.get("public_ipv4_denied") is True
                and control_counters.get("denied_ipv4") == 1
            ),
            "public_ipv6_denied": (
                control_probe.get("public_ipv6_denied") is True
                and control_counters.get("denied_ipv6") == 1
            ),
            "network_trace": (
                all(address in control_trace for address in control_trace_addresses)
                and "sin_port=htons(53)" in control_trace
            ),
            "no_unclassified_control": control_counters.get("unexpected") == 0,
        },
        {
            "subject_unprivileged": profile_probe.get("subject_unprivileged") is True,
            "private_only": all(
                profile_probe.get(name) is True
                for name in ("loopback", "private_ipv4", "private_ipv6")
            ),
            "zero_unexpected": all(value == 0 for value in profile_counters.values()),
            "zero_dns_capture": not profile_dns.strip(),
            "network_trace_private_only": (
                all(address in profile_trace for address in profile_trace_addresses)
                and all(
                    address not in profile_trace
                    for address in forbidden_profile_addresses
                )
            ),
        },
    )


def _validate_attempt_raw_evidence(
    document: dict[str, Any],
    attempt: dict[str, Any],
    attempt_index: int,
    evidence_root: Path,
) -> None:
    resources = _require_mapping(attempt, "resources")
    required_resources = (
        "nft_table",
        "peer_interface",
        "peer_ipv4",
        "peer_ipv6",
        "peer_namespace",
        "subject_interface",
        "subject_ipv4",
        "subject_ipv6",
        "subject_namespace",
    )
    if set(resources) != set(required_resources) or any(
        not isinstance(resources[key], str) or not resources[key]
        for key in required_resources
    ):
        raise EvidenceValidationError(
            f"PASS attempt {attempt_index} has incomplete resource identity"
        )

    artifacts = _require_list(document, "artifacts")
    artifact_paths = {row.get("path") for row in artifacts if isinstance(row, dict)}
    prefix = f"attempt-{attempt_index}"
    topology = _read_authenticated_text(
        evidence_root, f"{prefix}/topology.txt", artifact_paths
    )
    rules = _read_authenticated_text(
        evidence_root, f"{prefix}/rules.nft", artifact_paths
    )
    control_dns = _read_authenticated_text(
        evidence_root, f"{prefix}/control-dns.txt", artifact_paths
    )
    profile_dns = _read_authenticated_text(
        evidence_root, f"{prefix}/profile-dns.txt", artifact_paths
    )
    control_trace_paths = sorted(
        path
        for path in artifact_paths
        if isinstance(path, str) and path.startswith(f"{prefix}/control.strace.")
    )
    profile_trace_paths = sorted(
        path
        for path in artifact_paths
        if isinstance(path, str) and path.startswith(f"{prefix}/profile.strace.")
    )
    if not control_trace_paths or not profile_trace_paths:
        raise EvidenceValidationError(
            f"PASS attempt {attempt_index} lacks authenticated syscall traces"
        )
    control_trace = "\n".join(
        _read_authenticated_text(evidence_root, path, artifact_paths)
        for path in control_trace_paths
    )
    profile_trace = "\n".join(
        _read_authenticated_text(evidence_root, path, artifact_paths)
        for path in profile_trace_paths
    )

    for key, value in resources.items():
        if key == "nft_table":
            continue
        if value not in topology:
            raise EvidenceValidationError(
                f"PASS attempt {attempt_index} topology omits {value!r}"
            )
    for anchor in (
        f"table inet {resources['nft_table']}",
        "policy drop",
        f"ip daddr {resources['peer_ipv4']} udp dport 53",
        f"ip daddr {resources['peer_ipv4']} tcp dport 53",
        "counter name denied_private drop",
        "counter name denied_ipv4 drop",
        "counter name denied_ipv6 drop",
    ):
        if anchor not in rules:
            raise EvidenceValidationError(
                f"PASS attempt {attempt_index} nft rules omit {anchor!r}"
            )
    if (
        control_dns.count("nnc47-udp-control.invalid") != 1
        or control_dns.count("nnc47-tcp-control.invalid") != 1
    ):
        raise EvidenceValidationError(
            f"PASS attempt {attempt_index} lacks exact UDP/TCP DNS capture"
        )
    if profile_dns.strip():
        raise EvidenceValidationError(
            f"PASS attempt {attempt_index} profile DNS capture is nonempty"
        )
    for address in (
        "127.0.0.1",
        resources["peer_ipv4"],
        resources["peer_ipv6"],
        "10.253.0.1",
        "192.0.2.1",
        "2001:db8::1",
    ):
        if address not in control_trace:
            raise EvidenceValidationError(
                f"PASS attempt {attempt_index} control trace omits {address}"
            )
    if "sin_port=htons(53)" not in control_trace:
        raise EvidenceValidationError(
            f"PASS attempt {attempt_index} control trace omits DNS port 53"
        )
    for address in ("127.0.0.1", resources["peer_ipv4"], resources["peer_ipv6"]):
        if address not in profile_trace:
            raise EvidenceValidationError(
                f"PASS attempt {attempt_index} profile trace omits {address}"
            )
    if any(
        address in profile_trace
        for address in ("10.253.0.1", "192.0.2.1", "2001:db8::1")
    ):
        raise EvidenceValidationError(
            f"PASS attempt {attempt_index} profile trace contains forbidden output"
        )
    derived_control, derived_profile = _derive_phase_assertions(
        attempt,
        control_dns=control_dns,
        profile_dns=profile_dns,
        control_trace=control_trace,
        profile_trace=profile_trace,
        resources=resources,
    )
    if attempt["control"].get("assertions") != derived_control:
        raise EvidenceValidationError(
            f"PASS attempt {attempt_index} control assertions are not derived"
        )
    if attempt["profile"].get("assertions") != derived_profile:
        raise EvidenceValidationError(
            f"PASS attempt {attempt_index} profile assertions are not derived"
        )

    commands = _require_list(document, "commands")
    command_rows = [row for row in commands if isinstance(row, dict)]
    start_candidates = [
        row.get("index")
        for row in command_rows
        if _command_equals(
            row.get("argv", []),
            "ip",
            "netns",
            "add",
            resources["subject_namespace"],
        )
        and isinstance(row.get("index"), int)
    ]
    end_candidates = [
        row.get("index")
        for row in command_rows
        if _command_equals(
            row.get("argv", []),
            "ip",
            "netns",
            "delete",
            resources["peer_namespace"],
        )
        and isinstance(row.get("index"), int)
    ]
    if (
        len(start_candidates) < attempt_index
        or len(end_candidates) < attempt_index
        or start_candidates[attempt_index - 1] >= end_candidates[attempt_index - 1]
    ):
        raise EvidenceValidationError(
            f"PASS attempt {attempt_index} lacks an ordered namespace lifecycle"
        )
    start = start_candidates[attempt_index - 1]
    end = end_candidates[attempt_index - 1]
    segment = [row for row in command_rows if start <= row.get("index", -1) <= end]
    argvs = [row.get("argv") for row in segment]
    exact_commands = (
        ["ip", "netns", "add", resources["peer_namespace"]],
        [
            "ip",
            "link",
            "add",
            resources["subject_interface"],
            "type",
            "veth",
            "peer",
            "name",
            resources["peer_interface"],
        ],
        ["ip", "netns", "delete", resources["subject_namespace"]],
        ["ip", "netns", "delete", resources["peer_namespace"]],
    )
    for argv in exact_commands:
        if not any(_command_equals(observed, *argv) for observed in argvs):
            raise EvidenceValidationError(
                f"PASS attempt {attempt_index} lacks command {' '.join(argv)}"
            )
    for setting in ("net.ipv4.ip_forward", "net.ipv6.conf.all.forwarding"):
        if not any(
            _command_contains(row.get("argv", []), "sysctl", "-n", setting)
            for row in segment
        ):
            raise EvidenceValidationError(
                f"PASS attempt {attempt_index} lacks forwarding read-back {setting}"
            )
    for phase in ("control", "profile"):
        matches = [
            row
            for row in segment
            if _command_contains(row.get("argv", []), "strace", "-ff")
            and _command_contains(row.get("argv", []), "--mode", phase)
            and _command_contains(
                row.get("argv", []), "--dns-ipv4", resources["peer_ipv4"]
            )
            and _command_contains(row.get("argv", []), "--bounding-set=-all")
            and _command_contains(row.get("argv", []), "--no-new-privs")
        ]
        if len(matches) != 1:
            raise EvidenceValidationError(
                f"PASS attempt {attempt_index} lacks one exact {phase} probe command"
            )
        stdout_path = matches[0].get("stdout")
        if not isinstance(stdout_path, str):
            raise EvidenceValidationError(
                f"PASS attempt {attempt_index} {phase} probe lacks stdout"
            )
        observed_probe = _parse_probe(
            _read_authenticated_text(evidence_root, stdout_path, artifact_paths),
            phase,
        )
        if observed_probe != attempt[phase].get("probe"):
            raise EvidenceValidationError(
                f"PASS attempt {attempt_index} {phase} probe summary is not derived"
            )
    snapshots = [
        row
        for row in segment
        if _command_contains(
            row.get("argv", []),
            "nft",
            "-j",
            "list",
            "counters",
            "table",
            "inet",
            resources["nft_table"],
        )
    ]
    if len(snapshots) != 3:
        raise EvidenceValidationError(
            f"PASS attempt {attempt_index} lacks control/reset/profile nft snapshots"
        )
    expected_snapshots = (
        attempt["control"]["counters"],
        attempt["reset"]["counters"],
        attempt["profile"]["counters"],
    )
    for phase, row, expected in zip(
        ("control", "reset", "profile"), snapshots, expected_snapshots, strict=True
    ):
        stdout_path = row.get("stdout")
        if not isinstance(stdout_path, str):
            raise EvidenceValidationError(
                f"PASS attempt {attempt_index} {phase} counters lack stdout"
            )
        observed = _parse_nft_counters(
            _read_authenticated_text(evidence_root, stdout_path, artifact_paths),
            resources["nft_table"],
        )
        if observed != expected:
            raise EvidenceValidationError(
                f"PASS attempt {attempt_index} {phase} counters are not derived"
            )
    if sum(_command_contains(row.get("argv", []), "tcpdump") for row in segment) != 2:
        raise EvidenceValidationError(
            f"PASS attempt {attempt_index} lacks two DNS capture phases"
        )


def _validate_probe_security(
    probe: dict[str, Any], phase_name: str, attempt_index: int
) -> None:
    security = probe.get("security")
    if (
        not isinstance(security, dict)
        or set(security) != REQUIRED_PROBE_SECURITY_FIELDS
        or any(
            security.get(key) != 0
            for key in ("capamb", "capbnd", "capeff", "capinh", "capprm")
        )
        or security.get("uids") != [65534, 65534, 65534, 65534]
        or security.get("gids") != [65534, 65534, 65534, 65534]
        or security.get("no_new_privs") != 1
        or probe.get("cap_net_admin") is not False
        or probe.get("subject_unprivileged") is not True
        or probe.get("passed") is not True
    ):
        raise EvidenceValidationError(
            f"PASS attempt {attempt_index} {phase_name} probe retained authority"
        )


def _validate_pass_attempts(
    document: dict[str, Any], evidence_root: Path | None
) -> None:
    if document["result"]["status"] != "PASS":
        return
    attempts = _require_list(document, "attempts")
    if len(attempts) != 2:
        raise EvidenceValidationError(
            "PASS requires exactly two same-identity attempts for cleanup/re-entry"
        )
    exact_counters = {
        "denied_ipv4": 1,
        "denied_ipv6": 1,
        "denied_private": 1,
        "dns_tcp": 1,
        "dns_udp": 1,
        "unexpected": 0,
    }
    zero_counters = {name: 0 for name in exact_counters}
    for index, attempt in enumerate(attempts, start=1):
        if not isinstance(attempt, dict) or attempt.get("attempt") != index:
            raise EvidenceValidationError(
                f"PASS attempt {index} lacks stable sequential identity"
            )
        setup = _require_mapping(attempt, "setup")
        control = _require_mapping(attempt, "control")
        reset = _require_mapping(attempt, "reset")
        profile = _require_mapping(attempt, "profile")
        cleanup = _require_mapping(attempt, "cleanup")
        if (
            setup.get("preexisting_namespaces_absent") is not True
            or setup.get("preexisting_veths_absent") is not True
            or setup.get("peer_forwarding") != {"ipv4": 0, "ipv6": 0}
        ):
            raise EvidenceValidationError(
                f"PASS attempt {index} has an invalid isolation baseline"
            )
        if control.get("counters") != exact_counters:
            raise EvidenceValidationError(
                f"PASS attempt {index} lacks exact control counter deltas"
            )
        for phase_name, phase, required_assertions in (
            ("control", control, REQUIRED_CONTROL_ASSERTIONS),
            ("profile", profile, REQUIRED_PROFILE_ASSERTIONS),
        ):
            assertions = phase.get("assertions")
            if (
                not isinstance(assertions, dict)
                or set(assertions) != required_assertions
                or any(value is not True for value in assertions.values())
            ):
                raise EvidenceValidationError(
                    f"PASS attempt {index} has failed {phase_name} assertions"
                )
            probe = phase.get("probe")
            if not isinstance(probe, dict):
                raise EvidenceValidationError(
                    f"PASS attempt {index} lacks its {phase_name} probe"
                )
            _validate_probe_security(probe, phase_name, index)
        if reset.get("passed") is not True or reset.get("counters") != zero_counters:
            raise EvidenceValidationError(
                f"PASS attempt {index} has a nonzero reset baseline"
            )
        if profile.get("counters") != zero_counters:
            raise EvidenceValidationError(
                f"PASS attempt {index} has nonzero profile counters"
            )
        if (
            cleanup.get("passed") is not True
            or cleanup.get("namespaces_absent") is not True
            or cleanup.get("root_veths_absent") is not True
            or cleanup.get("errors") != []
            or cleanup.get("deferred_signals") != []
        ):
            raise EvidenceValidationError(
                f"PASS attempt {index} lacks exact cleanup evidence"
            )
        if evidence_root is not None:
            attempt_path = evidence_root / f"attempt-{index}/attempt.json"
            if not attempt_path.is_file() or attempt_path.is_symlink():
                raise EvidenceValidationError(
                    f"PASS attempt {index} lacks its authenticated attempt artifact"
                )
            try:
                artifact_attempt = json.loads(attempt_path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError) as error:
                raise EvidenceValidationError(
                    f"PASS attempt {index} artifact is unreadable"
                ) from error
            if artifact_attempt != attempt:
                raise EvidenceValidationError(
                    f"PASS attempt {index} artifact does not match evidence"
                )
            _validate_attempt_raw_evidence(
                document,
                attempt,
                index,
                evidence_root,
            )


def _validate_pass_assertion_bindings(document: dict[str, Any]) -> None:
    if document["result"]["status"] != "PASS":
        return
    attempts = _require_list(document, "attempts")
    runner = _require_mapping(document, "runner")
    rows = {
        row["id"]: row
        for row in _require_list(document, "assertions")
        if isinstance(row, dict) and isinstance(row.get("id"), str)
    }
    controls = [attempt["control"]["assertions"] for attempt in attempts]
    profiles = [attempt["profile"]["assertions"] for attempt in attempts]
    expected: dict[str, tuple[Any, Any]] = {
        "preflight.named_runner": (
            runner["expected_hostname"],
            runner["observed_hostname"],
        ),
        "preflight.offline_inputs": (True, True),
        "isolation.outer_namespaces": (
            True,
            [
                attempt["setup"]["preexisting_namespaces_absent"]
                and attempt["setup"]["preexisting_veths_absent"]
                for attempt in attempts
            ],
        ),
        "isolation.subject_unprivileged": (
            True,
            [control["subject_unprivileged"] for control in controls],
        ),
        "isolation.peer_forwarding_disabled": (
            {"ipv4": 0, "ipv6": 0},
            [attempt["setup"]["peer_forwarding"] for attempt in attempts],
        ),
        "reset.zero_baseline": (
            True,
            [attempt["reset"]["passed"] for attempt in attempts],
        ),
        "cleanup.absent": (
            True,
            [attempt["cleanup"]["passed"] for attempt in attempts],
        ),
        "cleanup.same_identity_reentry": (2, len(attempts)),
        "evidence.artifacts_authenticated": (True, True),
    }
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
    ):
        expected[f"control.{name}"] = (
            True,
            [control[name] for control in controls],
        )
    for name in ("private_only", "zero_unexpected"):
        expected[f"profile.{name}"] = (
            True,
            [profile[name] for profile in profiles],
        )
    for identity, (expected_value, observed_value) in expected.items():
        row = rows[identity]
        if (
            row.get("expected") != expected_value
            or row.get("observed") != observed_value
            or row.get("passed") is not True
        ):
            raise EvidenceValidationError(
                f"assertion {identity} is not derived from attempt evidence"
            )


def _validate_commands(document: dict[str, Any]) -> None:
    commands = _require_list(document, "commands")
    if document["result"]["status"] != "PASS":
        return
    if not commands:
        raise EvidenceValidationError("PASS requires recorded effect commands")
    artifacts = _require_list(document, "artifacts")
    artifact_paths = {
        artifact.get("path") for artifact in artifacts if isinstance(artifact, dict)
    }
    attempts = _require_list(document, "attempts")
    resources = [
        _require_mapping(attempt, "resources")
        for attempt in attempts
        if isinstance(attempt, dict)
    ]
    runner = _require_mapping(document, "runner")
    tools = _require_mapping(runner, "tools")
    trusted_paths = {
        name: record.get("path") if isinstance(record, dict) else None
        for name, record in tools.items()
    }
    trusted_path_names = {
        path: name for name, path in trusted_paths.items() if isinstance(path, str)
    }
    trusted_basenames = {Path(path).name for path in trusted_path_names}
    owned_interfaces = {
        item[name]
        for item in resources
        for name in ("subject_interface", "peer_interface")
    }
    peer_namespaces = {item["peer_namespace"] for item in resources}
    for index, command in enumerate(commands, start=1):
        if not isinstance(command, dict) or command.get("index") != index:
            raise EvidenceValidationError(
                f"PASS command {index} lacks stable sequential identity"
            )
        argv = command.get("argv")
        if (
            not isinstance(argv, list)
            or not argv
            or any(not isinstance(token, str) for token in argv)
        ):
            raise EvidenceValidationError(f"PASS command {index} has invalid argv")
        for token in argv:
            tool_name = Path(token).name
            if token in trusted_path_names:
                continue
            if tool_name in REQUIRED_RUNNER_TOOLS or tool_name in trusted_basenames:
                raise EvidenceValidationError(
                    f"PASS command {index} uses unauthenticated tool path: {token}"
                )
        exit_code = command.get("exit_code")
        if not isinstance(exit_code, int):
            raise EvidenceValidationError(
                f"PASS command {index} has no exact exit code"
            )
        argv = command["argv"]
        allowed_nonzero = (
            (
                exit_code == 1
                and len(argv) == 4
                and _command_equals(argv[:3], "ip", "link", "show")
                and argv[3] in owned_interfaces
            )
            or (
                exit_code == -15
                and _command_contains(argv, "peer-server")
                and any(
                    _command_contains(argv, "ip", "netns", "exec", namespace)
                    for namespace in peer_namespaces
                )
            )
            or (
                exit_code == -2
                and _command_contains(argv, "tcpdump")
                and any(
                    _command_contains(argv, "ip", "netns", "exec", namespace)
                    for namespace in peer_namespaces
                )
            )
        )
        if exit_code != 0 and not allowed_nonzero:
            raise EvidenceValidationError(
                f"PASS command {index} has unexpected exit {exit_code}"
            )
        if command.get("timed_out") is not False:
            raise EvidenceValidationError(f"PASS command {index} reports a timeout")
        if command.get("interrupted") is not False:
            raise EvidenceValidationError(
                f"PASS command {index} reports an interruption"
            )
        if command.get("deferred_signals") != []:
            raise EvidenceValidationError(
                f"PASS command {index} reports deferred termination signals"
            )
        if not isinstance(command.get("process_group"), int):
            raise EvidenceValidationError(
                f"PASS command {index} has no owned process-group evidence"
            )
        for stream in ("stdout", "stderr"):
            path = command.get(stream)
            if not isinstance(path, str) or path not in artifact_paths:
                raise EvidenceValidationError(
                    f"PASS command {index} {stream} is not authenticated"
                )


def _parse_evidence_timestamp(document: dict[str, Any], key: str) -> datetime:
    result = _require_mapping(document, "result")
    value = result.get(key)
    if not isinstance(value, str):
        raise EvidenceValidationError(f"re-entry evidence lacks result.{key}")
    try:
        timestamp = datetime.fromisoformat(value)
    except ValueError as error:
        raise EvidenceValidationError(
            f"re-entry evidence has invalid result.{key}"
        ) from error
    if timestamp.tzinfo is None:
        raise EvidenceValidationError(
            f"re-entry evidence result.{key} lacks a timezone"
        )
    return timestamp


def _validate_result_interval(document: dict[str, Any]) -> tuple[datetime, datetime]:
    started = _parse_evidence_timestamp(document, "started_at")
    finished = _parse_evidence_timestamp(document, "finished_at")
    if started > finished:
        raise EvidenceValidationError(
            "evidence result.finished_at precedes result.started_at"
        )
    return started, finished


def validate_reentry_pair(
    predecessor: dict[str, Any], successor: dict[str, Any]
) -> None:
    """Bind two independently validated full runs into one fresh-process proof."""

    if (
        _require_mapping(predecessor, "result").get("status") != "PASS"
        or _require_mapping(successor, "result").get("status") != "PASS"
    ):
        raise EvidenceValidationError("re-entry pair requires two PASS documents")
    predecessor_runner = dict(_require_mapping(predecessor, "runner"))
    successor_runner = dict(_require_mapping(successor, "runner"))
    predecessor_process = (
        predecessor_runner.pop("process_id", None),
        predecessor_runner.pop("process_start_ticks", None),
    )
    successor_process = (
        successor_runner.pop("process_id", None),
        successor_runner.pop("process_start_ticks", None),
    )
    if predecessor_runner != successor_runner:
        raise EvidenceValidationError(
            "re-entry pair does not describe the exact same named runner"
        )
    if predecessor_process == successor_process:
        raise EvidenceValidationError(
            "re-entry pair does not cross a fresh process incarnation"
        )
    if _require_mapping(predecessor, "source") != _require_mapping(successor, "source"):
        raise EvidenceValidationError(
            "re-entry pair source identity changed between runs"
        )
    if _require_mapping(predecessor, "inputs") != _require_mapping(successor, "inputs"):
        raise EvidenceValidationError("re-entry pair inputs changed between runs")
    predecessor_attempts = _require_list(predecessor, "attempts")
    successor_attempts = _require_list(successor, "attempts")
    predecessor_resources = [
        _require_mapping(attempt, "resources") for attempt in predecessor_attempts
    ]
    successor_resources = [
        _require_mapping(attempt, "resources") for attempt in successor_attempts
    ]
    if predecessor_resources != successor_resources:
        raise EvidenceValidationError(
            "re-entry pair resource identity changed between runs"
        )
    if any(
        _require_mapping(attempt, "cleanup").get("passed") is not True
        for attempt in predecessor_attempts
    ):
        raise EvidenceValidationError(
            "re-entry predecessor did not prove exact cleanup"
        )
    successor_setup = _require_mapping(successor_attempts[0], "setup")
    if (
        successor_setup.get("preexisting_namespaces_absent") is not True
        or successor_setup.get("preexisting_veths_absent") is not True
    ):
        raise EvidenceValidationError(
            "re-entry successor did not start from an absent-resource baseline"
        )
    _, predecessor_finished = _validate_result_interval(predecessor)
    successor_started, _ = _validate_result_interval(successor)
    if predecessor_finished > successor_started:
        raise EvidenceValidationError(
            "re-entry successor began before its predecessor completed"
        )


def validate_evidence(
    document: dict[str, Any],
    *,
    evidence_root: Path | None = None,
    source_root: Path | None = None,
) -> None:
    """Fail closed on contradictory, incomplete, or unauthenticated evidence."""

    if not isinstance(document, dict):
        raise EvidenceValidationError("evidence must be a JSON object")
    if document.get("schema_version") != EVIDENCE_SCHEMA_VERSION:
        raise EvidenceValidationError(
            f"unsupported evidence schema: {document.get('schema_version')!r}"
        )

    result = _require_mapping(document, "result")
    status = result.get("status")
    exit_code = result.get("exit_code")
    reason = result.get("reason")
    if status not in {"PASS", "FAIL", "SKIPPED"}:
        raise EvidenceValidationError(f"invalid result status: {status!r}")
    if not isinstance(exit_code, int):
        raise EvidenceValidationError("result.exit_code must be an integer")
    if status in _STATUS_EXITS and exit_code != _STATUS_EXITS[status]:
        raise EvidenceValidationError(
            f"{status} must use exit {_STATUS_EXITS[status]}, got {exit_code}"
        )
    if status == "FAIL" and exit_code in {0, 77}:
        raise EvidenceValidationError("FAIL must use a nonzero, non-77 exit")
    if status == "PASS" and reason not in {None, ""}:
        raise EvidenceValidationError("PASS cannot carry a skip/failure reason")
    if status != "PASS" and (not isinstance(reason, str) or not reason.strip()):
        raise EvidenceValidationError(f"{status} requires an exact reason")

    runner = _require_mapping(document, "runner")
    runner_id = runner.get("asserted_id")
    if not isinstance(runner_id, str) or not _SAFE_ID.fullmatch(runner_id):
        raise EvidenceValidationError(f"invalid runner identity: {runner_id!r}")
    _validate_runner(document, runner)

    source = _require_mapping(document, "source")
    _validate_source(document, source, source_root)
    inputs = _require_mapping(document, "inputs")
    if status == "PASS" and (
        inputs.get("offline") is not True
        or inputs.get("payload_argv") != []
        or inputs.get("prestage_argv") != []
    ):
        raise EvidenceValidationError("PASS inputs are not an offline empty payload")
    _validate_offline_commands(document)
    _validate_assertions(document)
    _validate_artifacts(document, evidence_root)
    _validate_pass_attempts(document, evidence_root)
    _validate_pass_assertion_bindings(document)
    _validate_commands(document)
    _validate_result_interval(document)
