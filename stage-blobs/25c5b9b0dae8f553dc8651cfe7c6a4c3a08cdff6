import base64
import json
import struct
import tempfile
import unittest
from pathlib import Path

from migrate_project import inspect_rpp, mapped_instances, prepare_migration


FIXTURE = r'''<REAPER_PROJECT 0.1 "7.20" 0
  <TRACK {A}
    <FXCHAIN
      <VST "VST3: Kontakt 7" Kontakt 0 "" 0
        <VSTCHUNK
          opaque-state
        >
        PRESET "C:\Kontakt Libraries\A \"quoted\" Piano.nki"
        <PARMENV 0 0
          PT 0 0 0
        >
      >
      <VST "VST3: Contact Utility" Acme 0 "" 0
        PRESET "C:\ignored.nki"
      >
    >
    NAME "Strings"
  >
>'''


class RppInventoryTests(unittest.TestCase):
    def test_nested_plugin_blocks_and_lookalike(self):
        report = inspect_rpp(FIXTURE.splitlines(keepends=True), "fixture.RPP")
        self.assertEqual(report["track_count"], 1)
        self.assertEqual(report["kontakt_instance_count"], 1)
        kontakt = report["kontakt_instances"][0]
        self.assertEqual((kontakt["track_name"], kontakt["fx_chain"], kontakt["fx_index"]), ("Strings", "FXCHAIN", 0))
        self.assertGreater(kontakt["opaque_state"]["plugin_block_bytes"], 0)
        self.assertEqual(kontakt["opaque_state"]["chunk_markers"], ["VSTCHUNK"])
        lines = FIXTURE.splitlines(keepends=True)
        start = next(i for i, line in enumerate(lines) if "Kontakt 7" in line)
        depth = 0
        for end in range(start, len(lines)):
            if lines[end].lstrip().startswith("<"):
                depth += 1
            elif lines[end].strip() == ">":
                depth -= 1
                if depth == 0:
                    break
        expected = sum(len(line.encode("utf-8")) for line in lines[start : end + 1])
        self.assertEqual(kontakt["opaque_state"]["plugin_block_bytes"], expected)

    def test_quoted_escaped_path_is_visible_but_unconfirmed(self):
        report = inspect_rpp(FIXTURE.splitlines(keepends=True), "fixture.RPP")
        refs = report["kontakt_instances"][0]["visible_instrument_refs"]
        self.assertEqual(len(refs), 1)
        self.assertEqual(refs[0]["path"], 'C:\\Kontakt Libraries\\A "quoted" Piano.nki')
        self.assertFalse(refs[0]["confirmed_active_preset"])

    def test_automation_is_reported_as_unmapped(self):
        report = inspect_rpp(FIXTURE.splitlines(keepends=True), "fixture.RPP")
        risk = report["kontakt_instances"][0]["automation_risk"]
        self.assertEqual([x["kind"] for x in risk["track_markers"]], ["PARMENV"])
        self.assertEqual([x["kind"] for x in risk["inside_plugin_block_markers"]], ["PARMENV"])
        self.assertEqual(risk["mapping"], "unverified")

    def test_malformed_nesting_is_refused(self):
        with self.assertRaisesRegex(ValueError, "unclosed"):
            inspect_rpp(["<REAPER_PROJECT 0.1\n", "  <TRACK\n", ">\n"], "broken.RPP")

    def test_automation_mapping_is_skipped_before_script_preparation(self):
        report = inspect_rpp(FIXTURE.splitlines(keepends=True), "fixture.RPP")
        with tempfile.TemporaryDirectory() as root:
            multi = Path(root) / "Strings.kontra-multi"
            multi.write_text("{}", encoding="utf-8")
            plans, skipped = mapped_instances(report, [f"0:0={multi}"])
        self.assertEqual(plans, [])
        self.assertIn("automation", skipped[0]["reason"])

    def test_prepare_embeds_verified_state_without_touching_source(self):
        with tempfile.TemporaryDirectory() as root:
            directory = Path(root)
            source = directory / "song.RPP"
            source.write_text(FIXTURE, encoding="utf-8")
            original = source.read_bytes()
            multi = directory / "Strings.kontra-multi"
            multi.write_text("{}", encoding="utf-8")
            exporter = directory / "state-exporter"
            exporter.write_text(
                "#!/usr/bin/env python3\n"
                "import json, pathlib, sys\n"
                "out = pathlib.Path(sys.argv[3])\n"
                "blob = b'KONTRA fixture state'\n"
                "out.write_bytes(blob)\n"
                "print(json.dumps({'state': str(out), 'state_bytes': len(blob), "
                "'envelope_round_trip_verified': True, 'imported_parts': ["
                "{'missing_samples': ['sample.wav'], 'effect_warnings': ['unsupported effect']}]}))\n",
                encoding="utf-8",
            )
            exporter.chmod(0o755)
            safe_fixture = FIXTURE.replace(
                "        <PARMENV 0 0\n          PT 0 0 0\n        >\n", ""
            )
            report = inspect_rpp(safe_fixture.splitlines(keepends=True), str(source))
            plans, skipped = mapped_instances(report, [f"0:0={multi}"])
            self.assertEqual(skipped, [])
            output = directory / "song-k.CONVERTED.RPP"
            script = directory / "migrate.lua"
            prepared = prepare_migration(source, output, script, exporter, plans)
            contents = script.read_text(encoding="utf-8")
            embedded = prepared["prepared_mappings"][0]
            raw = b"KONTRA fixture state"
            framed = struct.pack("<II", len(raw), 1) + raw
            framed_base64 = base64.b64encode(framed).decode("ascii")
            readback_base64 = base64.b64encode(framed + bytes(8)).decode("ascii")
            self.assertTrue(embedded["state_embedded_in_reaper_script"])
            self.assertEqual(embedded["reaper_chunk_frame"], "little-endian OAST byte length, flags=1")
            self.assertEqual(plans[0]["state_base64"], framed_base64)
            self.assertEqual(plans[0]["expected_readback_base64"], readback_base64)
            self.assertNotIn("state", embedded)
            self.assertIn("missing_samples", json.dumps(embedded))
            self.assertIn("TrackFX_SetNamedConfigParm", contents)
            self.assertIn("state_back ~= plan.expected", contents)
            self.assertIn("local settle_at = reaper.time_precise() + 2.0", contents)
            self.assertIn('exists(OUTPUT) or exists(TEMP) or exists(REPORT)', contents)
            self.assertNotIn('io.open(REPORT, "x")', contents)
            self.assertFalse(output.exists())
            self.assertEqual(source.read_bytes(), original)


if __name__ == "__main__":
    unittest.main()
