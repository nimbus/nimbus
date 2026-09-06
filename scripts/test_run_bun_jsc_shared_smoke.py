import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
RUNNER = REPO_ROOT / "scripts" / "run_bun_jsc_shared_smoke.py"


class BunJscSharedSmokeRunnerTests(unittest.TestCase):
    def test_success_preserves_loader_output(self) -> None:
        result = self.run_fixture(
            """
            import pathlib
            import sys

            print(f"loaded {pathlib.Path(sys.argv[1]).name}", flush=True)
            """,
        )

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("running Bun/JSC shared adapter smoke", result.stdout)
        self.assertIn("loaded adapter.so", result.stdout)

    def test_nonzero_loader_status_is_preserved(self) -> None:
        result = self.run_fixture("raise SystemExit(7)")

        self.assertEqual(result.returncode, 7, result.stdout + result.stderr)

    def test_missing_loader_fails_before_execution(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            library = root / "adapter.so"
            library.touch()
            result = self.run_runner(root / "missing.py", library)

        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertIn("missing Bun/JSC shared adapter smoke loader", result.stderr)

    def test_missing_shared_library_fails_before_execution(self) -> None:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            loader = root / "loader.py"
            loader.write_text("raise SystemExit(0)\n", encoding="utf-8")
            result = self.run_runner(loader, root / "missing.so")

        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertIn("missing Bun/JSC shared adapter artifact", result.stderr)

    def test_timeout_returns_standard_timeout_status(self) -> None:
        result = self.run_fixture(
            """
            import time

            time.sleep(60)
            """,
            timeout_seconds="0.05",
        )

        self.assertEqual(result.returncode, 124, result.stdout + result.stderr)
        self.assertIn("exceeded 0.05 seconds", result.stderr)

    def run_fixture(
        self, source: str, *, timeout_seconds: str = "5"
    ) -> subprocess.CompletedProcess[str]:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            loader = root / "loader.py"
            library = root / "adapter.so"
            loader.write_text(textwrap.dedent(source).lstrip(), encoding="utf-8")
            library.touch()
            return self.run_runner(loader, library, timeout_seconds=timeout_seconds)

    @staticmethod
    def run_runner(
        loader: Path, library: Path, *, timeout_seconds: str = "5"
    ) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [
                sys.executable,
                str(RUNNER),
                "--timeout-seconds",
                timeout_seconds,
                str(loader),
                str(library),
            ],
            text=True,
            capture_output=True,
            check=False,
        )


if __name__ == "__main__":
    unittest.main()
