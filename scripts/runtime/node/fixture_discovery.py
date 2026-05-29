from __future__ import annotations

import subprocess
from pathlib import Path


TEST_FILE_SUFFIXES = {".js", ".mjs", ".cjs"}


def repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def is_node_test_file(path: Path) -> bool:
    return path.name.startswith("test-") and path.suffix in TEST_FILE_SUFFIXES


def discover_fixture_files(fixture_root: Path) -> list[str]:
    """Return Git-visible fixture test files relative to the fixture root.

    Node fixture trees can contain ignored support directories such as
    node_modules. Those files must not affect checked-in evidence, because CI
    and fresh clones only see tracked files plus intentional unignored local
    additions during a refresh.
    """

    root = repo_root()
    resolved_root = fixture_root.resolve()
    fixture_arg = str(resolved_root.relative_to(root))
    result = subprocess.run(
        [
            "git",
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            fixture_arg,
        ],
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    fixtures: set[str] = set()
    for line in result.stdout.splitlines():
        path = (root / line).resolve()
        if not path.is_file() or not is_node_test_file(path):
            continue
        try:
            relative = path.relative_to(resolved_root)
        except ValueError:
            continue
        fixtures.add(relative.as_posix())
    return sorted(fixtures)
