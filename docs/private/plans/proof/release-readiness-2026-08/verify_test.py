#!/usr/bin/env python3
import contextlib
import importlib.util
import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

VERIFY_PATH = Path(__file__).with_name("verify.py")
SPEC = importlib.util.spec_from_file_location("release_readiness_verify", VERIFY_PATH)
assert SPEC is not None and SPEC.loader is not None
VERIFY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)

CANDIDATE = {
    "nimbus": "1" * 40,
    "desktop": "2" * 40,
    "deno": "3" * 40,
    "main": "4" * 40,
}


class ReleaseReadinessVerifierTests(unittest.TestCase):
    def run_verifier(
        self,
        *,
        passing_condition: str = "tenant_lifecycle",
        proof_revisions: tuple[str, ...] = (
            CANDIDATE["nimbus"],
            CANDIDATE["deno"],
            CANDIDATE["main"],
        ),
        candidate: dict[str, str] | None = None,
    ) -> str:
        with tempfile.TemporaryDirectory() as tempdir:
            root = Path(tempdir)
            proof = root / "proof.md"
            proof.write_text(
                "# Proof\n\n## Bound evidence\n\n" + "\n".join(proof_revisions) + "\n",
                encoding="utf-8",
            )
            conditions = []
            for condition_id in VERIFY.EXPECTED_IDS:
                row: dict[str, object] = {"id": condition_id, "state": "blocked"}
                if condition_id == passing_condition:
                    row = {
                        "id": condition_id,
                        "state": "pass",
                        "evidence": {"path": "proof.md", "anchor": "## Bound evidence"},
                    }
                conditions.append(row)
            matrix = root / "matrix.json"
            matrix.write_text(
                json.dumps(
                    {
                        "schemaVersion": 1,
                        "candidate": CANDIDATE if candidate is None else candidate,
                        "conditions": conditions,
                    }
                ),
                encoding="utf-8",
            )
            previous_argv = sys.argv
            output = io.StringIO()
            try:
                sys.argv = [str(VERIFY_PATH), str(matrix)]
                with contextlib.redirect_stdout(output):
                    VERIFY.main()
            finally:
                sys.argv = previous_argv
            return output.getvalue()

    def test_exact_candidate_bound_proof_has_no_structural_error(self) -> None:
        output = self.run_verifier()
        self.assertNotIn("ERROR:", output)
        self.assertIn("0 structural errors", output)

    def test_stale_pass_proof_is_rejected(self) -> None:
        output = self.run_verifier(proof_revisions=(CANDIDATE["deno"], CANDIDATE["main"]))
        self.assertIn("evidence is not bound to the nimbus candidate", output)
        self.assertIn("1 structural errors", output)

    def test_desktop_pass_requires_exact_desktop_revision(self) -> None:
        output = self.run_verifier(
            passing_condition="desktop_app",
            proof_revisions=(
                CANDIDATE["nimbus"],
                CANDIDATE["deno"],
                CANDIDATE["main"],
            ),
        )
        self.assertIn("evidence is not bound to the desktop candidate", output)

    def test_candidate_revisions_must_be_full_lowercase_shas(self) -> None:
        malformed = dict(CANDIDATE)
        malformed["nimbus"] = "abc"
        output = self.run_verifier(candidate=malformed)
        self.assertIn("candidate.nimbus must be a full lowercase commit SHA", output)


if __name__ == "__main__":
    unittest.main()
