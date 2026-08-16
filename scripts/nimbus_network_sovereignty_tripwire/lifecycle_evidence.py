#!/usr/bin/env python3
"""Fail-closed evidence contract for the NNC9.2 sovereign lifecycle."""

from __future__ import annotations

from datetime import datetime
import json
from pathlib import Path
import re
import stat
from typing import Any

from .evidence import REQUIRED_CONTROL_ASSERTIONS
from .integrity import sha256_file

SCHEMA_VERSION = 1
REQUIRED_TRANSITIONS = (
    "config-admitted",
    "positive-controls-passed",
    "counter-reset",
    "first-owner-started",
    "first-ready",
    "first-inspected",
    "first-served",
    "first-exit-triggered",
    "restart-observed",
    "second-served",
    "restart-inspected",
    "first-owner-stopped",
    "fresh-owner-started",
    "fresh-owner-reconciled",
    "fresh-owner-stopped",
    "withdrawal-started",
    "retirement-recorded",
    "retired-endpoint-unreachable",
    "cleanup-proven",
)
REQUIRED_ASSERTIONS = frozenset(f"K{index}" for index in range(1, 15))
EXPECTED_PROVISION_OBSERVATIONS = (
    "network_reserved",
    "execution_prepared",
    "network_attached",
    "execution_activated",
    "ready",
    "publication_present",
    "publication_observed",
)
EXPECTED_TERMINAL_OBSERVATIONS = (
    "publication_absent",
    "execution_drained",
    "execution_stopped",
    "network_detached",
    "network_released",
)
EXPECTED_CONTROL_COUNTERS = {
    "denied_ipv4": 1,
    "denied_ipv6": 1,
    "denied_private": 1,
    "dns_tcp": 1,
    "dns_udp": 1,
    "unexpected": 0,
}
EXPECTED_ZERO_COUNTERS = {name: 0 for name in EXPECTED_CONTROL_COUNTERS}
FORBIDDEN_EFFECT_TOKENS = frozenset(
    {
        "apt",
        "apt-get",
        "cargo-fetch",
        "curl",
        "dnf",
        "docker-pull",
        "git-clone",
        "git-fetch",
        "npm-install",
        "podman-pull",
        "registry-pull",
        "wget",
        "yum",
    }
)
HASH_PATTERN = re.compile(r"^[0-9a-f]{64}$")
GIT_OBJECT_PATTERN = re.compile(r"^[0-9a-f]{40}$")


class LifecycleEvidenceError(ValueError):
    """Raised when evidence could produce a false NNC9.2 PASS claim."""


def _mapping(value: Any, label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise LifecycleEvidenceError(f"{label} must be an object")
    return value


def _list(value: Any, label: str) -> list[Any]:
    if not isinstance(value, list):
        raise LifecycleEvidenceError(f"{label} must be an array")
    return value


def _timestamp(value: Any, label: str) -> datetime:
    if not isinstance(value, str):
        raise LifecycleEvidenceError(f"{label} must be a timestamp")
    try:
        parsed = datetime.fromisoformat(value)
    except ValueError as error:
        raise LifecycleEvidenceError(f"{label} is not an ISO timestamp") from error
    if parsed.tzinfo is None:
        raise LifecycleEvidenceError(f"{label} lacks a timezone")
    return parsed


def _require_hash(value: Any, label: str) -> str:
    if not isinstance(value, str) or HASH_PATTERN.fullmatch(value) is None:
        raise LifecycleEvidenceError(f"{label} is not a SHA-256 digest")
    return value


def _normalized_command_tokens(commands: list[Any]) -> set[str]:
    tokens: set[str] = set()
    prior = ""
    for row in commands:
        command = _mapping(row, "command")
        argv = _list(command.get("argv"), "command.argv")
        for raw in argv:
            if not isinstance(raw, str):
                raise LifecycleEvidenceError("command argv contains a non-string token")
            token = Path(raw).name.lower()
            tokens.add(token)
            if token in {"clone", "fetch", "install", "pull"} and prior:
                tokens.add(f"{prior}-{token}")
            prior = token
    return tokens


def _validate_commands(commands: list[Any]) -> None:
    if not commands:
        raise LifecycleEvidenceError("PASS evidence has no recorded commands")
    for expected_index, value in enumerate(commands, start=1):
        command = _mapping(value, f"commands[{expected_index - 1}]")
        if command.get("index") != expected_index:
            raise LifecycleEvidenceError("commands lack stable sequential identity")
        if command.get("timed_out") is not False:
            raise LifecycleEvidenceError(f"command {expected_index} timed out")
        if command.get("interrupted") is not False:
            raise LifecycleEvidenceError(f"command {expected_index} was interrupted")
        if command.get("critical") is True:
            accepted = command.get("accepted_exit_codes", [0])
            if command.get("exit_code") not in accepted:
                raise LifecycleEvidenceError(
                    f"critical command {expected_index} exited {command.get('exit_code')}"
                )
        for stream in ("stdout", "stderr"):
            if not isinstance(command.get(stream), str):
                raise LifecycleEvidenceError(
                    f"command {expected_index} lacks authenticated {stream}"
                )
    forbidden = sorted(_normalized_command_tokens(commands) & FORBIDDEN_EFFECT_TOKENS)
    if forbidden:
        raise LifecycleEvidenceError(
            "offline lifecycle contains a forbidden install/download command: "
            + ", ".join(forbidden)
        )


def _validate_inputs(document: dict[str, Any]) -> None:
    inputs = _mapping(document.get("inputs"), "inputs")
    candidate_value = inputs.get("nimbus_path")
    if not isinstance(candidate_value, str) or not Path(candidate_value).is_absolute():
        raise LifecycleEvidenceError("candidate path is not absolute")
    candidate = Path(candidate_value)
    if not candidate.is_file() or candidate.is_symlink():
        raise LifecycleEvidenceError("authenticated candidate is absent or not regular")
    if candidate.stat().st_size != inputs.get("nimbus_size"):
        raise LifecycleEvidenceError("candidate size changed after admission")
    if sha256_file(candidate) != inputs.get("nimbus_sha256"):
        raise LifecycleEvidenceError("candidate digest changed after admission")

    fixture_value = inputs.get("fixture_root")
    if not isinstance(fixture_value, str) or not Path(fixture_value).is_absolute():
        raise LifecycleEvidenceError("fixture root is not absolute")
    fixture_root = Path(fixture_value)
    fixture_files = _list(inputs.get("fixture_files"), "inputs.fixture_files")
    observed: dict[str, str] = {}
    for index, value in enumerate(fixture_files):
        row = _mapping(value, f"inputs.fixture_files[{index}]")
        relative = row.get("path")
        if not isinstance(relative, str) or relative.startswith("/"):
            raise LifecycleEvidenceError("fixture manifest has an invalid path")
        path = (fixture_root / relative).resolve()
        try:
            path.relative_to(fixture_root.resolve())
        except ValueError as error:
            raise LifecycleEvidenceError("fixture input escapes its root") from error
        if not path.is_file() or path.is_symlink():
            raise LifecycleEvidenceError(f"fixture input is absent: {relative}")
        if stat.S_IMODE(path.stat().st_mode) != row.get("mode"):
            raise LifecycleEvidenceError(f"fixture input mode changed after admission: {relative}")
        if path.stat().st_size != row.get("size") or sha256_file(path) != row.get("sha256"):
            raise LifecycleEvidenceError(f"fixture input changed after admission: {relative}")
        observed[relative] = row["sha256"]
    exact = {
        "rootfs/bin/busybox": inputs.get("busybox_sha256"),
        "compose.yaml": inputs.get("compose_sha256"),
        "Dockerfile": inputs.get("dockerfile_sha256"),
        "lifecycle.sh": inputs.get("lifecycle_sha256"),
    }
    if any(observed.get(path) != digest for path, digest in exact.items()):
        raise LifecycleEvidenceError("fixture headline hashes cross the exact manifest")


def _validate_artifacts(document: dict[str, Any], evidence_root: Path) -> None:
    rows = _list(document.get("artifacts"), "artifacts")
    if not rows:
        raise LifecycleEvidenceError("PASS evidence has no artifact manifest")
    observed: set[str] = set()
    for index, value in enumerate(rows):
        row = _mapping(value, f"artifacts[{index}]")
        relative = row.get("path")
        if not isinstance(relative, str) or not relative or relative.startswith("/"):
            raise LifecycleEvidenceError(f"artifacts[{index}] has an invalid path")
        path = (evidence_root / relative).resolve()
        try:
            path.relative_to(evidence_root.resolve())
        except ValueError as error:
            raise LifecycleEvidenceError(f"artifact escapes evidence root: {relative}") from error
        if relative in observed:
            raise LifecycleEvidenceError(f"duplicate artifact path: {relative}")
        observed.add(relative)
        if not path.is_file() or path.is_symlink():
            raise LifecycleEvidenceError(f"artifact is absent or not regular: {relative}")
        if row.get("size") != path.stat().st_size:
            raise LifecycleEvidenceError(f"artifact size mismatch: {relative}")
        if _require_hash(row.get("sha256"), f"artifact {relative} hash") != sha256_file(path):
            raise LifecycleEvidenceError(f"artifact digest mismatch: {relative}")
    expected = {
        path.relative_to(evidence_root).as_posix()
        for path in evidence_root.rglob("*")
        if path.is_file() and not path.is_symlink() and path.name != "evidence.json"
    }
    if observed != expected:
        raise LifecycleEvidenceError(
            "artifact manifest does not cover the exact evidence root: "
            f"missing={sorted(expected - observed)} extra={sorted(observed - expected)}"
        )


def _manifest_identity(manifest: dict[str, Any]) -> tuple[Any, ...]:
    handle = _mapping(manifest.get("handle"), "manifest.handle")
    plan = _mapping(manifest.get("provision_network_plan"), "manifest.provision_network_plan")
    return (
        handle.get("tenant_id"),
        handle.get("id"),
        handle.get("name"),
        _mapping(plan.get("network_plan"), "manifest.network_plan").get("generation"),
    )


def _saga_identity(saga: dict[str, Any]) -> tuple[Any, ...]:
    return (
        saga.get("sagaId"),
        saga.get("tenantId"),
        saga.get("workloadId"),
        saga.get("desiredGeneration"),
        saga.get("desiredDigest"),
    )


def _observed_saga_exact(
    saga: dict[str, Any],
    manifest: dict[str, Any],
    *,
    restart_epoch: int,
    automatic_restart_count: int,
) -> bool:
    handle = _mapping(manifest.get("handle"), "manifest.handle")
    detail = _mapping(saga.get("phaseDetail"), "saga.phaseDetail")
    if detail.get("kind") != "provision":
        return False
    value = _mapping(detail.get("value"), "saga.phaseDetail.value")
    references = _mapping(value.get("references"), "saga.references")
    execution = _mapping(references.get("execution"), "saga.execution")
    publication = _mapping(references.get("publication"), "saga.publication")
    observations = _list(value.get("observations"), "saga.observations")
    endpoints = _list(publication.get("endpoints"), "saga.publication.endpoints")
    restart = _mapping(saga.get("restartState"), "saga.restartState")
    disposition = _mapping(saga.get("provisionDisposition"), "saga.provisionDisposition")
    plan = _mapping(
        _mapping(manifest.get("provision_network_plan"), "manifest.provision_network_plan").get(
            "network_plan"
        ),
        "manifest.network_plan",
    )
    desired_generation = saga.get("desiredGeneration")
    return (
        saga.get("tenantId") == handle.get("tenant_id")
        and saga.get("workloadId") == handle.get("name")
        and saga.get("desiredState") == "running"
        and saga.get("phase") == "observed"
        and disposition.get("kind") == "ready"
        and [
            _mapping(row, f"saga.observations[{index}]").get("kind")
            for index, row in enumerate(observations)
        ]
        == list(EXPECTED_PROVISION_OBSERVATIONS)
        and execution.get("executionId") == handle.get("id")
        and execution.get("attemptId") == manifest.get("execution_attempt_id")
        and execution.get("restartEpoch") == str(restart_epoch)
        and execution.get("generation") == desired_generation
        and str(plan.get("generation")) == desired_generation
        and publication.get("execution") == execution
        and len(endpoints) == 1
        and isinstance(endpoints[0], str)
        and restart.get("currentExecutionAttemptId") == execution.get("attemptId")
        and restart.get("completedRestartEpoch") == str(restart_epoch)
        and restart.get("completedAutomaticRestartCount") == automatic_restart_count
        and restart.get("active") is None
    )


def teardown_order_exact(history: Any, source_saga: dict[str, Any]) -> bool:
    """Authenticate the durable publication-withdrawal-before-stop prefix."""

    if not isinstance(history, dict):
        return False
    entries = history.get("entries")
    if (
        not isinstance(entries, list)
        or not entries
        or history.get("saga_id") != source_saga.get("sagaId")
        or history.get("tenant_id") != source_saga.get("tenantId")
        or history.get("workload_id") != source_saga.get("workloadId")
    ):
        return False
    try:
        references = _mapping(
            _mapping(
                _mapping(source_saga.get("phaseDetail"), "source_saga.phaseDetail").get("value"),
                "source_saga.phaseDetail.value",
            ).get("references"),
            "source_saga.references",
        )
        publication = _mapping(references.get("publication"), "source_saga.publication")
        execution = _mapping(references.get("execution"), "source_saga.execution")
    except LifecycleEvidenceError:
        return False

    prior_sequence = -1
    publication_sequence: int | None = None
    stop_sequence: int | None = None
    final_kinds: list[str] = []
    prior_observations: list[dict[str, Any]] = []
    for raw_entry in entries:
        if not isinstance(raw_entry, dict):
            return False
        sequence = raw_entry.get("commit_sequence")
        observations = raw_entry.get("terminal_observations")
        if (
            not isinstance(sequence, int)
            or isinstance(sequence, bool)
            or sequence <= prior_sequence
            or not isinstance(observations, list)
        ):
            return False
        prior_sequence = sequence
        kinds: list[str] = []
        for observation in observations:
            if not isinstance(observation, dict) or not isinstance(
                observation.get("kind"), str
            ):
                return False
            kind = observation["kind"]
            kinds.append(kind)
            if kind == "publication_absent":
                if observation.get("reference") != publication:
                    return False
                publication_sequence = publication_sequence or sequence
            elif kind in {"execution_drained", "execution_stopped"}:
                if observation.get("reference") != execution:
                    return False
                if kind == "execution_stopped":
                    stop_sequence = stop_sequence or sequence
        if kinds != list(EXPECTED_TERMINAL_OBSERVATIONS[: len(kinds)]):
            return False
        if observations[: len(prior_observations)] != prior_observations:
            return False
        prior_observations = observations
        final_kinds = kinds
    return (
        publication_sequence is not None
        and stop_sequence is not None
        and publication_sequence < stop_sequence
        and final_kinds == list(EXPECTED_TERMINAL_OBSERVATIONS)
    )
def _derive_attempt(attempt: dict[str, Any]) -> dict[str, bool]:
    transitions = attempt.get("transitions")
    exact_transitions = transitions == list(REQUIRED_TRANSITIONS)
    namespace_view = _mapping(attempt.get("namespace_view"), "attempt.namespace_view")
    control = _mapping(attempt.get("control"), "attempt.control")
    control_assertions = _mapping(
        control.get("assertions"), "attempt.control.assertions"
    )
    control_counters = _mapping(control.get("counters"), "attempt.control.counters")
    reset = _mapping(attempt.get("reset"), "attempt.reset")
    reset_counters = _mapping(reset.get("counters"), "attempt.reset.counters")
    profile = _mapping(attempt.get("profile"), "attempt.profile")
    lifecycle = _mapping(attempt.get("lifecycle"), "attempt.lifecycle")
    first_manifest = _mapping(lifecycle.get("first_manifest"), "first_manifest")
    restart_manifest = _mapping(lifecycle.get("restart_manifest"), "restart_manifest")
    fresh_manifest = _mapping(lifecycle.get("fresh_manifest"), "fresh_manifest")
    first_saga = _mapping(lifecycle.get("first_saga"), "first_saga")
    restart_saga = _mapping(lifecycle.get("restart_saga"), "restart_saga")
    stable_identity = (
        _manifest_identity(first_manifest)
        == _manifest_identity(restart_manifest)
        == _manifest_identity(fresh_manifest)
    )
    attempt_changed = (
        first_manifest.get("execution_attempt_id")
        != restart_manifest.get("execution_attempt_id")
        and restart_manifest.get("execution_attempt_id")
        == fresh_manifest.get("execution_attempt_id")
    )
    counters = _mapping(profile.get("counters"), "profile.counters")
    return {
        "namespace_view": (
            namespace_view.get("passed") is True
            and namespace_view.get("host") in {"cgroup", "cgroup2fs"}
            and namespace_view.get("subject") == namespace_view.get("host")
        ),
        "control": (
            control.get("passed") is True
            and set(control_assertions) == REQUIRED_CONTROL_ASSERTIONS
            and all(value is True for value in control_assertions.values())
            and control_counters == EXPECTED_CONTROL_COUNTERS
        ),
        "reset": (
            reset.get("passed") is True and reset_counters == EXPECTED_ZERO_COUNTERS
        ),
        "provider": lifecycle.get("provider_exact") is True,
        "readiness": (
            lifecycle.get("readiness_exact") is True
            and _observed_saga_exact(
                first_saga,
                first_manifest,
                restart_epoch=0,
                automatic_restart_count=0,
            )
        ),
        "serve": lifecycle.get("first_http", {}).get("passed") is True,
        "restart": (
            lifecycle.get("restart_count") == 1
            and lifecycle.get("second_http", {}).get("passed") is True
            and stable_identity
            and attempt_changed
            and _saga_identity(first_saga) == _saga_identity(restart_saga)
            and _observed_saga_exact(
                restart_saga,
                restart_manifest,
                restart_epoch=1,
                automatic_restart_count=1,
            )
        ),
        "fresh_owner": (
            lifecycle.get("fresh_owner_exact") is True
            and lifecycle.get("fresh_http", {}).get("passed") is True
            and stable_identity
        ),
        "withdrawal": (
            lifecycle.get("retirement_exit") == 0
            and lifecycle.get("retired_http", {}).get("passed") is True
            and teardown_order_exact(
                lifecycle.get("terminal_saga_history"), restart_saga
            )
        ),
        "cleanup": (
            lifecycle.get("terminal_projection_exact") is True
            and attempt.get("cleanup", {}).get("passed") is True
        ),
        "network_quiet": (
            counters
            and all(value == 0 for value in counters.values())
            and profile.get("dns_capture") == ""
            and profile.get("forbidden_trace_addresses") == []
        ),
        "transitions": exact_transitions,
    }


def derive_assertions(document: dict[str, Any]) -> list[dict[str, Any]]:
    runner = _mapping(document.get("runner"), "runner")
    source = _mapping(document.get("source"), "source")
    inputs = _mapping(document.get("inputs"), "inputs")
    attempts = [_mapping(value, "attempt") for value in _list(document.get("attempts"), "attempts")]
    derived = [_derive_attempt(attempt) for attempt in attempts]
    hashes = [
        source.get("harness_sha256"),
        inputs.get("nimbus_sha256"),
        inputs.get("busybox_sha256"),
        inputs.get("compose_sha256"),
        inputs.get("dockerfile_sha256"),
        inputs.get("lifecycle_sha256"),
    ]
    k1 = (
        all(
            isinstance(source.get(key), str)
            and GIT_OBJECT_PATTERN.fullmatch(source[key])
            for key in ("commit", "tree")
        )
        and all(
            isinstance(value, str) and HASH_PATTERN.fullmatch(value)
            for value in hashes
        )
        and inputs.get("fixture_executable") is True
    )
    k2 = (
        runner.get("admitted") is True
        and runner.get("uid") == 0
        and runner.get("kvm_access") is True
        and not runner.get("missing_tools")
    )
    k3 = inputs.get("offline_after_admission") is True
    two_attempts = len(attempts) == 2
    values = {
        "K1": k1,
        "K2": k2,
        "K3": k3,
        "K4": two_attempts
        and all(
            row["namespace_view"] and row["control"] and row["reset"]
            for row in derived
        ),
        "K5": two_attempts and all(row["provider"] for row in derived),
        "K6": two_attempts and all(row["readiness"] for row in derived),
        "K7": two_attempts and all(row["serve"] for row in derived),
        "K8": two_attempts and all(row["restart"] for row in derived),
        "K9": two_attempts and all(row["fresh_owner"] for row in derived),
        "K10": two_attempts and all(row["withdrawal"] for row in derived),
        "K11": two_attempts and all(row["cleanup"] for row in derived),
        "K12": two_attempts and all(row["network_quiet"] for row in derived),
        "K13": two_attempts and all(row["transitions"] for row in derived),
        "K14": (
            two_attempts
            and attempts[0].get("durable_root") != attempts[1].get("durable_root")
            and attempts[0].get("cleanup", {}).get("passed") is True
            and _timestamp(attempts[0].get("finished_at"), "attempt 1 finish")
            <= _timestamp(attempts[1].get("started_at"), "attempt 2 start")
            and inputs.get("repeat") == 2
            and document.get("self_tests", {}).get("passed")
            == document.get("self_tests", {}).get("required")
            == 8
        ),
    }
    return [
        {"id": identity, "expected": True, "observed": value, "passed": value is True}
        for identity, value in sorted(values.items(), key=lambda item: int(item[0][1:]))
    ]


def validate_evidence(
    document: dict[str, Any], *, evidence_root: Path | None = None
) -> None:
    if document.get("schema_version") != SCHEMA_VERSION:
        raise LifecycleEvidenceError("unsupported lifecycle evidence schema")
    result = _mapping(document.get("result"), "result")
    started = _timestamp(result.get("started_at"), "result.started_at")
    finished = _timestamp(result.get("finished_at"), "result.finished_at")
    if started > finished:
        raise LifecycleEvidenceError("result interval is reversed")
    status = result.get("status")
    if status not in {"PASS", "FAIL", "SKIPPED"}:
        raise LifecycleEvidenceError("result status is invalid")
    expected_exit = {"PASS": 0, "FAIL": 1, "SKIPPED": 77}[status]
    if result.get("exit_code") != expected_exit:
        raise LifecycleEvidenceError("result status and exit code disagree")
    commands = _list(document.get("commands"), "commands")
    if status != "PASS":
        return
    _validate_inputs(document)
    _validate_commands(commands)
    expected = derive_assertions(document)
    if {row["id"] for row in expected} != REQUIRED_ASSERTIONS:
        raise LifecycleEvidenceError("internal K1-K14 assertion set is incomplete")
    if document.get("assertions") != expected:
        raise LifecycleEvidenceError("stored PASS assertions are not derived")
    failed = [row["id"] for row in expected if row["passed"] is not True]
    if failed:
        raise LifecycleEvidenceError("candidate PASS fails: " + ", ".join(failed))
    if evidence_root is None:
        raise LifecycleEvidenceError("PASS validation requires the evidence root")
    _validate_artifacts(document, evidence_root)


def load_and_validate(path: Path) -> dict[str, Any]:
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise LifecycleEvidenceError(f"cannot load lifecycle evidence: {error}") from error
    validate_evidence(document, evidence_root=path.parent)
    return document
