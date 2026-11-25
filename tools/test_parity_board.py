#!/usr/bin/env python3
"""docs/PARITY.md is current with its inputs, and its rules hold. `python3 tools/test_parity_board.py`"""
import importlib.util
import pathlib
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("board", HERE / "parity-board.py")
board = importlib.util.module_from_spec(spec)
spec.loader.exec_module(board)

# A parameter is tested when every applicable cell passes, done when it acts, partial when any passes.
assert board.grade([True, None, True, True]) == "tested"
assert board.grade([False, False, False, True]) == "done"
assert board.grade([True, False, False, False]) == "partial"
assert board.grade([False, None, None, False]) == "missing"
# Processing types come from the module's name.
assert board.ptype("Replika") == "delay" and board.ptype("Solid G-EQ") == "EQ" and board.ptype("Raum") == "reverb"
# Only the public KONTRA repo's PRs are linked.
assert "KONTRA#12](" in board.pr_md("K12,34") and "#34" in board.pr_md("K12,34") and "/34)" not in board.pr_md("K12,34")

r = subprocess.run([sys.executable, str(HERE / "parity-board.py"), "--check"], capture_output=True, text=True)
assert r.returncode == 0, r.stderr or r.stdout
print("parity board ok")
