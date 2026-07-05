#!/usr/bin/env python3
"""Maintain Nimbus test taxonomy metadata and nextest config fragments."""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import re
import sys
import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Sequence


REPO_ROOT = Path(__file__).resolve().parents[1]
LEDGER_PATH = REPO_ROOT / "tests" / "taxonomy" / "exclusions.toml"
NEXTEST_CONFIG_PATH = REPO_ROOT / ".config" / "nextest.toml"
CASE_MATRIX_PATH = REPO_ROOT / "docs" / "private" / "testing" / "case-matrix.toml"
NEXTEST_INVENTORY_PATH = REPO_ROOT / "target" / "test-inventory" / "nextest-list.json"
RUST_RECONCILIATION_PATH = (
    REPO_ROOT / "docs" / "private" / "testing" / "inventory" / "rust-reconciliation.md"
)

GENERATED_BEGIN = "# BEGIN GENERATED: test-taxonomy exclusions"
GENERATED_END = "# END GENERATED: test-taxonomy exclusions"

VALID_REASONS = {
    "duration-outlier",
    "serial",
    "privileged",
    "heavy-resource",
    "external-service",
    "platform-specific",
    "flaky-quarantine",
}

TEST_ATTR_RE = re.compile(r"#\[(?:[\w:]+::)?test(?:\(|\])")
IGNORE_ATTR_RE = re.compile(r"#\[\s*ignore(?:\s*=|\s*\])")
FN_RE = re.compile(r"(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\b")
MOD_RE = re.compile(r"(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*\{")
ENV_RE = re.compile(r'env!\("CARGO_MANIFEST_DIR"\)|env!\("CARGO_BIN_EXE_[^"]*"\)')
FILTER_TEST_RE = re.compile(r"test\(/((?:\\/|[^/])*)/\)")
# F5: evidence for scope=filter rows must cite a MEASURED duration (number+unit)
# or a NAMED lane — bare words like "duration"/"lane" do not pass.
EVIDENCE_DURATION_RE = re.compile(
    r"\b[0-9]+(?:\.[0-9]+)?\s*(?:ms|s|sec|secs|second|seconds|min|minutes?)\b",
    re.IGNORECASE,
)
EVIDENCE_LANE_RE = re.compile(
    r"\b(?:ci-pr|ci-nightly|ci-runtime|ci-harness-[a-z]+|external-provider|"
    r"node-compat|cage|coverage|nightly)\s+lane\b|\blane\s*[:=]?\s*"
    r"(?:ci-pr|ci-nightly|ci-runtime|ci-harness-[a-z]+|external-provider|"
    r"node-compat|cage|coverage|nightly)\b",
    re.IGNORECASE,
)

# F1: scope=filter validation must never run against a platform-wrong or
# unversioned inventory. The canonical inventory is generated in the ubuntu CI
# environment (B4 archive job) alongside this meta file.
INVENTORY_META_PATH = REPO_ROOT / "target" / "test-inventory" / "nextest-list.meta.json"
CANONICAL_INVENTORY_PLATFORM = "x86_64-unknown-linux-gnu"
REQUIRED_NEXTEST_VERSION = "0.9.138"

MODULE_ALIASES = (
    ("runtime::tests::node::", "runtime::tests::node_compat::"),
)

# Existing F2 violations are slated for the B5 env! conversion batch. Baseline is
# per-file EXPECTED COUNTS (not line numbers) so unrelated edits that shift lines
# cannot false-positive the gate, while any NEW use in a listed file still trips it
# (count exceeds baseline). B5 shrinks this to empty as it converts each file.
F2_ENV_BASELINE = {
    "crates/nimbus-bin/tests/launcher.rs": 1,
    "crates/nimbus-cli/src/dev/tests/adoption.rs": 3,
    "crates/nimbus-cli/src/start/tests.rs": 1,
    "crates/nimbus-kv/tests/spawn_harness.rs": 1,
    "crates/nimbus-proxy/src/tests/reachability_lint.rs": 1,
    "crates/nimbus-runtime/src/runtime/tests/basic_invocation/support.rs": 1,
    "crates/nimbus-runtime/src/runtime/tests/node/canary_registry.rs": 1,
    "crates/nimbus-runtime/src/runtime/tests/node/manifest_catalog.rs": 1,
    "crates/nimbus-runtime/src/runtime/tests/node/mod.rs": 2,
    "crates/nimbus-runtime/src/test_support/isolation.rs": 2,
    "crates/nimbus-server/src/tests/firebase/rest_crud.rs": 1,
    "crates/nimbus-server/src/tests/rest_route_parity.rs": 1,
    "crates/nimbus-server/src/tests/tls_serve.rs": 1,
    # Inline #[cfg(test)] modules outside tests/ trees (F4 review finding):
    "crates/nimbus-assets/src/js_packages.rs": 1,
    "crates/nimbus-cli/src/codegen.rs": 1,
    "crates/nimbus-cli/src/deploy.rs": 1,
    "crates/nimbus-cli/src/dev/redetect.rs": 1,
    "crates/nimbus-cli/src/typeinfo.rs": 1,
    "crates/nimbus-operator/src/token.rs": 1,
    "crates/nimbus-server/src/adapters/cloud_functions/execution.rs": 1,
    "crates/nimbus-server/src/adapters/cloud_functions/http.rs": 1,
    "crates/nimbus-server/src/router.rs": 1,
    "crates/nimbus-server/src/tests.rs": 1,
    "crates/nimbus-server/src/tls.rs": 1,
}


VALID_SCOPES = {
    # Row enters the generated ci-pr default-filter (excludes tests that would
    # otherwise RUN on PR). Requires per-test measured evidence. GATED: until the
    # B2 authoritative nextest-list validator lands, `check` fails on any
    # scope=filter row so unvalidatable exclusions cannot be introduced.
    "filter",
    # Row documents an #[ignore] family for the ignored-without-ledger gate only.
    # Ignored tests never run under a default profile, so these rows must NOT
    # enter the generated filter — keeping PR default-include exact.
    "ignored",
    # Row documents a NON-ignored exclusion enforced by a hand-written profile
    # default-filter in .config/nextest.toml (e.g. pool_reuse::isol_ excluded by
    # ci-runtime and run in the dedicated cage lane). `check` verifies the
    # pattern text actually appears in the config so the row cannot go stale.
    "profile",
}

# KNOWN LIMITATION (tracked as a B2 obligation): canonical test IDs here are
# derived from source paths, not rustc module resolution. `#[path = ...]` module
# aliasing (e.g. nimbus-runtime's `#[path = "node/mod.rs"] mod node_compat`)
# means scanner IDs and nextest IDs can differ. B2 replaces this heuristic with
# `cargo nextest list --message-format json` as the authoritative source; until
# then only scope=ignored/profile rows exist and the generated filter is all(),
# so the aliasing cannot change what runs on PR.


@dataclass(frozen=True)
class Exclusion:
    pattern: str
    reason: str
    evidence: str
    measured_at: str
    owner: str
    issue: str
    scope: str = "filter"
    expiry: str | None = None


@dataclass(frozen=True)
class RustTest:
    path: str
    line: int
    name: str
    crate: str
    ignored: bool
    canonical_id: str


@dataclass(frozen=True)
class NextestTest:
    path: str
    line: int
    name: str
    crate: str
    ignored: bool
    canonical_id: str
    binary_name: str
    kind: str


def load_exclusions(text: str) -> list[Exclusion]:
    data = tomllib.loads(text)
    rows = data.get("exclusions", [])
    if not isinstance(rows, list):
        raise ValueError("exclusions must be an array of tables")

    exclusions: list[Exclusion] = []
    for index, row in enumerate(rows, start=1):
        if not isinstance(row, dict):
            raise ValueError(f"exclusion row {index} must be a table")
        missing = [
            key
            for key in ("pattern", "reason", "evidence", "measured_at", "owner", "issue")
            if not row.get(key)
        ]
        if missing:
            raise ValueError(f"exclusion row {index} missing required field(s): {', '.join(missing)}")
        exclusions.append(
            Exclusion(
                pattern=str(row["pattern"]),
                reason=str(row["reason"]),
                evidence=str(row["evidence"]),
                measured_at=str(row["measured_at"]),
                owner=str(row["owner"]),
                issue=str(row["issue"]),
                scope=str(row.get("scope", "filter")),
                expiry=str(row["expiry"]) if row.get("expiry") else None,
            )
        )
    return exclusions


def generate_nextest_section(exclusions: Sequence[Exclusion]) -> str:
    filter_rows = [row for row in exclusions if row.scope == "filter"]
    if filter_rows:
        expression = "not (" + " or ".join(row.pattern for row in filter_rows) + ")"
    else:
        expression = "all()"
    return "\n".join(
        [
            GENERATED_BEGIN,
            f"default-filter = {expression!r}",
            GENERATED_END,
            "",
        ]
    )


def extract_generated_section(config_text: str) -> str | None:
    begin = config_text.find(GENERATED_BEGIN)
    end = config_text.find(GENERATED_END)
    if begin == -1 or end == -1 or end < begin:
        return None
    end += len(GENERATED_END)
    if end < len(config_text) and config_text[end] == "\n":
        end += 1
    return config_text[begin:end]


def validate_exclusions(exclusions: Sequence[Exclusion], today: dt.date) -> list[str]:
    errors: list[str] = []
    seen: set[str] = set()
    for index, row in enumerate(exclusions, start=1):
        label = f"exclusion row {index} ({row.pattern})"
        if row.pattern in seen:
            errors.append(f"{label}: duplicate pattern")
        seen.add(row.pattern)
        if row.reason not in VALID_REASONS:
            errors.append(f"{label}: invalid reason {row.reason!r}")
        if row.scope not in VALID_SCOPES:
            errors.append(f"{label}: invalid scope {row.scope!r} (allowed: filter, ignored, profile)")
        if not row.evidence.strip():
            errors.append(f"{label}: evidence is required")
        if not row.measured_at.strip():
            errors.append(f"{label}: measured_at is required")
        else:
            try:
                dt.date.fromisoformat(row.measured_at)
            except ValueError:
                errors.append(f"{label}: measured_at must be YYYY-MM-DD")
        if not row.owner.strip():
            errors.append(f"{label}: owner is required")
        if not row.issue.strip():
            errors.append(f"{label}: issue is required")
        if row.reason == "flaky-quarantine":
            if not row.expiry:
                errors.append(f"{label}: flaky-quarantine rows require expiry")
            else:
                try:
                    expiry = dt.date.fromisoformat(row.expiry)
                except ValueError:
                    errors.append(f"{label}: expiry must be YYYY-MM-DD")
                else:
                    if expiry < today:
                        errors.append(f"{label}: flaky quarantine expired on {expiry.isoformat()}")
    return errors


def filter_regexes(pattern: str) -> list[re.Pattern[str]]:
    regexes: list[re.Pattern[str]] = []
    for raw in FILTER_TEST_RE.findall(pattern):
        normalized = raw.replace("\\/", "/")
        try:
            regexes.append(re.compile(normalized))
        except re.error as error:
            raise ValueError(f"invalid test regex /{normalized}/ in pattern {pattern!r}: {error}")
    return regexes


def exclusion_matches_test(row: Exclusion, test: RustTest | NextestTest) -> bool:
    candidates = [
        test.canonical_id,
        test.name,
        test.path,
        f"{test.path}::{test.name}",
    ]
    for regex in filter_regexes(row.pattern):
        if any(regex.search(candidate) for candidate in candidates):
            return True
    return False


def load_nextest_tests_from_json(text: str) -> list[NextestTest]:
    start_match = re.search(r"^\{", text, re.MULTILINE)
    if not start_match:
        raise ValueError("nextest JSON does not contain a JSON object")
    data = json.loads(text[start_match.start() :])
    tests: list[NextestTest] = []
    for suite in data.get("rust-suites", {}).values():
        package = str(suite.get("package-name") or suite.get("package_name") or "")
        binary_name = str(suite.get("binary-name") or suite.get("binary_name") or "")
        kind = str(suite.get("kind") or "")
        suite_path = str(suite.get("binary-path") or suite.get("binary_path") or "")
        for name, case in suite.get("testcases", {}).items():
            tests.append(
                NextestTest(
                    path=suite_path,
                    line=0,
                    name=str(name),
                    crate=package,
                    ignored=bool(case.get("ignored", False)),
                    canonical_id=str(name),
                    binary_name=binary_name,
                    kind=kind,
                )
            )
    return sorted(tests, key=lambda item: (item.crate, item.binary_name, item.canonical_id))


def read_nextest_tests(path: Path) -> list[NextestTest]:
    return load_nextest_tests_from_json(path.read_text(encoding="utf-8"))


def rust_files(root: Path) -> list[Path]:
    ignored_dirs = {".git", "target", "node_modules", ".nextest", "third_party"}
    files: list[Path] = []
    for path in root.rglob("*.rs"):
        if any(part in ignored_dirs for part in path.parts):
            continue
        files.append(path)
    return sorted(files)


def crate_name(path: Path) -> str:
    parts = path.parts
    if "crates" in parts:
        index = parts.index("crates")
        if index + 1 < len(parts):
            return parts[index + 1]
    return "workspace"


def canonical_test_id(
    root: Path,
    path: Path,
    name: str,
    inline_modules: Sequence[str] = (),
) -> str:
    rel = path.relative_to(root)
    parts = list(rel.parts)
    module_parts: list[str]
    if "src" in parts:
        src_index = parts.index("src")
        module_parts = parts[src_index + 1 :]
    elif "tests" in parts:
        tests_index = parts.index("tests")
        module_parts = parts[tests_index + 1 :]
    else:
        module_parts = [parts[-1]]

    if module_parts:
        module_parts[-1] = module_parts[-1].removesuffix(".rs")
    module_parts = [part for part in module_parts if part not in {"lib", "main", "mod"}]
    module_parts.extend(inline_modules)
    return "::".join([*module_parts, name]) if module_parts else name


def _brace_delta(text: str) -> int:
    delta, _ = _brace_delta_with_raw_state(text, _LexState())
    return delta


@dataclass
class _LexState:
    """Cross-line lexer state for the brace tracker (F6 review finding)."""

    raw_terminator: str | None = None
    comment_depth: int = 0  # Rust block comments nest


_CHAR_LITERAL_RE = re.compile(r"'(?:\\.|[^'\\])'")


def _brace_delta_with_raw_state(text: str, state: "_LexState | None") -> "tuple[int, _LexState]":
    if state is None:
        state = _LexState()
    in_string = False
    escaped = False
    delta = 0
    index = 0
    while index < len(text):
        if state.raw_terminator is not None:
            end = text.find(state.raw_terminator, index)
            if end == -1:
                return delta, state
            index = end + len(state.raw_terminator)
            state.raw_terminator = None
            continue
        if state.comment_depth > 0:
            open_at = text.find("/*", index)
            close_at = text.find("*/", index)
            if close_at == -1 and open_at == -1:
                return delta, state
            if open_at != -1 and (close_at == -1 or open_at < close_at):
                state.comment_depth += 1
                index = open_at + 2
            else:
                state.comment_depth -= 1
                index = close_at + 2
            continue
        ch = text[index]
        if not in_string and ch == "/" and index + 1 < len(text) and text[index + 1] == "/":
            break
        if not in_string and ch == "/" and index + 1 < len(text) and text[index + 1] == "*":
            state.comment_depth += 1
            index += 2
            continue
        if not in_string and ch == "'":
            literal = _CHAR_LITERAL_RE.match(text, index)
            if literal:
                index = literal.end()
                continue
            index += 1  # lifetime like 'a — no closing quote
            continue
        if not in_string and ch == "r":
            hash_index = index + 1
            while hash_index < len(text) and text[hash_index] == "#":
                hash_index += 1
            if hash_index < len(text) and text[hash_index] == '"':
                terminator = '"' + ("#" * (hash_index - index - 1))
                end = text.find(terminator, hash_index + 1)
                if end == -1:
                    state.raw_terminator = terminator
                    return delta, state
                index = end + len(terminator)
                continue
        if escaped:
            escaped = False
            index += 1
            continue
        if ch == "\\":
            escaped = True
            index += 1
            continue
        if ch == '"':
            in_string = not in_string
            index += 1
            continue
        if not in_string:
            if ch == "{":
                delta += 1
            elif ch == "}":
                delta -= 1
        index += 1
    return delta, state


def _attr_open(text: str) -> bool:
    """True while an attribute is unterminated across lines.

    Handles `#[ignore = "multi-line \
    string"]` forms: inside a string literal, brackets do not count. The
    attribute is open while quotes are unbalanced or more `[` than `]` seen.
    """
    in_string = False
    escaped = False
    depth = 0
    for ch in text:
        if escaped:
            escaped = False
            continue
        if ch == "\\":
            escaped = True
            continue
        if ch == '"':
            in_string = not in_string
            continue
        if in_string:
            continue
        if ch == "[":
            depth += 1
        elif ch == "]":
            depth -= 1
    return in_string or depth > 0


def scan_rust_tests(root: Path) -> list[RustTest]:
    tests: list[RustTest] = []
    for path in rust_files(root):
        pending_attrs: list[tuple[int, str]] = []
        module_stack: list[tuple[str, int]] = []
        brace_depth = 0
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except UnicodeDecodeError:
            lines = path.read_text(errors="ignore").splitlines()
        lex_state = _LexState()
        for lineno, line in enumerate(lines, start=1):
            while module_stack and brace_depth <= module_stack[-1][1]:
                module_stack.pop()
            stripped = line.strip()
            module_match = MOD_RE.match(stripped)
            starting_depth = brace_depth
            line_delta, lex_state = _brace_delta_with_raw_state(line, lex_state)
            if pending_attrs and _attr_open(pending_attrs[-1][1]):
                start, text = pending_attrs[-1]
                pending_attrs[-1] = (start, text + " " + stripped)
                brace_depth += line_delta
                continue
            if stripped.startswith("#["):
                pending_attrs.append((lineno, stripped))
                brace_depth += line_delta
                continue
            match = FN_RE.match(stripped)
            if match:
                attrs = " ".join(attr for _, attr in pending_attrs)
                if TEST_ATTR_RE.search(attrs):
                    name = match.group(1)
                    tests.append(
                        RustTest(
                            path=str(path.relative_to(root)),
                            line=pending_attrs[0][0] if pending_attrs else lineno,
                            name=name,
                            crate=crate_name(path),
                            ignored=bool(IGNORE_ATTR_RE.search(attrs)),
                            canonical_id=canonical_test_id(
                                root,
                                path,
                                name,
                                tuple(module for module, _ in module_stack),
                            ),
                        )
                    )
                pending_attrs = []
            elif stripped and not stripped.startswith("//") and not stripped.startswith("#!["):
                pending_attrs = []
            brace_depth += line_delta
            if module_match and brace_depth > starting_depth:
                module_stack.append((module_match.group(1), starting_depth))
    return sorted(tests, key=lambda item: (item.path, item.line, item.name))


def is_test_tree(path: Path, root: Path) -> bool:
    parts = path.relative_to(root).parts
    if "tests" in parts:
        return True
    if path.name in {"test.rs", "tests.rs"}:
        return True
    if "test_support" in parts:
        return True
    return any(parts[index] == "src" and index + 1 < len(parts) and parts[index + 1] == "tests" for index in range(len(parts)))


def find_compile_time_env_violations(
    root: Path, baseline: dict[str, int] | None = None
) -> list[str]:
    if baseline is None:
        baseline = F2_ENV_BASELINE
    counts: dict[str, int] = {}
    for path in rust_files(root):
        text = path.read_text(errors="ignore")
        # In-scope: dedicated test trees, plus any file with an inline
        # #[cfg(test)] module (F4: inline test modules outside tests/ paths).
        if not is_test_tree(path, root) and "#[cfg(test)]" not in text:
            continue
        hits = sum(1 for line in text.splitlines() if ENV_RE.search(line))
        if hits:
            counts[str(path.relative_to(root))] = hits
    violations: list[str] = []
    for rel in sorted(counts):
        allowed = baseline.get(rel, 0)
        if counts[rel] > allowed:
            violations.append(
                f"{rel}: {counts[rel]} compile-time Cargo env macro use(s) in test tree"
                f" (baseline {allowed}); use runtime env / NEXTEST_BIN_EXE_* instead (F2)"
            )
    return violations


def ignored_without_ledger(
    tests: Sequence[RustTest] | Sequence[NextestTest],
    exclusions: Sequence[Exclusion],
) -> list[str]:
    errors: list[str] = []
    for test in tests:
        if test.ignored and not any(exclusion_matches_test(row, test) for row in exclusions):
            location = f"{test.path}:{test.line}" if test.line else f"{test.crate}:{test.binary_name}"
            errors.append(f"{location}: #[ignore] test {test.canonical_id} has no exclusions ledger row")
    return errors


def evidence_cites_duration_or_lane(evidence: str) -> bool:
    return bool(EVIDENCE_DURATION_RE.search(evidence) or EVIDENCE_LANE_RE.search(evidence))


def check_taxonomy(
    *,
    exclusions: Sequence[Exclusion],
    nextest_config_text: str,
    tests: Sequence[RustTest],
    nextest_tests: Sequence[NextestTest] | None = None,
    env_violations: Sequence[str],
    today: dt.date,
) -> list[str]:
    errors = validate_exclusions(exclusions, today)
    for index, row in enumerate(exclusions, start=1):
        label = f"exclusion row {index} ({row.pattern})"
        if row.scope == "filter":
            if nextest_tests is None:
                errors.append(
                    f"{label}: scope=filter rows are gated until the B2 authoritative "
                    "nextest-list validator lands; use scope=ignored or scope=profile"
                )
            else:
                matches = [
                    test
                    for test in nextest_tests
                    if not test.ignored and exclusion_matches_test(row, test)
                ]
                if not matches:
                    errors.append(
                        f"{label}: scope=filter row matches zero real non-ignored "
                        "nextest tests in target/test-inventory/nextest-list.json"
                    )
                if not evidence_cites_duration_or_lane(row.evidence):
                    errors.append(
                        f"{label}: scope=filter evidence must cite a measured duration (number+unit) or a named lane"
                    )
        elif row.scope == "profile":
            raws = FILTER_TEST_RE.findall(row.pattern)
            if not raws:
                errors.append(f"{label}: scope=profile row has no test(/…/) pattern to verify")
            elif not any(raw.replace("\\/", "/") in nextest_config_text for raw in raws):
                errors.append(
                    f"{label}: scope=profile pattern not found in any hand-written "
                    "profile filter in .config/nextest.toml (stale row?)"
                )
    expected = generate_nextest_section(exclusions)
    actual = extract_generated_section(nextest_config_text)
    if actual is None:
        errors.append("nextest config missing generated exclusions section")
    elif actual != expected:
        errors.append("nextest generated exclusions section is out of date; run scripts/test-taxonomy.py generate-nextest")
    errors.extend(ignored_without_ledger(tests, exclusions))
    errors.extend(env_violations)
    return sorted(errors)


def inventory_report(tests: Sequence[RustTest], nextest_visible: int | None = None) -> str:
    by_crate: dict[str, dict[str, int]] = {}
    for test in tests:
        bucket = by_crate.setdefault(test.crate, {"test_attributes": 0, "ignored": 0})
        bucket["test_attributes"] += 1
        if test.ignored:
            bucket["ignored"] += 1

    lines = [
        f"test_attributes = {len(tests)}",
        f"nextest_visible = {nextest_visible if nextest_visible is not None else 'unmeasured'}",
        f"ignored = {sum(1 for test in tests if test.ignored)}",
        "per_crate:",
    ]
    for crate in sorted(by_crate):
        bucket = by_crate[crate]
        lines.append(f"  {crate}: test_attributes={bucket['test_attributes']} ignored={bucket['ignored']}")
    return "\n".join(lines) + "\n"


def nextest_visible_from_json(text: str) -> int:
    start_match = re.search(r"^\{", text, re.MULTILINE)
    if not start_match:
        raise ValueError("nextest JSON does not contain a JSON object")
    data = json.loads(text[start_match.start() :])
    visible = 0
    for suite in data.get("rust-suites", {}).values():
        for case in suite.get("testcases", {}).values():
            match = case.get("filter-match", {}).get("status")
            if match in {None, "matches"} and not case.get("ignored", False):
                visible += 1
    return visible


def aliased_scanner_id(scanner_id: str) -> str:
    for scanner_prefix, nextest_prefix in MODULE_ALIASES:
        if scanner_id.startswith(scanner_prefix):
            return nextest_prefix + scanner_id.removeprefix(scanner_prefix)
    return scanner_id


def _format_markdown_list(items: Sequence[str]) -> list[str]:
    if not items:
        return ["- None"]
    return [f"- `{item}`" for item in items]


def reconciliation_report(
    *,
    scanner_tests: Sequence[RustTest],
    nextest_tests: Sequence[NextestTest],
    exclusions: Sequence[Exclusion],
) -> str:
    scanner_by_id = {test.canonical_id: test for test in scanner_tests}
    nextest_by_id = {test.canonical_id: test for test in nextest_tests}
    mapped_scanner_ids = {
        scanner_id: aliased_scanner_id(scanner_id)
        for scanner_id in scanner_by_id
    }
    aliased = [
        (scanner_id, mapped_id)
        for scanner_id, mapped_id in mapped_scanner_ids.items()
        if scanner_id != mapped_id and mapped_id in nextest_by_id
    ]
    scanner_only = [
        scanner_id
        for scanner_id, mapped_id in mapped_scanner_ids.items()
        if mapped_id not in nextest_by_id
    ]
    nextest_only = [
        nextest_id
        for nextest_id in nextest_by_id
        if nextest_id not in set(mapped_scanner_ids.values())
    ]
    ignored_disagreements = []
    for scanner_id, mapped_id in mapped_scanner_ids.items():
        if mapped_id not in nextest_by_id:
            continue
        scanner = scanner_by_id[scanner_id]
        nextest = nextest_by_id[mapped_id]
        if scanner.ignored != nextest.ignored:
            ignored_disagreements.append(
                f"{scanner_id} -> {mapped_id}: scanner ignored={scanner.ignored}, "
                f"nextest ignored={nextest.ignored}"
            )
    stale_rows = []
    for index, row in enumerate(exclusions, start=1):
        real_matches = [test for test in nextest_tests if exclusion_matches_test(row, test)]
        if not real_matches:
            stale_rows.append(
                f"row {index} scope={row.scope} reason={row.reason}: {row.pattern}"
            )

    node_scanner_ids = [
        scanner_id
        for scanner_id in scanner_by_id
        if scanner_id.startswith("runtime::tests::node::")
    ]
    unresolved_aliases = [
        scanner_id
        for scanner_id in node_scanner_ids
        if aliased_scanner_id(scanner_id) not in nextest_by_id
    ]

    lines = [
        "# Rust Test Inventory Reconciliation",
        "",
        "Source of truth: `target/test-inventory/nextest-list.json` generated with "
        "`cargo nextest list --workspace --run-ignored all --message-format json` "
        "(or the same list invocation with `--override-version-check` when a local "
        "nextest binary is older than the repo-required version).",
        "",
        "## Counts",
        "",
        f"- scanner test attributes: {len(scanner_tests)}",
        f"- scanner ignored attributes: {sum(1 for test in scanner_tests if test.ignored)}",
        f"- nextest tests: {len(nextest_tests)}",
        f"- nextest ignored tests: {sum(1 for test in nextest_tests if test.ignored)}",
        f"- nextest non-ignored tests: {sum(1 for test in nextest_tests if not test.ignored)}",
        f"- scanner-only IDs after aliasing: {len(scanner_only)}",
        f"- nextest-only IDs after aliasing: {len(nextest_only)}",
        f"- ignored-status disagreements: {len(ignored_disagreements)}",
        f"- aliased scanner IDs resolved to nextest IDs: {len(aliased)}",
        f"- stale ledger rows matching zero real nextest tests: {len(stale_rows)}",
        "",
        "## `node_compat` Module Alias",
        "",
        "`crates/nimbus-runtime/src/runtime.rs` declares "
        "`#[path = \"node/mod.rs\"] mod node_compat;`. The source scanner sees "
        "`runtime::tests::node::*`, while nextest reports "
        "`runtime::tests::node_compat::*`.",
        "",
        f"- scanner `runtime::tests::node::*` IDs: {len(node_scanner_ids)}",
        f"- aliased IDs resolved: {len(aliased)}",
        f"- aliased IDs still scanner-only: {len(unresolved_aliases)}",
        "",
        "## Scanner-Only IDs",
        "",
        *_format_markdown_list(sorted(scanner_only)),
        "",
        "## Nextest-Only IDs",
        "",
        *_format_markdown_list(sorted(nextest_only)),
        "",
        "## Ignored-Status Disagreements",
        "",
        *_format_markdown_list(sorted(ignored_disagreements)),
        "",
        "## Stale Ledger Pattern Findings",
        "",
        *_format_markdown_list(stale_rows),
        "",
    ]
    return "\n".join(lines)


def coverage_report(exclusions: Sequence[Exclusion], tests: Sequence[RustTest]) -> str:
    by_reason: dict[str, int] = {}
    for row in exclusions:
        by_reason[row.reason] = by_reason.get(row.reason, 0) + 1
    ignored = [test for test in tests if test.ignored]
    covered_ignored = [test for test in ignored if any(exclusion_matches_test(row, test) for row in exclusions)]
    lines = [
        f"exclusions = {len(exclusions)}",
        f"ignored_tests = {len(ignored)}",
        f"ignored_tests_with_ledger_row = {len(covered_ignored)}",
        "by_reason:",
    ]
    for reason in sorted(by_reason):
        lines.append(f"  {reason}: {by_reason[reason]}")
    return "\n".join(lines) + "\n"


def validate_case_matrix_text(text: str) -> list[str]:
    data = tomllib.loads(text)
    surfaces = data.get("surfaces", [])
    if not isinstance(surfaces, list):
        return ["case matrix: surfaces must be an array of tables"]
    errors: list[str] = []
    for index, surface in enumerate(surfaces, start=1):
        if not isinstance(surface, dict):
            errors.append(f"case matrix surface {index}: must be a table")
            continue
        name = str(surface.get("surface") or f"surface {index}")
        cases = surface.get("cases", [])
        if not isinstance(cases, list) or not cases:
            errors.append(f"case matrix {name}: must declare at least one case")
            continue
        seen_classes: set[str] = set()
        for case_index, case in enumerate(cases, start=1):
            if not isinstance(case, dict):
                errors.append(f"case matrix {name} case {case_index}: must be a table")
                continue
            case_class = str(case.get("class") or "")
            if not case_class:
                errors.append(f"case matrix {name} case {case_index}: class is required")
            seen_classes.add(case_class)
            tests = case.get("tests", [])
            has_tests = isinstance(tests, list) and len(tests) > 0
            has_tracking = bool(case.get("issue") or case.get("gap"))
            if not has_tests and not has_tracking:
                errors.append(f"case matrix {name} {case_class or case_index}: empty tests require issue or gap")
        if surface.get("mission_critical", False):
            for required in ("error", "recovery"):
                if required not in seen_classes:
                    errors.append(f"case matrix {name}: mission-critical surface missing {required} case")
    return sorted(errors)


def run_case_matrix_check(path: Path) -> tuple[int, str, str]:
    if not path.exists():
        try:
            display = path.relative_to(REPO_ROOT)
        except ValueError:
            display = path
        return (0, "", f"warning: {display} is absent; case-matrix-check is a no-op for B1\n")
    errors = validate_case_matrix_text(path.read_text(encoding="utf-8"))
    if errors:
        return (1, "", "\n".join(errors) + "\n")
    return (0, "case matrix ok\n", "")


def read_nextest_visible(path: Path | None) -> int | None:
    if path is None:
        return None
    return nextest_visible_from_json(path.read_text(encoding="utf-8"))


def cmd_generate_nextest(_args: argparse.Namespace) -> int:
    print(generate_nextest_section(load_exclusions(LEDGER_PATH.read_text(encoding="utf-8"))), end="")
    return 0


def load_inventory_for_gating() -> "tuple[list[NextestTest] | None, str]":
    """Return (tests, reason). Tests are ONLY returned for a canonical inventory.

    F1 review finding: a platform-wrong or unversioned JSON must never validate
    scope=filter rows. Canonical = generated on the CI platform with the pinned
    nextest, attested by nextest-list.meta.json {platform, nextest_version}.
    """
    if not NEXTEST_INVENTORY_PATH.exists():
        return None, "inventory JSON absent"
    if not INVENTORY_META_PATH.exists():
        return None, "inventory meta absent (need nextest-list.meta.json with platform + nextest_version)"
    meta = json.loads(INVENTORY_META_PATH.read_text(encoding="utf-8"))
    platform = str(meta.get("platform") or "")
    version = str(meta.get("nextest_version") or "")
    if platform != CANONICAL_INVENTORY_PLATFORM:
        return None, f"inventory platform {platform!r} is not canonical {CANONICAL_INVENTORY_PLATFORM!r}"
    if version != REQUIRED_NEXTEST_VERSION:
        return None, f"inventory nextest_version {version!r} is not the required {REQUIRED_NEXTEST_VERSION!r}"
    return read_nextest_tests(NEXTEST_INVENTORY_PATH), "canonical"


def cmd_check(_args: argparse.Namespace) -> int:
    exclusions = load_exclusions(LEDGER_PATH.read_text(encoding="utf-8"))
    tests = scan_rust_tests(REPO_ROOT)
    nextest_tests, _gate_reason = load_inventory_for_gating()
    errors = check_taxonomy(
        exclusions=exclusions,
        nextest_config_text=NEXTEST_CONFIG_PATH.read_text(encoding="utf-8"),
        tests=tests,
        nextest_tests=nextest_tests,
        env_violations=find_compile_time_env_violations(REPO_ROOT),
        today=dt.date.today(),
    )
    matrix_status, _, matrix_err = run_case_matrix_check(CASE_MATRIX_PATH)
    if matrix_status:
        errors.extend(line for line in matrix_err.splitlines() if line)
    elif matrix_err:
        print(matrix_err, file=sys.stderr, end="")

    if errors:
        for error in errors:
            print(error, file=sys.stderr)
        return 1
    print("test taxonomy ok")
    return 0


def cmd_inventory(args: argparse.Namespace) -> int:
    if args.authoritative:
        nextest_json = args.nextest_json or NEXTEST_INVENTORY_PATH
        if not nextest_json.exists():
            print(
                f"error: authoritative inventory JSON missing at {nextest_json}",
                file=sys.stderr,
            )
            return 1
        scanner_tests = scan_rust_tests(REPO_ROOT)
        nextest_tests = read_nextest_tests(nextest_json)
        exclusions = load_exclusions(LEDGER_PATH.read_text(encoding="utf-8"))
        RUST_RECONCILIATION_PATH.parent.mkdir(parents=True, exist_ok=True)
        RUST_RECONCILIATION_PATH.write_text(
            reconciliation_report(
                scanner_tests=scanner_tests,
                nextest_tests=nextest_tests,
                exclusions=exclusions,
            ),
            encoding="utf-8",
        )
        print(f"wrote {RUST_RECONCILIATION_PATH.relative_to(REPO_ROOT)}")
        return 0
    visible = read_nextest_visible(args.nextest_json)
    print(inventory_report(scan_rust_tests(REPO_ROOT), visible), end="")
    return 0


def cmd_coverage_report(_args: argparse.Namespace) -> int:
    exclusions = load_exclusions(LEDGER_PATH.read_text(encoding="utf-8"))
    print(coverage_report(exclusions, scan_rust_tests(REPO_ROOT)), end="")
    return 0


def cmd_case_matrix_check(_args: argparse.Namespace) -> int:
    status, out, err = run_case_matrix_check(CASE_MATRIX_PATH)
    if out:
        print(out, end="")
    if err:
        print(err, file=sys.stderr, end="")
    return status


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)

    generate = subcommands.add_parser("generate-nextest", help="emit the generated nextest section")
    generate.set_defaults(func=cmd_generate_nextest)

    check = subcommands.add_parser("check", help="validate taxonomy ledger, config, and source gates")
    check.set_defaults(func=cmd_check)

    inventory = subcommands.add_parser("inventory", help="summarize Rust test inventory")
    inventory.add_argument("--nextest-json", type=Path, help="optional cargo nextest list JSON output")
    inventory.add_argument(
        "--authoritative",
        action="store_true",
        help="use nextest JSON as the source of truth and write the reconciliation report",
    )
    inventory.set_defaults(func=cmd_inventory)

    coverage = subcommands.add_parser("coverage-report", help="summarize exclusion and ignored-test coverage")
    coverage.set_defaults(func=cmd_coverage_report)

    matrix = subcommands.add_parser("case-matrix-check", help="validate docs/private/testing/case-matrix.toml when present")
    matrix.set_defaults(func=cmd_case_matrix_check)

    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
