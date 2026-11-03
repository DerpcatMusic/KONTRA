#!/usr/bin/env python3
"""Self-tests for the census harness. No library, no binaries, no network.

    python3 tools/census/test_census.py

Each case is a defect the 2026-11-01 harness had, reduced to the smallest
fixture that shows it: a corpus that is not under `/home/<user>/Armory/VSTs/
Kontakt1`, a silent instrument whose renders did not all answer, a shard swept
twice, a `compile.tsv` whose paths are absolute.
"""
import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import extract  # noqa: E402
import instruments  # noqa: E402
import library  # noqa: E402
import merge_census  # noqa: E402


def dump(obj, path):
    with open(path, "w") as f:
        json.dump(obj, f)


def slurp(path):
    with open(path) as f:
        return json.load(f)


class Paths(unittest.TestCase):
    """The product folder is derived from the library root, not from an index."""

    def setUp(self):
        self.root = library.ROOT
        library.ROOT = "/mnt/lib"
        self.addCleanup(lambda: setattr(library, "ROOT", self.root))

    def test_product_folder_above_instruments(self):
        p = "/mnt/lib/SomeLibrary/Instruments/Keys/a.nki"
        self.assertEqual(library.library_dir(p), "/mnt/lib/SomeLibrary")
        self.assertEqual(library.library_of(p), "SomeLibrary")

    def test_nested_product_folder(self):
        p = "/mnt/lib/Publisher/Product/Instruments/Sub/a.nki"
        self.assertEqual(library.library_of(p), "Product")

    def test_loose_tree_without_instruments(self):
        p = "/mnt/lib/#Non NICNT/Publisher/Product/a.nki"
        self.assertEqual(library.library_dir(p), "/mnt/lib/#Non NICNT")
        self.assertEqual(library.library_of(p), "#Non NICNT")

    def test_instruments_at_the_root(self):
        self.assertEqual(library.library_of("/mnt/lib/Instruments/a.nki"), "lib")

    def test_file_directly_in_the_root(self):
        self.assertEqual(library.library_dir("/mnt/lib/a.nki"), "/mnt/lib")

    def test_path_outside_the_root(self):
        self.assertEqual(library.library_of("/elsewhere/lib/Instruments/a.nki"), "lib")

    def test_the_old_index_raises_on_these_paths(self):
        """The defect, as it was: evidence that the fixture is a real one."""
        # `split("/")[6]` on a library one level shallower than the 2026-11-01
        # corpus: no such component at all.
        with self.assertRaises(IndexError):
            "/mnt/lib/SomeLibrary/Instruments/a.nki".split("/")[6]
        # ...and where a component does exist at 6 it is the file, not the
        # library: `/mnt/lib/SomeLibrary/Instruments/Keys/a.nki`.
        self.assertEqual(
            "/mnt/lib/SomeLibrary/Instruments/Keys/a.nki".split("/")[6], "a.nki"
        )
        # `ui_sample.py` needed a `VSTs` component, which such a root has not.
        with self.assertRaises(ValueError):
            "/mnt/lib/SomeLibrary/Instruments/Keys/a.nki".split("/").index("VSTs")


class Silence(unittest.TestCase):
    """An instrument is silent only when every probed key answered."""

    def run_analysis(self, keysweep, zones=None):
        work = tempfile.mkdtemp(prefix="census-test-")
        zones = zones or [4] * len(keysweep)
        paths = [f"/mnt/lib/Lib/Instruments/i{n}.nki" for n in range(len(keysweep))]
        census = [{"path": p, "name": f"i{n}", "loaded": True, "zones_ready": zones[n]}
                  for n, p in enumerate(paths)]
        dump(census, os.path.join(work, "census.json"))
        with open(os.path.join(work, "keysweep-results.jsonl"), "w") as f:
            for n, peaks in enumerate(keysweep):
                f.write(json.dumps({
                    "path": paths[n], "name": f"i{n}", "zones_ready": zones[n], "scripts": 1,
                    "peaks": peaks, "sounds_at": [k for k, v in peaks.items() if v > 0],
                    "best_peak": max(peaks.values()) if peaks else 0.0,
                    "error": None if len(peaks) == 9 else "kontra-render timeout 180s",
                }) + "\n")
        wall = {str(k): 0.0 for k in (36, 43, 48, 55, 60, 64, 67, 72, 84)}
        eighth = {k: v for k, v in wall.items() if k != "84"}
        loud = dict(wall, **{"60": 0.5})
        env = dict(os.environ, CENSUS_WORK=work)
        subprocess.run([sys.executable, os.path.join(HERE, "analyze_census.py")],
                       env=env, check=True, capture_output=True)
        return slurp(os.path.join(work, "census_summary.json"))

    def test_only_fully_measured_silence_is_silent(self):
        wall = {str(k): 0.0 for k in (36, 43, 48, 55, 60, 64, 67, 72, 84)}
        eighth = {k: v for k, v in wall.items() if k != "84"}
        loud = dict(wall, **{"60": 0.5})
        out = self.run_analysis([wall, eighth, loud])
        self.assertEqual(out["totals"]["silent_with_zones"], 1, "only i0 answered all nine")
        self.assertEqual(out["totals"]["silent_unmeasured"], 1, "i1 lost a render")
        self.assertEqual([s["name"] for s in out["silent"]], ["i0"])
        self.assertEqual(out["silent"][0]["keys_answered"], 9)

    def test_a_record_that_counted_no_zones_is_not_silence(self):
        """`zones_ready` is null when the sweep did not match its sample line.

        `s.get("zones_ready", 0)` is then `None`, and `None > 0` raised
        TypeError out of the analysis -- one unmatched record ended the report.
        """
        wall = {str(k): 0.0 for k in (36, 43, 48, 55, 60, 64, 67, 72, 84)}
        out = self.run_analysis([wall], zones=[None])
        self.assertEqual(out["totals"]["silent_with_zones"], 0)
        self.assertEqual(out["totals"]["silent_unmeasured"], 0,
                         "a null count is unmeasured, not silent")


class CompileFail(unittest.TestCase):
    """`kspcheck`'s unattributable lines are counted, not raised."""

    def test_a_line_with_no_instrument_directory_is_reported(self):
        work = tempfile.mkdtemp(prefix="census-compile-")
        path = "/mnt/lib/SomeLibrary/Instruments/a.nki"
        with open(os.path.join(work, "manifest.jsonl"), "w") as f:
            f.write(json.dumps({"i": 7, "path": path}) + "\n")
        tsv = os.path.join(work, "compile.tsv")
        with open(tsv, "w") as f:
            f.write("ok\tscripts/7/slot-0.ksp\t\n")
            f.write("err\tscripts/7/slot-1.ksp\tunknown function num_slices_zone (at byte 2)\n")
            # A message with a tab in it: only the first two fields split.
            f.write("err\tscripts/7/slot-2.ksp\ta message\twith a tab\n")
            # `kspcheck` answers `<line N>` for an input line it could not read.
            f.write("err\t<line 9>\tnot UTF-8\n")
            f.write("err\t<line 10>\tnot UTF-8 either\n")
        out = os.path.join(work, "compile_fail.json")
        env = dict(os.environ, CENSUS_WORK=work)
        r = subprocess.run([sys.executable, os.path.join(HERE, "compile_fail.py"), tsv, out],
                           env=env, check=True, capture_output=True, text=True)
        rec = slurp(out)
        self.assertEqual(rec["7"]["path"], path)
        self.assertEqual(rec["7"]["errors"],
                         ["unknown function num_slices_zone (at byte 2)",
                          "a message\twith a tab"])
        self.assertIn("2 line(s) with no instrument directory", r.stderr)


class Extract(unittest.TestCase):
    """A timed-out extraction keeps the manifest's shape."""

    def test_a_timeout_record_has_every_field(self):
        work = tempfile.mkdtemp(prefix="census-extract-")
        slow = os.path.join(work, "slow.sh")
        with open(slow, "w") as f:
            f.write("#!/bin/sh\nsleep 5\n")
        os.chmod(slow, 0o755)
        saved = (extract.BIN, extract.TIMEOUT)
        extract.BIN, extract.TIMEOUT = slow, 0.2
        try:
            rec = extract.one((7, {"path": "/mnt/lib/L/Instruments/a.nki", "name": "a"},
                              os.path.join(work, "out")))
        finally:
            extract.BIN, extract.TIMEOUT = saved
        # `instruments.py` reads `r["name"]` unguarded: a record without it made
        # one extraction timeout a KeyError in a later step.
        for field in ("i", "path", "name", "ok", "slots", "bytes", "err"):
            self.assertIn(field, rec)
        self.assertEqual((rec["name"], rec["ok"], rec["slots"], rec["bytes"], rec["err"]),
                         ("a", False, 0, [], "timeout"))


class Sweep(unittest.TestCase):
    """One render that exits 0 with unusable output is a record, not a dead shard."""

    def test_a_render_with_no_summary_does_not_end_the_sweep(self):
        work = tempfile.mkdtemp(prefix="census-sweep-")
        files = os.path.join(work, "list")
        with open(files, "w") as f:
            f.write("/mnt/lib/L/Instruments/a.nki\n/mnt/lib/L/Instruments/b.nki\n")
        inspect = os.path.join(work, "inspect")
        with open(inspect, "w") as f:
            f.write("#!/bin/sh\necho 'refused: nothing here' >&2\nexit 1\n")
        render = os.path.join(work, "render")
        with open(render, "w") as f:
            f.write("#!/bin/sh\necho 'not a summary'\nexit 0\n")
        for p in (inspect, render):
            os.chmod(p, 0o755)
        out = os.path.join(work, "partA")
        env = dict(os.environ, KONTRA_INSPECT=inspect, KONTRA_RENDER=render, WORKERS="1")
        subprocess.run([sys.executable, os.path.join(HERE, "census_sweep.py"),
                        files, "0", out], env=env, check=True, capture_output=True)
        with open(os.path.join(out, "shard-0.jsonl")) as f:
            rows = [json.loads(line) for line in f if line.strip()]
        # Both files got a record: the second used to be lost with the first,
        # because the parse error came out of `one` and `ex.map` re-raised it.
        self.assertEqual(len(rows), 2)
        for row in rows:
            self.assertIn("without a usable summary", row["render_refusal"])
            self.assertNotIn("peak", row)


class UISample(unittest.TestCase):
    """The sample is drawn across the libraries, and a stale probe is not read."""

    def test_sample_spreads_and_a_failed_load_does_not_reuse_the_last_probe(self):
        work = tempfile.mkdtemp(prefix="census-ui-")
        libs, instruments = 50, 2
        census = []
        for lib in range(libs):
            for n in range(instruments):
                d = os.path.join(work, f"Lib{lib}", "Instruments")
                os.makedirs(d, exist_ok=True)
                census.append({"path": os.path.join(d, f"i{n}.nki"), "loaded": True,
                               "ui_unresolved": 1, "ui_resolved": 0})
        dump(census, os.path.join(work, "census.json"))
        # A previous run's probe: a load that fails before writing its own must
        # not be read as this instrument's assets.
        dump({"result": {"ui": [{"assets": [
            {"path": "Resources/pictures/stale.png", "read_status": "not_resolved"}]}]}},
             os.path.join(work, "ui-probe.json"))
        inspect = os.path.join(work, "inspect")
        with open(inspect, "w") as f:
            f.write("#!/bin/sh\ncase \"$2\" in\n"
                    "  *i0.nki) printf '%s' '{\"result\":{\"ui\":[{\"assets\":"
                    "[{\"path\":\"Resources/pictures/a.png\","
                    "\"read_status\":\"not_resolved\"}]}]}}' > \"$4\" ; exit 0 ;;\n"
                    "  *) sleep 2; exit 1 ;;\n"
                    "esac\n")
        os.chmod(inspect, 0o755)
        out = os.path.join(work, "ui_sample.json")
        env = dict(os.environ, CENSUS_WORK=work, KONTRA_INSPECT=inspect,
                   LIBRARY_ROOT=work, UI_SAMPLE_TIMEOUT="0.2")
        subprocess.run([sys.executable, os.path.join(HERE, "ui_sample.py"), "9", out],
                       env=env, check=True, capture_output=True)
        rows = slurp(out)
        self.assertLessEqual(len(rows), 9)
        # 49 libraries hold unresolved assets; the old truncation kept the
        # first five's two-apiece quota and could never reach the rest.
        drawn = {r["path"].split("/Instruments/")[0] for r in rows}
        self.assertGreater(len(drawn), 5,
                           "the sample is drawn from a handful of libraries only")
        for row in rows:
            if row["path"].endswith("i1.nki"):
                self.assertIn("error", row, "a failed load was given the last probe's data")
                self.assertIn("timed out", row["error"])
                self.assertNotIn("now_unresolved", row)
            else:
                self.assertEqual(row["now_unresolved"], 1)


class Merge(unittest.TestCase):
    """A shard swept twice must not double an instrument."""

    def test_last_write_wins_and_first_position_is_kept(self):
        records = [
            {"path": "/a", "peak": 0.0},
            {"path": "/b", "peak": 1.0},
            {"path": "/a", "peak": 2.0},
        ]
        merged = merge_census.merge(records)
        self.assertEqual([r["path"] for r in merged], ["/a", "/b"])
        self.assertEqual(merged[0]["peak"], 2.0, "the later record wins")

    def test_the_script_merges_two_shards(self):
        """A shard swept twice into the same directory, and two shards that
        overlap, both used to double the instrument."""
        work = tempfile.mkdtemp(prefix="census-merge-")
        for name, peak in (("shard-0.jsonl", 0.0), ("shard-1.jsonl", 9.0)):
            with open(os.path.join(work, name), "w") as f:
                f.write(json.dumps({"path": "/a", "peak": peak}) + "\n")
        out = os.path.join(work, "census.json")
        env = dict(os.environ, CENSUS_WORK=work)
        r = subprocess.run([sys.executable, os.path.join(HERE, "merge_census.py"),
                            os.path.join(work, "shard-0.jsonl"),
                            os.path.join(work, "shard-1.jsonl"), out],
                           env=env, check=True, capture_output=True, text=True)
        self.assertEqual(slurp(out)[0]["peak"], 9.0, "last write wins")
        self.assertIn("2 records from 2 shards -> 1 instruments", r.stderr)
        self.assertIn("1 repeated paths replaced", r.stderr)


class CompilePaths(unittest.TestCase):
    """`compile.tsv` holds the paths the README's `ls` produced — absolute."""

    def test_absolute_and_relative(self):
        for path in ("/var/tmp/gaps/scripts/1067/slot-0.ksp",
                     "scripts/1067/slot-0.ksp",
                     "../gaps/scripts/1067/slot-1.ksp"):
            self.assertEqual(instruments.instrument_of(path), "1067", path)

    def test_the_old_parse_took_the_wrong_component(self):
        path = "/var/tmp/gaps/scripts/1067/slot-0.ksp"
        self.assertEqual(path.split("/")[1], "var", "what the old parse returned")


class Build(unittest.TestCase):
    """`build_tsv.py` reads a corpus that is not at the old depth."""

    def test_a_library_under_mnt_builds(self):
        work = tempfile.mkdtemp(prefix="census-build-")
        path = "/mnt/lib/SomeLibrary/Instruments/a.nki"
        dump({
            "totals": {"keysweep_instruments": 0, "ui_instruments": 0,
                       "ui_unresolved_assets": 0, "silent_unmeasured": 0},
            "actions": [], "silent": [], "no_zones": [], "ui_by_library": [],
            "refusals": {"modulator category 3": {"instruments": 3,
                                                  "libraries": {"A": 2, "B": 1}}},
        }, os.path.join(work, "census_summary.json"))
        dump({"instruments": []}, os.path.join(work, "symbols.json"))
        dump({"7": {"path": path, "errors": ["unknown command hide_inst_icon (at byte 2)"]}},
             os.path.join(work, "compile_fail.json"))
        dump([], os.path.join(work, "ui_sample.json"))
        out = os.path.join(work, "gap.tsv")
        env = dict(os.environ, CENSUS_WORK=work, LIBRARY_ROOT="/mnt/lib")
        subprocess.run([sys.executable, os.path.join(HERE, "build_tsv.py"), out],
                       env=env, check=True, capture_output=True)
        with open(out) as f:
            rows = [line.split("\t") for line in f.read().splitlines()[1:]]
        by_gap = {r[0]: r for r in rows}
        self.assertEqual(by_gap["unknown command hide_inst_icon"][3], "1")
        # The three refusal counts are read from the summary, not typed.
        self.assertEqual(by_gap["modulator category 3"][2:4], ["3", "2"])

    def test_a_failed_ui_probe_is_not_a_library(self):
        """A probe that could not load writes no `library`.

        Counting that row made the picture-directory figure report a library
        that was never inspected as one without `pictures/`, so the sentence
        said two of three while only two were ever looked at.
        """
        work = tempfile.mkdtemp(prefix="census-ui-")
        dump({
            "totals": {"keysweep_instruments": 0, "ui_instruments": 3,
                       "ui_unresolved_assets": 0, "silent_unmeasured": 0},
            "actions": [], "silent": [], "no_zones": [],
            "ui_by_library": [{"library": "A"}, {"library": "B"}],
            "refusals": {"modulator category 3": {"instruments": 3,
                                                  "libraries": {"A": 2, "B": 1}}},
        }, os.path.join(work, "census_summary.json"))
        dump({"instruments": []}, os.path.join(work, "symbols.json"))
        dump({}, os.path.join(work, "compile_fail.json"))
        dump([
            {"path": "/mnt/lib/A/Instruments/a.nki", "library": "A",
             "library_has_pictures": True, "now_unresolved": 1, "now_resolved": 0,
             "buckets": {"absent_from_library": 1}},
            {"path": "/mnt/lib/B/Instruments/b.nki", "library": "B",
             "library_has_pictures": False, "now_unresolved": 2, "now_resolved": 0,
             "buckets": {"absent_from_library": 2}},
            {"path": "/mnt/lib/C/Instruments/c.nki", "error": "load timed out after 600s"},
        ], os.path.join(work, "ui_sample.json"))
        out = os.path.join(work, "gap.tsv")
        env = dict(os.environ, CENSUS_WORK=work, LIBRARY_ROOT="/mnt/lib")
        subprocess.run([sys.executable, os.path.join(HERE, "build_tsv.py"), out],
                       env=env, check=True, capture_output=True)
        with open(out) as f:
            rows = [line.split("\t") for line in f.read().splitlines()[1:]]
        sentence = {r[0]: r for r in rows}["UI asset not found"][5]
        self.assertIn("1 of the 2 libraries ship no `pictures/` folder", sentence)
        self.assertIn("1 probe failed to load and is not counted above", sentence)


if __name__ == "__main__":
    unittest.main(verbosity=2)
