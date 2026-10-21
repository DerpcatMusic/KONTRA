"""Process block 0x890a00 on the constructor state (shared helpers).

Entry, as read from the disassembly (section 17 of ../../re-latency-fft-scale.md):
  wrapper (0x58 bytes, built by the factory 0x88f430) holds the DSP object at [+0]
  and the host slots at +0x08 setParam, +0x10 process, +0x18 prepare, +0x28 getLatency,
  +0x50 flag setter. The process slot is the thunk 0x88f4f0 (`mov rcx,[rcx]; jmp 0x890a00`).
  process(this, tabA, tabB, opt, N):
    this = DSP object, tabA = rdx, tabB = r8: channel-pointer tables, count = [this+0x10]
    opt  = r9: optional second frame (null -> the frame is built from tabB)
    N    = 5th argument (stack): samples per channel in this block
  Channel count of the staged frames = [this+0x14]. Both come from the layout enums in the
  constructor config (cfg+0 and cfg+8) through 0x56cad0 (index 2 = 2 channels).

Evidence scope: isolated emulation, synthetic host, stand-in layout and block size.
"""

import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from fake_host import run_ctor, u32_at  # noqa: E402
from pe_emu import image_va  # noqa: E402

PROCESS_THUNK = image_va(0x88F4F0)  # wrapper slot +0x10
PREPARE = image_va(0x88F500)  # wrapper slot +0x18
GETLATENCY = image_va(0x88F610)  # wrapper slot +0x28 (rcx = &holder)
FLAG_SET = image_va(0x88F660)  # wrapper slot +0x50, (obj, which, bool)
SETPARAM_THUNK = image_va(0x88F4E0)  # wrapper slot +0x08
STEREO_CFG = {0: 2, 8: 2}  # layout indices 2 -> 2 channels (0x56cad0), stand-in
# Host slot +0x28 is a bool query `isset(host, id)` (the block calls it with id 9,
# band_solo_channels). Stand-in: every id answers 0 (no band solo). Not a plug-in value.
HOST_BOOL_DEFAULTS = {9: 0}


class Session:
    """A constructed DSP object wrapped the way the host holds it."""

    def __init__(self, rate, cfg_fields=None):
        self.rate = rate
        self.emu, self.obj, self.log = run_ctor(rate, cfg_fields or STEREO_CFG)
        host = self.emu.r64(self.obj)  # obj[0] = host (the constructor's rdx)
        self.emu.w64(host + 0x28, self.emu.stub("Host::isset", _host_isset))
        # Typed host values. The constructor reads id 12 (samplerate) as a float at [rbp+0x218]
        # (0x890401, 0x8903f9) and id 10 (input_channel_layout) as an int. Stand-ins: the rate
        # passed to the constructor, and layout index 2 (stereo) as in cfg.
        self.values = {12: struct.pack("<f", rate) + b"\0" * 4, 10: struct.pack("<I", 2) + b"\0" * 4}
        self.emu.w64(host + 0x20, self.emu.stub("Host::get2", self._host_get))
        self.wrapper = self.emu.arena(0x58)
        self.emu.w64(self.wrapper, self.obj)
        self.emu.w64(self.wrapper + 0x10, PROCESS_THUNK)
        self.emu.w64(self.wrapper + 0x18, PREPARE)
        self.emu.w64(self.wrapper + 0x28, GETLATENCY)

    @property
    def channels(self):
        return u32_at(self.emu, self.obj + 0x10)

    def _host_get(self, emu, ev):
        """Host slot +0x20: get(host, id, out). Writes the typed value for `id`, else 0."""
        emu.wr(ev.r8, self.values.get(ev.rdx, b"\0" * 8))
        return 0

    def prepare(self):
        self.emu.call(PREPARE, self.wrapper, max_insns=10_000_000)

    def latency(self):
        holder = self.emu.arena(8)
        self.emu.w64(holder, self.obj)
        return self.emu.call(GETLATENCY, holder, max_insns=10_000_000)


def _host_isset(emu, ev):
    return HOST_BOOL_DEFAULTS.get(ev.rdx, 0)


def f32s(values):
    return struct.pack(f"<{len(values)}f", *values)


def alloc_frames(emu, nch, n, samples_per_ch):
    """Allocate nch channel buffers of n floats and a table of their pointers."""
    table = emu.arena(8 * max(nch, 1))
    bufs = []
    for c in range(nch):
        buf = emu.arena(4 * n + 64)
        emu.wr(buf, f32s(samples_per_ch[c]))
        bufs.append(buf)
        emu.w64(table + 8 * c, buf)
    return table, bufs


def read_frames(emu, bufs, n):
    return [list(struct.unpack(f"<{n}f", emu.rd(b, 4 * n))) for b in bufs]


def add_probe(emu, va, name, fn, hits):
    """Run fn(emu) each time the instruction at `va` executes; append (name, value) to hits."""
    from unicorn import UC_HOOK_CODE

    def cb(uc, addr, size, user):
        hits.append((name, fn(emu)))

    emu.uc.hook_add(UC_HOOK_CODE, cb, begin=va, end=va)


def fault_report(emu, exc, depth=6):
    """Print the faulting PC, its function, the code around it, registers and a stack walk.

    The stack walk takes qwords on the stack that point into the image; those are return
    addresses of the active calls (heuristic, good enough to name the callers).
    """
    from unicorn.x86_const import UC_X86_REG_RSP
    from detector_run import function_of
    from disasm_rva import disasm
    from pe_emu import IMAGE_BASE

    text = str(exc)
    pc = int(text.split("pc=")[1].split()[0].rstrip(","), 16)
    fn = function_of(pc)
    lines = [f"FAULT {text.split(' at pc')[0]}  pc={pc:#x} rva={pc - IMAGE_BASE:#x}"]
    if fn:
        lines.append(f"  in fn rva {fn[0]:#x}..{fn[1]:#x}")
    lines.append(disasm(emu, pc - IMAGE_BASE - 0x18, 0x30))
    names = ["rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rbp", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15"]
    from unicorn.x86_const import (UC_X86_REG_RAX, UC_X86_REG_RBX, UC_X86_REG_RCX, UC_X86_REG_RDX,
                                   UC_X86_REG_RSI, UC_X86_REG_RDI, UC_X86_REG_RBP, UC_X86_REG_R8,
                                   UC_X86_REG_R9, UC_X86_REG_R10, UC_X86_REG_R11, UC_X86_REG_R12,
                                   UC_X86_REG_R13, UC_X86_REG_R14, UC_X86_REG_R15)
    regs = [UC_X86_REG_RAX, UC_X86_REG_RBX, UC_X86_REG_RCX, UC_X86_REG_RDX, UC_X86_REG_RSI,
            UC_X86_REG_RDI, UC_X86_REG_RBP, UC_X86_REG_R8, UC_X86_REG_R9, UC_X86_REG_R10,
            UC_X86_REG_R11, UC_X86_REG_R12, UC_X86_REG_R13, UC_X86_REG_R14, UC_X86_REG_R15]
    lines.append("  regs: " + " ".join(f"{n}={emu.reg(r):#x}" for n, r in zip(names, regs)))
    rsp = emu.reg(UC_X86_REG_RSP)
    callers = []
    for off in range(0, 0x4000, 8):
        try:
            q = emu.r64(rsp + off)
        except Exception:
            break
        if IMAGE_BASE <= q < IMAGE_BASE + emu.image_size:
            f = function_of(q)
            callers.append(f"{q - IMAGE_BASE:#x}" + (f" (fn {f[0]:#x})" if f else ""))
            if len(callers) >= depth:
                break
    lines.append("  stack return addrs: " + ", ".join(callers))
    return "\n".join(lines)
