#!/usr/bin/env python3
"""Synthetic AR wrapper receipts; natural clock/dispatcher, no host or library."""
import argparse
import hashlib
import importlib.util
import json
import math
import struct
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("ar_native", Path(__file__).with_name("w15-ar-native.py"))
ar = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ar)

KNOBS = [(0.5135, 0.7), (0.35, 0.3), (0.35, 0.3), (0.8, 0.9)]
POINTS = [0, 1, 7, 15, 31]
RAMP_OFFSETS = [0x2bc, 0x2e4, 0x30c]


def run_case(oracle, mode, rate, mask, partitions):
    oracle.initialize(100 + mode, rate, *KNOBS[0])
    oracle.put(0x1bc, 0, "i", oracle.OBJ)
    oracle.put(0x124, 2, "I", oracle.OBJ)
    for lane in range(3):
        oracle.put(0x550 + lane, (mask >> lane) & 1, "B", oracle.OBJ)
        oracle.put(0x558 + lane * 8, oracle.IO + 0x8000 + lane * 0x1000, "Q", oracle.OBJ)
    audio, states = [], []
    previous = KNOBS[0]
    for quantum, knobs in enumerate(KNOBS):
        # Unrouted physical writes request a setter only when the value changes.
        for lane, value in enumerate(knobs):
            if not mask & (1 << lane) and value != previous[lane]:
                oracle.run(0x140b05840, arg2=lane, f3=value)
            oracle.cpu.mem_write(oracle.IO + 0x8000 + lane * 0x1000, struct.pack("<f", value))
        previous = knobs
        offset = 0
        for count in partitions:
            for ch in range(2):
                oracle.put(0x20 + ch * 8, oracle.IO + 0x1000 + ch * 0x2000, "Q", oracle.OBJ)
                signal = [ar.q(0.1 * math.sin((quantum * 32 + offset + i + 1) * 0.17 + ch * 0.4))
                          for i in range(count)]
                oracle.cpu.mem_write(oracle.IO + 0x1000 + ch * 0x2000, struct.pack("<" + "f" * count, *signal))
            oracle.put(0x120, count, "I", oracle.OBJ)
            oracle.run(0x1408f9d90, obj=oracle.OBJ, arg2=oracle.CTX)
            output = [struct.unpack("<" + "f" * count, oracle.cpu.mem_read(oracle.CTX + 0x9c010 + ch * 0x1000, count * 4))
                      for ch in range(2)]
            audio.extend([output[ch][i] for ch in range(2)] for i in range(count))
            offset += count
        assert offset == 32 and oracle.get(0x1bc, "i", oracle.OBJ) == 0
        states.append([dict(current=oracle.get(o), target=oracle.get(o + 12), delta=oracle.get(o + 4),
                            remaining=oracle.get(o + 20, "i"), active=oracle.get(o + 17, "B"))
                       for o in RAMP_OFFSETS])
    assert all(math.isfinite(v) for frame in audio for v in frame)
    return audio, states


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    oracle = ar.Oracle(args.metadata)
    cases = []
    for mode in range(9):
        for rate in [8000, 44100, 48000, 96000, 192000]:
            for mask in range(4):
                audio, states = run_case(oracle, mode, rate, mask, [32])
                partitioned, split_states = run_case(oracle, mode, rate, mask, [1, 7, 24])
                assert audio == partitioned and states == split_states, (mode, rate, mask, "native partition")
                cases.append(dict(mode=mode, rate=rate, mask=mask,
                                  checkpoints=[[audio[q * 32 + p] for p in POINTS] for q in range(4)],
                                  ramps=states))
    result = dict(binary_sha256=oracle.meta["sha256"],
                  producer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                  knobs=KNOBS, checkpoint_frames=POINTS, cases=cases,
                  limitations=["Synthetic natural wrapper only; outer rack mixing helpers replaced.",
                               "No library, service-address, voice/bus scheduling, heap or CPU/RAM admission.",
                               "Native exponential table initialized from verified law."])
    args.receipt.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(dict(cases=len(cases), native_partition_equal=True)))


if __name__ == "__main__":
    main()
