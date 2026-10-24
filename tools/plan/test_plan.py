"""Checks for tools/plan/plan.py. Run: python3 -m unittest tools/plan/test_plan.py"""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import plan  # noqa: E402


class Globs(unittest.TestCase):
    def test_match(self):
        self.assertTrue(plan.match("crates/dsp/eq/src/lib.rs", ["crates/dsp/eq/**"]))
        self.assertTrue(plan.match("src/sound/v2.rs", ["src/sound/v2.rs"]))
        self.assertFalse(plan.match("src/sound/v2s.rs", ["src/sound/v2.rs"]))
        self.assertTrue(plan.match("a/color/bite.rs", ["a/color/{bite,dirt}.rs"]))
        self.assertFalse(plan.match("a/color/amp/x.rs", ["a/color/*.rs"]))
        self.assertTrue(plan.match("crates/x/Cargo.toml", ["crates/*/Cargo.toml"]))

    def test_overlap_new_paths(self):
        self.assertTrue(plan.overlap(["crates/dsp/filter/**"], ["crates/dsp/filter/src/ladder*"], []))
        self.assertFalse(plan.overlap(["crates/dsp/filter/src/ladder*"], ["crates/dsp/filter/src/formant*"], []))
        self.assertFalse(plan.overlap(["crates/dsp/eq/**"], ["crates/dsp/dynamics/**"], []))

    def test_overlap_existing(self):
        self.assertEqual(plan.overlap(["a/*.rs"], ["a/b.rs"], ["a/b.rs", "a/c.rs"]), ["a/b.rs"])


class Manifest(unittest.TestCase):
    def setUp(self):
        self.plan = plan.load()

    def test_valid(self):
        errors, _ = plan.validate(self.plan)
        self.assertEqual(errors, [])

    def test_cycle_detected(self):
        p = plan.load()
        p["by_id"]["P0.1"]["deps"] = ["P0.6"]
        with self.assertRaises(ValueError):
            plan.topo(p)

    def test_offline_ready_is_conflict_free(self):
        st = plan.status(self.plan, {})
        batch, _ = plan.ready_batch(self.plan, st)
        self.assertIn("P0.1", batch)
        skip = ("append", "regenerate")
        for i, a in enumerate(batch):
            for b in batch[i + 1:]:
                self.assertFalse(plan.overlap(plan.owns(self.plan, self.plan["by_id"][a], skip_modes=skip),
                                              plan.owns(self.plan, self.plan["by_id"][b], skip_modes=skip),
                                              list(plan.tracked())), (a, b))

    def test_brief_and_branch(self):
        text = plan.brief(self.plan, "P1.2")
        self.assertIn("<!-- v3-task: P1.2 -->", text)
        self.assertIn("crates/sampler-kontakt/src/stream.rs", text)
        self.assertEqual(plan.task_of("v3/P5.3a-filter-part-a", self.plan), "P5.3a")
        self.assertIsNone(plan.task_of("v3/P9.9-nope", self.plan))
        self.assertIsNone(plan.task_of("fix/other", self.plan))

    def test_slug(self):
        self.assertEqual(plan.slug("5. Current → new mapping"), "5-current--new-mapping")
        self.assertEqual(plan.slug("Phase 1–3: playback (users hear it)"), "phase-13-playback-users-hear-it")


if __name__ == "__main__":
    unittest.main()
