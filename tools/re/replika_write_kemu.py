"""Native vectors for the Kontakt 8 Replika head write stage `141acd110` (597 B) under kemu, piece 3c part 3.

`141acd110(H, x_l, x_r)`: x_l in xmm1, x_r in xmm2 (f32). Native order (decompile `0141ac0000.c` 8440-8500):
1. hold (`H+0x6c` != 0) with the fade byte `H+0x5824` clear: store `H+0x348 = x_l`, `H+0x34c = x_r`, return.
2. fade byte set: store the inputs, then scale both by `1 - f32[H+0x5808]`.
3. `H+0xc0 == 0`: write the mode-0 ring slots and return. L: `**(H+0x150)[slot_l]`, R: `**(H+0x188)[slot_r]`.
4. `H+0xc0 == 1`: WriteFilter `141aa3c60` on `H+0x2dc0` (L) and `H+0x4240` (R).
   `H+0xc0 == 3` (one-pole, `141aa31f0`) and `H+0xc0 == 2` (`14199c870`) are not vectored (the port blocks them).
5. `140d947a0` (section tick) on `H+0x350` and `H+0x384`, then the deposit `141accfb0` on K1 = `H+0x1a8` with
   x = `H+0x368` (the section's lp) and K2 = `H+0x278` with x = `H+0x39c`. The deposit's ring is `**(K+0xc0)`,
   which the emulated head sets to the same handles as `H+0x150` / `H+0x188` (identity closed by emulation).

Each case is one head object run for several frames. Branch sets:
- `mode1`: mode 1, hold clear, fade clear (WriteFilter, sections, deposit).
- `hold`: mode 1, hold set, fade clear (the inputs are stored, nothing else is written).
- `fade`: mode 1, fade set (inputs stored, scaled by `1 - f32[H+0x5808]`), hold random.
- `mode0`: mode 0, hold and fade random (the ring slots).

The lane objects (`H+0x2dc0`, `H+0x4240`) are the real write-side state of head 0 after index 28 = 0.3 on noise,
taken from `replika-native-mode1-write-{l,r}.json`; every case starts from them. The snapshot after each frame holds
the head fields the write touches, the section states and lp, the deposit history and taps, the WriteFilter scalars
(`prev_t`, `prev_f`, `env`, `env_idx`, the two sections, `prev_z`, `prev_cubic`) and both synthetic rings.

    nice -n 19 python3 -I tools/re/replika_write_kemu.py KONTAKT8_EXE OUT_DIR
"""
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import replika_kemu_vectors as rk  # noqa: E402

WRITE = 0x141ACD110
H_BYTES = 0x5900
RING = 64  # synthetic ring length; all slots are generated in [0, 64) (the native rings are 2 MB)
LANE = (0x2DC0, 0x4240)  # WriteFilter objects, L and R; 0x1480 bytes each
LANE_BYTES = 0x1480
K = (0x1A8, 0x278)  # read taps (the deposit objects); K+0xc0 is the ring handle
DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "docs", "re", "engine", "vectors")
LANE_SRC = {"l": "replika-native-mode1-write-l.json", "r": "replika-native-mode1-write-r.json"}

BRANCHES = {
    # name: (mode, hold, fade, seeds, frames)
    "mode1": (1, 0, 0, (1, 2, 3, 4), 4),
    "hold": (1, 1, 0, (1, 2), 3),
    "fade": (1, None, 1, (1, 2), 3),  # hold None: random
    "mode0": (0, None, None, (1, 2, 3), 2),  # hold and fade random
}


def snapshot(e, h, rings):
    s = {
        "mode": e.read(h + 0xC0, "i"),
        "hold": e.read(h + 0x6C, "B"),
        "fade_on": e.read(h + 0x5824, "B"),
        "fade_amount": e.read(h + 0x5808, "f"),
        "ring_pos": [e.read(h + 0x158, "i"), e.read(h + 0x194, "i")],
        "ring_wrap": [e.read(h + 0x15C, "i"), e.read(h + 0x190, "i")],
        "stored": [e.read(h + 0x348, "f"), e.read(h + 0x34C, "f")],
        "sec": [],
        "sec_out": [e.read(h + 0x368, "f"), e.read(h + 0x39C, "f")],
        "taps": [],
        "wf": [],
        "ring_l": list(e.read(rings[0], "f", RING)),
        "ring_r": list(e.read(rings[1], "f", RING)),
    }
    for sec in (0x350, 0x384):
        s["sec"].append({
            "s0": e.read(h + sec, "f"), "s1": e.read(h + sec + 4, "f"),
            "c2": e.read(h + sec + 8, "f"), "c3": e.read(h + sec + 12, "f"), "c4": e.read(h + sec + 16, "f"),
            "bp": e.read(h + sec + 20, "f"), "lp": e.read(h + sec + 24, "f"),
        })
    for k in K:
        s["taps"].append({
            "wr": e.read(h + k + 0x08, "i"), "frac": e.read(h + k + 0x10, "d"),
            "mask": e.read(h + k + 0x18, "i"), "delta": e.read(h + k + 0x1C, "i"),
            "rate0": e.read(h + k + 0x90, "d"), "rate1": e.read(h + k + 0x98, "d"),
            "hist": [e.read(h + k + 0x84, "f"), e.read(h + k + 0x88, "f"), e.read(h + k + 0x8C, "f")],
        })
    for lane in LANE:
        s["wf"].append([
            e.read(h + lane + off, "f")
            for off in (0x30, 0x3C, 0x50, 0x1458, 0x58, 0x5C, 0x8C, 0x90, 0x145C, 0x1460)
        ])
    return s


def case(e, lanes, branch, seed):
    mode, hold, fade, _, frames = BRANCHES[branch]
    rnd = random.Random("%s-%d" % (branch, seed))
    h = e.alloc(H_BYTES)
    e.uc.mem_write(h, bytes(H_BYTES))
    for side, off in zip(("l", "r"), LANE):
        e.write(h + off, "I", *lanes[side])
    e.write(h + 0xC0, "i", mode)
    hold_v = rnd.randrange(2) if hold is None else hold
    fade_v = rnd.randrange(2) if fade is None else fade
    e.write(h + 0x6C, "B", hold_v)
    e.write(h + 0x5824, "B", fade_v)
    e.write(h + 0x5808, "f", rnd.uniform(0.0, 1.0))

    # Synthetic rings behind H+0x150 / H+0x188 (mode 0) and the taps' handles K+0xc0 (= H+0x268 / H+0x338).
    rings, handles = [], []
    for _ in range(2):
        data = e.alloc(RING * 4)
        e.write(data, "f", *[rnd.uniform(-0.5, 0.5) for _ in range(RING)])
        handle = e.alloc(0x18)
        e.write(handle, "Q", data)
        e.write(handle + 8, "Q", data + RING * 4)
        rings.append(data)
        handles.append(handle)
    e.write(h + 0x150, "Q", handles[0])
    e.write(h + 0x188, "Q", handles[1])
    for k, handle in zip(K, handles):
        e.write(h + k + 0xC0, "Q", handle)

    # Mode-0 slots: the indices and wraps in [0, 64) keep every slot inside the synthetic ring.
    e.write(h + 0x158, "i", rnd.randrange(64))
    e.write(h + 0x15C, "i", rnd.randrange(64))
    e.write(h + 0x194, "i", rnd.randrange(64))
    e.write(h + 0x190, "i", rnd.randrange(64))

    # Sections (H+0x350, H+0x384): coefficients in the TPT range, random integrator states.
    for sec in (0x350, 0x384):
        e.write(h + sec, "f", rnd.uniform(-0.3, 0.3), rnd.uniform(-0.3, 0.3))
        e.write(h + sec + 8, "f", rnd.uniform(0.6, 1.0), rnd.uniform(0.0, 0.5), rnd.uniform(0.0, 0.25))
        e.write(h + sec + 24, "f", rnd.uniform(-0.5, 0.5))

    # Taps: the deposit's write index, fraction, mask (63 for the 64-sample ring), count and rates, plus history.
    for k in K:
        e.write(h + k + 0x08, "i", rnd.randrange(64))
        e.write(h + k + 0x10, "d", rnd.uniform(0.0, 1.0))
        e.write(h + k + 0x18, "i", 63)
        e.write(h + k + 0x1C, "i", rnd.randrange(0, 4))
        e.write(h + k + 0x90, "d", rnd.uniform(0.5, 3.0))
        e.write(h + k + 0x98, "d", rnd.uniform(-3.0, -0.5))
        e.write(h + k + 0x84, "f", *[rnd.uniform(-0.5, 0.5) for _ in range(3)])

    init = snapshot(e, h, rings)
    case_frames = []
    for _ in range(frames):
        x = [rk.f32(rnd.uniform(-0.5, 0.5)), rk.f32(rnd.uniform(-0.5, 0.5))]
        e.call(WRITE, h, f={1: x[0], 2: x[1]})
        case_frames.append({"x": x, "after": snapshot(e, h, rings)})
    return {"branch": branch, "seed": seed, "hold": hold_v, "fade": fade_v, "init": init, "frames": case_frames}


def main():
    exe, out_dir = sys.argv[1], sys.argv[2]
    lanes = {}
    for side, name in LANE_SRC.items():
        with open(os.path.join(DIR, name)) as f:
            words = json.load(f)["words"]
        assert len(words) == LANE_BYTES // 4, len(words)
        lanes[side] = words
    e = rk.fresh(exe)
    names = {"mode1": "mode1", "hold": "hold", "fade": "fade", "mode0": "mode0"}
    for branch, (_, _, _, seeds, _) in BRANCHES.items():
        cases = [case(e, lanes, branch, s) for s in seeds]
        doc = {
            "name": "replika-head-write-" + names[branch],
            "units": {"141acd110": "head write stage", "141aa3c60": "WriteFilter (mode 1)",
                      "140d947a0": "section tick", "141accfb0": "deposit"},
            "ring_len": RING,
            "lane_words": lanes,
            "cases": cases,
        }
        path = os.path.join(out_dir, "replika-native-head-write-%s.json" % names[branch])
        with open(path, "w") as f:
            json.dump(doc, f, separators=(",", ":"))
        for c in cases:
            print("%s seed %d: hold %d fade %d, %d frames, L after %.6g, R after %.6g" % (
                branch, c["seed"], c["hold"], c["fade"], len(c["frames"]),
                c["frames"][-1]["after"]["sec_out"][0], c["frames"][-1]["after"]["sec_out"][1]), flush=True)


if __name__ == "__main__":
    main()
