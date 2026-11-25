#!/usr/bin/env python3
"""held-dead-rows.py < names > (rewrites held_fields.tsv in place)

Drops the rows of crates/format/kontakt/data/held_fields.tsv whose KSP name
`engine_pars()` binds to a knob or a processor: the engine reads a held
value only through `EnginePar::Opaque`, so such a row is never read
(audit KT-17). Feed it the names the ratchet
`held::tests::held_rows_are_controls_no_processor_plays` prints, one per
line, after regenerating the table with tools/re/held_fields.py.
"""
import pathlib
import sys

TSV = pathlib.Path(__file__).resolve().parents[2] / "crates/format/kontakt/data/held_fields.tsv"
dead = {n.strip().upper() for n in sys.stdin if n.strip()}
lines = TSV.read_text().splitlines(keepends=True)
kept = [l for l in lines if l.startswith("#") or l.split("\t")[2].upper() not in dead]
TSV.write_text("".join(kept))
print(f"dropped {len(lines) - len(kept)} rows, kept {len(kept)}", file=sys.stderr)
