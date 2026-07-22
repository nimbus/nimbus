#!/usr/bin/env python3
"""Remove inline Rust test modules while preserving production source lines."""

from __future__ import annotations

import argparse
import csv
import re
from pathlib import Path


INLINE_TEST_MODULE = re.compile(
    r"(?m)^[ \t]*#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\][ \t]*"
    r"(?:\r?\n[ \t]*)*(?:pub(?:\([^)]*\))?[ \t]+)?mod[ \t]+tests[ \t]*\{"
)
SYNC_TENANT_CALL = re.compile(
    r"\.create_tenant\s*\(|Engine::create_tenant(?:[^_A-Za-z0-9]|$)"
)


class VerificationError(Exception):
    """A falsifiable lifecycle-inventory contract failed."""


def _blank(result: list[str], source: str, start: int, end: int) -> None:
    for index in range(start, end):
        if source[index] not in "\r\n":
            result[index] = " "


def _rust_code_mask(source: str) -> str:
    """Mask comments and literals so only structural Rust braces remain."""

    result = list(source)
    index = 0
    length = len(source)
    while index < length:
        if source.startswith("//", index):
            end = source.find("\n", index + 2)
            end = length if end < 0 else end
            _blank(result, source, index, end)
            index = end
            continue

        if source.startswith("/*", index):
            depth = 1
            cursor = index + 2
            while cursor < length and depth:
                if source.startswith("/*", cursor):
                    depth += 1
                    cursor += 2
                elif source.startswith("*/", cursor):
                    depth -= 1
                    cursor += 2
                else:
                    cursor += 1
            if depth:
                raise ValueError("unterminated Rust block comment")
            _blank(result, source, index, cursor)
            index = cursor
            continue

        raw = re.match(r"(?:br|cr|r)(?P<hashes>#{0,255})\"", source[index:])
        if raw is not None and (index == 0 or not (source[index - 1].isalnum() or source[index - 1] == "_")):
            delimiter = '"' + raw.group("hashes")
            end = source.find(delimiter, index + raw.end())
            if end < 0:
                raise ValueError("unterminated Rust raw string")
            end += len(delimiter)
            _blank(result, source, index, end)
            index = end
            continue

        string_prefix = 2 if source.startswith(("b\"", "c\""), index) else 1
        if source[index] == '"' or string_prefix == 2:
            cursor = index + string_prefix
            escaped = False
            while cursor < length:
                character = source[cursor]
                cursor += 1
                if escaped:
                    escaped = False
                elif character == "\\":
                    escaped = True
                elif character == '"':
                    break
            else:
                raise ValueError("unterminated Rust string")
            _blank(result, source, index, cursor)
            index = cursor
            continue

        character = re.match(r"(?:b)?'(?:\\.|[^\\'\r\n])'", source[index:])
        if character is not None:
            end = index + character.end()
            _blank(result, source, index, end)
            index = end
            continue

        index += 1

    return "".join(result)


def production_source(source: str) -> str:
    """Blank every conventional inline `#[cfg(test)] mod tests` item."""

    code = _rust_code_mask(source)
    result = list(source)
    cursor = 0
    while match := INLINE_TEST_MODULE.search(code, cursor):
        opening = code.find("{", match.start(), match.end())
        if opening < 0:
            raise ValueError("inline test module has no opening brace")
        depth = 1
        end = opening + 1
        while end < len(code) and depth:
            if code[end] == "{":
                depth += 1
            elif code[end] == "}":
                depth -= 1
            end += 1
        if depth:
            raise ValueError("inline test module has no matching closing brace")
        _blank(result, source, match.start(), end)
        cursor = end
    return "".join(result)


def sync_tenant_occurrences(source: str) -> list[str]:
    # Most Rust sources cannot contain a lifecycle call. Avoid the structural
    # comment/literal pass unless the cheap raw-text prefilter finds a candidate;
    # the regex below remains the authority for what counts as a call.
    if ".create_tenant" not in source and "Engine::create_tenant" not in source:
        return []
    filtered = production_source(source)
    code = _rust_code_mask(filtered)
    occurrences = []
    for match in SYNC_TENANT_CALL.finditer(code):
        line_number = filtered.count("\n", 0, match.start()) + 1
        line_start = filtered.rfind("\n", 0, match.start()) + 1
        match_line_end = filtered.find("\n", match.end())
        if match_line_end < 0:
            snippet_end = len(filtered)
        else:
            following_line_end = filtered.find("\n", match_line_end + 1)
            snippet_end = len(filtered) if following_line_end < 0 else following_line_end
        snippet = " ".join(filtered[line_start:snippet_end].splitlines())
        occurrences.append(f"{line_number}:{snippet}")
    return occurrences


def _inventory_rows(inventory: Path) -> list[tuple[str, str, str, int, str, str]]:
    try:
        with inventory.open(encoding="utf-8", newline="") as handle:
            rows = []
            for row in csv.reader(handle, delimiter="\t"):
                if not row or row[0] == "path" or row[0].startswith("#"):
                    continue
                if len(row) != 6:
                    raise VerificationError(
                        f"inventory row must have 6 tab-separated fields: {row!r}"
                    )
                path, classification, needle, count, enforcement, representative_test = row
                if classification not in {
                    "provider_async",
                    "embedded_sync",
                    "provider_internal",
                }:
                    raise VerificationError(
                        f"unknown classification {classification!r} for {path}"
                    )
                if not count.isdigit() or int(count) < 1:
                    raise VerificationError(f"invalid expected_count {count!r} for {path}")
                if not enforcement or not representative_test:
                    raise VerificationError(f"missing enforcement/test evidence for {path}")
                rows.append(
                    (
                        path,
                        classification,
                        needle,
                        int(count),
                        enforcement,
                        representative_test,
                    )
                )
    except OSError as error:
        raise VerificationError(f"cannot read inventory {inventory}: {error}") from error
    return rows


def _production_rust_files(repo_root: Path) -> list[Path]:
    files = []
    for root_name in ("crates", "packages", "examples"):
        root = repo_root / root_name
        if not root.is_dir():
            continue
        for path in root.rglob("*.rs"):
            relative = path.relative_to(repo_root)
            if any(part in {"tests", "test", "fixtures"} for part in relative.parts[:-1]):
                continue
            if path.name == "tests.rs" or path.name.endswith("_tests.rs") or path.name.startswith(
                "test_"
            ):
                continue
            if path.is_file():
                files.append(path)
    return sorted(files)


def verify_inventory(repo_root: Path, inventory: Path) -> int:
    repo_root = repo_root.resolve()
    rows = _inventory_rows(inventory)
    inventory_keys: set[tuple[str, str]] = set()
    for path_text, classification, needle, expected_count, _, _ in rows:
        path = (repo_root / path_text).resolve()
        try:
            path.relative_to(repo_root)
        except ValueError as error:
            raise VerificationError(f"inventory path escapes repository: {path_text}") from error
        if not path.is_file():
            raise VerificationError(f"inventory path does not exist: {path_text}")
        try:
            filtered = production_source(path.read_text(encoding="utf-8"))
        except (OSError, UnicodeError, ValueError) as error:
            raise VerificationError(f"cannot filter {path_text}: {error}") from error
        actual_count = sum(needle in line for line in filtered.splitlines())
        if actual_count != expected_count:
            raise VerificationError(
                f"{path_text}: expected {expected_count} production occurrence(s) of "
                f"{needle!r}, found {actual_count}"
            )
        inventory_keys.add((path_text, classification))

    found_sync = 0
    for path in _production_rust_files(repo_root):
        path_text = path.relative_to(repo_root).as_posix()
        try:
            source = path.read_text(encoding="utf-8")
            occurrences = sync_tenant_occurrences(source)
        except (OSError, UnicodeError, ValueError) as error:
            raise VerificationError(f"cannot scan {path_text}: {error}") from error
        for occurrence in occurrences:
            if "tenant-lifecycle: test-only" in occurrence:
                continue
            found_sync += 1
            if "tenant-lifecycle: embedded-only" in occurrence:
                classification = "embedded_sync"
            elif "tenant-lifecycle: provider-adapter-internal" in occurrence:
                classification = "provider_internal"
            else:
                raise VerificationError(
                    "unclassified production synchronous/internal tenant creation: "
                    f"{path_text}:{occurrence}"
                )
            if (path_text, classification) not in inventory_keys:
                raise VerificationError(
                    f"classified call missing from inventory: {path_text}:{occurrence}"
                )

    if found_sync == 0:
        raise VerificationError(
            "zero production synchronous/internal tenant-creation calls found; "
            "scanner/filter is vacuous"
        )
    print(
        "tenant-lifecycle-callers: pass "
        f"({found_sync} classified synchronous/internal call sites)"
    )
    return found_sync


def self_test() -> None:
    after_module = r'''
fn before() { engine.create_tenant(before); }
fn split() {
    engine.create_tenant
        (split); // tenant-lifecycle: embedded-only
}
const DECOY: &str = ".create_tenant(string_decoy)";
// engine.create_tenant(comment_decoy);
#[cfg(test)]
mod tests {
    const COOKED: &str = "}";
    const RAW: &str = r###"/* } */"###;
    /* nested { /* } */ } */
    fn nested() { if true { engine.create_tenant(test_only); } }
}
fn after() { engine.create_tenant(after); }
'''
    filtered = production_source(after_module)
    assert "create_tenant(before)" in filtered
    assert "create_tenant(after)" in filtered
    assert "create_tenant(test_only)" not in filtered
    assert filtered.count("\n") == after_module.count("\n")
    occurrences = sync_tenant_occurrences(after_module)
    assert any("create_tenant(before)" in occurrence for occurrence in occurrences)
    assert any("create_tenant" in occurrence and "(split)" in occurrence for occurrence in occurrences)
    assert any("create_tenant(after)" in occurrence for occurrence in occurrences)
    assert all("create_tenant(test_only)" not in occurrence for occurrence in occurrences)
    assert all("string_decoy" not in occurrence for occurrence in occurrences)
    assert all("comment_decoy" not in occurrence for occurrence in occurrences)

    multiple_modules = r'''
#[cfg(test)] mod tests { fn one() { engine.create_tenant(one); } }
fn middle() { engine.create_tenant(middle); }
#[cfg(test)] pub(crate) mod tests { fn two() { engine.create_tenant(two); } }
fn tail() { engine.create_tenant(tail); }
'''
    filtered = production_source(multiple_modules)
    assert "create_tenant(one)" not in filtered
    assert "create_tenant(two)" not in filtered
    assert "create_tenant(middle)" in filtered
    assert "create_tenant(tail)" in filtered

    comment_decoy = "// #[cfg(test)] mod tests { }\nfn live() {}\n"
    assert production_source(comment_decoy) == comment_decoy

    try:
        production_source("#[cfg(test)] mod tests {\n")
    except ValueError as error:
        assert "matching closing brace" in str(error)
    else:
        raise AssertionError("unterminated inline test module must fail closed")

    print("tenant-lifecycle-production-source: self-test pass (4 cases)")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("path", nargs="?", type=Path)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--verify-root", type=Path)
    parser.add_argument("--inventory", type=Path)
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    if args.verify_root is not None:
        if args.inventory is None:
            parser.error("--inventory is required with --verify-root")
        try:
            verify_inventory(args.verify_root, args.inventory)
        except VerificationError as error:
            parser.error(str(error))
        return 0
    if args.path is None:
        parser.error("path is required unless --self-test or --verify-root is used")
    try:
        source = args.path.read_text(encoding="utf-8")
        print(production_source(source), end="")
    except (OSError, UnicodeError, ValueError) as error:
        parser.error(f"cannot filter {args.path}: {error}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
