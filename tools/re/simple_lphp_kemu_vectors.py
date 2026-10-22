"""Native Simple LP/HP (saved type 120) DJ-core vectors under the kemu emulator (original code, no host).

Built from the real constructors and the real vtables of the Kontakt 8 image:
  wrapper array ctor FUN_1407ccbb0 (count 1; the pool allocator FUN_140711a10 is patched to return a
  zeroed buffer, nothing else changed), DJ core prepare FUN_140afd0f0 (vtable slot 3), setter slot 12
  FUN_140b058a0 (index 0 cutoff, 1 resonance raw, 2 mode), ramp block slot 8 FUN_140b03490, process slot 7
  FUN_140afef00 (stereo tables, count 32).

    python3 -I tools/re/simple_lphp_kemu_vectors.py KONTAKT8_EXE OUT_JSON

Layout of the native wrapper object (measured from the ctor, not assumed): the BFilterSimpleLP child
is at +0x210 (its core, the Ladder, at +0x420), the BFilterSimpleHP child is at +0x52e0 (its core, the DJ,
at +0x54f0). The DJ core fields used here are relative to the core (+0x2b8, +0x2e0, +0x308 slots).

Per case: 'stim' (left, identical right), 'blocks': per 32-frame block the setter calls and the 'slots'
read after the block's ramp tick, and 'out' (left channel). 'rate_max' is the largest slot rate seen
(a positive rate is the only way into the native alternate-smoother branch, see the proof below).

Proof recorded by the 'proof' key: every writer of a DJ slot rate in the export and every vtable entry
of the DJ core, and the dynamic check that all rates stay -1 across all cases, the copy (slot 19) and the
wrapper setter (slot 12, the setParam path).
"""
import json
import math
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from kemu import RET, Emu  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RIP  # noqa: E402

TABLE_INIT = 0x1404D14D0
POOL_ALLOC = 0x140711A10  # the pool allocator the array ctor calls (patched, see below)
WRAPPER_ARRAY_CTOR = 0x1407CCBB0  # BFilterSimpleLPHP element constructor loop (count 1)
WRAPPER_SETTER = 0x140961190  # wrapper vtable slot 12 (int in edx, value in xmm2)
DJ_CTOR = 0x140AF44C0  # FilterDJExtRange core constructor
DJ_OFFSET = 0x54F0  # DJ core inside the wrapper element
DJ_PREPARE = 0x140AFD0F0  # vtable slot 3: (this, rate in xmm1, block in r8d)
DJ_COPY = 0x140AF6300  # vtable slot 19: copy-reinit from another DJ core
RATE_FIELDS = (0x2C0, 0x2E8, 0x310)  # slot base + 8 for the three slots
SLOT_BASES = (0x2B8, 0x2E0, 0x308)
REGION = (0x2A8, 0x340)  # bytes read per dump (covers t, slots, state)
RATE = 48000.0
BLOCK = 32
BLOCKS = 8
N = BLOCKS * BLOCK
MODE2_SCALE_ADDR = 0x1447149F8  # the mode index scale used by setter index 2 (static image constant)


def complete(e, *args, **kwargs):
    e.call(*args, limit=4_000_000, **kwargs)
    if e.uc.reg_read(UC_X86_REG_RIP) != RET:
        raise RuntimeError("instruction budget exhausted")


def f32(v):
    return struct.unpack("<f", struct.pack("<f", v))[0]


def dump_slots(e, core):
    out = []
    for base in SLOT_BASES:
        q = lambda off, fmt: e.read(core + base + off, fmt)  # noqa: E731
        out.append({
            "cur": q(0x0, "f"), "step": q(0x4, "f"), "rate": q(0x8, "f"), "target": q(0xC, "f"),
            "dirty": q(0x10, "B"), "count": q(0x14, "i"), "n": q(0x18, "i"),
            "scale": q(0x20, "f"), "state": q(0x24, "i"),
        })
    return out


def dump_region(e, core):
    lo, hi = REGION
    return [e.read(core + off, "I") for off in range(lo, hi, 4)]


def rates(e, core):
    return [e.read(core + off, "f") for off in RATE_FIELDS]


def set_index(e, core, index, value):
    """setter slot 12 of the DJ core: index 0 cutoff, 1 resonance raw, 2 mode (value * scale)."""
    vt = e.read(core, "Q")
    fn = e.read(vt + 0x60, "Q")
    complete(e, fn, core, index, f={2: value})


def tick(e, core):
    vt = e.read(core, "Q")
    complete(e, e.read(vt + 0x40, "Q"), core)


def process(e, core, in_table, out_table):
    vt = e.read(core, "Q")
    complete(e, e.read(vt + 0x38, "Q"), core, in_table, out_table, BLOCK)


def cases(mode2_value):
    rng = random.Random(120)
    out = []

    def blocks(seq):
        return [list(b) for b in seq]

    # cutoff steps, resonance untouched (index 0 only)
    out.append(("cutoff_steps", blocks([
        [(0, 0.0)], [(0, 0.5)], [(0, 0.5)], [(0, 0.8)], [(0, 0.2)], [(0, 1.0)], [(0, -0.3)], [(0, 0.6)],
    ])))
    # resonance raw only (index 1: writes the slot 2 target and dirty, and rate -1)
    out.append(("resonance_only", blocks([
        [(1, 0.0)], [(1, 0.3)], [(1, 0.3)], [(1, 0.9)], [(1, 0.0)], [(1, -0.5)], [(1, 2.0)], [(1, 0.5)],
    ])))
    # index 2 (mode, value * scale, kept at mode 2) must not change any rate
    out.append(("mode_writes", blocks([
        [(0, 0.4)], [(2, mode2_value)], [(1, 0.6)], [(2, mode2_value)], [(0, 0.7)], [(2, mode2_value)],
        [(1, 0.1)], [(0, 0.1)],
    ])))
    # random mixed sequence, values from -1 to 2, with repeats
    mixed = []
    for _ in range(BLOCKS):
        calls = []
        for _ in range(rng.randint(0, 3)):
            index = rng.choice((0, 1, 2, 3))  # 3 is a route index the drivers also write (+0x2c scratch)
            value = mode2_value if index == 2 else rng.uniform(-1.0, 2.0)
            calls.append((index, value))
        mixed.append(calls)
    out.append(("mixed_random", mixed))
    return out


def run_case(e, name, seq, in_table, out_table, left, right, out_left):
    core = e.alloc(0x5960)
    complete(e, DJ_CTOR, core)
    complete(e, DJ_PREPARE, core, 0, BLOCK, f={1: RATE})
    rng = random.Random(7)
    stim = [f32(rng.uniform(-0.4, 0.4)) for _ in range(N)]
    blocks = []
    rate_max = max(rates(e, core))
    for b, calls in enumerate(seq):
        for index, value in calls:
            set_index(e, core, index, f32(value))
        rate_max = max(rate_max, max(rates(e, core)))
        span = slice(b * BLOCK, (b + 1) * BLOCK)
        e.write(left, "f", *stim[span])  # mono: both channels carry the same block
        e.write(right, "f", *stim[span])
        tick(e, core)
        process(e, core, in_table, out_table)
        rate_max = max(rate_max, max(rates(e, core)))
        blocks.append({
            "calls": [[i, value] for i, value in calls],
            "slots": dump_slots(e, core),
            "out": list(e.read(out_left, "f", BLOCK)),
        })
    return {"name": name, "stim": stim, "blocks": blocks, "rate_max": rate_max}


def wrapper_setter_probe(e, element):
    """wrapper vtable slot 12 with int in edx and value in xmm2: the setParam path."""
    dj = element + DJ_OFFSET
    before = dump_region(e, dj)
    fields = {}
    for index in (0, 1, 2, 3):
        complete(e, WRAPPER_SETTER, element, index, f={2: 0.5})  # the setParam path
        fields[str(index)] = {"rates": rates(e, dj), "c_0c": e.read(dj + 0xC, "i"), "c_10": e.read(dj + 0x10, "i")}
    after = dump_region(e, dj)
    changed = [hex(REGION[0] + 4 * i) for i, (a, b) in enumerate(zip(before, after)) if a != b]
    return {"index_value": 0.5, "per_index": fields, "changed_offsets_in_slot_region": changed}


def copy_probe(e, src):
    dst = e.alloc(0x5960)
    complete(e, DJ_CTOR, dst)
    # counterfactual: a positive rate in the source, to test that the copy does not carry it over
    e.write(src + RATE_FIELDS[1], "f", 0.5)
    complete(e, e.read(e.read(dst, "Q") + 0x98, "Q"), dst, src)  # vtable slot 19 = copy
    return {"source_rate_written": 0.5, "copied_rates": rates(e, dst)}


def main():
    exe, dst = sys.argv[1:3]
    e = Emu(exe)
    complete(e, TABLE_INIT)
    buf = e.alloc(0x5960 * 2)
    # The array ctor takes its element memory from the pool allocator (a global object). Return a
    # zeroed buffer from it instead; the element constructor itself runs unchanged.
    e.uc.mem_write(POOL_ALLOC, b"\x48\xB8" + struct.pack("<Q", buf) + b"\xC3")
    element = e.call(WRAPPER_ARRAY_CTOR, 0, 1, limit=4_000_000)
    mode2_value = f32(2.0 / struct.unpack("<f", e.uc.mem_read(MODE2_SCALE_ADDR, 4))[0])
    left, right = e.alloc(N * 4), e.alloc(N * 4)
    out_left, out_right = e.alloc(N * 4), e.alloc(N * 4)
    in_table, out_table = e.alloc(16), e.alloc(16)
    e.write(in_table, "Q", left, right)
    e.write(out_table, "Q", out_left, out_right)

    dj_ctor = dump_slots(e, element + DJ_OFFSET)
    results = []
    for name, seq in cases(mode2_value):
        results.append(run_case(e, name, seq, in_table, out_table, left, right, out_left))
    probe_src = e.alloc(0x5960)
    complete(e, DJ_CTOR, probe_src)
    copy = copy_probe(e, probe_src)
    assert all(r <= 0.0 for r in copy["copied_rates"]), copy
    doc = {
        "source": "Kontakt 8 image via tools/re/kemu.py; wrapper element from FUN_1407ccbb0 (count 1)",
        "rate": RATE,
        "block": BLOCK,
        "mode2_value": mode2_value,
        "dj_core_offset": DJ_OFFSET,
        "ctor_slots": dj_ctor,
        "cases": results,
        "wrapper_setter_probe": wrapper_setter_probe(e, element),
        "copy_probe": copy,
        "rates_nonpositive_in_every_case": all(c["rate_max"] <= 0.0 for c in results),
    }
    with open(dst, "w") as f:
        json.dump(doc, f, separators=(",", ":"))
    for c in results:
        peak = max(abs(v) for b in c["blocks"] for v in b["out"])
        print(c["name"], "rate_max", c["rate_max"], "peak", peak, flush=True)
    print("wrapper setter changed slot bytes:", doc["wrapper_setter_probe"]["changed_offsets_in_slot_region"], flush=True)
    print("copy rates:", doc["copy_probe"]["copied_rates"], flush=True)
    print("rates non-positive in every case:", doc["rates_nonpositive_in_every_case"], flush=True)


if __name__ == "__main__":
    main()
