#!/usr/bin/env python3
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
FORK_PINS = REPO_ROOT / "scripts" / "fork_pins.py"
REPIN_FORK = REPO_ROOT / "scripts" / "repin-fork.sh"

COMMIT = "a" * 40
NEW_COMMIT = "b" * 40

MANIFEST = f"""
ignore_paths = ["docs/"]

[forks.demo]
repo = "nimbus/demo"
upstream_repo = "upstream/demo"
upstream_tag = "v1.2.3"
upstream_version = "1.2.3"
tag = "v1.2.3-nimbus.4"
commit = "{COMMIT}"
kind = "full_source"
tracks_latest = true
after_repin = "Refresh the lockfile."

[[forks.demo.consumers]]
path = "pins.env"
patterns = ["DEMO_VERSION={{tag}}", "DEMO_UPSTREAM_VERSION={{upstream_version}}"]

[[forks.demo.consumers]]
path = "Cargo.lock"
patterns = ['source = "git+https://github.com/nimbus/demo?tag={{tag}}#{{commit}}"']

[[forks.demo.consumers]]
path = "fixture.sh"
patterns = [
  '''
  demo/releases/latest)
    printf '{{"tag_name":"{{tag}}"}}'
''',
]
"""

FILES = {
    "packaging/forks.toml": MANIFEST,
    "pins.env": "DEMO_VERSION=v1.2.3-nimbus.4\nDEMO_UPSTREAM_VERSION=1.2.3\n",
    "Cargo.lock": (
        '[[package]]\nname = "demo_a"\n'
        f'source = "git+https://github.com/nimbus/demo?tag=v1.2.3-nimbus.4#{COMMIT}"\n'
        '[[package]]\nname = "demo_b"\n'
        f'source = "git+https://github.com/nimbus/demo?tag=v1.2.3-nimbus.4#{COMMIT}"\n'
    ),
    "fixture.sh": (
        "case \"$url\" in\n"
        "  other/releases/latest)\n"
        "    printf '{\"tag_name\":\"v9.9.9-nimbus.1\"}'\n"
        "    ;;\n"
        "  demo/releases/latest)\n"
        "    printf '{\"tag_name\":\"v1.2.3-nimbus.4\"}'\n"
        "    ;;\n"
        "esac\n"
    ),
    "docs/history.md": "We once shipped v1.2.3-nimbus.4.\n",
}


class ForkPinsTest(unittest.TestCase):
    def setUp(self):
        self.tempdir = tempfile.TemporaryDirectory()
        self.root = Path(self.tempdir.name)
        for relative, text in FILES.items():
            path = self.root / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        self.git("init", "-q")
        self.git("add", "-A")

    def tearDown(self):
        self.tempdir.cleanup()

    def git(self, *args):
        subprocess.run(["git", "-C", str(self.root), *args], check=True, capture_output=True)

    def fork_pins(self, *args):
        return subprocess.run(
            ["python3", str(FORK_PINS), "--root", str(self.root), *args],
            capture_output=True,
            text=True,
        )

    def snapshot(self):
        return {relative: (self.root / relative).read_text() for relative in FILES}

    def test_check_passes_when_consumers_match(self):
        result = self.fork_pins("check")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("demo v1.2.3-nimbus.4: 3 consumer files, 5 pins", result.stdout)
        self.assertIn("fork-standardization: pass", result.stdout)

    def test_check_names_file_line_and_expected_value_on_drift(self):
        (self.root / "pins.env").write_text("DEMO_VERSION=v1.2.3-nimbus.3\nDEMO_UPSTREAM_VERSION=1.2.3\n")

        result = self.fork_pins("check")

        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "demo: pins.env:1: expected `DEMO_VERSION=v1.2.3-nimbus.4`, found `DEMO_VERSION=v1.2.3-nimbus.3`",
            result.stderr,
        )

    def test_check_fails_when_a_pattern_is_missing(self):
        (self.root / "pins.env").write_text("DEMO_VERSION=v1.2.3-nimbus.4\n")

        result = self.fork_pins("check")

        self.assertEqual(result.returncode, 1)
        self.assertIn("demo: pins.env: expected `DEMO_UPSTREAM_VERSION=1.2.3`, found no match", result.stderr)

    def test_check_fails_on_an_unregistered_copy(self):
        (self.root / "copy.sh").write_text('PIN="v1.2.3-nimbus.4"\n')
        self.git("add", "copy.sh")

        result = self.fork_pins("check")

        self.assertEqual(result.returncode, 1)
        self.assertIn(
            "demo: copy.sh: copies tag v1.2.3-nimbus.4 without a consumer entry in packaging/forks.toml",
            result.stderr,
        )

    def test_check_fails_on_an_uncovered_copy_in_a_consumer(self):
        with (self.root / "pins.env").open("a") as handle:
            handle.write("# fallback v1.2.3-nimbus.4\n")

        result = self.fork_pins("check")

        self.assertEqual(result.returncode, 1)
        self.assertIn("demo: pins.env: has 2 copies of tag v1.2.3-nimbus.4, but its consumer patterns cover 1", result.stderr)

    def test_check_ignores_longer_tags_and_ignored_paths(self):
        (self.root / "other.txt").write_text("v1.2.3-nimbus.40 and xv1.2.3-nimbus.4\n")
        self.git("add", "other.txt")

        result = self.fork_pins("check")

        self.assertEqual(result.returncode, 0, result.stderr)

    def test_repin_rewrites_every_consumer_and_the_manifest(self):
        result = self.fork_pins("repin", "demo", "v1.3.0-nimbus.1", "--commit", NEW_COMMIT)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("repin-fork: next: Refresh the lockfile.", result.stdout)
        self.assertEqual(
            (self.root / "pins.env").read_text(),
            "DEMO_VERSION=v1.3.0-nimbus.1\nDEMO_UPSTREAM_VERSION=1.3.0\n",
        )
        lock = (self.root / "Cargo.lock").read_text()
        self.assertEqual(lock.count(f"?tag=v1.3.0-nimbus.1#{NEW_COMMIT}"), 2)
        fixture = (self.root / "fixture.sh").read_text()
        self.assertIn("printf '{\"tag_name\":\"v1.3.0-nimbus.1\"}'", fixture)
        self.assertIn("printf '{\"tag_name\":\"v9.9.9-nimbus.1\"}'", fixture)
        manifest = (self.root / "packaging/forks.toml").read_text()
        for line in ('upstream_tag = "v1.3.0"', 'upstream_version = "1.3.0"',
                     'tag = "v1.3.0-nimbus.1"', f'commit = "{NEW_COMMIT}"'):
            self.assertIn(line, manifest)
        self.assertEqual((self.root / "docs/history.md").read_text(), FILES["docs/history.md"])

        self.assertEqual(self.fork_pins("check").returncode, 0)

    def test_repin_with_the_current_tag_changes_no_file(self):
        result = self.fork_pins("repin", "demo", "v1.2.3-nimbus.4")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("demo already pins v1.2.3-nimbus.4. No files changed.", result.stdout)
        self.assertEqual(self.snapshot(), FILES)

    def test_repin_rejects_a_tag_with_another_prefix(self):
        result = self.fork_pins("repin", "demo", "bun-v1.3.0-nimbus.1", "--commit", NEW_COMMIT)

        self.assertEqual(result.returncode, 1)
        self.assertIn("must look like vX.Y.Z-nimbus.N", result.stderr)
        self.assertEqual(self.snapshot(), FILES)

    def test_repin_writes_nothing_when_a_consumer_pattern_is_missing(self):
        (self.root / "fixture.sh").write_text("esac\n")
        before = self.snapshot()

        result = self.fork_pins("repin", "demo", "v1.3.0-nimbus.1", "--commit", NEW_COMMIT)

        self.assertEqual(result.returncode, 1)
        self.assertIn("demo: fixture.sh: no match for", result.stderr)
        self.assertEqual(self.snapshot(), before)

    def test_repin_rejects_an_unknown_fork(self):
        result = self.fork_pins("repin", "nope", "v1.3.0-nimbus.1")

        self.assertEqual(result.returncode, 1)
        self.assertIn("unknown fork 'nope'. Known forks: demo", result.stderr)

    def test_get_prints_one_manifest_value(self):
        result = self.fork_pins("get", "demo", "commit")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, f"{COMMIT}\n")

    def test_repin_fork_wrapper_rejects_malformed_arguments(self):
        result = subprocess.run(["bash", str(REPIN_FORK), "demo"], capture_output=True, text=True)

        self.assertEqual(result.returncode, 64)
        self.assertIn("usage: repin-fork.sh FORK TAG [--commit SHA]", result.stderr)


if __name__ == "__main__":
    unittest.main()
