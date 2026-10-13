#!/usr/bin/env python3
"""Effect binding probe: which stored field does each ENGINE_PAR drive, and by what law?

    probe_fx.py scan  EFFECT_DIR...            pass 1: every ENGINE_PAR name at one value
    probe_fx.py sweep EFFECT_DIR... --pars A,B pass 2: chosen names at many values
    EFFECT_DIR is a folder under the factory Presets tree, e.g. "Effects/ACBox" or "Filters/Solid G-EQ";
    its "- INIT ... -.nkp" (else the first preset) is loaded into instrument insert slot 0.

For each step the generated Lua (one Kontakt session, see NATIVE_ORACLE.md) loads the preset
(load_fx_preset), runs a one-line KSP `on init` that calls set_engine_par(par, value, -1, 0, 0),
and save_instrument()s. Kontakt runs the init as soon as set_instrument_script_source returns
(pilot: probes/pilot_fx.lua). The saved instruments are decoded with the nkp_decode example
(Kontakt's own reader programs) and every field that differs from the untouched preset is
recorded. Nothing but numeric field values is kept; the step files are deleted afterwards.

Output: TSV rows  effect, class, version, par, value, field_idx, slot, kind, preset_value, stored
"""
import argparse
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
PRESETS = Path("/home/derpcat/.wine/drive_c/Program Files/Common Files/VST3/Portapotty/Kontakt 8/Presets")
NAMES = ROOT / "crates/sampler-core/src/engine_parameter_names.rs"
DECODER = Path(os.environ.get("NKP_DECODE", "/mnt/Windows11/DEV_WORKSPACE/Toolchains/User/cargo-target/"
                              "kontakto-agent-a374c576a1f496725/debug/examples/nkp_decode"))
STEPS = Path("/tmp/w3/steps")


def winpath(p: Path) -> str:
    return "Z:" + str(p).replace("/", "\\")


def lua_s(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def all_pars() -> list[str]:
    return re.findall(r'"(\$ENGINE_PAR_[A-Z0-9_]+)"', NAMES.read_text())


def init_preset(rel: str) -> Path:
    d = PRESETS / rel
    inits = sorted(d.glob("- INIT*.nkp")) or sorted(d.glob("*.nkp"))
    return inits[0]


LOC = os.environ.get("PROBE_LOC", "insert")
GEN = os.environ.get("PROBE_GENERIC", "0")


def lua(effects: list[tuple[str, Path]], steps: list[tuple[int, str, int]]) -> str:
    """steps: (effect index, par, value)."""
    return f"""
local kt = Kontakt
local out = assert(io.open(ORACLE_OUT, "w"))
local EFFECTS = {{{",".join(lua_s(winpath(p)) for _, p in effects)}}}
local STEPS = {{{",".join(f'{{{e},{lua_s(par)},{v}}}' for e, par, v in steps)}}}
local DIR = {lua_s(winpath(STEPS) + chr(92))}
kt.reset_multi()
local inst = kt.add_instrument()
local last = 0
for n, s in ipairs(STEPS) do
  if s[1] ~= last then
    last = s[1]
    local ok, e = pcall(kt.load_fx_preset, EFFECTS[last], inst, -1, 0, kt.{LOC}_fx)
    if s[2] == "" then
      pcall(kt.save_instrument, inst, DIR .. string.format("base_%d.nki", last), {{}})
    end
  end
  if s[2] ~= "" then
    pcall(kt.load_fx_preset, EFFECTS[last], inst, -1, 0, kt.{LOC}_fx)
    local src = "on init\\n set_engine_par(" .. s[2] .. ", " .. s[3] .. ", -1, 0, {GEN})\\nend on\\n"
    pcall(kt.set_instrument_script_source, inst, 0, src)
    pcall(kt.save_instrument, inst, DIR .. string.format("s_%d.nki", n), {{}})
  end
  if n % 100 == 0 then out:write(string.format('{{"progress":%d}}\\n', n)) out:flush() end
end
out:write('{{"done":true}}\\n')
out:close()
"""


def fields(path: Path) -> dict:
    """(object path, field index) -> (class, version, slot, kind, value) for effect objects."""
    res = {}
    txt = subprocess.run([str(DECODER), "dump", str(path)], capture_output=True, text=True).stdout
    for line in txt.splitlines():
        c = line.split("\t")
        if len(c) < 9 or not re.search(r"/3a\[0\]/25", c[0]):
            continue
        res[(c[0], int(c[3]))] = (c[1], c[2], c[7], c[5], c[8])
    return res


def classes(fs: dict) -> str:
    return ",".join(sorted({f"{c[0]} {c[1]}" for c in fs.values()}))


def run(effects, steps, timeout):
    shutil.rmtree(STEPS, ignore_errors=True)
    STEPS.mkdir(parents=True)
    script = STEPS.parent / "probe_gen.lua"
    script.write_text(lua(effects, steps))
    env = dict(os.environ, ORACLE_LUA=str(script), ORACLE_TIMEOUT=str(timeout))
    for _ in range(30):  # a previous session may still be shutting down
        if subprocess.run(["pgrep", "-f", "[K]ontakt 8.exe"], capture_output=True).returncode:
            break
        time.sleep(2)
    subprocess.run([sys.executable, str(HERE / "dump.py"), str(STEPS.parent / "probe.jsonl")], env=env, check=True)


def measure(effects, steps, timeout):
    """Run one Kontakt session; rows (effect, class, version, par, value, field, slot, kind, preset, stored)."""
    run(effects, steps, timeout)
    rows, base, seen = [], {}, []
    for n, (ei, par, v) in enumerate(steps, 1):
        if par == "":
            base[ei] = fields(STEPS / f"base_{ei}.nki")
            continue
        f = STEPS / f"s_{n}.nki"
        if f.exists():
            seen.append((ei, par, v, fields(f)))
    # A field that moves for any value of a parameter is reported at every value, so laws
    # can be fitted on the whole sweep (values that happen to equal the preset included).
    # PROBE_KEEP=dir PROBE_KEEP_STEPS="Effects/ACBox|AC_BASS|900000;..." keeps those saved
    # instruments (a few KB each) as test fixtures.
    if os.environ.get("PROBE_KEEP"):
        want = {tuple(x.split("|")) for x in os.environ["PROBE_KEEP_STEPS"].split(";")}
        Path(os.environ["PROBE_KEEP"]).mkdir(parents=True, exist_ok=True)
        for n, (ei, par, v) in enumerate(steps, 1):
            key = (effects[ei - 1][0], par.replace("$ENGINE_PAR_", ""), str(v))
            if key in want:
                name = "_".join(re.sub(r"\W+", "-", k) for k in key) + ".nki"
                shutil.copy(STEPS / f"s_{n}.nki", Path(os.environ["PROBE_KEEP"]) / name)
    moved = {}
    for ei, par, v, got in seen:
        if classes(got) == classes(base[ei]):
            for k, cur in got.items():
                if base[ei].get(k) != cur:
                    moved.setdefault((ei, par), set()).add(k)
    for ei, par, v, got in seen:
        name = effects[ei - 1][0]
        if classes(got) != classes(base[ei]):
            rows.append([name, "-", "-", par, str(v), "-1", "", "class", classes(base[ei]), classes(got)])
            continue
        for k in sorted(moved.get((ei, par), ())):
            cur, old = got.get(k), base[ei].get(k)
            if cur:
                rows.append([name, cur[0], cur[1], par, str(v), str(k[1]), cur[2], cur[3], old[4] if old else "", cur[4]])
    shutil.rmtree(STEPS, ignore_errors=True)
    return rows


HEAD = "effect\tclass\tversion\tpar\tvalue\tfield\tslot\tkind\tpreset\tstored"


def write(path, rows):
    Path(path).write_text(HEAD + "\n" + "".join("\t".join(r) + "\n" for r in rows))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("mode", choices=["scan", "sweep", "auto"])
    ap.add_argument("effects", nargs="+")
    ap.add_argument("--pars", default="")
    ap.add_argument("--values", default="0,1,2,3,100000,200000,300000,400000,500000,600000,700000,800000,900000,1000000")
    ap.add_argument("--subtypes", default="0-80", help="EFFECT_SUBTYPE integers to sweep in auto mode (a-b)")
    ap.add_argument("--out", required=True, help="scan TSV (auto: PREFIX.scan.tsv + PREFIX.sweep.tsv)")
    ap.add_argument("--reuse-scan", action="store_true", help="auto: read PREFIX.scan.tsv instead of rescanning")
    ap.add_argument("--scan-value", type=int, default=700000)
    a = ap.parse_args()
    effects = [(e, init_preset(e)) for e in a.effects]
    pars = a.pars.split(",") if a.pars else all_pars()
    pars = [p if p.startswith("$") else "$ENGINE_PAR_" + p for p in pars]
    values = [int(v) for v in a.values.split(",")]

    def plan(per_effect_pars, vals):
        st = []
        for ei in range(len(effects)):
            st.append((ei + 1, "", 0))
            st += [(ei + 1, p, v) for p in per_effect_pars.get(ei + 1, []) for v in vals]
        return st

    if a.reuse_scan:
        scan = [l.split("\t") for l in Path(a.out + ".scan.tsv").read_text().splitlines()[1:]]
    elif a.mode in ("scan", "auto"):
        steps = plan({i + 1: pars for i in range(len(effects))}, [a.scan_value])
        scan = measure(effects, steps, 600 + len(steps) * 0.5)
        write(a.out if a.mode == "scan" else a.out + ".scan.tsv", scan)
        print(f"scan: {len(scan)} changed fields")
    if a.mode == "auto":
        lo, hi = map(int, a.subtypes.split("-"))
        hit = {}
        for r in scan:
            if r[3] != "$ENGINE_PAR_EFFECT_SUBTYPE":
                hit.setdefault(effects_index(effects, r[0]), set()).add(r[3])
        steps = []
        for ei in range(len(effects)):
            steps.append((ei + 1, "", 0))
            steps += [(ei + 1, p, v) for p in sorted(hit.get(ei + 1, [])) for v in values]
            steps += [(ei + 1, "$ENGINE_PAR_EFFECT_SUBTYPE", v) for v in range(lo, hi + 1)]
        sweep = measure(effects, steps, 600 + len(steps) * 0.5)
        write(a.out + ".sweep.tsv", sweep)
        print(f"sweep: {len(sweep)} changed fields")
    elif a.mode == "sweep":
        steps = plan({i + 1: pars for i in range(len(effects))}, values)
        write(a.out, measure(effects, steps, 600 + len(steps) * 0.5))
    return 0


def effects_index(effects, name):
    return [e for e, _ in effects].index(name) + 1


if __name__ == "__main__":
    sys.exit(main())
