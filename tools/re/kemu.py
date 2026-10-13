"""Emulate isolated, self-contained DSP functions of a Windows x64 PE (Kontakt 8,
UVI Workstation) with Unicorn, to read native vectors without running the
program. Only CRT heap/string/math imports are serviced; any other import
stops the emulation with its name, so a function that touches the OS, files,
licensing or library data cannot run here.

    from kemu import Emu
    e = Emu('/mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe')
    obj = e.alloc(0x7140)
    e.call(0x141ad9d00, obj, 0)             # integer / pointer args
    e.call(0x141aea370, obj, 3, 0, f={2: 0.5})  # float arg 2 in xmm2
"""
import math
import struct

import pefile
from unicorn import UC_ARCH_X86, UC_MODE_64, UC_HOOK_CODE, UC_HOOK_MEM_INVALID, Uc, UcError
from unicorn.x86_const import (
    UC_X86_REG_GS_BASE, UC_X86_REG_R8, UC_X86_REG_R9, UC_X86_REG_RAX, UC_X86_REG_RCX,
    UC_X86_REG_RDX, UC_X86_REG_RIP, UC_X86_REG_RSP, UC_X86_REG_XMM0, UC_X86_REG_XMM1,
    UC_X86_REG_XMM2, UC_X86_REG_XMM3,
)

XMM = [UC_X86_REG_XMM0, UC_X86_REG_XMM1, UC_X86_REG_XMM2, UC_X86_REG_XMM3]
INT = [UC_X86_REG_RCX, UC_X86_REG_RDX, UC_X86_REG_R8, UC_X86_REG_R9]
STUBS, STACK, HEAP, TEB, RET = 0x7F0000000000, 0x7E0000000000, 0x100000000, 0x7D0000000000, 0x7F00000FF000


def f32(x):
    try:
        return struct.unpack('<f', struct.pack('<f', x))[0]
    except OverflowError:
        return math.copysign(math.inf, x)


def ieee(fn):
    """`fn` with C's results at domain edges (log(0) = -inf, NaN for the rest)."""
    def g(*a):
        try:
            return fn(*a)
        except ValueError:
            return -math.inf if a[0] == 0 else math.nan
        except OverflowError:
            return math.inf
    return g


class Stop(Exception):
    pass


class Emu:
    def __init__(self, path, heap_mb=512):
        pe = pefile.PE(path, fast_load=True)
        pe.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY['IMAGE_DIRECTORY_ENTRY_IMPORT']])
        self.base = base = pe.OPTIONAL_HEADER.ImageBase
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_64)
        uc.mem_map(base, (pe.OPTIONAL_HEADER.SizeOfImage + 0xFFF) & ~0xFFF)
        uc.mem_write(base, pe.header)
        for s in pe.sections:
            uc.mem_write(base + s.VirtualAddress, s.get_data())
        self.names = {}
        uc.mem_map(STUBS, 0x100000)
        for i, imp in enumerate(x for d in pe.DIRECTORY_ENTRY_IMPORT for x in d.imports):
            stub = STUBS + 16 * i
            uc.mem_write(stub, b'\xc3')
            uc.mem_write(imp.address, struct.pack('<Q', stub))
            self.names[stub] = (imp.name or b'#%d' % imp.ordinal).decode()
        uc.mem_map(STACK, 0x400000)
        uc.mem_map(TEB, 0x10000)
        uc.reg_write(UC_X86_REG_GS_BASE, TEB)
        uc.mem_write(TEB + 0x30, struct.pack('<Q', TEB))
        uc.mem_map(HEAP, heap_mb << 20)
        self.brk = HEAP
        self.sizes = {}
        uc.hook_add(UC_HOOK_CODE, self._stub, begin=STUBS, end=STUBS + 0xFFFFF)

    # --- memory -------------------------------------------------------------
    def alloc(self, n, align=64):
        p = (self.brk + align - 1) & ~(align - 1)
        self.brk = p + max(n, 1)
        self.uc.mem_write(p, b'\0' * max(n, 1))
        self.sizes[p] = n
        return p

    def read(self, addr, fmt, n=1):
        w = struct.calcsize(fmt)
        v = struct.unpack('<%d%s' % (n, fmt), self.uc.mem_read(addr, w * n))
        return v if n > 1 else v[0]

    def write(self, addr, fmt, *vals):
        self.uc.mem_write(addr, struct.pack('<%d%s' % (len(vals), fmt), *vals))

    # --- imports ------------------------------------------------------------
    def _ret(self, v=0):
        self.uc.reg_write(UC_X86_REG_RAX, v & 0xFFFFFFFFFFFFFFFF)

    def _retf(self, v, single=True):
        b = struct.pack('<f', v) + b'\0' * 12 if single else struct.pack('<d', v) + b'\0' * 8
        self.uc.reg_write(UC_X86_REG_XMM0, int.from_bytes(b, 'little'))

    def _argf(self, i, single=True):
        raw = self.uc.reg_read(XMM[i]).to_bytes(16, 'little')
        return struct.unpack('<f' if single else '<d', raw[:4] if single else raw[:8])[0]

    def _stub(self, uc, addr, size, _):
        name = self.names.get(addr)
        if name is None:
            return
        a = [uc.reg_read(r) for r in INT]
        heap = {'malloc': 0, '_aligned_malloc': 0, 'calloc': 0, '??2@YAPEAX_K@Z': 0, '??_U@YAPEAX_K@Z': 0}
        if name in heap:
            n = a[0] * a[1] if name == 'calloc' else a[0]
            self._ret(self.alloc(n, a[1] if name == '_aligned_malloc' else 64))
        elif name == 'realloc':
            p = self.alloc(a[1])
            if a[0]:
                uc.mem_write(p, bytes(uc.mem_read(a[0], min(self.sizes.get(a[0], 0), a[1]))))
            self._ret(p)
        elif name in ('free', '_aligned_free', '??3@YAXPEAX@Z', '??3@YAXPEAX_K@Z', '??_V@YAXPEAX@Z'):
            self._ret(0)
        elif name == 'memset':
            uc.mem_write(a[0], bytes([a[1] & 0xFF]) * a[2])
            self._ret(a[0])
        elif name in ('memcpy', 'memmove'):
            uc.mem_write(a[0], bytes(uc.mem_read(a[1], a[2])))
            self._ret(a[0])
        elif name in MATH1F:
            self._retf(f32(ieee(MATH1F[name])(self._argf(0))))
        elif name in MATH1D:
            self._retf(ieee(MATH1D[name])(self._argf(0, False)), False)
        elif name == 'powf':
            self._retf(f32(ieee(math.pow)(self._argf(0), self._argf(1))))
        elif name == 'pow':
            self._retf(ieee(math.pow)(self._argf(0, False), self._argf(1, False)), False)
        elif name == 'atan2f':
            self._retf(f32(math.atan2(self._argf(0), self._argf(1))))
        elif name == 'fmodf':
            self._retf(f32(math.fmod(self._argf(0), self._argf(1))))
        elif name in ('_purecall', '__std_terminate', '_invalid_parameter_noinfo_noreturn'):
            raise Stop(name)
        else:
            raise Stop('import ' + name)

    # --- calls --------------------------------------------------------------
    def call(self, fn, *args, f=None, d=None, limit=0):
        """Call `fn(args)`; `f`/`d` map an argument index (0..3) to the f32/f64
        value placed in that xmm register."""
        uc = self.uc
        sp = STACK + 0x3F0000
        stack_args = list(args[4:])
        sp -= 8 * len(stack_args) + 0x20
        sp &= ~0xF
        for k, v in enumerate(stack_args):
            uc.mem_write(sp + 0x20 + 8 * k, struct.pack('<Q', v & 0xFFFFFFFFFFFFFFFF))
        sp -= 8
        uc.mem_write(RET, b'\xc3')
        uc.mem_write(sp, struct.pack('<Q', RET))
        uc.reg_write(UC_X86_REG_RSP, sp)
        for r, v in zip(INT, args[:4]):
            uc.reg_write(r, v & 0xFFFFFFFFFFFFFFFF)
        for single, vals in ((True, f or {}), (False, d or {})):
            for i, v in vals.items():
                b = struct.pack('<f', v) + b'\0' * 12 if single else struct.pack('<d', v) + b'\0' * 8
                uc.reg_write(XMM[i], int.from_bytes(b, 'little'))
        try:
            uc.emu_start(fn, RET, count=limit)
        except UcError as err:
            raise Stop('%s at %#x' % (err, uc.reg_read(UC_X86_REG_RIP))) from None
        return uc.reg_read(UC_X86_REG_RAX)

    def retf(self):
        return self._argf(0)


MATH1F = {
    'expf': math.exp, 'logf': math.log, 'log10f': math.log10, 'sqrtf': math.sqrt, 'sinf': math.sin,
    'cosf': math.cos, 'tanf': math.tan, 'atanf': math.atan, 'tanhf': math.tanh, 'floorf': math.floor,
    'ceilf': math.ceil, 'truncf': math.trunc, 'roundf': lambda x: math.floor(abs(x) + 0.5) * (1 if x >= 0 else -1),
    'exp2f': lambda x: 2.0 ** x, 'log2f': math.log2,
}
MATH1D = {
    'exp': math.exp, 'log': math.log, 'log10': math.log10, 'sqrt': math.sqrt, 'sin': math.sin, 'cos': math.cos,
    'tan': math.tan, 'atan': math.atan, 'tanh': math.tanh, 'floor': math.floor, 'ceil': math.ceil,
    'trunc': math.trunc, 'exp2': lambda x: 2.0 ** x, 'log2': math.log2,
}
