"""Native vectors for the timemod family, run from Kontakt 8.13.1 with kemu.

    python3 tools/re/kemu_timemod.py choral   # Choral (0x5d) core
    python3 tools/re/kemu_timemod.py legacy   # Legacy Chorus (0x11) / Flanger (0x12)

Prints f32 output bits at fixed frames for the stimulus the Rust tests use
(`0.5 sin(0.05 n) + impulse` left, `0.5 sin(0.031 n)` right, f32, 32-frame
blocks, 48 kHz): `crates/sampler-kontakt/src/effects/timemod.rs`
`choral_renders_native_kemu_vectors` and
`legacy_chorus_and_flanger_render_native_kemu_vectors`.
"""
import math
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kemu  # noqa: E402
from kemu import Emu, f32  # noqa: E402
from unicorn.x86_const import UC_X86_REG_R13, UC_X86_REG_R14, UC_X86_REG_RBP, UC_X86_REG_RDI, UC_X86_REG_RSP  # noqa: E402

EXE = '/mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe'
kemu.MATH1D.setdefault('acos', math.acos)  # legacy LFO phase readback 0x140884620


def stimulus(n):
    return [(f32(math.sin(i * 0.05) * 0.5 + (1.0 if i == 0 else 0.0)), f32(math.sin(i * 0.031) * 0.5)) for i in range(n)]


def choral(params, invert, scatter, mode, x, fs=48000.0, block=32):
    """Core ctor 0x141939b10 (voices, mode tables via 0x14193cd70, constants,
    reset), prepare 0x14193b840, mode setter 0x14193a230 and the parameter
    setters (called through a wrapper whose +8 is the core), process
    0x14193be50. `params`: amount, rate, delay, voices, width, feedback, mix."""
    e = Emu(EXE)
    core = e.alloc(0x4000)
    e.call(0x141939b10, core)
    e.write(core + 0x4c, 'f', *params[:6], float(scatter), float(invert), params[6], mode / 4)
    e.write(core + 0x2f8, 'B', 1)
    e.call(0x14193b840, core, 0, 0, block, f={1: fs})
    w = e.alloc(16)
    e.write(w + 8, 'Q', core)
    for fn in (0x14193a230, 0x14193a2f0, 0x14193a3e0, 0x14193a4a0, 0x14193a610, 0x14193a620, 0x14193a6a0, 0x14193a6d0):
        e.call(fn, w)
    n = len(x)
    ins, outs = [e.alloc(4 * n) for _ in range(2)], [e.alloc(4 * n) for _ in range(2)]
    for c in range(2):
        e.write(ins[c], 'f', *[f[c] for f in x])
    ip, op = e.alloc(16), e.alloc(16)
    for at in range(0, n, block):
        e.write(ip, 'Q', ins[0] + 4 * at, ins[1] + 4 * at)
        e.write(op, 'Q', outs[0] + 4 * at, outs[1] + 4 * at)
        e.call(0x14193be50, core, ip, op, min(block, n - at))
    return list(zip(e.read(outs[0], 'f', n), e.read(outs[1], 'f', n)))


# size, vtable, setter case start/end, slot in the parameter container, reset (slot 5), audio
LEGACY = {
    'chorus': (0x3b0, 0x144f13d98, 0x140963981, 0x140963b44, 0x58, 0x14094b670, 0x140aa0510),
    'flanger': (0x330, 0x144f13ea0, 0x140963801, 0x14096396b, 0xb0, 0x14094baa0, 0x140aa2350),
}


def legacy(kind, depth, speed, phase, x, feedback=0.0, color=0.0, fs=48000.0, block=32):
    """Object as the factories 0x1407c7010 / 0x1407c75a0 build it, prepare
    0x140960f00, the setter case run from its first instruction after the
    virtual call to the end, reset, audio. Send helpers 0x140ac28f0 /
    0x140ac21f0 and the tempo getter 0x1408a8390 are stubbed (unsynced speed
    never reads tempo). The LFO objects' rate field (+0xc) is set to `fs`:
    its writer is not located. Returns the wet output."""
    size, vtable, start, end, slot, reset, audio = LEGACY[kind]
    e = Emu(EXE)
    for fn in (0x140ac28f0, 0x140ac21f0, 0x1408a8390):
        e.uc.mem_write(fn, b'\xc3')
    obj = e.alloc(size)
    e.call(0x140a856c0, obj)
    e.write(obj, 'Q', 0x144f0f780)
    e.call(0x1407f3150, obj + 0x1c0)
    e.write(obj, 'Q', vtable)
    e.write(obj + 0x14, 'f', fs)
    e.call(0x140960f00, obj, 2)
    for k in range(4):
        e.write(obj + 0x28c + 0x14 * k, 'f', fs)
    rdi = e.alloc(0x1000)  # parameter container; slot table at +0x30 (0x140570e20)
    p = rdi + 0x30 + slot
    sync = 0x18 if kind == 'chorus' else 0x20
    e.write(p + 8, 'f', color)  # Chorus +8 stays 0 (never read from the file)
    e.write(p + 0xc, 'f', depth, speed, phase)
    if kind == 'flanger':
        e.write(p + 0x18, 'f', feedback)
    e.write(p + sync, 'Q', 0x144e74208)
    e.write(p + sync + 0x18, 'f', -1.0)  # unit: not synced
    e.write(p + sync + 0x20, 'B', 1)
    frame = e.alloc(0x400)
    for reg, v in ((UC_X86_REG_RDI, rdi), (UC_X86_REG_R14, obj), (UC_X86_REG_RBP, frame + 0x200), (UC_X86_REG_R13, 0),
                   (UC_X86_REG_RSP, kemu.STACK + 0x3F0000)):
        e.uc.reg_write(reg, v)
    e.uc.emu_start(start, end)
    e.call(reset, obj)
    n = len(x)
    ins = [e.alloc(4 * n) for _ in range(2)]
    for c in range(2):
        e.write(ins[c], 'f', *[f[c] for f in x])
    buf = e.alloc(0x9c010 + 0x2000)
    out = [[], []]
    for at in range(0, n, block):
        m = min(block, n - at)
        e.write(obj + 0x120, 'i', m)
        e.write(obj + 0x20, 'Q', ins[0] + 4 * at, ins[1] + 4 * at)
        e.call(audio, obj, buf, 0)
        for c in range(2):
            out[c].extend(e.read(buf + 0x9c010 + 0x1000 * c, 'f', m))
    return list(zip(*out))


def bits(y, frames):
    b = lambda v: '0x%08x' % struct.unpack('<I', struct.pack('<f', v))[0]
    return [(i, b(y[i][0]), b(y[i][1])) for i in frames]


if __name__ == '__main__':
    what = sys.argv[1] if len(sys.argv) > 1 else 'choral'
    frames = (0, 1, 150, 777, 1500)
    if what == 'choral':
        x = stimulus(3000)
        for case in (([0.6, 0.5, 0.18, 0.7, 0.4, 0.5, 0.8], True, True, 1), ([0.3, 0.8, 0.5, 1.0, 1.0, 0.9, 1.0], False, True, 2)):
            print(case, bits(choral(*case, x), frames + (2999,)))
    else:
        x = stimulus(2000)
        print('chorus', bits(legacy('chorus', 0.6, 1.5, 0.3, x), frames + (1999,)))
        print('flanger', bits(legacy('flanger', 0.7, 0.8, 0.4, x, feedback=0.6, color=2.5), frames + (1999,)))
