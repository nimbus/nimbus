"""Tests for the node_compat corpus baseline seam.

The nightly reads the baseline to decide which corpus failures stop the lane,
so every guard that protects the measurement behind it is a contract.
"""

import importlib.util
import sys
import unittest
from pathlib import Path


SCRIPT_PATH = (
    Path(__file__).resolve().parent / "runtime" / "node" / "corpus_baseline.py"
)
SPEC = importlib.util.spec_from_file_location("node_corpus_baseline", SCRIPT_PATH)
corpus_baseline = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = corpus_baseline
SPEC.loader.exec_module(corpus_baseline)


def start(batch, test_name="a_batch_test"):
    return {"kind": "batch_start", "batch": batch, "test_name": test_name}


def complete(batch, fixture_count, test_name="a_batch_test"):
    return {
        "kind": "batch_complete",
        "batch": batch,
        "fixture_count": fixture_count,
        "test_name": test_name,
    }


def fixture(batch=None, path="test/parallel/test-one.js", lane="node20"):
    record = {"lane": lane, "test_relative_path": path, "outcome": "passed"}
    if batch is not None:
        record["batch"] = batch
    return record


class CheckBatchCompletenessTests(unittest.TestCase):
    def test_accepts_a_batch_that_ran_to_its_end(self):
        records = [
            start("streams/node20"),
            fixture("streams/node20", "test/parallel/test-one.js"),
            fixture("streams/node20", "test/parallel/test-two.js"),
            complete("streams/node20", 2),
        ]
        corpus_baseline.check_batch_completeness(records)

    def test_accepts_a_measurement_with_no_batches(self):
        corpus_baseline.check_batch_completeness([fixture()])

    def test_refuses_a_batch_that_was_killed_partway(self):
        records = [
            start("streams/node20", test_name="node20_streams_subset"),
            fixture("streams/node20", "test/parallel/test-one.js"),
        ]
        with self.assertRaises(SystemExit) as caught:
            corpus_baseline.check_batch_completeness(records)
        message = str(caught.exception)
        self.assertIn("streams/node20", message)
        self.assertIn("node20_streams_subset", message)
        self.assertIn("measured 1 fixture(s)", message)

    def test_refuses_a_batch_that_lost_records(self):
        records = [
            start("streams/node20"),
            fixture("streams/node20", "test/parallel/test-one.js"),
            complete("streams/node20", 2),
        ]
        with self.assertRaises(SystemExit) as caught:
            corpus_baseline.check_batch_completeness(records)
        self.assertIn("executed 2 fixture(s)", str(caught.exception))

    def test_refuses_a_completion_whose_start_is_missing(self):
        records = [
            fixture("streams/node20", "test/parallel/test-one.js"),
            complete("streams/node20", 1),
        ]
        with self.assertRaises(SystemExit) as caught:
            corpus_baseline.check_batch_completeness(records)
        self.assertIn("never started", str(caught.exception))

    def test_refuses_a_batch_killed_before_its_first_fixture(self):
        # The start record is the only trace such a batch leaves. Without it
        # the batch would be absent rather than incomplete, and the merge
        # would succeed.
        with self.assertRaises(SystemExit) as caught:
            corpus_baseline.check_batch_completeness([start("networking/node22")])
        self.assertIn("measured 0 fixture(s)", str(caught.exception))

    def test_a_fixture_outside_a_batch_is_not_attributed_to_one(self):
        records = [
            start("streams/node20"),
            fixture("streams/node20", "test/parallel/test-one.js"),
            complete("streams/node20", 1),
            fixture(None, "test/parallel/test-standalone.js"),
        ]
        corpus_baseline.check_batch_completeness(records)


if __name__ == "__main__":
    unittest.main()
