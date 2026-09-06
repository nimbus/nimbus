#!/usr/bin/env python3
import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
VERIFY_SCRIPT = REPO_ROOT / "scripts" / "verify-release-version-contract.sh"
VERSION = "1.2.3"


class ReleaseVersionContractTests(unittest.TestCase):
    def test_staged_local_package_root_requires_an_exact_version(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            fixture = Path(tempdir)
            (fixture / "scripts").mkdir()
            (fixture / "crates" / "fixture").mkdir(parents=True)
            (fixture / "packages" / "foo").mkdir(parents=True)
            (fixture / "scaffold").mkdir()
            shutil.copy2(VERIFY_SCRIPT, fixture / "scripts" / VERIFY_SCRIPT.name)
            (fixture / "Cargo.toml").write_text(
                "[workspace]\nmembers = [\"crates/fixture\"]\n\n"
                f"[workspace.package]\nversion = \"{VERSION}\"\n",
                encoding="utf-8",
            )
            (fixture / "crates" / "fixture" / "Cargo.toml").write_text(
                "[package]\nname = \"fixture\"\nversion.workspace = true\nedition = \"2024\"\n",
                encoding="utf-8",
            )
            (fixture / "package.json").write_text(
                json.dumps({"private": True, "workspaces": ["packages/foo"]}),
                encoding="utf-8",
            )
            (fixture / "packages" / "foo" / "package.json").write_text(
                json.dumps({"name": "@nimbus/foo", "version": VERSION}),
                encoding="utf-8",
            )
            (fixture / "package-lock.json").write_text(
                json.dumps(
                    {
                        "lockfileVersion": 3,
                        "packages": {
                            "": {"workspaces": ["packages/foo"]},
                            "packages/foo": {
                                "name": "@nimbus/foo",
                                "version": VERSION,
                            },
                        },
                    }
                ),
                encoding="utf-8",
            )
            staged_lock = fixture / "scaffold" / "package-lock.json"
            staged_lock.write_text(
                json.dumps(
                    {
                        "lockfileVersion": 3,
                        "packages": {".nimbus/packages/@nimbus/foo": {}},
                    }
                ),
                encoding="utf-8",
            )
            (fixture / "CHANGELOG.md").write_text(
                f"# Changelog\n\n## [{VERSION}] - 2026-08-31\n",
                encoding="utf-8",
            )
            subprocess.run(
                ["git", "init", "--quiet"], cwd=fixture, check=True
            )
            subprocess.run(["git", "add", "."], cwd=fixture, check=True)

            missing = self.run_verifier(fixture)
            self.assertNotEqual(missing.returncode, 0)
            self.assertIn(
                "scaffold/package-lock.json .nimbus/packages/@nimbus/foo version "
                f"expected={VERSION} actual=<missing>",
                missing.stderr,
            )

            staged_lock.write_text(
                json.dumps(
                    {
                        "lockfileVersion": 3,
                        "packages": {
                            ".nimbus/packages/@nimbus/foo": {"version": VERSION}
                        },
                    }
                ),
                encoding="utf-8",
            )
            accepted = self.run_verifier(fixture)
            self.assertEqual(accepted.returncode, 0, accepted.stdout + accepted.stderr)
            self.assertIn(f"matches {VERSION}", accepted.stdout)

    @staticmethod
    def run_verifier(fixture: Path) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", "scripts/verify-release-version-contract.sh", VERSION],
            cwd=fixture,
            text=True,
            capture_output=True,
            check=False,
        )


if __name__ == "__main__":
    unittest.main()
