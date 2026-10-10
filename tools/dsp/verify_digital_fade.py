"""Bounded saved Digital Multi fade conversion, reset and fade probes."""
import argparse, ctypes, hashlib, json, pathlib, struct
import pefile
from unicorn import Uc, UC_ARCH_X86, UC_MODE_64, UC_HOOK_CODE
from unicorn.x86_const import *

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('metadata', type=pathlib.Path)
parser.add_argument('output', type=pathlib.Path)
args = parser.parse_args()
meta = json.loads(args.metadata.read_text())
raw = pathlib.Path(meta['path']).read_bytes()
assert hashlib.sha256(raw).hexdigest() == meta['sha256'] == '0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8'
pe = pefile.PE(data=raw, fast_load=True)
base = pe.OPTIONAL_HEADER.ImageBase
cpu = Uc(UC_ARCH_X86, UC_MODE_64)
cpu.mem_map(base, (pe.OPTIONAL_HEADER.SizeOfImage + 4095) & ~4095)
cpu.mem_write(base, pe.get_memory_mapped_image())
obj, out, stack, stop, parameter, sync = [0x2000000 + i * 0x100000 for i in range(6)]
for a in [obj, out, stack, stop, parameter, sync]: cpu.mem_map(a, 0x10000)
libm = ctypes.CDLL('libm.so.6')
libm.pow.argtypes = [ctypes.c_double, ctypes.c_double]; libm.pow.restype = ctypes.c_double

def f32(x): return struct.unpack('<f', struct.pack('<f', x))[0]
def bits(x): return struct.pack('<f', x).hex()
def put(a, fmt, x): cpu.mem_write(a, struct.pack('<' + fmt, x))
def xmm(reg, fmt):
    n = 4 if fmt == 'f' else 8
    return struct.unpack('<' + fmt, cpu.reg_read(reg).to_bytes(16, 'little')[:n])[0]
def ret(value, fmt):
    cpu.reg_write(UC_X86_REG_XMM0, int.from_bytes(struct.pack('<' + fmt, value), 'little'))
    rsp = cpu.reg_read(UC_X86_REG_RSP)
    cpu.reg_write(UC_X86_REG_RIP, struct.unpack('<Q', cpu.mem_read(rsp, 8))[0])
    cpu.reg_write(UC_X86_REG_RSP, rsp + 8)
def math_import(cpu, address, size, data):
    if address == 0x1443d10ae: ret(libm.pow(xmm(UC_X86_REG_XMM0, 'd'), xmm(UC_X86_REG_XMM1, 'd')), 'd')
cpu.hook_add(UC_HOOK_CODE, math_import)
def call(entry, registers, until=stop):
    rsp = stack + 0xff00 - 8; put(rsp, 'Q', stop)
    cpu.reg_write(UC_X86_REG_RSP, rsp); cpu.reg_write(UC_X86_REG_MXCSR, 0x1f80)
    for reg, value in registers.items(): cpu.reg_write(reg, value)
    cpu.emu_start(entry, until, timeout=1000000, count=500000)
    assert cpu.reg_read(UC_X86_REG_RIP) == until

cases = []
for rate in [44100., 48000., 96000.]:
 for saved in [0., .01, .1, struct.unpack('<f', bytes.fromhex('3cca243f'))[0], 1., 10., 100., 5000., 10000.]:
    cpu.mem_write(obj, bytes(0x200)); cpu.mem_write(parameter, bytes(0x100)); cpu.mem_write(sync, bytes(0x100))
    put(sync + 0x30, 'f', -1.)
    call(0x14072b380, {UC_X86_REG_RCX: sync,
        UC_X86_REG_XMM1: int.from_bytes(struct.pack('<f', saved), 'little')})
    duration = xmm(UC_X86_REG_XMM0, 'f')
    assert bits(duration) == bits(saved)
    put(obj + 0x14, 'f', rate / 32.)
    call(0x140977d85, {UC_X86_REG_RBX: obj,
        UC_X86_REG_XMM0: int.from_bytes(struct.pack('<f', duration), 'little'),
        UC_X86_REG_XMM8: int.from_bytes(struct.pack('<f', .001), 'little'),
        UC_X86_REG_XMM9: int.from_bytes(struct.pack('<d', 1.), 'little')}, until=0x140977dc5)
    count = struct.unpack('<I', cpu.mem_read(obj + 0xf8, 4))[0]
    factor = struct.unpack('<f', cpu.mem_read(obj + 0xf4, 4))[0]
    assert count == int(f32(f32(duration * f32(rate / 32.)) * f32(.001)))
    if count: assert bits(factor) == bits(libm.pow(1. + 1. / f32(.3), 1. / count))
    put(obj + 0xe8, 'd', .125)
    call(0x1405f45a0, {UC_X86_REG_RCX: obj})
    assert cpu.mem_read(obj + 0xfc, 4) == struct.pack('<I', count)
    assert cpu.mem_read(obj + 0xf0, 4) == struct.pack('<f', .3)
    assert cpu.mem_read(obj + 0xb0, 8) == struct.pack('<d', .125)
    level = f32(.3); left = count; checkpoints = []
    for start in range(0, count + 3, 127):
        n = min(127, count + 3 - start)
        cpu.mem_write(out, struct.pack('<' + 'f' * n, *([-.3] * n)))
        call(0x140b06c90, {UC_X86_REG_RCX: obj, UC_X86_REG_RDX: out, UC_X86_REG_R8: n})
        got = struct.unpack('<' + 'f' * n, cpu.mem_read(out, n * 4))
        for i, value in enumerate(got):
            expected = f32(f32(level - f32(.3)) * f32(-.3)) if left else f32(-.3)
            assert bits(value) == bits(expected)
            if start + i in {0, 1, count // 2, max(0, count - 1), count, count + 1}:
                checkpoints.append({'tick': start + i, 'signal_bits': bits(value)})
            if left:
                level = min(1., max(0., f32(level * factor))); left -= 1
        assert cpu.mem_read(obj + 0xfc, 4) == struct.pack('<I', left)
        assert cpu.mem_read(obj + 0xf0, 4) == struct.pack('<f', level)
    cases.append({'rate': rate, 'saved': saved, 'duration_bits': bits(duration),
                  'ticks': count, 'factor_bits': bits(factor), 'checkpoints': checkpoints})
result = {'binary_sha256': meta['sha256'], 'entries': ['0x14072b380', '0x140a74bf0', '0x140977d85', '0x1405f45a0', '0x140b06c90'],
          'helper_substitutions': ['CRT pow → system libm.pow'],
          'scope': 'saved unsynchronized fade, false alternate mode; original getter/count/reset/fade instructions; native CRT rounding, host scheduler and live setters not certified', 'cases': cases}
args.output.write_text(json.dumps(result, indent=2) + '\n')
print('PASS', len(cases), 'original saved fade configurations')
