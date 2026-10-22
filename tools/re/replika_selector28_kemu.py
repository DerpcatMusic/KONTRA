"""Native vectors for the Kontakt 8 Replika index-28 selector `141aca670` (1200 B) under kemu, through the real setter.

The core is built and prepared as in `replika_kemu_vectors.py`, then the state the selector reads and writes is
randomised (ducker envelope, saturator section states, the tap rates and the LFO glide `+0x57e8` of each head; the
same kind of seeded state the mode-writer vectors use). The snapshot is taken, index 28 is written through the
setter `141ac5980` (slot 12), and the snapshot is taken again. The mode-1 heads are inline in the core object
(`C+0xac0`, `C+0x6380`, `C+0xbc40`), so one snapshot covers the core, the heads and their taps.

The vector records every field the snapshot reads and the list of fields whose value changed. The port
(`Heads::select` in `replika/head.rs`) covers the subset documented there; `changed` shows what the native
selector writes beyond it (head reset `141aba100`, bypass fade `141acc600`, delay caller `141abd290`, ...).

Cases: index 28 = 0.1 (path 0, no change from the reset path 0), 0.3 (path 1), 0.5 (path 2), 0.9 (path 4). Path 3
(index 0.7) is not recorded: the port returns Blocked for it, and the native call runs the unported `141acb100`.

    nice -n 19 python3 -I tools/re/replika_selector28_kemu.py KONTAKT8_EXE OUT_DIR
"""
import json
import math
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import replika_kemu_vectors as rk  # noqa: E402
import replika_mode_writer_kemu as mw  # noqa: E402

SETTER_INDEX = 28
VALUES = {"v0p1": 0.1, "v0p3": 0.3, "v0p5": 0.5, "v0p9": 0.9}
HEADS = (0xAC0, 0x6380, 0xBC40)  # the three mode-1 heads, inline in the core
TAPS = {"L": 0x1A8, "R": 0x278}  # tap objects K inside a head
SATS = {"L": 0x14080, "R": 0x1410C}  # saturators (141aba680 zeroes their section states)
CORE_FIELDS = {  # (offset, format)
    "raw28": (0x8B0, "f"),  # the raw index-28 store the selector reads
    "live": (0x08, "B"),  # C+8: the path change is applied when set (or C+0x829 is clear)
    "sel829": (0x829, "B"),  # set by the setter before the call
    "path604": (0x604, "i"),  # C+0x604 = k
    "path830": (0x830, "i"),  # C+0x830 = k (the guard)
    "smooth938": (0x938, "B"),  # ducking reads smoothed copies (read by the unported caller 141abd290)
    "env": (0x13E38, "d"),  # ducker envelope in dB (141aba800 zeroes it)
}
SAT_FIELDS = {"a_s0": (0x24, "f"), "a_s1": (0x28, "f"), "b_s0": (0x58, "f"), "b_s1": (0x5C, "f")}
RATE_OFFS = (0x90, 0x98, 0xA0)
SEED = 28


def path_of(v):
    """`141aca670`: min(4, floor(5 v)) with the native float scale 5.0 (f32)."""
    return min(4, math.floor(rk.f32(v * 5.0)))


def core_snapshot(e, obj):
    out = {name: e.read(obj + off, fmt) for name, (off, fmt) in CORE_FIELDS.items()}
    for side, base in SATS.items():
        for name, (off, fmt) in SAT_FIELDS.items():
            out["sat%s_%s" % (side, name)] = e.read(obj + base + off, fmt)
    return out


def head_snapshot(e, h):
    out = {name: e.read(h + off, fmt) for name, (off, fmt) in mw.HEAD_FIELDS.items()}
    for side, off in TAPS.items():
        k = h + off
        for name, (o, fmt) in mw.TAP_FIELDS.items():
            out["%s_%s" % (side, name)] = e.read(k + o, fmt)
    return out


def inject(e, obj, rnd):
    """Seeded state the selector reads or writes (the mode-writer vectors use the same kind of randomisation)."""
    e.write(obj + 0x13E38, "d", rnd.uniform(-12.0, 0.0))
    for base in SATS.values():
        for off in (0x24, 0x28, 0x58, 0x5C):
            e.write(obj + base + off, "f", rnd.uniform(-0.5, 0.5))
    for h in HEADS:
        e.write(obj + h + 0x57E8, "f", rnd.uniform(0.0, 1.0))  # glide current
        for off in TAPS.values():
            for ro in RATE_OFFS:
                e.write(obj + h + off + ro, "d", rnd.uniform(0.5, 2.0))


def changed(init, end):
    """Names whose value changed across the setter (flat keys; nested by head index)."""
    core = [k for k in init["core"] if init["core"][k] != end["core"][k]]
    heads = [[k for k in init["heads"][i] if init["heads"][i][k] != end["heads"][i][k]] for i in range(3)]
    return {"core": core, "heads": heads}


def case(exe, name, v):
    e = rk.fresh(exe)
    core = rk.Core(e)
    obj = core.obj
    defaults = [[i, rk.f32(x)] for i, x in sorted(rk.DEFAULTS.items())]
    for i, x in defaults:
        core.set(i, x)
    core.prepare(float(rk.RATE))
    inject(e, obj, random.Random(SEED))
    init = {"core": core_snapshot(e, obj), "heads": [head_snapshot(e, obj + h) for h in HEADS]}
    core.set(SETTER_INDEX, rk.f32(v))
    end = {"core": core_snapshot(e, obj), "heads": [head_snapshot(e, obj + h) for h in HEADS]}
    assert end["core"]["raw28"] == rk.f32(v), end["core"]["raw28"]
    k = end["core"]["path604"]
    assert k == path_of(v), (k, path_of(v))
    return {
        "name": "replika-native-selector28-%s" % name,
        "setter": SETTER_INDEX,
        "v": rk.f32(v),
        "k": k,
        "rate": rk.RATE,
        "block": rk.QUANTUM,
        "defaults": defaults,
        "init": init,
        "end": end,
        "changed": changed(init, end),
    }


def main():
    exe, out_dir = sys.argv[1], sys.argv[2]
    for name, v in VALUES.items():
        doc = case(exe, name, v)
        path = os.path.join(out_dir, "replika-native-selector28-%s.json" % name)
        with open(path, "w") as f:
            json.dump(doc, f, separators=(",", ":"))
        end = doc["end"]["core"]
        print("%s v %.3g k %d path830 %d env %.6g changed core %s heads %s" % (
            name, v, doc["k"], end["path830"], end["env"], doc["changed"]["core"],
            [len(h) for h in doc["changed"]["heads"]]), flush=True)


if __name__ == "__main__":
    main()
