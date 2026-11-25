#!/usr/bin/env python3
"""What real Kontakt shows for `$ENGINE_PAR_*` values: get_engine_par_disp text.

    disp_sweep.py OUT.tsv [--points N] TYPE:PAR,PAR,... [TYPE:PAR,...]...

TYPE is an `$EFFECT_TYPE_*` name without the prefix (RAUM, REPLIKA, ...), or
TYPE/SUB to set its subtype too (FILTER/EQ3BAND sets `$FILTER_TYPE_EQ3BAND`),
loaded into instrument insert slot n in argument order (up to 8); PARs are
full `$ENGINE_PAR_*` names without `$ENGINE_PAR_` (BASSPRO_GAIN, not GAIN),
from the KSP manual. An unknown name is a silent KSP compile error in Kontakt;
`on init` saves a marker, so the tool tells that apart from a sweep that never ran. As fresh_probe.py: an empty instrument, a 200 ms timer
listener loads the types (tick 2; a render session is needed, with no audio
the listener never fires), then `on pgs_changed` sets each PAR to every point
of X, reads get_engine_par_disp and saves the texts with save_array_str.
Appends rows `name, x (0..1000000), text` to OUT.tsv. --points N sweeps N+1
even points instead of X (a curve's shape).
"""
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
X = [0, 50000, 100000, 250000, 333333, 500000, 666667, 750000, 900000, 1000000]


def ksp(specs, nka, ran, X):
    cells = sum(len(p) for _, p in specs) * len(X)
    sweep = "".join(
        f"  $p := $ENGINE_PAR_{p}\n  $i := 0\n  while ($i < {len(X)})\n"
        f"    set_engine_par($p, %x[$i], -1, {s}, 1)\n"
        f"    !out[$j] := get_engine_par_disp($p, -1, {s}, 1)\n    inc($j)\n    inc($i)\n  end while\n"
        for s, (_, pars) in enumerate(specs) for p in pars)
    loads = "".join(f"    set_engine_par($ENGINE_PAR_EFFECT_TYPE, $EFFECT_TYPE_{t.split('/')[0]}, -1, {s}, 1)\n"
                    + "".join(f"    set_engine_par($ENGINE_PAR_EFFECT_SUBTYPE, ${t.split('/')[0]}_TYPE_{sub}, -1, {s}, 1)\n"
                              for sub in t.split("/")[1:])
                    for s, (t, _) in enumerate(specs))
    return (f"on init\n  pgs_create_key(READ, 1)\n  declare $tick := 0\n  declare $i\n  declare $j\n  declare $p\n"
            f"  declare %x[{len(X)}] := ({', '.join(map(str, X))})\n  declare !out[{cells}]\n"
            f'  set_listener($NI_SIGNAL_TIMER_MS, 200000)\n  save_array_str(!out, "{ran}")\nend on\n'
            f"on listener\n  inc($tick)\n  if ($tick = 2)\n{loads}  end if\n"
            "  if ($tick = 12)\n    pgs_set_key_val(READ, 0, 1)\n  end if\nend on\n"
            f'on pgs_changed\n  $j := 0\n{sweep}  save_array_str(!out, "{nka}")\nend on\n')


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    args = sys.argv[1:]
    xs = X
    if "--points" in args:
        k = args.index("--points")
        n = int(args[k + 1])
        xs = [round(1e6 * i / n) for i in range(n + 1)]
        del args[k : k + 2]
    out = Path(args[0])
    specs = [(t, p.split(",")) for t, p in (a.split(":") for a in args[1:])]
    assert len(specs) <= 8, "8 insert slots"
    work = Path("/tmp/disp-sweep")
    work.mkdir(exist_ok=True)
    nka, ran = work / "out.nka", work / "ran.nka"
    nka.unlink(missing_ok=True)
    ran.unlink(missing_ok=True)
    src = ksp(specs, "Z:" + str(nka.resolve()), "Z:" + str(ran.resolve()), xs)
    (work / "sweep.ksp").write_text(src)
    lua = ("local kt = Kontakt\nlocal out = assert(io.open(ORACLE_OUT, \"w\"))\nkt.reset_multi()\n"
           "local i = kt.add_instrument()\n"
           f"local ok, err = pcall(kt.set_instrument_script_source, i, 0, [==[{src}]==])\n"
           "out:write(string.format('{\"compiled\":%s,\"err\":\"%s\"}\\n', tostring(ok), tostring(err):gsub('\"', \"'\")))\n"
           "out:write('{\"ready\":true}\\n')\nout:close()\n")
    (work / "sweep.lua").write_text(lua)
    subprocess.run([str(HERE.parent / "oracle/oracle-slot"), "python3", str(HERE / "render.py"),
                    str(work / "sweep.lua"), "/dev/null", "8"], check=False)
    # Kontakt reports no KSP compile error here: a script that never ran (an unknown name)
    # saves neither file; one that ran but never swept saves only `on init`'s.
    if not ran.exists():
        sys.exit(f"the script never ran: a KSP compile error, most often a PAR or TYPE name\n"
                 f"Kontakt 8.13.1 does not have (PAR is the name after $ENGINE_PAR_); see {work / 'sweep.ksp'}")
    if not nka.exists():
        sys.exit(f"the script ran but never swept: no listener tick reached pgs_changed; see the render log")
    texts = nka.read_text(errors="replace").splitlines()[1:]
    rows = [(f"$ENGINE_PAR_{p}", x) for _, pars in specs for p in pars for x in xs]
    if len(texts) < len(rows):
        sys.exit(f"{len(texts)} texts for {len(rows)} cells; see {nka}")
    with out.open("a") as f:
        f.writelines(f"{name}\t{x}\t{t}\n" for (name, x), t in zip(rows, texts))


if __name__ == "__main__":
    main()
