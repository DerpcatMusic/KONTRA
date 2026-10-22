"""Native Kontakt 8 Replika (NI::DSP::FXFDelayLight core) vectors under the kemu emulator.

Runs the unchanged core through its vtable, no host: ctor 141a7f4d0, slot 1 init 141a9e300, slot 3 prepare
141aa1580 (rate in xmm1, quantum), slot 12 setter 141ac5980 (index, value in xmm2), slot 8 update 141ab4a10,
slot 7 process 141aa9480 (stereo). The slot addresses are checked against the object's vtable at run time.

Per 32-frame quantum the harness does what the wrapper loop 1408f98d0 does: setParam for each routed index
(value clamped to 0..1), then update, then process. Every other index is written once before prepare with the
reset defaults of 1408ddbc0 (the harness TIME value is added, see DEFAULTS). A case's routes are written on every
quantum, as an enabled route is in the wrapper.

    python3 -I tools/re/replika_kemu_vectors.py KONTAKT8_EXE OUT_DIR   # case vectors, one JSON per case
    python3 -I tools/re/replika_kemu_vectors.py KONTAKT8_EXE --bind    # index binding table on stdout (impulse)
    python3 -I tools/re/replika_kemu_vectors.py KONTAKT8_EXE --bind-noise 13,17,... [5=0.8,4=0.0]  # noise
    python3 -I tools/re/replika_kemu_vectors.py KONTAKT8_EXE --state [INDICES]  # object bytes changed by one setter

Each JSON: name, rate, block, defaults ([index, value] before prepare), writes (per quantum [[index, value]...]),
stim ([L, R] float32 input), outL, outR (float32 output). Blockers raise with the instruction address.
"""
import json
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import RET, Emu, f32  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RIP  # noqa: E402

TABLE_INIT = 0x1404D14D0
CTOR = 0x141A7F4D0
INIT = 0x141A9E300  # vtable slot 1
PREPARE = 0x141AA1580  # slot 3
PROCESS = 0x141AA9480  # slot 7 (+0x38)
UPDATE = 0x141AB4A10  # slot 8 (+0x40)
SETTER = 0x141AC5980  # slot 12 (+0x60)
CORE_BYTES = 0x14980
RATE = 48000
QUANTUM = 32
N = 4096
BINDING_INDICES = range(53)
PREPARE_BUDGET = 400_000_000  # block setup zeroes the long delay rings (up to 4000 ms)

H = 0.762404978  # reset constant 144f38814 (indices 0, 1)
RESET = {0: H, 1: H, 4: 1.0, 7: 0.0, 10: 0.0, 11: 0.5, 12: 0.5, 13: 0.5, 14: 0.5, 15: 0.3, 16: 0.5,
         17: 0.0, 18: 0.0, 19: 0.0, 25: 0.0, 26: 1.0, 30: 0.0, 31: 0.0, 32: 1.0, 34: 0.0, 35: 0.0,
         36: 0.0, 37: 0.0, 39: 0.0, 40: 0.0, 41: 0.0, 42: 0.0, 43: 0.0, 44: 0.0, 45: 0.0, 46: 0.0,
         51: 1.0}
# Not in 1408ddbc0: TIME (8) is set here to 0.02 (40 ms at mode 3, 2000 ms x v) so that an echo lands inside
# the 4096-frame window. HIGHCUT (3) is set neutral (1.0) because raw 0 would be the 200 Hz cut. Both are
# harness choices, not the stored Kontakt defaults.
DEFAULTS = {**RESET, 8: 0.02, 3: 1.0}

CASES = {
    "default": {},
    "time-v0p01": {8: 0.01},
    "time-v0p03": {8: 0.03},
    "feedback-v0p8": {5: 0.8},
    "saturation-v0p5": {29: 0.5},
    "lowcut-v0p5": {2: 0.5},
    "highcut-v0p5": {3: 0.5},
}


def complete(e, *args, limit=4_000_000, **kwargs):
    e.call(*args, limit=limit, **kwargs)
    if e.uc.reg_read(UC_X86_REG_RIP) != RET:
        raise RuntimeError("instruction budget exhausted")


class Core:
    def __init__(self, e):
        self.e = e
        self.obj = obj = e.alloc(CORE_BYTES)
        complete(e, CTOR, obj)
        vt = e.read(obj, "Q")
        slots = {k: e.read(vt + 8 * k, "Q") for k in (1, 3, 7, 8, 12)}
        assert slots == {1: INIT, 3: PREPARE, 7: PROCESS, 8: UPDATE, 12: SETTER}, slots
        complete(e, INIT, obj)
        self.in_buf = [e.alloc(QUANTUM * 4), e.alloc(QUANTUM * 4)]
        self.out_buf = [e.alloc(QUANTUM * 4), e.alloc(QUANTUM * 4)]
        self.in_tab = e.alloc(16)
        self.out_tab = e.alloc(16)
        e.write(self.in_tab, "Q", *self.in_buf)
        e.write(self.out_tab, "Q", *self.out_buf)

    def set(self, index, value):
        complete(self.e, SETTER, self.obj, index, f={2: value})

    def prepare(self, rate):
        complete(self.e, PREPARE, self.obj, 0, QUANTUM, f={1: rate}, limit=PREPARE_BUDGET)

    def quantum(self, left, right):
        e = self.e
        e.write(self.in_buf[0], "f", *left)
        e.write(self.in_buf[1], "f", *right)
        complete(e, UPDATE, self.obj)
        complete(e, PROCESS, self.obj, self.in_tab, self.out_tab, QUANTUM)
        return e.read(self.out_buf[0], "f", QUANTUM), e.read(self.out_buf[1], "f", QUANTUM)


def render(e, routes, stim, base=None):
    """Run one case: `routes` {index: value} written every quantum, `stim` ([L], [R]) per frame.
    `base` {index: value} overrides DEFAULTS before prepare (binding with a feedback base)."""
    core = Core(e)
    defaults = [[i, f32(v)] for i, v in sorted({**DEFAULTS, **(base or {})}.items())]
    for i, v in defaults:
        core.set(i, v)
    core.prepare(float(RATE))
    writes, outL, outR = [], [], []
    for q in range(N // QUANTUM):
        span = slice(q * QUANTUM, (q + 1) * QUANTUM)
        q_writes = [[i, f32(v)] for i, v in sorted(routes.items())]
        for i, v in q_writes:
            core.set(i, v)
        writes.append(q_writes)
        left, right = core.quantum(stim[0][span], stim[1][span])
        outL.extend(left)
        outR.extend(right)
    return {"defaults": defaults, "writes": writes, "outL": outL, "outR": outR}


def impulse():
    left = [0.0] * N
    left[0] = 1.0
    return [left, [0.0] * N]


def metrics(outL, outR):
    k = 3 * N // 4
    mag = [max(abs(a), abs(b)) for a, b in zip(outL[64:], outR[64:])]
    echo = max(range(len(mag)), key=lambda j: mag[j])
    return {
        "peak": max(max(map(abs, outL)), max(map(abs, outR))),
        "energy": sum(x * x for x in outL) + sum(x * x for x in outR),
        "late": sum(x * x for x in outL[k:]) + sum(x * x for x in outR[k:]),
        "echo_frame": echo + 64,
        "echo_peak": mag[echo],
    }


def diff(a, b):
    return max(max(abs(x - y) for x, y in zip(a[0], b[0])), max(abs(x - y) for x, y in zip(a[1], b[1])))


def noise():
    rng = random.Random(1408)
    return [[rng.uniform(-0.25, 0.25) for _ in range(N)], [rng.uniform(-0.25, 0.25) for _ in range(N)]]


def bind(exe, stim, indices, base=None):
    ref = render(fresh(exe), {}, stim, base)
    base_out = (ref["outL"], ref["outR"])
    print("default peak %.6g energy %.6g echo_frame %d" % tuple(
        metrics(*base_out)[k] for k in ("peak", "energy", "echo_frame")), flush=True)
    print("index d(v0,default) d(v1,default) d(v0,v1) | v0: peak energy late echo | v1: peak energy late echo")
    for i in indices:
        r0 = render(fresh(exe), {i: 0.0}, stim, base)
        r1 = render(fresh(exe), {i: 1.0}, stim, base)
        o0, o1 = (r0["outL"], r0["outR"]), (r1["outL"], r1["outR"])
        m0, m1 = metrics(*o0), metrics(*o1)
        print("%2d %.3g %.3g %.3g | %.3g %.3g %.3g %d | %.3g %.3g %.3g %d" % (
            i, diff(o0, base_out), diff(o1, base_out), diff(o0, o1),
            m0["peak"], m0["energy"], m0["late"], m0["echo_frame"],
            m1["peak"], m1["energy"], m1["late"], m1["echo_frame"]), flush=True)


def ranges(offsets):
    out = []
    for o in offsets:
        if out and o <= out[-1][1] + 4:
            out[-1][1] = o
        else:
            out.append([o, o])
    return out


BLOCK_PTRS = {0x80: "P0", 0x98: "P1", 0xB0: "P2", 0xC8: "P3", 0xE0: "P4", 0xF8: "P5"}  # begin pointers


def where(k, post, obj):
    """Label a changed byte: core object offset, or the 112-byte control block P0..P5 it lies in."""
    if k < CORE_BYTES:
        return "C+%#x" % k
    a = obj + k
    for off, name in BLOCK_PTRS.items():
        p = struct.unpack_from("<Q", post, off)[0]
        if p <= a < p + 0x70:
            return "%s+%#x" % (name, a - p)
    return "heap+%#x" % k


def state(exe, indices):
    """Core state after one setter call and no processing: bytes changed, v=0 vs v=1 (no prepare)."""
    for i in indices:
        post = []
        for v in (0.0, 1.0):
            e = fresh(exe)
            core = Core(e)
            size = e.brk - core.obj
            core.set(i, v)
            post.append((core.obj, e.uc.mem_read(core.obj, size)))
        (obj, b0), (_, b1) = post
        changed = [k for k in range(len(b0)) if b0[k] != b1[k]]
        labels = []
        for k in changed:
            lab = where(k, b1, obj)
            if labels and labels[-1][1] == k - 1 and labels[-1][2] == lab.split("+")[0]:
                labels[-1][1] = k
            else:
                labels.append([k, k, lab.split("+")[0], lab])
        names = []
        for a, b, _, lab in labels:
            base = lab.split("+")[0]
            names.append("%s+%#x..%#x" % (base, int(lab.split("+")[1], 16), int(lab.split("+")[1], 16) + (b - a)))
        print("%2d bytes %d %s" % (i, len(changed), " ".join(names[:14])), flush=True)


def case_vectors(exe, out_dir):
    for name, routes in CASES.items():
        stim = impulse()
        r = render(fresh(exe), routes, stim)
        doc = {"name": name, "rate": RATE, "block": QUANTUM, "defaults": r["defaults"], "writes": r["writes"],
               "stim": stim, "outL": r["outL"], "outR": r["outR"]}
        path = os.path.join(out_dir, "replika-native-%s.json" % name)
        with open(path, "w") as f:
            json.dump(doc, f, separators=(",", ":"))
        m = metrics(r["outL"], r["outR"])
        print(name, "peak %.6g energy %.6g echo_frame %d echo_peak %.6g" % (
            m["peak"], m["energy"], m["echo_frame"], m["echo_peak"]), flush=True)


def fresh(exe):
    """A new emulator per render: kemu never frees, and a render allocates about 22 MB of rings."""
    e = Emu(exe)
    complete(e, TABLE_INIT)  # T4 dB table, filled at load in the original
    return e


def main():
    exe = sys.argv[1]
    if sys.argv[2] == "--bind":
        bind(exe, impulse(), BINDING_INDICES)
    elif sys.argv[2] == "--state":
        state(exe, range(53) if len(sys.argv) < 4 else [int(x) for x in sys.argv[3].split(",")])
    elif sys.argv[2] == "--bind-noise":  # indices as 1,2,3; optional base as index=value,index=value
        base = {int(k): float(v) for k, v in (kv.split("=") for kv in sys.argv[4].split(","))} if len(sys.argv) > 4 else None
        bind(exe, noise(), [int(x) for x in sys.argv[3].split(",")], base)
    else:
        case_vectors(exe, sys.argv[2])


if __name__ == "__main__":
    main()
