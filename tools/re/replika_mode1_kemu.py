"""Reachability probe for the Kontakt 8 Replika mode-1 head reader (`141ab8c60`) under kemu.

Question: can any setting the core harness drives (setter `141ac5980`, update `141ab4a10`, process
`141aa9480`) make a head's mode field `H+0xc0` equal 1, or reach the mode-1/2/3 reader or its tap kernels?
Decompile (`0141ab0000.c` 4397-4460): the head read `141ab7a90` calls `141ab82e0` when
`H+0xc0 == 0` and `141ab8c60` otherwise. `H+0xc0 = 1` is written only by `141ab4e30` (2611),
`141ab5890` (3030) and `141ab69f0` (3756). Their callers are the wrapper's lane objects
(`141ac5340` case 6, `141abab20`, `141abdaf0`), not the core setter.

Selector found by the sweep: index 28 (path select, raw `C+0x8b0`). `141aca670` sets k = min(4, trunc(5 v))
(`1447149f0` = 5.0) and `141aca4a0` writes `H+0xc0 = k` on the three inline heads. Index 9 (TYPE) only sets
`C+0x93c` and is kept as a negative control.

This script drives one emulator: the core is built and prepared once, then every index 0..52 is set to
0.0, 0.3, 0.5 and 1.0 for one 32-frame quantum (noise input), with the setting restored to its default
afterwards. It counts code hits on the head-read, head-update and lane functions and prints each head's
`H+0xc0`, then the index-28 quantiser boundaries. Last, it renders 4096-frame noise with index 9 and with
index 28 = 0.3 against the default, to compare outputs with only the selector changed.

    nice -n 19 python3 -I tools/re/replika_mode1_kemu.py KONTAKT8_EXE                  # probe
    nice -n 19 python3 -I tools/re/replika_mode1_kemu.py KONTAKT8_EXE --vectors OUT_DIR  # native vectors
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import replika_kemu_vectors as rk  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402

# Function entries watched by code hooks (decompile names, `0141ab0000.c` unless noted).
WATCH = {
    0x141AB7A90: "head read (dispatch)",
    0x141AB82E0: "mode-0 reader",
    0x141AB8C60: "mode 1/2/3 reader",
    0x141AB8A00: "tap kernel 0",
    0x141AB87D0: "tap kernel 1",
    0x141AB7FE0: "tap kernel 2",
    0x141AA3E00: "mode-1 post filter",
    0x141AB4E30: "head update (writes H+0xc0=1)",
    0x141AB5890: "lane head writer A (H+0xc0=1)",
    0x141AB69F0: "lane head writer B (H+0xc0=1)",
    0x141AC5340: "lane setter (calls 141ab4e30)",
    0x141ABAB20: "lane update (thunk 141a9f650)",
    0x141ABDAF0: "lane gate",
}
# Head objects inline in the core (`141aa9480` step 12: `C+0xac0`, `C+0x6380`, `C+0xbc40`).
HEADS = (0xAC0, 0x6380, 0xBC40)
MODE_OFF = 0xC0  # H+0xc0, int
# Index 28 values either side of the k = trunc(5 v) bins (k = 1 from 0.2, 2 from 0.4, 3 from 0.6, 4 from 0.8).
BOUNDARY = (0.19, 0.21, 0.39, 0.41, 0.59, 0.61, 0.79, 0.81, 1.0)
# Hits every quantum in the default run: not evidence of mode 1.
NORMAL = {"head read (dispatch)", "mode-0 reader"}


def attach(e):
    counts = {name: 0 for name in WATCH.values()}

    def hit(name):
        def cb(uc, addr, size, _):
            counts[name] += 1
        return cb

    for addr, name in WATCH.items():
        e.uc.hook_add(UC_HOOK_CODE, hit(name), begin=addr, end=addr)
    return counts


def head_modes(e, core):
    return [e.read(core.obj + h + MODE_OFF, "i") for h in HEADS]


def sweep(exe):
    e = rk.fresh(exe)
    counts = attach(e)
    core = rk.Core(e)
    for i, v in sorted(rk.DEFAULTS.items()):
        core.set(i, v)
    core.prepare(float(rk.RATE))
    stim = rk.noise()
    left, right = [x[:rk.QUANTUM] for x in stim]
    core.quantum(left, right)
    print("after prepare and one default quantum: head H+0xc0 = %s, counts %s"
          % (head_modes(e, core), {k: v for k, v in counts.items() if v}), flush=True)
    seen = {}
    for i in range(rk.BINDING_INDICES[-1] + 1):
        for v in (0.0, 0.3, 0.5, 1.0):
            before = dict(counts)
            core.set(i, v)
            core.quantum(left, right)
            modes = head_modes(e, core)
            fired = {k: counts[k] - before[k] for k in counts if counts[k] != before[k]}
            if any(modes) or set(fired) - NORMAL:
                seen[(i, v)] = (modes, fired)
            core.set(i, rk.DEFAULTS.get(i, 0.0))
    print("sweep: %d (index, value) pairs over 0..52 x {0, 0.3, 0.5, 1}, one quantum each" % (53 * 4))
    if not seen:
        print("sweep result: no setting moved H+0xc0 off 0, and no run reached the mode-1 reader,")
        print("the tap kernels, the head update or the lane functions.")
    for (i, v), (modes, fired) in sorted(seen.items()):
        print("  index %2d = %.1f: H+0xc0 %s, hits %s" % (i, v, modes, fired))
    print("total hits over the sweep:", {k: v for k, v in counts.items() if v}, flush=True)
    # Quantiser boundaries of index 28: k = min(4, trunc(5 v)) (`141aca670`, 1447149f0 = 5.0).
    for v in BOUNDARY:
        core.set(28, v)
        core.quantum(left, right)
        print("  index 28 = %.2f: H+0xc0 %s" % (v, head_modes(e, core)), flush=True)
        core.set(28, 0.0)
    return seen


def render(exe, index, value):
    """A 4096-frame noise render with one setter routed every quantum (as the wrapper does)."""
    e = rk.fresh(exe)
    counts = attach(e)
    routes = {index: value} if index is not None else {}
    return rk.render(e, routes, rk.noise()), counts


def compare(exe, index, value):
    base, c0 = render(exe, None, 0.0)
    mode, c1 = render(exe, index, value)
    print("index %d = %.2f vs default (noise, 4096 frames): max abs diff %.6g" % (
        index, value, rk.diff((mode["outL"], mode["outR"]), (base["outL"], base["outR"]))))
    print("  default hits %s" % {k: v for k, v in c0.items() if v})
    print("  index %d hits %s" % (index, {k: v for k, v in c1.items() if v}))


def vectors(exe, out_dir):
    """Native reference vectors for index 28 = 0.3 (path select k = 1, head mode 1), 4096 frames."""
    for name, stim in (("index28-v0p3", rk.impulse()), ("index28-noise-v0p3", rk.noise())):
        e = rk.fresh(exe)
        counts = attach(e)
        r = rk.render(e, {28: 0.3}, stim)
        doc = {"name": "mode1-" + name, "rate": rk.RATE, "block": rk.QUANTUM, "defaults": r["defaults"],
               "writes": r["writes"], "stim": stim, "outL": r["outL"], "outR": r["outR"]}
        with open(os.path.join(out_dir, "replika-native-mode1-%s.json" % name), "w") as f:
            json.dump(doc, f, separators=(",", ":"))
        m = rk.metrics(r["outL"], r["outR"])
        print("%s: peak %.6g energy %.6g hits %s" % (name, m["peak"], m["energy"],
                                                    {k: v for k, v in counts.items() if v}), flush=True)


def main():
    exe = sys.argv[1]
    if len(sys.argv) > 2 and sys.argv[2] == "--vectors":
        vectors(exe, sys.argv[3])
        return
    sweep(exe)
    compare(exe, 9, 0.3)  # negative control: TYPE sets C+0x93c only, H+0xc0 stays 0
    compare(exe, 28, 0.3)  # path select: k = trunc(4v) = 1, head mode H+0xc0 = 1


if __name__ == "__main__":
    main()
