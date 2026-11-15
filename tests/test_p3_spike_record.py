#!/usr/bin/env python3
"""Offline evidence tooling checks; no TV, credentials or network required."""
import contextlib
import copy
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("p3_spike_record", Path(__file__).resolve().parents[1] / "scripts/p3_spike_record.py")
spike = importlib.util.module_from_spec(spec)
spec.loader.exec_module(spike)


class SpikeRecordTests(unittest.TestCase):
    def fresh(self):
        return spike.new_record("a" * 40, False, "2026-11-14")

    def test_complete_matrix_starts_untested_and_never_claims_hardware_proof(self):
        record = self.fresh()
        self.assertEqual(len(record["observations"]), 30)
        self.assertEqual(set(record["observations"].values()), {"untested"})
        report = spike.render(record)
        self.assertIn("no automated TV connection or wire evidence", report)
        self.assertIn("No row establishes unsupported", report)

    def test_invalid_or_private_data_cannot_enter_report(self):
        for mutate in (
            lambda r: r.update(token="private-marker"),
            lambda r: r.update(build="private-marker"),
            lambda r: r.update(date="2026-02-30"),
            lambda r: r.update(dirty=1),
            lambda r: r.update(format=True),
            lambda r: r["observations"].update({"baseline/hdmi1": "unsupported"}),
            lambda r: r["observations"].update({"baseline/ime_start": "private-marker"}),
            lambda r: r["observations"].pop("baseline/hdmi1"),
        ):
            record = copy.deepcopy(self.fresh())
            mutate(record)
            with self.assertRaises(ValueError):
                spike.render(record)

    def test_cli_records_one_row_preserving_input_and_refuses_overwrite(self):
        with tempfile.TemporaryDirectory() as directory:
            original = Path(directory) / "original.json"
            updated = Path(directory) / "updated.json"
            self.assertEqual(spike.main(["prepare", str(original), "--build", "a" * 40]), 0)
            self.assertEqual(spike.main(["record", str(original), str(updated), "--row", "baseline/hdmi1", "--result", "not_observed"]), 0)
            self.assertEqual(spike.load(original)["observations"]["baseline/hdmi1"], "untested")
            observations = spike.load(updated)["observations"]
            self.assertEqual(observations["baseline/hdmi1"], "not_observed")
            self.assertEqual(list(observations.values()).count("untested"), 29)
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(spike.main(["prepare", str(updated), "--build", "b" * 40]), 2)
            self.assertEqual(spike.load(updated)["build"], "a" * 40)

    def test_invalid_cli_values_are_not_echoed(self):
        for arguments in (
            ["record", "input", "output", "--row", "private-marker", "--result", "observed"],
            ["prepare", "output", "--build", "a" * 40, "--private-marker"],
        ):
            output, errors = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
                with self.assertRaises(SystemExit) as failure:
                    spike.main(arguments)
            self.assertEqual(failure.exception.code, 2)
            self.assertNotIn("private-marker", output.getvalue() + errors.getvalue())

    def test_duplicate_oversized_and_secret_input_fail_without_echo(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "record.json"
            for payload in ('{"format":1,"format":1}', 'x' * (spike.MAX_RECORD_BYTES + 1), '{"token":"private-marker"}'):
                path.write_text(payload)
                output, errors = io.StringIO(), io.StringIO()
                with contextlib.redirect_stdout(output), contextlib.redirect_stderr(errors):
                    self.assertEqual(spike.main(["render", str(path)]), 2)
                self.assertEqual(output.getvalue(), "")
                self.assertNotIn("private-marker", errors.getvalue())


if __name__ == "__main__":
    unittest.main()
