#!/usr/bin/env python3
"""Locate the first AR ramp state divergence using synthetic input only.

Uses the recovered W15 original-byte oracle without starting a native host.
The result is diagnostic evidence, not filter admission or a relaxed tolerance.
"""
import argparse
import importlib.util
import json
import math
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--metadata', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, required=True)
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location('ar_native', Path(__file__).with_name('w15-ar-native.py'))
    ar = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(ar)
    oracle = ar.Oracle(args.metadata)
    oracle.initialize(100, 48000., .5135, .7)
    model = ar.Model(oracle)
    for ramp in [True, False]:
        signal = [[ar.q(.1 * math.sin((i + 1) * .17 + ch * .4)) for i in range(64)] for ch in range(2)]
        actual = oracle.process(signal, ramp)
        expected = model.process(signal, ramp)
        assert max(abs(a - b) for xs, ys in zip(actual, expected) for a, b in zip(xs, ys)) <= 2e-6
    oracle.run(0x140b05840, arg2=0, f3=.35)
    oracle.run(0x140b05840, arg2=1, f3=.3)
    oracle.run(0x140b03300)
    delta = tuple(oracle.get(o) for o in [0x2c0, 0x2e8, 0x310])
    before = bytes(oracle.cpu.mem_read(oracle.OBJ, 0x10000))
    signal = [[ar.q(.1 * math.sin((i + 1) * .13 + ch * .7)) for i in range(32)] for ch in range(2)]
    whole = oracle.process(signal, True)
    oracle.cpu.mem_write(oracle.OBJ, before)
    rows = []
    for i in range(32):
        signal = [[ar.q(.1 * math.sin((i + 1) * .13 + ch * .7))] for ch in range(2)]
        actual = oracle.process(signal, True)
        expected = model.process(signal, True, delta)
        native_state = {
            'controls': [oracle.get(o) for o in [0x2bc, 0x2e4, 0x30c]],
            'channels': [[oracle.get(0x70 + ch * 0x24 + k * 4) for k in range(9)] for ch in range(2)],
        }
        rows.append({'frame': i, 'actual': actual, 'whole_block': [channel[i] for channel in whole],
                     'expected': expected,
                     'native': native_state,
                     'model': {'controls': [model.g, model.hz_lane, model.res], 'channels': [list(s) for s in model.state]},
                     'peak_error': max(abs(a[0] - b[0]) for a, b in zip(actual, expected))})
    result = {'binary_sha256': oracle.meta['sha256'], 'saved': 100, 'rate': 48000,
              'delta': delta, 'rows': rows, 'tolerance': 2e-6,
              'max_error': max(row['peak_error'] for row in rows),
              'whole_block_error': max(abs(channel[row['frame']] - row['expected'][ch][0])
                                       for row in rows for ch, channel in enumerate(whole)),
              'partition_error': max(abs(row['whole_block'][ch] - row['actual'][ch][0])
                                     for row in rows for ch in range(2))}
    args.receipt.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({key: result[key] for key in ['max_error', 'whole_block_error', 'partition_error']}))


if __name__ == '__main__':
    main()
