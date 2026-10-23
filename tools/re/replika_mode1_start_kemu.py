"""Start state of the Kontakt 8 Replika mode-1 chain (index 28 = 0.3, path k = 1) under kemu, piece 3c part 4.

Dumps the native state the frame loop `141aa9480` starts from: the setter 28 = 0.3 has run (selector `141aca670`,
mode writers, design chain `141abbc90`, reset of the taps), no frame has been processed. The dump holds the three
inline head objects (`C+0xac0`, `C+0x6380`, `C+0xbc40`, 0x5900 bytes each, as their nonzero u32 words, so the
write lanes (`+0x2dc0`, `+0x4240`, with the MT state) and the sections and taps are exact), the smoothed spread
table `C+0x14208..0x142b0` (four entries of value, target, threshold and flags, with the coefficient in the
previous entry's last slot), the bounds `C+0xad0`/`C+0xad8`, the path `C+0x604`, the flag `C+0x600`, the
smoothing flag `C+0x938` and the P0 control block that holds the spread targets (`+0x2c`, `+0x30`, `+0x34`, `+0x38`).

The Rust test `replika::chain::tests::start_matches_native_dump` compares `Mode1::start` (built in Rust, `replika/start.rs`)
with this dump field by field; the production start does not read it. The vectors (`replika-native-mode1-index28-*.json`) are
the output of the same run, with the same routes.

    nice -n 19 python3 -I tools/re/replika_mode1_start_kemu.py KONTAKT8_EXE OUT_JSON

Output: {"name", "rate", "heads": {"H1": [[offset, u32], ...], "H2": ..., "H3": ...}, "core": [[offset, u32], ...],
"p0": [[offset, u32], ...]} with offsets relative to the head, core and P0 block objects (P0 = the block at `C+0x80`).
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import replika_kemu_vectors as rk  # noqa: E402

HEADS = (("H1", 0xAC0), ("H2", 0x6380), ("H3", 0xBC40))
HEAD_BYTES = 0x5900
TABLE = (0x14208, 0x142B0)  # coefficients, entry values, targets, thresholds and flags
CORE_RANGES = ((0x938, 0x939), (0x600, 0x604), (0x604, 0x608), (0xAD0, 0xAE0), (0x8B0, 0x8B4))
P0_PTR = 0x80
P0_BYTES = 0x70


def words(raw, base, lo, hi):
    out = []
    for k in range(lo & ~3, hi, 4):
        w = int.from_bytes(raw[k:k + 4], "little")
        if w:
            out.append([base + k, w])
    return out


def main():
    exe, out_path = sys.argv[1], sys.argv[2]
    e = rk.fresh(exe)
    core = rk.Core(e)
    for i, v in sorted(rk.DEFAULTS.items()):
        core.set(i, rk.f32(v))
    core.prepare(float(rk.RATE))
    core.set(28, rk.f32(0.3))  # the selector runs here; no frame has run yet
    raw_core = e.uc.mem_read(core.obj, rk.CORE_BYTES)
    doc = {"name": "mode1-index28-start", "rate": rk.RATE, "heads": {}, "core": [], "p0": []}
    for name, off in HEADS:
        raw = e.uc.mem_read(core.obj + off, HEAD_BYTES)
        doc["heads"][name] = words(raw, 0, 0, HEAD_BYTES)
    doc["core"] += words(raw_core, 0, TABLE[0], TABLE[1])
    for lo, hi in CORE_RANGES:
        doc["core"] += [[k, int.from_bytes(raw_core[k:k + 4], "little")] for k in range(lo & ~3, hi, 4)]
    p0 = int.from_bytes(raw_core[P0_PTR:P0_PTR + 8], "little")
    block = e.uc.mem_read(p0, P0_BYTES)
    doc["p0"] = [[k, int.from_bytes(block[k:k + 4], "little")] for k in range(0, P0_BYTES, 4)]
    with open(out_path, "w") as f:
        json.dump(doc, f, separators=(",", ":"))
    print("heads", {k: len(v) for k, v in doc["heads"].items()}, "core words", len(doc["core"]))


if __name__ == "__main__":
    main()
