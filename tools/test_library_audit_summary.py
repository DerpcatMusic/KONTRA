#!/usr/bin/env python3
"""Fail-closed checks for audit completeness and same-audition evidence."""
import csv
import json
from pathlib import Path
import tempfile
import unittest

import library_audit_summary as audit


class EvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.item = "/owner/Libraries/Kontakt/Example/Instruments/one.nki"

    def tsv(self, rows):
        path = self.root / "results.tsv"
        with path.open("w", newline="") as stream:
            writer = csv.DictWriter(stream, delimiter="\t", fieldnames=list(rows[0]))
            writer.writeheader()
            writer.writerows(rows)
        return path

    def row(self):
        return dict(path=self.item, library="Example", loads="yes", ui="original-ok",
                    plays_note="yes", audition_status="matched-note-plan", fallback_note="0",
                    note_picked='{"0":[60,64]}', pick_source='{"0":"zone_coverage"}',
                    keyswitch_picked='{"0":null}')

    def record(self):
        return dict(self.row(), revision="a" * 64,
                    programs=[dict(program=0, loaded=True, pick=[60, 64], keyswitch=None)])

    def cache(self, name, record):
        (self.root / "cache").mkdir(exist_ok=True)
        (self.root / "cache" / name).write_text(json.dumps(record))

    def test_absent_metrics_and_explicit_unknown_remain_distinct(self):
        row = self.row()
        row["family_match"] = "UNKNOWN"
        row["fx_slots_dropped"] = '{"enabled":0,"bypassed":0}'
        result, _, _ = audit.summarize("fixture", self.tsv([row]), {"Example": {self.item}})
        info = result["libraries"]["Example"]
        self.assertEqual(info["statuses"]["family_match"], {"UNKNOWN": 1})
        self.assertTrue(info["dropped_slots"]["fx_slots_dropped"]["totals_complete"])
        missing = info["dropped_slots"]["filter_slots_dropped"]
        self.assertEqual(missing["schema_unknown_items"], 1)
        self.assertFalse(missing["totals_complete"])
        self.assertEqual(result["engine_sha256_counts"], {audit.UNKNOWN: 1})

    def test_partial_manifest_is_not_complete_library_coverage(self):
        unseen = self.item.replace("one.nki", "two.nki")
        result, _, _ = audit.summarize("fixture", self.tsv([self.row()]), {"Example": {self.item, unseen}})
        self.assertEqual(result["libraries"]["Example"]["coverage"], "partial")
        self.assertEqual(result["libraries"]["Example"]["missing_items"], [unseen])

    def test_ambiguous_cache_does_not_claim_a_binary_digest(self):
        self.cache("one.json", self.record())
        other = self.record()
        other["revision"] = "b" * 64
        self.cache("two.json", other)
        result, _, cache = audit.summarize("fixture", self.tsv([self.row()]), {})
        self.assertEqual(cache[self.item]["cache_status"], "ambiguous-cache-records")
        self.assertEqual(result["engine_sha256_counts"], {audit.UNKNOWN: 1})

    def test_duplicate_items_fail_instead_of_inflating_coverage(self):
        with self.assertRaisesRegex(ValueError, "duplicate"):
            audit.read_scan(self.tsv([self.row(), self.row()]))

    def test_invalid_drop_counts_cannot_become_known_zero(self):
        for value in (None, "unknown", '{"enabled":true,"bypassed":0}',
                      '{"enabled":-1,"bypassed":0}', '{"enabled":0}'):
            self.assertIsNone(audit.drop_counts(value))

    def test_partial_multi_fallback_and_held_note_exclude_audio_comparison(self):
        left, right = self.row(), self.row()
        lr, rr = self.record(), self.record()
        self.assertEqual(audit.pair_eligibility(left, right, lr, rr), [])
        rr["programs"][0]["held_key"] = 61
        self.assertIn("different-held_key-plans", audit.pair_eligibility(left, right, lr, rr))
        right["note_picked"] = '{"0":null,"1":[60,64]}'
        right["fallback_note"] = "1"
        rr["programs"].append(dict(program=1, loaded=False))
        reasons = audit.pair_eligibility(left, right, lr, rr)
        self.assertIn("right:missing-or-partial-program-picks", reasons)
        self.assertIn("right:fallback-or-unknown", reasons)
        self.assertIn("right:program-admission-not-complete", reasons)

    def test_corpus_distinguishes_first_program_from_complete_multi(self):
        item = self.item.replace("one.nki", "rack.nkm")
        path = self.root / "parse.jsonl"
        records = [dict(id=item, status="started"),
                   dict(id=item, status="done", tier="parse", stage="ok", load=dict(ok=False, kind="NoZone"),
                        multi=dict(programs=4, checked=1)),
                   dict(id=self.item, status="started")]
        path.write_text("".join(json.dumps(record) + "\n" for record in records))
        result = audit.summarize_corpus("fixture", path, {"Example": {item, self.item}})
        info = result["libraries"]["Example"]
        self.assertEqual(result["finished_items"], 1)
        self.assertEqual(result["started_without_finished_items"], [self.item])
        self.assertEqual(info["coverage"], "partial")
        self.assertEqual(info["embedded_programs_reported"], 4)
        self.assertEqual(info["embedded_programs_checked"], 1)
        self.assertEqual(info["multis_with_partial_program_checks"], 1)
        self.assertEqual(info["audio_witnessed"], 0)
        self.assertEqual(info["load_kinds"], {"NoZone": 1})


if __name__ == "__main__":
    unittest.main()
