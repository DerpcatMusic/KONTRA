"""Execute pinned Workstation Robotizer and Dispersor bytes with authored state.

Reuses the existing isolated CPU harness. No reference application is launched.
Public parameter conversion, factory tables and complete effects remain untested.
"""
import json
import math
from pathlib import Path
import random
import struct
import sys

REPO = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(REPO / 'artifacts/engine-analysis-2026-10-07/uvi-workstation'))
import verify_lfo_code as h
from unicorn import UC_HOOK_CODE


def pack(values): return struct.pack('<' + 'f' * len(values), *values)
def add(a, b): return h.f32(a + b)
def sub(a, b): return h.f32(a - b)
def mul(a, b): return h.f32(a * b)


def dispersor():
    rows = []
    rng = random.Random(0x446973706572736f72)
    for count in [0, 1, 3, 4, 8, 16, 64]:
        for coefficient in [-.75, 0., .75]:
            h.cpu.mem_write(h.OBJ, bytes(0x108))
            h.put(h.OBJ, 'f', coefficient)
            h.put(h.OBJ + 0x104, 'I', count)
            history = [h.f32(rng.uniform(-.25, .25)) for _ in range(count)]
            h.cpu.mem_write(h.OBJ + 4, pack(history))
            for frames in [0, 1, 3, 4, 17, 32, 33, 129]:
                samples = [h.f32(rng.uniform(-1., 1.)) for _ in range(frames)]
                h.cpu.mem_write(h.OUT, pack(samples) + b'\xcd' * 16)
                expected = []
                for value in samples:
                    for stage in range(count):
                        output = add(mul(value, coefficient), history[stage])
                        history[stage] = sub(value, mul(output, coefficient))
                        value = output
                    expected.append(value)
                h.run(0x1412ea230, h.OUT, frames)
                assert bytes(h.cpu.mem_read(h.OUT, 4 * frames)) == pack(expected), (count, coefficient, frames, 'PCM')
                assert bytes(h.cpu.mem_read(h.OBJ + 4, 4 * count)) == pack(history), (count, coefficient, frames, 'history')
                assert h.get(h.OBJ + 0x104, 'I') == count
                assert bytes(h.cpu.mem_read(h.OUT + 4 * frames, 16)) == b'\xcd' * 16
                rows.append(dict(stages=count, coefficient=coefficient, frames=frames, exact_output_and_history=True))
    return rows


def lookup(table, phase):
    index = phase >> 24
    fraction = h.f32((phase & 0xffffff) * 2**-24)
    return add(mul(sub(1., fraction), table[index]), mul(fraction, table[index + 1]))


def robotizer():
    rows = []
    rng = random.Random(0x526f626f74697a6572)
    entry = 0x141307d20
    for address, expected in [(0x141e4e3c8, 2**-24), (0x141cfabf8, 2**31),
                              (0x141cfaa20, .5), (0x141cf03dc, 1.)]:
        assert h.get(address, 'f') == expected, (hex(address), 'native constant')
    tables = {'ramp': [h.f32(i / 128 - 1) for i in range(257)],
              'sin': [h.f32(math.sin(i * math.tau / 256)) for i in range(257)]}
    fm_table = [h.f32(i / 128 - 1) for i in range(257)]
    for table_name, carrier in tables.items():
        for channels in [1, 2, 12]:
            for fm_depth in [0., .25]:
                for mix in [0., .5, 1.]:
                    for filter_mode in [-1., 0., 1.]:
                        h.cpu.mem_write(h.OBJ, bytes(0x300))
                        h.cpu.mem_write(h.STATE, bytes(0x100))
                        h.cpu.mem_write(h.CTX, bytes(0x4000))
                        h.put(h.OBJ + 0x1f0, 'I', channels)
                        h.put(h.STATE + 0x20, 'f', fm_depth)
                        h.put(h.STATE + 0x24, 'f', mix)
                        h.put(h.STATE + 0x28, 'Q', h.CTX + 0x1000)
                        h.put(h.STATE + 0x38, 'Q', h.CTX + 0x2000)
                        h.put(h.STATE + 0x58, 'Q', h.CTX + 0x3000)
                        h.cpu.mem_write(h.CTX + 0x2000, pack(carrier))
                        h.cpu.mem_write(h.CTX + 0x3000, pack(fm_table))
                        phase, fm_phase, fm_offset = 0xffbe7700, 0x7fffffff, 0
                        increment, fm_increment = 0x01543210, 0x00987654
                        h.put(h.STATE + 0x4c, 'I', phase)
                        h.put(h.STATE + 0x50, 'I', increment)
                        h.put(h.STATE + 0x6c, 'I', fm_phase)
                        h.put(h.STATE + 0x70, 'I', fm_increment)
                        h.put(h.CTX + 0x11e0, 'f', filter_mode)
                        a1, a2, b0, b1, b2 = -.25, .125, .5, .25, .125
                        for base in [0, 0xf0]:
                            for offset, value in [(8, a1), (12, a2), (16, b0), (20, b1), (24, b2)]:
                                h.put(h.CTX + 0x1000 + base + offset, 'f', value)
                        histories = [[0., 0., 0., 0.] for _ in range(channels)]
                        for frames in [0, 1, 3, 16, 17, 32, 33, 129]:
                            samples = []
                            for channel in range(channels):
                                address = h.OUT + 0x1000 * channel
                                h.put(h.CTX + 0x300 + channel * 8, 'Q', address)
                                tape = [h.f32(rng.uniform(-1., 1.)) for _ in range(frames)]
                                h.cpu.mem_write(address, pack(tape) + b'\xcd' * 16)
                                samples.append(tape)
                            expected = [[] for _ in range(channels)]
                            for frame in range(frames):
                                carrier_gain = mul(add(lookup(carrier, (phase + fm_offset) & 0xffffffff), 1.), .5)
                                for channel in range(channels):
                                    dry = samples[channel][frame]
                                    wet = mul(carrier_gain, dry)
                                    if filter_mode:
                                        x1, x2, y1, y2 = histories[channel]
                                        output = sub(sub(add(add(mul(x1, b1), mul(wet, b0)), mul(b2, x2)), mul(y1, a1)), mul(a2, y2))
                                        histories[channel] = [wet, x1, output, y1]
                                        wet = output
                                    expected[channel].append(add(mul(sub(1., mix), dry), mul(wet, mix)))
                                phase = (phase + increment) & 0xffffffff
                                fm_offset = math.trunc(mul(mul(add(lookup(fm_table, fm_phase), 1.), fm_depth), 2**31)) & 0xffffffff
                                fm_phase = (fm_phase + fm_increment) & 0xffffffff
                            def arguments(cpu, address, size, data):
                                rsp = cpu.reg_read(h.UC_X86_REG_RSP)
                                h.put(rsp + 0x28, 'Q', h.CTX + 0x300)
                                h.put(rsp + 0x30, 'Q', h.CTX + 0x3f0)
                            hook = h.cpu.hook_add(UC_HOOK_CODE, arguments, begin=entry, end=entry)
                            h.run(entry, h.CTX, h.STATE, frames)
                            h.cpu.hook_del(hook)
                            for channel in range(channels):
                                address = h.OUT + 0x1000 * channel
                                assert bytes(h.cpu.mem_read(address, frames * 4)) == pack(expected[channel]), (table_name, channels, fm_depth, mix, filter_mode, frames, channel, 'PCM')
                                assert bytes(h.cpu.mem_read(address + frames * 4, 16)) == b'\xcd' * 16
                                if filter_mode:
                                    base = h.CTX + 0x1000 + (0xf0 if filter_mode > 0 else 0)
                                    for offset, value in zip([0x1c, 0x4c, 0x7c, 0xac], histories[channel]):
                                        assert h.get(base + offset + channel * 4, 'f') == value, 'biquad history'
                            for offset, value in [(0x4c, phase), (0x54, fm_offset), (0x6c, fm_phase)]:
                                assert h.get(h.STATE + offset, 'I') == value, (hex(offset), 'phase')
                            assert h.get(h.CTX + 0x3f0, 'B') == 0
                            rows.append(dict(table=table_name, channels=channels, fm_depth=fm_depth,
                                             mix=mix, filter_mode=filter_mode, frames=frames,
                                             exact_output_and_history=True))
    return rows


if __name__ == '__main__':
    result = dict(reference_sha256=h.META['sha256'], snapshot_sha256=h.HASH,
                  dispersor_kernel_va='0x1412ea230', dispersor_cases=dispersor(),
                  robotizer_kernel_va='0x141307d20', robotizer_cases=robotizer(),
                  helper_substitutions=[], authored_internal_parameters=True,
                  public_parameter_mapping_verified=False, full_host_parity_verified=False)
    Path(__file__).with_name('specialized-kernel-results.json').write_text(json.dumps(result, indent=2) + '\n')
    print('Original-byte checks passed:', len(result['dispersor_cases']), 'Dispersor blocks;',
          len(result['robotizer_cases']), 'Robotizer blocks; exact PCM/history, no helper substitutions.')
