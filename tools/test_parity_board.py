#!/usr/bin/env python3
"""The parity board generates from well-formed, sorted snapshots, and its rules hold. `python3 tools/test_parity_board.py`"""
import importlib.util
import pathlib
import subprocess
import sys
import tempfile

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("board", HERE / "parity-board.py")
board = importlib.util.module_from_spec(spec)
spec.loader.exec_module(board)

# A parameter is tested when every applicable cell passes, done ONLY when verified law AND acts
# both pass, partial when any passes without both law+acts, and missing when none pass.
assert board.grade([True, None, True, True]) == "tested"
assert board.grade([True, False, False, True]) == "done"
assert board.grade([False, False, False, True]) == "partial"
assert board.grade([True, False, False, False]) == "partial"
assert board.grade([False, None, None, False]) == "missing"

# Processing types come from the module's name.
assert board.ptype("Replika") == "delay" and board.ptype("Solid G-EQ") == "EQ" and board.ptype("Raum") == "reverb"
# Only the public KONTRA repo's PRs are linked.
assert "KONTRA#12](" in board.pr_md("K12,34") and "#34" in board.pr_md("K12,34") and "/34)" not in board.pr_md("K12,34")

# Regex `Titles` and bag-of-words `Index` are gone.
assert not hasattr(board, "Titles"), "Titles regex matcher must stay deleted"
assert not hasattr(board, "Index"), "Index bag-of-words matcher must stay deleted"

# A snapshot row out of byte order, of the wrong width, or a dsp-oracle.tsv citing a missing/failing test fails.
real_snap = board.SNAP
try:
    with tempfile.TemporaryDirectory() as d:
        board.SNAP = pathlib.Path(d)
        (board.SNAP / "a.tsv").write_text("k\tv\nb\t1\na\t2\nc\n")
        (board.SNAP / "dsp-oracle.tsv").write_text(
            "effect_id\ttest_name\tmax_err_db\tstatus\tcommit\n"
            "bad\tcrates/does/not/exist.rs::fake_test\t0.1\ttested\tdeadbeef\n"
            "fail\tcrates/dsp/filter/tests/formant.rs::the_formant_matches_kontakt_within_a_tenth_of_a_db\t5.0\tfailing\tdeadbeef\n"
        )
        errs = board.snapshot_errors()
        assert any("not sorted" in e for e in errs), errs
        assert any("1 columns" in e for e in errs), errs
        assert any("cited test file missing" in e for e in errs), errs
        assert any("marked failing" in e for e in errs), errs
finally:
    board.SNAP = real_snap

# Verify real board invariants: absent oracle entries grade missing; host-uninstantiable stubs grade partial;
# oracle-verified modules grade tested; all 11 ni-file partial classes appear.
assert board.oracle_row("kontakt.dsp.fake.measured", "utility", "Fake: output vs oracle", "fake", True, {})["status"] == "missing"
_, _, _, _ = board.build(refresh=False)
manual, status = board.ksp_inputs()
dsp_rows = {r["id"]: r for r in board.kontakt_dsp(status)}
assert dsp_rows["kontakt.dsp.core_cell.measured"]["status"] == "partial", dsp_rows["kontakt.dsp.core_cell.measured"]
assert dsp_rows["kontakt.dsp.send_levels.measured"]["status"] == "tested", dsp_rows["kontakt.dsp.send_levels.measured"]
assert dsp_rows["kontakt.dsp.jump.measured"]["status"] == "tested", dsp_rows["kontakt.dsp.jump.measured"]
assert dsp_rows["kontakt.dsp.cry_wah.measured"]["status"] == "tested", dsp_rows["kontakt.dsp.cry_wah.measured"]
assert dsp_rows["kontakt.dsp.plate_reverb.measured"]["status"] == "tested", dsp_rows["kontakt.dsp.plate_reverb.measured"]
assert dsp_rows["kontakt.dsp.kolor.measured"]["status"] == "tested", dsp_rows["kontakt.dsp.kolor.measured"]
assert dsp_rows["kontakt.dsp.solid_g_eq.measured"]["status"] == "tested", dsp_rows["kontakt.dsp.solid_g_eq.measured"]
assert dsp_rows["kontakt.dsp.lo_fi.measured"]["status"] == "tested", dsp_rows["kontakt.dsp.lo_fi.measured"]
assert dsp_rows["kontakt.dsp.solid_bus_comp.measured"]["status"] == "tested", dsp_rows["kontakt.dsp.solid_bus_comp.measured"]
assert dsp_rows["kontakt.dsp.filter.measured"]["status"] == "tested", dsp_rows["kontakt.dsp.filter.measured"]

fmt_rows, _, _ = board.kontakt_format()
fmt_unseen = [r for r in fmt_rows if r["status"] == "unseen"]
for r in fmt_rows:
    # A class is tested only on census evidence that read all of its objects.
    if r["status"] == "tested":
        assert r["ev"] and r["ev"][0].endswith("ni-file-census.tsv"), r
        assert " objects full" in r["note"] and not r["note"].startswith("0/"), r
assert len(fmt_unseen) > 0, "corpus-absent classes must report unseen, not done"
assert not any(r["status"] == "done" for r in fmt_rows), "ni-file classes with 0 objects must not report done"

r = subprocess.run([sys.executable, str(HERE / "parity-board.py"), "--check"], capture_output=True, text=True)
assert r.returncode == 0, r.stderr or r.stdout
print("parity board ok")
