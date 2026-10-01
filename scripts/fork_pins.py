#!/usr/bin/env python3
"""Read, verify, and rewrite the fork pins that packaging/forks.toml owns.

packaging/forks.toml records each Nimbus-owned source fork, its release tag,
and every tracked file that copies the pin. This module is the only reader of
that manifest:

  check                     every consumer matches the manifest, and no tracked
                            file copies a fork tag or commit without a consumer
                            entry
  repin FORK TAG [--commit] rewrite the manifest and every consumer
  get FORK FIELD            print one manifest value for a shell caller
  inventory [--fork NAME]   compare the local fork checkouts and their remotes
                            with the manifest (network)

scripts/repin-fork.sh and scripts/verify-fork-upstream-standardization.sh are
the operator entry points.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    sys.exit("fork-pins: Python 3.11 or later is required to read packaging/forks.toml")

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST = Path("packaging/forks.toml")
PREFIX = "fork-standardization"

VERSION_RE = r"[0-9]+(?:\.[0-9]+)+"
COMMIT_RE = r"[0-9a-f]{40}"
PLACEHOLDER_RE = re.compile(r"\{(tag|upstream_version|commit)\}")
FORK_FIELDS = ("repo", "upstream_repo", "upstream_tag", "upstream_version", "tag", "kind")


class ManifestError(Exception):
    pass


@dataclass
class Consumer:
    path: str
    patterns: list[str]


@dataclass
class Fork:
    name: str
    fields: dict

    @property
    def tag(self) -> str:
        return self.fields["tag"]

    @property
    def commit(self) -> str | None:
        return self.fields.get("commit")

    @property
    def tag_prefix(self) -> str:
        return tag_prefix(self.tag)

    @property
    def consumers(self) -> list[Consumer]:
        return [Consumer(entry["path"], list(entry["patterns"])) for entry in self.fields.get("consumers", [])]

    def values(self) -> dict[str, str]:
        values = {"tag": self.tag, "upstream_version": self.fields["upstream_version"]}
        if self.commit is not None:
            values["commit"] = self.commit
        return values

    def placeholder_regexes(self) -> dict[str, str]:
        return {
            "tag": re.escape(self.tag_prefix) + VERSION_RE + r"-nimbus\.[0-9]+",
            "upstream_version": VERSION_RE,
            "commit": COMMIT_RE,
        }


def tag_prefix(tag: str) -> str:
    match = re.fullmatch(r"(?P<prefix>[^0-9]*)" + VERSION_RE + r"-nimbus\.[0-9]+", tag)
    if match is None:
        raise ManifestError(f"tag {tag!r} does not look like <prefix>X.Y.Z-nimbus.N")
    return match["prefix"]


def load_manifest(root: Path) -> tuple[dict, dict[str, Fork]]:
    path = root / MANIFEST
    try:
        document = tomllib.loads(path.read_text())
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ManifestError(f"cannot read {MANIFEST}: {error}") from error

    forks: dict[str, Fork] = {}
    for name, fields in document.get("forks", {}).items():
        missing = [field for field in FORK_FIELDS if field not in fields]
        if missing:
            raise ManifestError(f"{MANIFEST}: forks.{name} is missing {', '.join(missing)}")
        fork = Fork(name, fields)
        tag_prefix(fork.tag)
        if fork.commit is not None and re.fullmatch(COMMIT_RE, fork.commit) is None:
            raise ManifestError(f"{MANIFEST}: forks.{name}.commit must be a 40-hex commit")
        for consumer in fork.consumers:
            for pattern in consumer.patterns:
                used = set(PLACEHOLDER_RE.findall(pattern))
                if not used:
                    raise ManifestError(f"{MANIFEST}: forks.{name} pattern for {consumer.path} has no placeholder")
                if "commit" in used and fork.commit is None:
                    raise ManifestError(f"{MANIFEST}: forks.{name} pattern for {consumer.path} uses {{commit}} without a commit")
        forks[name] = fork
    if not forks:
        raise ManifestError(f"{MANIFEST}: no [forks.*] tables")
    return document, forks


def pattern_regex(fork: Fork, pattern: str) -> re.Pattern[str]:
    regexes = fork.placeholder_regexes()
    parts: list[str] = []
    last = 0
    for match in PLACEHOLDER_RE.finditer(pattern):
        parts.append(re.escape(pattern[last:match.start()]))
        parts.append(regexes[match[1]])
        last = match.end()
    parts.append(re.escape(pattern[last:]))
    return re.compile("".join(parts))


def render(pattern: str, values: dict[str, str]) -> str:
    return PLACEHOLDER_RE.sub(lambda match: values[match[1]], pattern)


def line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def literal_regex(value: str) -> re.Pattern[str]:
    # A tag must not match inside a longer tag, such as nimbus.1 in nimbus.10.
    return re.compile(r"(?<![0-9A-Za-z.-])" + re.escape(value) + r"(?![0-9A-Za-z])")


def one_line(text: str) -> str:
    return text.strip().replace("\n", "\\n")


def tracked_files_containing(root: Path, value: str, ignore_paths: list[str]) -> list[str]:
    excludes = [f":(exclude){path}" for path in ignore_paths] + [f":(exclude){MANIFEST}"]
    result = subprocess.run(
        ["git", "-C", str(root), "grep", "-l", "-z", "-F", "-e", value, "--", ".", *excludes],
        capture_output=True,
        text=True,
    )
    if result.returncode not in (0, 1):
        raise ManifestError(f"git grep failed in {root}: {result.stderr.strip()}")
    return [path for path in result.stdout.split("\0") if path]


def check(root: Path, only: list[str] | None = None) -> int:
    try:
        document, forks = load_manifest(root)
    except ManifestError as error:
        print(f"{PREFIX}: {error}", file=sys.stderr)
        return 1

    issues: list[str] = []
    selected = select_forks(forks, only, issues)
    ignore_paths = list(document.get("ignore_paths", []))

    for fork in selected:
        values = fork.values()
        pin_count = 0
        # Literal pin text that the consumer patterns cover, per file.
        covered: dict[str, dict[str, int]] = {}
        for consumer in fork.consumers:
            path = root / consumer.path
            try:
                text = path.read_text()
            except OSError:
                issues.append(f"{fork.name}: {consumer.path}: consumer file is missing")
                continue
            for pattern in consumer.patterns:
                expected = render(pattern, values)
                matches = list(pattern_regex(fork, pattern).finditer(text))
                if not matches:
                    issues.append(f"{fork.name}: {consumer.path}: expected `{one_line(expected)}`, found no match")
                    continue
                for match in matches:
                    pin_count += 1
                    if match[0] != expected:
                        issues.append(
                            f"{fork.name}: {consumer.path}:{line_of(text, match.start())}: "
                            f"expected `{one_line(expected)}`, found `{one_line(match[0])}`"
                        )
                    for kind in ("tag", "commit"):
                        if kind in values:
                            count = len(literal_regex(values[kind]).findall(match[0]))
                            file_counts = covered.setdefault(consumer.path, {})
                            file_counts[kind] = file_counts.get(kind, 0) + count

        for kind in ("tag", "commit"):
            if kind not in values:
                continue
            value = values[kind]
            try:
                paths = tracked_files_containing(root, value, ignore_paths)
            except ManifestError as error:
                issues.append(f"{fork.name}: {error}")
                continue
            for relative in paths:
                text = (root / relative).read_text(errors="replace")
                found = len(literal_regex(value).findall(text))
                owned = covered.get(relative, {}).get(kind, 0)
                if found == 0 or found == owned:
                    continue
                if relative not in {consumer.path for consumer in fork.consumers}:
                    issues.append(
                        f"{fork.name}: {relative}: copies {kind} {value} without a consumer entry in {MANIFEST}"
                    )
                else:
                    issues.append(
                        f"{fork.name}: {relative}: has {found} copies of {kind} {value}, "
                        f"but its consumer patterns cover {owned}"
                    )

        print(f"{PREFIX}: {fork.name} {fork.tag}: {len(fork.consumers)} consumer files, {pin_count} pins")

    for issue in issues:
        print(f"{PREFIX}: {issue}", file=sys.stderr)
    if issues:
        print(f"{PREFIX}: {len(issues)} issue(s) detected. Repin with scripts/repin-fork.sh.", file=sys.stderr)
        return 1
    print(f"{PREFIX}: pass")
    return 0


def select_forks(forks: dict[str, Fork], only: list[str] | None, issues: list[str]) -> list[Fork]:
    if not only:
        return list(forks.values())
    selected: list[Fork] = []
    for name in only:
        fork = forks.get(name) or next((fork for fork in forks.values() if fork.fields["repo"] == name), None)
        if fork is None:
            issues.append(f"{name}: unknown fork. Known forks: {', '.join(forks)}")
        elif fork not in selected:
            selected.append(fork)
    return selected


def resolve_commit(fork: Fork, tag: str) -> str:
    url = f"https://github.com/{fork.fields['repo']}.git"
    result = subprocess.run(
        ["git", "ls-remote", "--tags", url, f"refs/tags/{tag}", f"refs/tags/{tag}^{{}}"],
        capture_output=True,
        text=True,
    )
    refs = dict(line.split("\t")[::-1] for line in result.stdout.splitlines() if "\t" in line)
    commit = refs.get(f"refs/tags/{tag}^{{}}") or refs.get(f"refs/tags/{tag}")
    if result.returncode != 0 or commit is None:
        raise ManifestError(f"cannot resolve {tag} in {url}. Publish the tag, or pass --commit.")
    return commit


def rewrite_manifest_table(text: str, fork: str, updates: dict[str, str]) -> str:
    lines = text.splitlines(keepends=True)
    header = f"[forks.{fork}]"
    try:
        start = next(index for index, line in enumerate(lines) if line.strip() == header)
    except StopIteration as error:
        raise ManifestError(f"{MANIFEST}: no {header} table") from error
    pending = dict(updates)
    for index in range(start + 1, len(lines)):
        if lines[index].startswith("["):
            break
        key = lines[index].split("=", 1)[0].strip()
        if key in pending:
            lines[index] = f'{key} = "{pending.pop(key)}"\n'
    if pending:
        raise ManifestError(f"{MANIFEST}: {header} has no {', '.join(pending)} line")
    return "".join(lines)


def repin(root: Path, name: str, tag: str, commit: str | None) -> int:
    try:
        _, forks = load_manifest(root)
        fork = forks.get(name)
        if fork is None:
            raise ManifestError(f"unknown fork {name!r}. Known forks: {', '.join(forks)}")

        prefix = fork.tag_prefix
        match = re.fullmatch(re.escape(prefix) + f"(?P<version>{VERSION_RE})" + r"-nimbus\.[0-9]+", tag)
        if match is None:
            raise ManifestError(f"{name}: tag {tag!r} must look like {prefix}X.Y.Z-nimbus.N")
        old_version = fork.fields["upstream_version"]
        if old_version not in fork.fields["upstream_tag"]:
            raise ManifestError(f"{MANIFEST}: forks.{name}.upstream_tag does not contain upstream_version")

        updates = {
            "tag": tag,
            "upstream_version": match["version"],
            "upstream_tag": fork.fields["upstream_tag"].replace(old_version, match["version"]),
        }
        if fork.commit is not None:
            if commit is not None:
                if re.fullmatch(COMMIT_RE, commit) is None:
                    raise ManifestError(f"--commit {commit!r} must be a 40-hex commit")
                updates["commit"] = commit
            elif tag == fork.tag:
                updates["commit"] = fork.commit
            else:
                updates["commit"] = resolve_commit(fork, tag)
        elif commit is not None:
            raise ManifestError(f"{name}: no consumer pins a commit. Omit --commit.")

        # Compute every rewrite before writing, so a missing pattern writes nothing.
        writes: dict[Path, str] = {}
        for consumer in fork.consumers:
            path = root / consumer.path
            original = writes.get(path)
            if original is None:
                try:
                    original = path.read_text()
                except OSError as error:
                    raise ManifestError(f"{name}: {consumer.path}: consumer file is missing") from error
            text = original
            for pattern in consumer.patterns:
                regex = pattern_regex(fork, pattern)
                if regex.search(text) is None:
                    raise ManifestError(f"{name}: {consumer.path}: no match for `{one_line(pattern)}`")
                replacement = render(pattern, updates)
                text = regex.sub(lambda _: replacement, text)
            writes[path] = text

        manifest_path = root / MANIFEST
        writes[manifest_path] = rewrite_manifest_table(manifest_path.read_text(), name, updates)
    except ManifestError as error:
        print(f"repin-fork: {error}", file=sys.stderr)
        return 1

    changed = []
    for path, text in writes.items():
        if path.read_text() != text:
            path.write_text(text)
            changed.append(path.relative_to(root).as_posix())

    if not changed:
        print(f"repin-fork: {name} already pins {tag}. No files changed.")
    else:
        print(f"repin-fork: {name} {fork.tag} -> {tag}")
        for path in changed:
            print(f"  {path}")
        note = fork.fields.get("after_repin")
        if note:
            print(f"repin-fork: next: {note}")
    return check(root, [name])


def get(root: Path, name: str, field: str) -> int:
    try:
        _, forks = load_manifest(root)
    except ManifestError as error:
        print(f"fork-pins: {error}", file=sys.stderr)
        return 1
    value = forks[name].fields.get(field) if name in forks else None
    if not isinstance(value, str):
        print(f"fork-pins: {MANIFEST} has no string forks.{name}.{field}", file=sys.stderr)
        return 1
    print(value)
    return 0


def git_output(*args: str) -> str:
    result = subprocess.run(["git", *args], capture_output=True, text=True)
    return result.stdout.strip() if result.returncode == 0 else ""


def latest_tag_glob(upstream_tag: str) -> str:
    return re.match(r"[^0-9]*", upstream_tag)[0] + "[0-9]*"


def remote_head(url: str) -> str:
    for line in git_output("ls-remote", "--symref", url, "HEAD").splitlines():
        if line.startswith("ref:"):
            return line.split()[1].removeprefix("refs/heads/")
    return "unavailable"


def remote_tag_state(url: str, tag: str) -> str:
    result = subprocess.run(
        ["git", "ls-remote", "--exit-code", "--tags", "--refs", url, tag],
        capture_output=True,
    )
    return "present" if result.returncode == 0 else "missing"


def remote_latest_tag(url: str, glob: str) -> str:
    tags = [line.split("\t")[1].removeprefix("refs/tags/") for line in
            git_output("ls-remote", "--tags", "--refs", url, glob).splitlines() if "\t" in line]
    if not tags:
        return "unavailable"
    return subprocess.run(["sort", "-V"], input="\n".join(tags), capture_output=True, text=True).stdout.split()[-1]


def inventory(root: Path, only: list[str] | None) -> int:
    try:
        _, forks = load_manifest(root)
    except ManifestError as error:
        print(f"{PREFIX}: {error}", file=sys.stderr)
        return 1

    issues: list[str] = []
    selected = select_forks(forks, only, issues)
    print("fork\tpath\tkind\tbranch\texpected_branch\torigin_url\tupstream_url\torigin_head\t"
          "upstream_tag\ttag\tlocal_upstream_tag\tremote_upstream_tag\tlocal_tag\tremote_tag\t"
          "latest_upstream_tag\ttracks_latest\tclean_state")
    for fork in selected:
        fields = fork.fields
        repo = fields["repo"]
        path = Path.home() / "src/github.com" / repo
        expected_branch = f"nimbus/{fields['upstream_tag']}"
        expected_origin = f"git@github.com:{repo}.git"
        expected_upstream = f"git@github.com:{fields['upstream_repo']}.git"
        tracks_latest = "yes" if fields.get("tracks_latest") else "no"

        if git_output("-C", str(path), "rev-parse", "--is-inside-work-tree") != "true":
            issues.append(f"{repo}: missing git checkout at {path}")
            continue

        def local_tag(tag: str) -> str:
            found = git_output("-C", str(path), "rev-parse", "-q", "--verify", f"refs/tags/{tag}^{{commit}}")
            return "present" if found else "missing"

        branch = git_output("-C", str(path), "branch", "--show-current") or git_output(
            "-C", str(path), "rev-parse", "--short", "HEAD")
        origin = git_output("-C", str(path), "remote", "get-url", "origin")
        upstream = git_output("-C", str(path), "remote", "get-url", "upstream")
        origin_head = remote_head(expected_origin)
        local_upstream_tag = local_tag(fields["upstream_tag"])
        remote_upstream_tag = remote_tag_state(expected_upstream, fields["upstream_tag"])
        local_fork_tag = local_tag(fork.tag)
        remote_fork_tag = remote_tag_state(expected_origin, fork.tag)
        latest = remote_latest_tag(expected_upstream, latest_tag_glob(fields["upstream_tag"]))
        dirty = len(git_output("-C", str(path), "status", "--short").splitlines())
        clean_state = "clean" if dirty == 0 else f"dirty:{dirty}"

        print("\t".join([
            repo, str(path), fields["kind"], branch, expected_branch, origin or "<missing>",
            upstream or "<missing>", origin_head, fields["upstream_tag"], fork.tag, local_upstream_tag,
            remote_upstream_tag, local_fork_tag, remote_fork_tag, latest, tracks_latest, clean_state,
        ]))

        if origin != expected_origin:
            issues.append(f"{repo}: origin mismatch expected={expected_origin} actual={origin or '<missing>'}")
        if upstream != expected_upstream:
            issues.append(f"{repo}: upstream mismatch expected={expected_upstream} actual={upstream or '<missing>'}")
        if origin_head not in ("unavailable", expected_branch):
            issues.append(f"{repo}: origin default branch mismatch expected={expected_branch} actual={origin_head}")
        if remote_upstream_tag == "missing":
            issues.append(f"{repo}: upstream tag missing upstream: {fields['upstream_tag']}")
        if fields["kind"] == "full_source" and local_upstream_tag == "missing":
            issues.append(f"{repo}: upstream tag missing locally: {fields['upstream_tag']}")
        if remote_fork_tag == "missing":
            issues.append(f"{repo}: fork tag missing from fork remote: {fork.tag}")
        if tracks_latest == "yes" and latest not in ("unavailable", fields["upstream_tag"]):
            issues.append(f"{repo}: newer upstream tag detected expected={fields['upstream_tag']} latest={latest}")

        for companion in fields.get("companions", []):
            url = f"https://github.com/{companion['upstream_repo']}.git"
            state = remote_tag_state(url, companion["tag"])
            latest = remote_latest_tag(url, latest_tag_glob(companion["tag"]))
            print(f"{repo}/{companion['name']}\t{url}\t{companion['tag']}\t{state}\t{latest}")
            if state == "missing":
                issues.append(f"{repo}/{companion['name']}: companion tag missing upstream: {companion['tag']}")
            if companion.get("tracks_latest") and latest not in ("unavailable", companion["tag"]):
                issues.append(
                    f"{repo}/{companion['name']}: newer upstream tag detected "
                    f"expected={companion['tag']} latest={latest}"
                )

    for issue in issues:
        print(f"{PREFIX}: {issue}", file=sys.stderr)
    if issues:
        print(f"{PREFIX}: {len(issues)} remote issue(s) detected", file=sys.stderr)
        return 1
    print(f"{PREFIX}: remote pass")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--root", type=Path, default=REPO_ROOT, help="repository root (default: this checkout)")
    commands = parser.add_subparsers(dest="command", required=True)

    check_parser = commands.add_parser("check")
    check_parser.add_argument("--fork", action="append", dest="forks")

    repin_parser = commands.add_parser("repin")
    repin_parser.add_argument("fork")
    repin_parser.add_argument("tag")
    repin_parser.add_argument("--commit")

    get_parser = commands.add_parser("get")
    get_parser.add_argument("fork")
    get_parser.add_argument("field")

    inventory_parser = commands.add_parser("inventory")
    inventory_parser.add_argument("--fork", action="append", dest="forks")

    args = parser.parse_args(argv)
    root = args.root.resolve()
    if args.command == "check":
        return check(root, args.forks)
    if args.command == "repin":
        return repin(root, args.fork, args.tag, args.commit)
    if args.command == "get":
        return get(root, args.fork, args.field)
    return inventory(root, args.forks)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
