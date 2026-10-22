"""Native vectors for the Kontakt 8 Replika mode-1 head driver (`141ab8c60`, 1240 B).

`141ab8c60(H, outL, outR)` runs one frame of a head with mode H+0xc0 in 1..3 (kernel flags H+0x270 and H+0x340
= 1, the `141ab87d0` read tap): the level crossfade (H+0x57c0..0x57e1), the LFO glide (H+0x57e8..0x5805), the
L and R tap speeds (H+0x58, H+0x60), the tap reads at x = f + H+0x48, the mode-1 post filters (`141aa3e00` at
H+0x56c0 and H+0x5724), the tap positions (K+0x00 = idx + frac), and the two section updates (`14199dcc0` at
H+0x350 and H+0x384, speeds 0.8 scaled and clamped at 0.9999999). The inputs of each frame (LFO A, B and pair,
s, the delay positions, the base source, the target and trigger of the crossfade, the glide target) are written
into the object before the call; the vector stores them with the outputs and the fields the port reads or writes.

Mode 2 takes its modulation and speed from the pair at H+0x5880 and H+0x80; modes 1 and 3 mix A and B with the
glide. Only flag 1 (kernel `141ab87d0`) is set. Flags 0 and 2 (kernels `141ab8a00`, `141ab7fe0`) are not exercised,
and the vectors run mode 2 with flag 1, which is not a native configuration (`141aca4a0` writes flag 2 for mode 2).

    nice -n 19 python3 -I tools/re/replika_driver_kemu.py KONTAKT8_EXE OUT_JSON
"""
import json
import os
import random
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import replika_kemu_vectors as rk  # noqa: E402

DRIVER = 0x141AB8C60
H_BYTES = 0x5900
K1, K2 = 0x1A8, 0x278
POST_L, POST_R = 0x56C0, 0x5724
SEC_L, SEC_R = 0x350, 0x384
STEPS = 128
RATE = 1.0 / 48000.0
# (seed, mode): mode 1 runs the post filters, modes 2 and 3 the other speed and modulation laws.
SEEDS = ((1, 1), (2, 2), (3, 3), (4, 1))


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


def post_coefficients(rnd):
    """c2, c3, c4 of a section with a random cutoff and damping (`Section::new`)."""
    g = rnd.uniform(0.02, 0.4)
    r = rnd.uniform(0.3, 1.2)
    c2 = 1.0 / ((g + r) * g + 1.0)
    c3 = g * c2
    return c2, c3, g * c3


def setup_post(e, h, off, rnd):
    obj = h + off
    c2, c3, c4 = post_coefficients(rnd)
    e.write(obj + 0x20, "f", f32(rnd.uniform(-20.0, 0.0)))  # env (dB)
    e.write(obj + 0x24, "f", f32(rnd.uniform(0.5, 0.99)))  # coef
    e.write(obj + 0x28, "f", f32(rnd.uniform(-0.05, 0.05)))  # s0
    e.write(obj + 0x2C, "f", f32(rnd.uniform(-0.05, 0.05)))  # s1
    e.write(obj + 0x30, "f", f32(c2))
    e.write(obj + 0x34, "f", f32(c3))
    e.write(obj + 0x38, "f", f32(c4))
    e.write(obj + 0x54, "f", f32(rnd.uniform(0.5, 1.5)))  # gain


def setup_tap(e, h, k, mask, rnd, ring_words):
    """Kernel object at H+k (the read tap state and its ring handle), the ring as f32 words."""
    ring = e.alloc(4 * (mask + 1))
    handle = e.alloc(8)
    e.write(handle, "Q", ring)
    e.write(h + k + 0xC0, "Q", handle)
    e.write(h + k + 0x18, "i", mask)
    e.write(h + k + 0x00, "d", rnd.uniform(0.0, mask))  # pos
    e.write(h + k + 0x28, "d", rnd.uniform(0.0, mask))  # phase
    e.write(h + k + 0x30, "i", rnd.randrange(mask + 1))  # idx
    e.write(h + k + 0x38, "d", 0.0)  # x_prev
    e.write(h + k + 0x48, "d", rnd.uniform(0.0, 1.0))  # offset (the driver's previous rate)
    e.write(h + k + 0x50, "f", 0.0)
    e.write(h + k + 0x54, "f", 0.0)
    e.write(h + k + 0x7C, "f", f32(rnd.uniform(0.1, 0.9)))  # damping k
    e.write(h + k + 0x80, "f", f32(RATE))  # rate factor (preset)
    e.write(h + k + 0xC8, "i", 1)  # kernel flag: 87d0
    for i, w in enumerate(ring_words):
        e.write(ring + 4 * i, "f", w)
    return ring


def setup_section(e, h, off, rnd):
    e.write(h + off + 0x2C, "f", f32(rnd.uniform(0.1, 0.9)))  # damping k
    e.write(h + off + 0x30, "f", f32(RATE))  # rate factor (preset)


# Fields of the head object H (offsets from H) that the port reads or writes, by name: (offset, format).
# The same table is the init snapshot and the per-frame state.
FIELDS = {
    "delay_base": (0x48, "d"), "base_src": (0x50, "d"), "r58": (0x58, "d"), "r60": (0x60, "d"),
    "factor": (0x80, "f"), "s": (0x98, "d"), "a": (0xC8, "d"), "b": (0xD0, "d"), "mode": (0xC0, "i"),
    "flag1": (0x270, "i"), "flag2": (0x340, "i"),
    "f_coef": (0x57C0, "d"), "f_level": (0x57C8, "d"), "f_target": (0x57D0, "d"), "f_thr": (0x57D8, "d"),
    "f_pend": (0x57E0, "B"), "f_trig": (0x57E1, "B"),
    "g_cur": (0x57E8, "f"), "g_tgt": (0x57EC, "f"), "g_step": (0x57F0, "f"), "g_thr": (0x57F4, "f"),
    "g_count": (0x57FC, "i"), "g_pend": (0x5804, "B"), "g_fresh": (0x5805, "B"),
    "pl_env": (POST_L + 0x20, "f"), "pl_coef": (POST_L + 0x24, "f"), "pl_s0": (POST_L + 0x28, "f"),
    "pl_s1": (POST_L + 0x2C, "f"), "pl_c2": (POST_L + 0x30, "f"), "pl_c3": (POST_L + 0x34, "f"),
    "pl_c4": (POST_L + 0x38, "f"), "pl_gain": (POST_L + 0x54, "f"),
    "pr_env": (POST_R + 0x20, "f"), "pr_coef": (POST_R + 0x24, "f"), "pr_s0": (POST_R + 0x28, "f"),
    "pr_s1": (POST_R + 0x2C, "f"), "pr_c2": (POST_R + 0x30, "f"), "pr_c3": (POST_R + 0x34, "f"),
    "pr_c4": (POST_R + 0x38, "f"), "pr_gain": (POST_R + 0x54, "f"),
    "sl_c2": (SEC_L + 0x08, "f"), "sl_c3": (SEC_L + 0x0C, "f"), "sl_c4": (SEC_L + 0x10, "f"),
    "sl_f": (SEC_L + 0x24, "f"), "sl_k": (SEC_L + 0x2C, "f"),
    "sr_c2": (SEC_R + 0x08, "f"), "sr_c3": (SEC_R + 0x0C, "f"), "sr_c4": (SEC_R + 0x10, "f"),
    "sr_f": (SEC_R + 0x24, "f"), "sr_k": (SEC_R + 0x2C, "f"),
}
for _name, _k in (("k1", K1), ("k2", K2)):
    FIELDS.update({
        _name + "_pos": (_k + 0x00, "d"), _name + "_mask": (_k + 0x18, "i"), _name + "_phase": (_k + 0x28, "d"),
        _name + "_idx": (_k + 0x30, "i"), _name + "_xprev": (_k + 0x38, "d"),
        _name + "_offset": (_k + 0x48, "d"), _name + "_s0": (_k + 0x50, "f"), _name + "_s1": (_k + 0x54, "f"),
        _name + "_damp": (_k + 0x7C, "f"), _name + "_rate": (_k + 0x80, "f"),
    })


def snapshot(e, h, names=None):
    return {name: e.read(h + off, fmt) for name, (off, fmt) in FIELDS.items() if names is None or name in names}


# Per-frame fields in the vector (the init snapshot is complete). Each is written or read by the port.
FRAME_FIELDS = (
    "delay_base", "r58", "r60", "f_level", "f_target", "f_pend", "f_trig", "g_cur", "g_count", "g_pend",
    "g_fresh", "k1_pos", "k2_pos", "k1_offset", "k2_offset", "k1_s0", "k1_s1", "k2_s0", "k2_s1",
    "pl_s0", "pl_s1", "pr_s0", "pr_s1", "sl_c2", "sl_c3", "sl_c4", "sl_f", "sr_c2", "sr_c3", "sr_c4", "sr_f",
)


def case(e, seed, mode, rnd):
    h = e.alloc(H_BYTES)
    e.uc.mem_write(h, bytes(H_BYTES))
    e.write(h + 0xC0, "i", mode)
    e.write(h + 0x80, "f", f32(rnd.uniform(0.2, 1.0)))  # H+0x80 (mode-2 factor)
    e.write(h + 0x57C0, "d", rnd.uniform(0.9, 0.999))  # level crossfade coefficient
    e.write(h + 0x57C8, "d", rnd.uniform(0.2, 1.0))  # level
    e.write(h + 0x57D0, "d", 1.0)  # target
    e.write(h + 0x57D8, "d", 1e-3)  # threshold
    e.write(h + 0x57E8, "f", f32(rnd.uniform(0.1, 0.9)))  # glide current
    e.write(h + 0x57EC, "f", f32(rnd.uniform(0.0, 1.0)))  # glide target
    e.write(h + 0x57F0, "f", f32(rnd.uniform(0.001, 0.05)))  # glide step
    e.write(h + 0x57F4, "f", f32(rnd.uniform(0.001, 0.02)))  # glide threshold
    e.write(h + 0x57FC, "i", rnd.randrange(6))
    e.write(h + 0x5804, "B", 0)
    e.write(h + 0x5805, "B", 0)
    e.write(h + 0x48, "d", rnd.uniform(0.0, 5.0))
    e.write(h + 0x50, "d", rnd.uniform(0.0, 5.0))
    e.write(h + 0x58, "d", rnd.uniform(0.5, 3.0))
    e.write(h + 0x60, "d", rnd.uniform(0.5, 3.0))
    e.write(h + 0x98, "d", 0.0)
    e.write(h + 0xC8, "d", rnd.uniform(1.0, 500.0))
    e.write(h + 0xD0, "d", rnd.uniform(1.0, 500.0))
    e.write(h + 0x270, "i", 1)  # K1 flag
    e.write(h + 0x340, "i", 1)  # K2 flag

    lfo = {k: e.alloc(4) for k in ("A", "B")}
    pair = e.alloc(8)
    e.write(h + 0x5868, "Q", lfo["A"])
    e.write(h + 0x5870, "Q", lfo["B"])
    e.write(h + 0x5880, "Q", pair)

    ring_l = [f32(rnd.uniform(-0.9, 0.9)) for _ in range(256)]
    ring_r = [f32(rnd.uniform(-0.9, 0.9)) for _ in range(128)]
    setup_tap(e, h, K1, 255, rnd, ring_l)
    setup_tap(e, h, K2, 127, rnd, ring_r)
    setup_post(e, h, POST_L, rnd)
    setup_post(e, h, POST_R, rnd)
    setup_section(e, h, SEC_L, rnd)
    setup_section(e, h, SEC_R, rnd)
    outL, outR = e.alloc(4), e.alloc(4)

    init = snapshot(e, h)
    frames = []
    for _ in range(STEPS):
        # Per-frame inputs. Rare writes (the target, trigger, glide target, sources) happen on about 5 % of frames.
        s = rnd.uniform(-1.0, 1.0)
        e.write(h + 0x98, "d", s)
        a, b = rnd.uniform(-1.0, 1.0), rnd.uniform(-1.0, 1.0)
        pr = [rnd.uniform(-1.0, 1.0), rnd.uniform(-1.0, 1.0)]
        e.write(lfo["A"], "f", f32(a))
        e.write(lfo["B"], "f", f32(b))
        e.write(pair, "f", f32(pr[0]))
        e.write(pair + 4, "f", f32(pr[1]))
        inp = {"s": s, "a": f32(a), "b": f32(b), "pair": [f32(pr[0]), f32(pr[1])]}
        if rnd.random() < 0.05:
            e.write(h + 0x57D0, "d", rnd.choice((0.0, 1.0, 0.5)))
        if rnd.random() < 0.05:
            e.write(h + 0x57E1, "B", 1)
        if rnd.random() < 0.05:
            e.write(h + 0x57EC, "f", f32(rnd.uniform(0.0, 1.0)))
        if rnd.random() < 0.05:
            e.write(h + 0x50, "d", rnd.uniform(0.0, 5.0))
        if rnd.random() < 0.05:
            e.write(h + 0xC8, "d", rnd.uniform(1.0, 500.0))
            e.write(h + 0xD0, "d", rnd.uniform(1.0, 500.0))
        inp.update({
            "target": e.read(h + 0x57D0, "d"),
            "trig": e.read(h + 0x57E1, "B"),
            "gtgt": e.read(h + 0x57EC, "f"),
            "base_src": e.read(h + 0x50, "d"),
            "c8": e.read(h + 0xC8, "d"),
            "d0": e.read(h + 0xD0, "d"),
        })
        e.call(DRIVER, h, outL, outR)
        state = snapshot(e, h, FRAME_FIELDS)
        state["outL"], state["outR"] = e.read(outL, "f"), e.read(outR, "f")
        frames.append({"in": inp, "state": state})
    return {"seed": seed, "mode": mode, "ring_l": ring_l, "ring_r": ring_r, "init": init, "frames": frames}


def main():
    exe, out_path = sys.argv[1], sys.argv[2]
    e = rk.fresh(exe)
    rnd_cases = [case(e, seed, mode, random.Random(seed)) for seed, mode in SEEDS]
    doc = {"name": "replika-mode1-driver", "units": "141ab8c60", "cases": rnd_cases}
    with open(out_path, "w") as f:
        json.dump(doc, f, separators=(",", ":"))
    for c in rnd_cases:
        outs = [fr["state"]["outL"] for fr in c["frames"]]
        print("seed %d mode %d: outL %.4g..%.4g, level %.4g, trig events %d" % (
            c["seed"], c["mode"], min(outs), max(outs), c["frames"][-1]["state"]["f_level"],
            sum(1 for fr in c["frames"] if fr["in"]["trig"])))


if __name__ == "__main__":
    main()
