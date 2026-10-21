"""Differential fixtures for the process block 0x890a00: seeded noise in, emulated samples out.

Writes oracle/process_fixture_<rate>_m<mode>.json. Each file holds the f32-rounded input
and the emulated output of K blocks on the constructor state (identity band state, synthetic
host; see process_oracle.py for the stand-ins). The Rust differential test
(src/reference/analysis.rs, `emulated_identity_fixtures`) reads these files.

Input is uniform noise from random.Random(SEED) per channel, rounded to f32 before it is
given to the emulator, so the Rust side can feed exactly the same values.

Run with: python3 -I process_fixture.py [--rates 44100,48000,96000,192000] [--modes 0,1]
"""

import argparse
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from process_lib import Session  # noqa: E402
import process_oracle as po  # noqa: E402

SEED = 20261021
ORACLE_DIR = po.ORACLE_DIR


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


def fixture(rate, mode):
    total = po.K * po.N
    rnd = random.Random(SEED + int(rate) + mode)
    inp = [[f32(rnd.uniform(-0.5, 0.5)) for _ in range(total)] for _ in range(2)]
    sess = Session(rate)
    sess.prepare()
    if mode == 1:
        sess.emu.wr(sess.obj + po.ZL_OFFSET, b"\x01")
    latency = sess.latency()
    out0, out1, _, fault = po.run_blocks(sess, inp, total)
    if fault:
        raise RuntimeError(f"rate {rate} mode {mode}: {fault}")
    return {
        "source": "process block 0x890a00 under Unicorn, constructor state, synthetic host",
        "evidence_scope": "isolated emulation; identity band state; see process_oracle.py",
        "rate": rate,
        "mode": mode,
        "seed": SEED + int(rate) + mode,
        "block_size": po.N,
        "blocks": po.K,
        "latency_getter": latency,
        "input": [[float(f"{v:.9g}") for v in ch] for ch in inp],
        "output": [[float(f"{v:.9g}") for v in ch] for ch in (out0, out1)],
    }


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument("--rates", default="44100,48000,96000,192000")
    ap.add_argument("--modes", default="0,1")
    ap.add_argument("--out-dir", default=str(ORACLE_DIR))
    args = ap.parse_args(argv)
    out_dir = Path(args.out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    for r in (float(x) for x in args.rates.split(",")):
        for mode in (int(m) for m in args.modes.split(",")):
            doc = fixture(r, mode)
            path = out_dir / f"process_fixture_{int(r)}_m{mode}.json"
            path.write_text(json.dumps(doc, separators=(",", ":")) + "\n")
            print(path, "latency", doc["latency_getter"])


if __name__ == "__main__":
    main(sys.argv[1:])
