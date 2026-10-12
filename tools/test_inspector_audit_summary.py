"""Synthetic contract tests; no installed libraries or plugin are opened."""

import json
from pathlib import Path
import tempfile
import unittest

from inspector_audit_summary import summarize


KONTAKT = "/mnt/MAIN_STORAGE/Libraries/Kontakt/Example/Instrument.nki"
MULTI = "/mnt/MAIN_STORAGE/Libraries/Kontakt/Example/Rack.nkm"
BANK = "/mnt/MAIN_STORAGE/Libraries/UVI/Example/Bank.ufs"
MEMBER = "Presets/Part.uvip"


def report(kind="kontakt", path=KONTAKT, program=None):
    metadata = ({"program_count": 1, "programs": [{"program": 0, "rack_slot": 2,
                 "program_within_slot": 0}]} if kind.startswith("kontakt") else
                {"xml_bytes": 100, "element_counts": {"Program": 1}})
    stages = [{"name": "container", "status": "ok"},
              {"name": "source_frontend", "status": "ok"}]
    if kind == "uvi-bank":
        stages.append({"name": "translate", "status": "ok"})
    stages += [{"name": "runtime", "status": "skipped"},
               {"name": "audio", "status": "skipped"}]
    return {"format": "kontra-library-inspector", "schema": 1,
            "item": {"kind": kind, "path": path, "program": program},
            "mode": {"initialize": False, "ephemeral_source": False, "summary": True},
            "status": "ok", "stages": stages, "metadata": metadata,
            "scripts": [{"id": "ksp:slot0", "program": 0 if kind.startswith("kontakt") else None,
                         "construct_count": 2, "status_counts": {"Implemented": 2},
                         "compiler": {"status": "ok", "source_language": "Falcon Lua",
                                      "executable_frontend": "Luau"} if kind == "uvi-bank" else None}],
            "converted": {"status": "ok" if kind == "uvi-bank" else "not_run",
                          "summary": {"groups": 1} if kind == "uvi-bank" else None},
            "ui": {"status": "not_evaluated", "interfaces": []}, "diagnostics": [],
            "evidence": {"runtime": "not_run", "audio": "not_run",
                         "native_equivalence": "unknown"}}


class SummaryTests(unittest.TestCase):
    def run_summary(self, manifest_rows, report_rows, initialized_rows=None):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            manifest = root / "manifest.tsv"
            manifest.write_text("\n".join(manifest_rows) + "\n")
            results = root / "reports.jsonl"
            results.write_text("\n".join(json.dumps(x) for x in report_rows) + "\n")
            initialized_paths = []
            if initialized_rows is not None:
                initialized = root / "initialized.jsonl"
                initialized.write_text("\n".join(json.dumps(x) for x in initialized_rows) + "\n")
                initialized_paths.append(initialized)
            return summarize(manifest, [results], initialized_paths)

    def test_uvi_bank_member_identity_and_static_stage_boundaries(self):
        s = self.run_summary([f"uvi-program\t{BANK}::{MEMBER}"],
                             [report("uvi-bank", BANK, MEMBER)])
        self.assertEqual(s["admission"], "complete_static_catalog")
        self.assertTrue(s["catalog_records_complete"])
        self.assertEqual(s["totals"]["ir_converted"], 1)
        self.assertEqual(s["totals"]["compiler_status_counts"], {"ok": 1})
        self.assertEqual(s["totals"]["ui_projected"], 0)
        self.assertEqual(s["native_parity"], "unknown")
        self.assertEqual(s["libraries"]["UVI/Example"]["runtime"], {"not_run": 1})

    def test_missing_duplicate_and_collision_fail_closed(self):
        s = self.run_summary([f"kontakt\t{KONTAKT}", f"kontakt\t{KONTAKT}",
                              f"uvi-program\t{BANK}::{MEMBER}"],
                             [report(), report(), report("uvi-bank", BANK, "Presets/Other.uvip")])
        self.assertEqual(s["admission"], "incomplete")
        self.assertFalse(s["catalog_records_complete"])
        for code in ("duplicate_manifest_id", "duplicate_report_id", "missing_report_id",
                     "unexpected_report_id"):
            self.assertGreater(s["fault_counts"][code], 0)
        self.assertEqual(s["totals"]["observed"], 1)

    def test_compact_script_counts_and_gap_retained(self):
        r = report()
        r["status"] = "partial"
        r["scripts"][0]["status_counts"] = {"Implemented": 1, "Unsupported": 1}
        s = self.run_summary([f"kontakt\t{KONTAKT}"], [r])
        self.assertEqual(s["totals"]["constructs"], 2)
        self.assertEqual(s["totals"]["script_status_counts"], {"Implemented": 1, "Unsupported": 1})
        self.assertEqual(s["totals"]["source_inspected"], 1)
        self.assertEqual(s["totals"]["source_semantics_complete"], 0)

    def test_multi_partial_programs_never_all_programs(self):
        r = report("kontakt-multi", MULTI)
        r["metadata"]["program_count"] = 2
        s = self.run_summary([f"kontakt-multi\t{MULTI}"], [r])
        self.assertEqual(s["totals"]["kontakt_all_programs"], 0)
        self.assertEqual(s["admission"], "incomplete")
        self.assertFalse(s["catalog_records_complete"])
        self.assertIn("programs", s["fault_counts"])

    def test_skipped_stages_cannot_claim_success(self):
        r = report("uvi-bank", BANK, MEMBER)
        r["stages"][2]["status"] = "skipped"
        s = self.run_summary([f"uvi-program\t{BANK}::{MEMBER}"], [r])
        self.assertEqual(s["totals"]["ir_converted"], 0)
        self.assertEqual(s["totals"]["ui_projected"], 0)

    def test_translation_failure_retains_completed_source_scan(self):
        r = report("uvi-bank", BANK, MEMBER)
        r["status"] = "failed"
        r["stages"][2]["status"] = "failed"
        r["converted"]["status"] = "not_run"
        r["diagnostics"] = [{"stage": "translate", "category": "Translation"}]
        s = self.run_summary([f"uvi-program\t{BANK}::{MEMBER}"], [r])
        self.assertEqual(s["totals"]["source_inspected"], 1)
        self.assertEqual(s["totals"]["ir_converted"], 0)
        self.assertEqual(s["totals"]["source_semantics_complete"], 0)
        self.assertEqual(s["admission"], "incomplete")
        self.assertTrue(s["catalog_records_complete"])

    def test_uvi_compiler_failure_is_a_source_gap(self):
        r = report("uvi-bank", BANK, MEMBER)
        r["status"] = "partial"
        r["stages"][1]["status"] = "failed"
        r["scripts"][0]["compiler"]["status"] = "failed"
        s = self.run_summary([f"uvi-program\t{BANK}::{MEMBER}"], [r])
        self.assertEqual(s["totals"]["compiler_status_counts"], {"failed": 1})
        self.assertEqual(s["totals"]["source_inspected"], 0)
        self.assertEqual(s["totals"]["ir_converted"], 1)
        self.assertTrue(s["catalog_records_complete"])
        self.assertEqual(s["admission"], "incomplete")

    def test_provenance_hashes_and_expected_observed_per_library(self):
        s = self.run_summary([f"kontakt\t{KONTAKT}", f"uvi-program\t{BANK}::{MEMBER}"],
                             [report()])
        self.assertEqual(len(s["provenance"]), 2)
        self.assertTrue(all(len(p["sha256"]) == 64 for p in s["provenance"]))
        self.assertEqual(s["libraries"]["UVI/Example"]["expected"], 1)
        self.assertEqual(s["libraries"]["UVI/Example"]["observed"], 0)
        self.assertEqual(s["libraries"]["Kontakt/Example"]["observed"], 1)

    def test_initialized_sample_has_separate_denominator(self):
        r = report()
        r["item"]["program"] = 0
        r["mode"]["initialize"] = True
        r["stages"].insert(2, {"name": "initialize", "status": "ok"})
        r["stages"].insert(3, {"name": "ui_projection", "status": "ok"})
        r["converted"] = {"status": "ok", "summary": {"groups": 1}}
        r["ui"] = {"status": "ok", "interfaces": []}
        s = self.run_summary([f"kontakt\t{KONTAKT}"], [report()], [r])
        self.assertEqual(s["totals"]["expected"], 1)
        self.assertEqual(s["totals"]["initialized_attempted"], 1)
        self.assertEqual(s["totals"]["initialized_ir_ok"], 1)
        self.assertEqual(s["totals"]["initialized_ui_ok"], 1)
        self.assertEqual(s["totals"]["ir_converted"], 0)
        self.assertEqual(s["native_parity"], "unknown")
        self.assertEqual(s["provenance"][-1]["role"], "initialized_report_jsonl")

    def test_initialized_skipped_stage_does_not_count_ui(self):
        r = report("uvi-bank", BANK, MEMBER)
        r["mode"]["initialize"] = True
        r["stages"].insert(3, {"name": "initialize", "status": "skipped"})
        r["ui"] = {"status": "ok", "interfaces": []}
        s = self.run_summary([f"uvi-program\t{BANK}::{MEMBER}"],
                             [report("uvi-bank", BANK, MEMBER)], [r])
        self.assertEqual(s["totals"]["initialized_ir_ok"], 1)
        self.assertEqual(s["totals"]["initialized_ui_ok"], 0)
        self.assertEqual(s["totals"]["initialized_failed"], 1)

    def test_initialized_ui_omission_is_valid_partial_stage_not_success(self):
        initialized = report("uvi-bank", BANK, MEMBER)
        initialized["mode"]["initialize"] = True
        initialized["stages"].insert(3, {"name": "ui_projection", "status": "partial"})
        initialized["stages"].insert(4, {"name": "initialize", "status": "ok"})
        initialized["ui"] = {"status": "partial", "interfaces": [{"unsupported": 1}]}
        initialized["diagnostics"] = [{"stage": "ui_projection", "category": "NotModeled"}]
        s = self.run_summary([f"uvi-program\t{BANK}::{MEMBER}"],
                             [report("uvi-bank", BANK, MEMBER)], [initialized])
        self.assertTrue(s["catalog_records_complete"])
        self.assertEqual(s["totals"]["initialized_attempted"], 1)
        self.assertEqual(s["totals"]["initialized_ir_ok"], 1)
        self.assertEqual(s["totals"]["initialized_ui_ok"], 0)
        self.assertEqual(s["totals"]["initialized_failed"], 1)
        self.assertNotIn("initialized_stages", s["fault_counts"])
        initialized["ui"]["status"] = "ok"
        inconsistent = self.run_summary([f"uvi-program\t{BANK}::{MEMBER}"],
                                        [report("uvi-bank", BANK, MEMBER)], [initialized])
        self.assertEqual(inconsistent["totals"]["initialized_ui_ok"], 0)

    def test_initialized_identity_fault_does_not_change_static_record_coverage(self):
        stray = report("uvi-bank", BANK, "Presets/NotInManifest.uvip")
        stray["mode"]["initialize"] = True
        s = self.run_summary([f"uvi-program\t{BANK}::{MEMBER}"],
                             [report("uvi-bank", BANK, MEMBER)], [stray])
        self.assertTrue(s["catalog_records_complete"])
        self.assertEqual(s["fault_counts"]["unexpected_initialized_id"], 1)
        self.assertEqual(s["admission"], "incomplete")

    def test_honest_parse_failures_keep_record_coverage_complete(self):
        kontakt = report()
        kontakt["status"] = "failed"
        kontakt["stages"][1] = {"name": "parse", "status": "failed"}
        kontakt["metadata"] = {}
        kontakt["scripts"] = []
        kontakt["diagnostics"] = [{"stage": "parse", "category": "ProgramDecode"}]
        uvi = report("uvi-bank", BANK, MEMBER)
        uvi["status"] = "failed"
        uvi["stages"] = [{"name": "container", "status": "ok"},
                         {"name": "parse", "status": "failed"},
                         {"name": "runtime", "status": "skipped"},
                         {"name": "audio", "status": "skipped"}]
        uvi["metadata"] = {}
        uvi["scripts"] = []
        uvi["converted"] = {"status": "not_run"}
        uvi["diagnostics"] = [{"stage": "parse", "category": "Xml"}]
        s = self.run_summary([f"kontakt\t{KONTAKT}", f"uvi-program\t{BANK}::{MEMBER}"],
                             [kontakt, uvi])
        self.assertTrue(s["catalog_records_complete"])
        self.assertEqual(s["totals"]["observed"], 2)
        self.assertEqual(s["totals"]["source_inspected"], 0)
        self.assertEqual(s["admission"], "incomplete")
        self.assertEqual(s["fault_counts"], {})

    def test_selected_multi_numeric_collision_is_duplicate(self):
        s = self.run_summary([f"kontakt-multi\t{MULTI}#0", f"kontakt-multi\t{MULTI}#00"], [])
        self.assertEqual(s["admission"], "incomplete")
        self.assertEqual(s["fault_counts"]["duplicate_manifest_id"], 1)

    def test_false_runtime_evidence_fails_closed(self):
        r = report()
        r["evidence"]["runtime"] = "ok"
        s = self.run_summary([f"kontakt\t{KONTAKT}"], [r])
        self.assertEqual(s["admission"], "incomplete")
        self.assertIn("evidence", s["fault_counts"])


if __name__ == "__main__":
    unittest.main()
