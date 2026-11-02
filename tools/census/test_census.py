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

    def run_analysis(self, keysweep):
        work = tempfile.mkdtemp(prefix="census-test-")
        paths = [f"/mnt/lib/Lib/Instruments/i{n}.nki" for n in range(3)]
        census = [{"path": p, "name": f"i{n}", "loaded": True, "zones_ready": 4}
                  for n, p in enumerate(paths)]
        dump(census, os.path.join(work, "census.json"))
        with open(os.path.join(work, "keysweep-results.jsonl"), "w") as f:
            for n, peaks in enumerate(keysweep):
                f.write(json.dumps({
                    "path": paths[n], "name": f"i{n}", "zones_ready": 4, "scripts": 1,
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


if __name__ == "__main__":
    unittest.main(verbosity=2)
