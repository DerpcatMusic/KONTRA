"""Native PRO-53 (saved 16) vectors under the kemu emulator (original code, no host).

Runs the unchanged Pro-53 code: rate setter FUN_1408de8e0 (rate at +0x14, scale K/rate at +0x364),
coefficient setter FUN_14095d940, process FUN_1409099a0 and the block wrapper FUN_140ae9f90, which
runs 32-frame chunks through the process, moves the cutoff and resonance clock and calls the setter.

    python3 -I tools/re/cascade_kemu_vectors.py KONTAKT8_EXE OUT_JSON

The harness allocates its own vtable with the slot offsets of the Pro-53 vtable in the image (base
0x144F0FBD0: slot 0x108 process, 0x120 setter; the image holds both there). The constructor was not
located, so the harness builds the object: zeroed memory, the vtable pointer, the rate setter,
channel count (+0x124), input and output channel pointers (+0x20, +0xA0). Stored raw knobs
(+0x1E4/+0x1E8) start at -123 so the first setter updates (its guard only skips identical values).
Slots 0x28, 0x168 and 0x170 are no-op stubs (rate hook and wrapper modulation checks; none here).

Cases ('name', 'rate', 'start', 'end', 'input', 'frames', 'out' = one list per channel):
  static    one setter, then 32-frame process calls (48, 44.1, 96 kHz).
  single128 one 128-frame process call (the 32+ frame call path; equal to 32-frame calls).
  moving    64-frame wrapper calls; goals move from start to end, one 32-frame clock step per chunk
            and one more step at the end of each call; the step is taken once per call.
  stereo    two channels with separate inputs (left impulse, right square) over shared coefficients.
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import RET, Emu  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RIP  # noqa: E402

TABLE_INIT = 0x1404D14D0
SET_RATE = 0x1408DE8E0  # FUN_1408de8e0(this, rate): +0x14 = rate (via FUN_140a9a300), +0x364 = K / rate
SETTER = 0x14095D940  # FUN_14095d940(this, cutoff, resonance)
PROCESS = 0x1409099A0  # FUN_1409099a0(this, _, offset, frames, replace)
WRAPPER = 0x140AE9F90  # FUN_140ae9f90(this, _, replace): 32-frame chunks, clock, setter
VTABLE = 0x144F0FBD0  # slot 0x8 = SET_RATE, 0x108 = PROCESS, 0x120 = SETTER (checked in the image)
OFF_FRAMES, OFF_CHANNELS, OFF_IN, OFF_OUT = 0x120, 0x124, 0x20, 0xA0
OFF_RAW, OFF_DIRTY, OFF_GOAL_CUT, OFF_GOAL_RES = 0x1E4, 0x1A1, 0x1DC, 0x1E0
OBJ_BYTES = 0x2000
STUB = bytes([0x31, 0xC0, 0xC3])  # xor eax, eax; ret


def complete(e, fn, *args, **kwargs):
    e.call(fn, *args, limit=20_000_000, **kwargs)
    if e.uc.reg_read(UC_X86_REG_RIP) != RET:
        raise RuntimeError("instruction budget exhausted")


class Pro53:
    def __init__(self, e, rate, channels, frames):
        self.e = e
        stub = e.alloc(16)
        e.uc.mem_write(stub, STUB)
        vt = e.alloc(0x200)
        for off in (0x28, 0x168, 0x170):
            e.write(vt + off, "Q", stub)
        e.write(vt + 0x108, "Q", PROCESS)
        e.write(vt + 0x120, "Q", SETTER)
        self.obj = obj = e.alloc(OBJ_BYTES)
        e.write(obj, "Q", vt)
        e.write(obj + OFF_RAW, "f", -123.0)
        e.write(obj + OFF_RAW + 4, "f", -123.0)
        complete(e, SET_RATE, obj, f={1: rate})
        self.frames = frames
        self.inputs = [e.alloc(frames * 4) for _ in range(channels)]
        self.outputs = [e.alloc(frames * 4) for _ in range(channels)]
        for ch in range(channels):
            e.write(obj + OFF_IN + 8 * ch, "Q", self.inputs[ch])
            e.write(obj + OFF_OUT + 8 * ch, "Q", self.outputs[ch])
        e.write(obj + OFF_CHANNELS, "i", channels)

    def feed(self, stim):
        for buf, samples in zip(self.inputs, stim):
            self.e.write(buf, "f", *samples)

    def read(self, count):
        return [list(self.e.read(buf, "f", count)) for buf in self.outputs]

    def setter(self, cut, res):
        complete(self.e, SETTER, self.obj, f={1: cut, 2: res})

    def process(self, offset, frames):
        complete(self.e, PROCESS, self.obj, 0, offset, frames, 1)

    def wrapper(self, frames):
        self.e.write(self.obj + OFF_FRAMES, "i", frames)
        complete(self.e, WRAPPER, self.obj, 0, 1)


def stimulus(kind, n):
    if kind == "imp":
        return [1.0] + [0.0] * (n - 1)
    return [4.0 if (i // 20) % 2 == 0 else -4.0 for i in range(n)]


def static_case(e, rate, cut, res, kind, n=64, channels=1, stim=None, chunk=32):
    p = Pro53(e, rate, channels, n)
    p.feed(stim or [stimulus(kind, n)] * channels)
    p.setter(cut, res)
    for offset in range(0, n, chunk):
        p.process(offset, min(chunk, n - offset))
    return p.read(n)


def moving_case(e, rate, start, end, kind, frames, channels=1, call=64):
    """Wrapper calls of `call` frames: the clock step is taken once per call (from the resonance at
    the call start), so the call size is part of the vector. The buffers advance per call."""
    p = Pro53(e, rate, channels, frames)
    p.feed([stimulus(kind, frames)] * channels)
    e.write(p.obj + OFF_GOAL_CUT, "f", start[0])
    e.write(p.obj + OFF_GOAL_RES, "f", start[1])
    e.write(p.obj + OFF_DIRTY, "B", 1)
    p.wrapper(0)  # snap the clock to the start
    p.setter(*start)
    e.write(p.obj + OFF_GOAL_CUT, "f", end[0])
    e.write(p.obj + OFF_GOAL_RES, "f", end[1])
    for at in range(0, frames, call):
        for ch in range(channels):
            e.write(p.obj + OFF_IN + 8 * ch, "Q", p.inputs[ch] + 4 * at)
            e.write(p.obj + OFF_OUT + 8 * ch, "Q", p.outputs[ch] + 4 * at)
        p.wrapper(call)
    return p.read(frames)


def main():
    exe, dst = sys.argv[1:3]
    e = Emu(exe, heap_mb=512)
    complete(e, TABLE_INIT)  # exponential table, filled at load in the original
    cases = []
    # Static: one setter, then 32-frame process calls (48 kHz PRO52 keys are the old vectors).
    for rate, knobs in [(48000, [(0.30, 0.0), (0.60, 0.5), (0.45, 0.9)]),
                        (44100, [(0.30, 0.0), (0.60, 0.5)]), (96000, [(0.30, 0.0), (0.60, 0.5)])]:
        for cut, res in knobs:
            for kind in ("imp", "loud"):
                cases.append({"name": "static", "rate": rate, "start": [cut, res], "end": [cut, res],
                              "input": [kind], "frames": 64,
                              "out": static_case(e, float(rate), cut, res, kind)})
    # One 128-frame call versus 32-frame calls (native call size does not change the output).
    cases.append({"name": "single128", "rate": 48000, "start": [0.45, 0.9], "end": [0.45, 0.9],
                  "input": ["loud"], "frames": 128,
                  "out": static_case(e, 48000.0, 0.45, 0.9, "loud", n=128, chunk=128)})
    # Moving: the wrapper snaps to start, then moves the goals to end in 32-frame chunks.
    for rate in (48000, 44100, 96000):
        for start, end in [((0.30, 0.0), (0.90, 0.50)), ((0.80, 0.30), (0.20, 0.30))]:
            for kind in ("imp", "loud"):
                cases.append({"name": "moving", "rate": rate, "start": list(start), "end": list(end),
                              "input": [kind], "frames": 256, "out": moving_case(e, float(rate), start, end, kind, 256)})
    # The clock step is taken once per call from the resonance at its start: a sweep whose
    # resonance keeps rising across calls separates that from a step fixed at the block start.
    cases.append({"name": "moving", "rate": 48000, "start": [0.00, 0.50], "end": [1.00, 0.90],
                  "input": ["loud"], "frames": 512,
                  "out": moving_case(e, 48000.0, (0.00, 0.50), (1.00, 0.90), "loud", 512)})
    # Stereo: two channels, own stage state, shared coefficients (left impulse, right square).
    cases.append({"name": "stereo", "rate": 48000, "start": [0.30, 0.0], "end": [0.30, 0.0],
                  "input": ["imp", "loud"], "frames": 64,
                  "out": static_case(e, 48000.0, 0.30, 0.0, None, channels=2,
                                     stim=[stimulus("imp", 64), stimulus("loud", 64)])})
    for case in cases:  # 9 significant digits round-trip float32 exactly
        case["out"] = [[float("%.9g" % v) for v in ch] for ch in case["out"]]
    with open(dst, "w") as f:
        json.dump({"native": "Kontakt8 FUN_1409099a0 / FUN_14095d940 / FUN_140ae9f90 under kemu",
                    "cases": cases}, f, separators=(",", ":"))
    print("cases", len(cases), flush=True)


if __name__ == "__main__":
    main()
