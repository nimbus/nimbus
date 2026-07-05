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
ENV_RE = re.compile(r'env!\("CARGO_MANIFEST_DIR"\)|env!\("CARGO_BIN_EXE_[^"]*"\)')
FILTER_TEST_RE = re.compile(r"test\(/((?:\\/|[^/])*)/\)")

# Existing F2 violations are slated for the B5 env! conversion batch. Baseline is
# per-file EXPECTED COUNTS (not line numbers) so unrelated edits that shift lines
# cannot false-positive the gate, while any NEW use in a listed file still trips it
# (count exceeds baseline). B5 shrinks this to empty as it converts each file.
F2_ENV_BASELINE = {
    "crates/nimbus-bin/tests/launcher.rs": 1,
    "crates/nimbus-cli/src/dev/tests/adoption.rs": 3,
    "crates/nimbus-kv/tests/spawn_harness.rs": 1,
    "crates/nimbus-proxy/src/tests/reachability_lint.rs": 1,
    "crates/nimbus-runtime/src/runtime/tests/basic_invocation/support.rs": 1,
    "crates/nimbus-runtime/src/runtime/tests/node/canary_registry.rs": 1,
    "crates/nimbus-runtime/src/runtime/tests/node/manifest_catalog.rs": 1,
    "crates/nimbus-runtime/src/runtime/tests/node/mod.rs": 2,
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
            errors.append(f"{label}: invalid scope {row.scope!r} (allowed: filter, ignored)")
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


def exclusion_matches_test(row: Exclusion, test: RustTest) -> bool:
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


def canonical_test_id(root: Path, path: Path, name: str) -> str:
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
    return "::".join([*module_parts, name]) if module_parts else name


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
        try:
            lines = path.read_text(encoding="utf-8").splitlines()
        except UnicodeDecodeError:
            lines = path.read_text(errors="ignore").splitlines()
        for lineno, line in enumerate(lines, start=1):
            stripped = line.strip()
            if pending_attrs and _attr_open(pending_attrs[-1][1]):
                start, text = pending_attrs[-1]
                pending_attrs[-1] = (start, text + " " + stripped)
                continue
            if stripped.startswith("#["):
                pending_attrs.append((lineno, stripped))
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
                            canonical_id=canonical_test_id(root, path, name),
                        )
                    )
                pending_attrs = []
            elif stripped and not stripped.startswith("//") and not stripped.startswith("#!["):
                pending_attrs = []
    return sorted(tests, key=lambda item: (item.path, item.line, item.name))


def is_test_tree(path: Path, root: Path) -> bool:
    parts = path.relative_to(root).parts
    if "tests" in parts:
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


def ignored_without_ledger(tests: Sequence[RustTest], exclusions: Sequence[Exclusion]) -> list[str]:
    errors: list[str] = []
    for test in tests:
        if test.ignored and not any(exclusion_matches_test(row, test) for row in exclusions):
            errors.append(f"{test.path}:{test.line}: #[ignore] test {test.canonical_id} has no exclusions ledger row")
    return errors


def check_taxonomy(
    *,
    exclusions: Sequence[Exclusion],
    nextest_config_text: str,
    tests: Sequence[RustTest],
    env_violations: Sequence[str],
    today: dt.date,
) -> list[str]:
    errors = validate_exclusions(exclusions, today)
    for index, row in enumerate(exclusions, start=1):
        label = f"exclusion row {index} ({row.pattern})"
        if row.scope == "filter":
            errors.append(
                f"{label}: scope=filter rows are gated until the B2 authoritative "
                "nextest-list validator lands; use scope=ignored or scope=profile"
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


def cmd_check(_args: argparse.Namespace) -> int:
    exclusions = load_exclusions(LEDGER_PATH.read_text(encoding="utf-8"))
    tests = scan_rust_tests(REPO_ROOT)
    errors = check_taxonomy(
        exclusions=exclusions,
        nextest_config_text=NEXTEST_CONFIG_PATH.read_text(encoding="utf-8"),
        tests=tests,
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
