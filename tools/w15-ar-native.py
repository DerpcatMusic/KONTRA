#!/usr/bin/env python3
"""Bounded AR checks on original x64 bytes; never starts the native host.

Approved specification: engine-analysis-2026-10-07, FilterDJ entry points.
Only synthetic signals enter the oracle. The exponential table is initialized
from its independently verified law; no DSP instruction is replaced.
"""
import argparse
import hashlib
import json
import math
import struct
from pathlib import Path

import pefile
from unicorn import Uc, UC_ARCH_X86, UC_MODE_64, UC_HOOK_CODE
from unicorn.x86_const import (
    UC_X86_REG_RCX, UC_X86_REG_RDX, UC_X86_REG_R8, UC_X86_REG_R9,
    UC_X86_REG_RAX, UC_X86_REG_RIP, UC_X86_REG_RSP, UC_X86_REG_MXCSR,
    UC_X86_REG_XMM1, UC_X86_REG_XMM2,
)


def q(x):
    return struct.unpack('<f', struct.pack('<f', x))[0]


TABLE = [q(2 ** (i / 60 - 20)) for i in range(2401)]


def lookup(x):
    # Original CVTTSS2SI followed by signed AX range checks / MOVSX.
    i = ((int(x) + 32768) & 65535) - 32768
    if i < 0:
        return 0.0
    if i >= 2400:
        return TABLE[2400]
    return q(TABLE[i] + q(q(x - i) * q(TABLE[i + 1] - TABLE[i])))


class Oracle:
    OBJ, CORE, IO, STACK, STOP, CTX = 0x2000000, 0x2000210, 0x3000000, 0x4000000, 0x5000000, 0x6000000

    def __init__(self, metadata):
        self.meta = json.loads(metadata.read_text())
        binary = Path(self.meta['path'])
        assert hashlib.sha256(binary.read_bytes()).hexdigest() == self.meta['sha256']
        pe = pefile.PE(str(binary), fast_load=True)
        base = pe.OPTIONAL_HEADER.ImageBase
        self.cpu = Uc(UC_ARCH_X86, UC_MODE_64)
        self.cpu.mem_map(base, (pe.OPTIONAL_HEADER.SizeOfImage + 4095) & ~4095)
        self.cpu.mem_write(base, pe.get_memory_mapped_image())
        for address in [self.OBJ, self.IO, self.STACK, self.STOP]:
            self.cpu.mem_map(address, 0x10000)
        self.cpu.mem_map(self.CTX, 0xa0000)
        pairs = [(v, q(TABLE[i + 1] - v) if i < 2400 else 0.) for i, v in enumerate(TABLE)]
        self.cpu.mem_write(0x14a6b2600, b''.join(struct.pack('<ff', *p) for p in pairs))
        self.cpu.hook_add(UC_HOOK_CODE, self.helper)
        self.seen = set()
        self.events = []

    def helper(self, cpu, address, size, _):
        self.seen.add(address)
        if address == 0x140b05840:
            raw = cpu.reg_read(UC_X86_REG_XMM2) & 0xffffffff
            self.events.append(('set', cpu.reg_read(UC_X86_REG_RDX), struct.unpack('<f', struct.pack('<I', raw))[0]))
        elif address == 0x140b03300:
            self.events.append(('clock',))
        elif address == 0x140afedc0:
            pointer = self.get(0, 'Q', cpu.reg_read(UC_X86_REG_RDX))
            self.events.append(('process', cpu.reg_read(UC_X86_REG_R9), (pointer - self.IO - 0x1000) // 4))
        if address == 0x1443d1000:  # CRT memset only
            cpu.mem_write(cpu.reg_read(UC_X86_REG_RCX),
                          bytes([cpu.reg_read(UC_X86_REG_RDX) & 255]) * cpu.reg_read(UC_X86_REG_R8))
            cpu.reg_write(UC_X86_REG_RAX, cpu.reg_read(UC_X86_REG_RCX))
        elif address == 0x1443c9054:  # CRT array traversal; constructors run separately below
            pass
        elif address in [0x140ac28f0, 0x140ac21f0]:  # outer rack mix only
            pass
        else:
            return
        rsp = cpu.reg_read(UC_X86_REG_RSP)
        cpu.reg_write(UC_X86_REG_RIP, struct.unpack('<Q', cpu.mem_read(rsp, 8))[0])
        cpu.reg_write(UC_X86_REG_RSP, rsp + 8)

    def put(self, offset, value, fmt='f', base=None):
        self.cpu.mem_write((self.CORE if base is None else base) + offset, struct.pack('<' + fmt, value))

    def get(self, offset, fmt='f', base=None):
        return struct.unpack('<' + fmt, self.cpu.mem_read(
            (self.CORE if base is None else base) + offset, struct.calcsize(fmt)))[0]

    def run(self, entry, obj=None, arg2=0, arg3=0, arg4=0, f2=None, f3=None):
        rsp = self.STACK + 0xff00 - 8
        self.cpu.mem_write(rsp, struct.pack('<Q', self.STOP))
        for register, value in [(UC_X86_REG_RSP, rsp), (UC_X86_REG_RCX, self.CORE if obj is None else obj),
                                (UC_X86_REG_RDX, arg2), (UC_X86_REG_R8, arg3), (UC_X86_REG_R9, arg4),
                                (UC_X86_REG_MXCSR, 0x1f80)]:
            self.cpu.reg_write(register, value & 0xffffffffffffffff)
        for register, value in [(UC_X86_REG_XMM1, f2), (UC_X86_REG_XMM2, f3)]:
            if value is not None:
                self.cpu.reg_write(register, struct.unpack('<I', struct.pack('<f', value))[0])
        self.cpu.emu_start(entry, self.STOP, timeout=2_000_000, count=2_000_000)
        assert self.cpu.reg_read(UC_X86_REG_RIP) == self.STOP, hex(entry)

    def initialize(self, saved, rate, cutoff, resonance):
        self.cpu.mem_write(self.OBJ, bytes(0x10000))
        self.run(0x140cf4c80, obj=saved, arg2=self.IO)
        internal = self.get(0, 'I', self.IO)
        assert internal == saved - 24
        self.run(0x140af43f0)
        for ramp in [0x2bc, 0x2e4, 0x30c]:
            self.run(0x140af4bc0, obj=self.CORE + ramp)
        self.run(0x140af0080, obj=self.OBJ, arg2=internal)
        self.run(0x140afb6b0)
        self.run(0x140afcfd0, arg3=32, f2=rate)
        self.run(0x140b05840, arg2=0, f3=cutoff)
        self.run(0x140b05840, arg2=1, f3=resonance)
        self.run(0x140afc960)
        self.run(0x140b03300)
        assert self.get(0x30, 'I') == saved - 100
        assert [self.get(i) for i in [0x60, 0x64, 0x68]] == [float(i == (saved - 100) % 3) for i in range(3)]

    def process(self, samples, ramp=False):
        channels = len(samples)
        frames = len(samples[0])
        assert 0 < channels <= 2 and frames <= 1024
        self.put(0xc, channels, 'I')
        self.put(0x6c, int(ramp), 'B')
        for flag in [0x2cd, 0x2f5, 0x31d]:
            self.put(flag, int(ramp), 'B')
        for ch, data in enumerate(samples):
            self.put(8 * ch, self.IO + 0x1000 + ch * 0x2000, 'Q', self.IO)
            self.put(0x100 + 8 * ch, self.IO + 0x2000 + ch * 0x2000, 'Q', self.IO)
            self.cpu.mem_write(self.IO + 0x1000 + ch * 0x2000, struct.pack('<' + 'f' * frames, *data))
        self.run(0x140afedc0, arg2=self.IO, arg3=self.IO + 0x100, arg4=frames)
        return [list(struct.unpack('<' + 'f' * frames, self.cpu.mem_read(
            self.IO + 0x2000 + ch * 0x2000, frames * 4))) for ch in range(channels)]

    def wrapper_clock(self, initial, mode, enabled, varying=False):
        self.put(0x1bc, initial, 'i', self.OBJ)
        self.put(0x124, 2, 'I', self.OBJ)
        knobs = [.4, .7, mode / 8.]
        values = [[q(value + (i - 1) * .25) if varying else q(value) for i in range(64)] for value in knobs]
        for lane, value in enumerate(knobs):
            self.put(0x550 + lane, (enabled >> lane) & 1, 'B', self.OBJ)
            self.put(0x558 + lane * 8, self.IO + 0x8000 + lane * 0x1000, 'Q', self.OBJ)
            self.cpu.mem_write(self.IO + 0x8000 + lane * 0x1000, struct.pack('<64f', *values[lane]))
        for ch in range(2):
            self.put(0x20 + 8 * ch, self.IO + 0x1000 + ch * 0x2000, 'Q', self.OBJ)
            self.cpu.mem_write(self.IO + 0x1000 + ch * 0x2000,
                               struct.pack('<128f', *(q(.1 * math.sin(i * .13 + ch)) for i in range(128))))
        remaining = initial
        for frames in [0, 1, 31, 1, 7, 24, 63]:
            self.put(0x120, frames, 'I', self.OBJ)
            self.events.clear()
            self.run(0x1408f9d90, obj=self.OBJ, arg2=self.CTX)
            expected = []
            offset = 0
            control_index = 0
            while offset < frames:
                if remaining < 1:
                    remaining = 32
                    expected.extend(('set', i, min(1., max(0., values[i][control_index])))
                                    for i in range(3) if enabled & (1 << i))
                    control_index += 1
                    expected.append(('clock',))
                count = min(remaining, frames - offset)
                expected.append(('process', count, offset))
                offset += count
                remaining -= count
            assert self.events == expected, (mode, initial, frames, self.events, expected)
            assert self.get(0x1bc, 'i', self.OBJ) == remaining


class Model:
    """Independent equations: three nonlinear two-pole sections, shared detector."""
    def __init__(self, native):
        self.mode = native.get(0x30, 'I')
        self.rate_inverse = native.get(0x2b4)
        self.g = native.get(0x2bc)
        self.hz_lane = native.get(0x2e4)
        self.res = native.get(0x30c)
        self.floor = native.get(0x34)
        self.res_offset = native.get(0x38)
        self.res_scale = native.get(0x3c)
        self.weights = [native.get(x) for x in [0x60, 0x64, 0x68]]
        self.state = [[0.] * 9 for _ in range(2)]
        self.detector = self.feedback = self.adaptation = self.cap = 0.

    @staticmethod
    def saturate(x):
        x = min(1000., max(-1000., x))
        return q(x - q(q(abs(x) * x) * q(.0005)))

    def section(self, ch, section, x, p, d, inverse):
        s = self.state[ch]
        h0, b0, l0 = s[section * 2], s[section * 2 + 1], s[6 + section]
        h = q(q(x - q(q(q(b0 * d) + l0) + q(h0 * p))) * inverse)
        b = q(q(q(h0 + h) * self.g) + b0)
        low = q(q(q(b + b0) * self.g) + l0)
        s[section * 2] = h
        s[section * 2 + 1] = self.saturate(b)
        s[6 + section] = self.saturate(low)
        return h, b, low

    def mix(self, h, b, low):
        w = self.weights
        return q(q(q(low * w[0]) + q(q(b + b) * w[1])) + q(h * w[2]))

    def process(self, samples, ramp=False, delta=(0., 0., 0.)):
        out = [[] for _ in samples]
        family = self.mode // 3
        gain_a = q(.9965783953666687 if family == 0 else 1.1958940029144287)
        gain_b = q([.14948676526546478, .24914462864398956, .29897353053092957][family])
        for frame in range(len(samples[0])):
            if ramp:
                self.g = q(self.g + delta[0])
                self.hz_lane = q(self.hz_lane + delta[1])
                self.res = q(self.res + delta[2])
            r = q(self.res * self.res_scale)
            damping = q(1. - self.feedback)
            damping = q(q(damping + damping) + self.g)
            p = q(self.g * damping)
            d = q(self.g + damping)
            inverse = q(1. / q(p + 1.))
            gain_pos = q(q(q(q(q(gain_a - q(r * gain_b)) * self.weights[1]) - gain_a) * r) + 1200.)
            gain = lookup(gain_pos)
            detector = 0.
            for ch, data in enumerate(samples):
                h, band, low = self.section(ch, 0, data[frame], p, d, inverse)
                if family == 0:
                    value = self.mix(h, band, low)
                    detected = q(band + band)
                elif family == 2:
                    _, b2, _ = self.section(ch, 1, q(band + band), p, d, inverse)
                    detected = q(b2 + b2)
                    cross = q(q(self.res * .5) * detected)
                    value = q(q(q(q(low + cross) * self.weights[0]) + q(detected * self.weights[1]))
                              + q(q(h + cross) * self.weights[2]))
                else:
                    h2, b2, l2 = self.section(ch, 1, self.mix(h, band, low), p, d, inverse)
                    if self.mode == 4:
                        detected = q(b2 + b2)
                        value = self.mix(h2, b2, l2)
                    else:
                        _, b3, _ = self.section(ch, 2, q(band + band), p, d, inverse)
                        detected = q(b3 + b3)
                        value = self.mix(h2, b3, l2)
                # Native adds the two band terms separately in non-combined paths.
                if family == 2:
                    detector = q(detector + detected)
                else:
                    detector = q(q(detector + q(detected * .5)) + q(detected * .5))
                out[ch].append(q(value * gain))
            level = abs(q(detector * q(1. / len(samples))))
            if level < self.detector:
                level = q(q(q(q(level - self.detector) * self.rate_inverse) * q(21.903446197509766)) + self.detector)
            self.detector = level
            if ramp:
                # This lane conversion is preserved as observed, including its cap.
                f = lookup(q(q(self.hz_lane * 5.) + q(1381.881591796875)))
                a, b = min(70., max(24., f)), min(130., max(85., f))
                self.cap = q(q(q(.8600000143051147) - q(q(b - 130.) * q(.003111110767349601)))
                             - q(q(q(a - 70.) * q(-.005982609000056982))
                                 - q(q(q(b - 130.) * q(a - 70.)) * q(-2.164251054637134e-05))))
            amount = lookup(q(q(q(r - self.res_offset) * q(9.965784072875977)) + 1200.))
            denominator = q(q(q(amount - 1.) * q(self.floor / min(1.e8, max(self.floor, level)))) + 1.)
            target = max(0., q(1. - q(1. / denominator)))
            # Native combines rate and adaptation coefficient before the state delta.
            adaptation_rate = q(self.rate_inverse * q(2092.300048828125))
            self.adaptation = q(q(q(target - self.adaptation) * adaptation_rate) + self.adaptation)
            self.feedback = q(self.adaptation * self.cap)
        return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--metadata', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, required=True)
    args = parser.parse_args()
    oracle = Oracle(args.metadata)
    rows = []
    vectors = []
    retune_vectors = []
    scenarios = []
    for internal in [-1, 0, 75, 85, 2**31 - 1]:
        oracle.cpu.mem_write(oracle.OBJ, bytes(0x10000))
        oracle.run(0x140af0080, obj=oracle.OBJ, arg2=internal)
        assert oracle.get(0x30, 'I') == 0
    for subtype in [-1., 0., .0625, .1875, .5, 1., 2.]:
        oracle.run(0x140b05840, arg2=2, f3=subtype)
        assert oracle.get(0x30, 'I') == min(8, max(0, round(q(subtype * 8.))))
    for saved in range(100, 109):
        for rate, cutoff, resonance, amplitude in [(48000., .5135, .7, .1), (44100., .1, 0., 1.),
                                                  (96000., .8, 1., 4.), (48000., 0., 0., 0.),
                                                  (48000., 1., 1., 4.), (8000., .1, .7, .1),
                                                  (192000., 1., .3, .1), (8000., 1., 1., 4.),
                                                  (44100., 1., 0., .1)]:
            oracle.initialize(saved, rate, cutoff, resonance)
            expected_hz = lookup(q(q(min(q(q(cutoff) * 145.), 140.) * 5.) + q(1381.881591796875)))
            assert oracle.get(0x2f0) == expected_hz
            assert oracle.get(0x318) == q(resonance)
            assert oracle.get(0x2d4, 'I') == max(1, int(q(q(q(rate) * q(.001)) / 32.) + .5))
            inverse = q(1. / rate)
            f = max(0., expected_hz)
            fifth = q(q(q(q(q(q(f * f) * f) * f) * inverse) * inverse) * inverse)
            fifth = q(fifth * q(40.80262756347656))
            third = q(q(q(f * f) * inverse) * q(10.335426330566406))
            expected_g = q(q(q(q(q(fifth + third) * inverse) + q(math.pi)) * f) * inverse)
            assert oracle.get(0x2c8) == expected_g
            model = Model(oracle)
            checkpoints = []
            for ramp in [True, False]:
                signal = [[q(amplitude * math.sin((i + 1) * .17 + ch * .4)) for i in range(64)] for ch in range(2)]
                actual = oracle.process(signal, ramp)
                expected = model.process(signal, ramp)
                error = max(abs(a - b) for xs, ys in zip(actual, expected) for a, b in zip(xs, ys))
                assert error <= 2e-6, (saved, rate, ramp, error)
                assert all(math.isfinite(v) for xs in actual for v in xs)
                checkpoints.append([[actual[ch][i] for ch in range(2)] for i in [0, 1, 2, 3, 7, 15, 31, 63]])
                if rate == 48000. and cutoff == .5135 and ramp:
                    vectors.append([[actual[ch][i] for ch in range(2)] for i in [0, 1, 2, 3, 7, 15, 31, 63]])
                rows.append(dict(saved=saved, internal=saved - 24, mode=saved - 100, rate=rate,
                                 cutoff=cutoff, resonance=resonance, amplitude=amplitude,
                                 ramp=ramp, peak_error=error, peak=max(abs(v) for xs in actual for v in xs)))
            oracle.run(0x140b05840, arg2=0, f3=.35)
            oracle.run(0x140b05840, arg2=1, f3=.3)
            oracle.run(0x140b03300)
            delta = tuple(oracle.get(o) for o in [0x2c0, 0x2e8, 0x310])
            for current, offset, step in zip([model.g, model.hz_lane, model.res], [0x2bc, 0x2e4, 0x30c], delta):
                expected_step = q(q(oracle.get(offset + 12) - current) * oracle.get(offset + 32))
                if q(expected_step * expected_step) < 1e-15:
                    expected_step = 0.
                assert step == expected_step
            signal = [[q(amplitude * math.sin((i + 1) * .13 + ch * .7)) for i in range(32)] for ch in range(2)]
            actual = oracle.process(signal, True)
            expected = model.process(signal, True, delta)
            error = max(abs(a - b) for xs, ys in zip(actual, expected) for a, b in zip(xs, ys))
            assert error <= 2e-6, (saved, rate, 'control-ramp', error)
            checkpoints.append([[actual[ch][i] for ch in range(2)] for i in [0, 1, 2, 3, 7, 15, 31]])
            scenarios.append(dict(mode=saved - 100, rate=rate, cutoff=cutoff, resonance=resonance,
                                  amplitude=amplitude, checkpoints=checkpoints))
            if rate == 48000. and cutoff == .5135:
                retune_vectors.append([[actual[ch][i] for ch in range(2)] for i in [0, 1, 2, 3, 7, 15, 31]])
            rows.append(dict(saved=saved, internal=saved - 24, mode=saved - 100, rate=rate,
                             ramp='control-change', peak_error=error,
                             peak=max(abs(v) for xs in actual for v in xs)))
        for initial in [0, 5, 32]:
            for enabled in range(8):
                oracle.initialize(saved, 48000., .4, .7)
                oracle.wrapper_clock(initial, saved - 100, enabled, varying=True)
    receipt = dict(binary_sha256=oracle.meta['sha256'], cases=rows, scenarios=scenarios,
                   synthetic_checkpoint_frames=[0, 1, 2, 3, 7, 15, 31, 63],
                   synthetic_checkpoints=vectors,
                   retune_checkpoint_frames=[0, 1, 2, 3, 7, 15, 31],
                   retune_checkpoints=retune_vectors,
                   limitations=['Exponential table initialized from verified law; CRT traversal/memset replaced.',
                                'Outer rack mix helpers replaced for wrapper clock checks; core DSP executes.',
                                'Synthetic inputs only; no library or whole-voice scheduling admission.'])
    args.receipt.write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(dict(cases=len(rows), max_error=max(r['peak_error'] for r in rows))))


if __name__ == '__main__':
    main()
