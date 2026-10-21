"""Isolated-function emulation of the soothe3 restored PE image under Unicorn.

Evidence scope: this runs single functions of the image on a synthetic
address space. It is NOT a Windows process, so it proves what a function
computes from the inputs we give it, not what the plug-in does in a host.

What the loader provides:
  - the image mapped at its preferred base (no relocations needed)
  - every imported function replaced by a stub that logs its call and
    returns 0 (or a value a scenario supplies)
  - a bump heap (operator new / malloc / HeapAlloc) and a scratch arena for
    synthetic objects
  - Win64 calling convention for calls into the image (rcx rdx r8 r9,
    xmm0..3 for floats, stack for the rest)
  - hooks: memory writes and reads inside watched ranges, and an optional
    instruction trace for coverage

Run with the venv's python using -I (see README.md).
"""

import struct

import pefile
from unicorn import (
    UC_HOOK_CODE,
    UC_HOOK_MEM_FETCH_UNMAPPED,
    UC_HOOK_MEM_READ,
    UC_HOOK_MEM_READ_UNMAPPED,
    UC_HOOK_MEM_WRITE,
    UC_HOOK_MEM_WRITE_UNMAPPED,
    UC_PROT_ALL,
    UC_ARCH_X86,
    UC_MODE_64,
    Uc,
)
from unicorn.x86_const import (
    UC_X86_REG_GS_BASE,
    UC_X86_REG_R8,
    UC_X86_REG_R9,
    UC_X86_REG_RAX,
    UC_X86_REG_RCX,
    UC_X86_REG_RDX,
    UC_X86_REG_RIP,
    UC_X86_REG_RSP,
    UC_X86_REG_XMM0,
    UC_X86_REG_XMM1,
    UC_X86_REG_XMM2,
    UC_X86_REG_XMM3,
)

IMAGE_PATH = "/tmp/opencode/rea/soothe3-restored.bin"
IMAGE_BASE = 0x180000000
PAGE = 0x1000

STUB_BASE = 0x7E000000  # import and virtual-call stubs, 16 bytes each
STUB_SIZE = 0x00100000
RET_SENTINEL = 0x7DFF0000  # emu_start(until=...) stops here
STACK_TOP = 0x7F000000
STACK_SIZE = 0x00200000
TEB_BASE = 0x6FF00000
HEAP_BASE = 0x40000000
HEAP_SIZE = 0x10000000
ARENA_BASE = 0x20000000  # synthetic objects: Knob, Input, Output, fake vtables
ARENA_SIZE = 0x04000000

_GPR_ARGS = [UC_X86_REG_RCX, UC_X86_REG_RDX, UC_X86_REG_R8, UC_X86_REG_R9]
_XMM_ARGS = [UC_X86_REG_XMM0, UC_X86_REG_XMM1, UC_X86_REG_XMM2, UC_X86_REG_XMM3]


def _align(value, to):
    return (value + to - 1) & ~(to - 1)


class Event:
    """One logged stub call: name, args seen in registers, return address."""

    __slots__ = ("name", "rcx", "rdx", "r8", "r9", "xmm0", "xmm1", "ret")

    def __init__(self, name, rcx, rdx, r8, r9, xmm0, xmm1, ret):
        self.name = name
        self.rcx = rcx
        self.rdx = rdx
        self.r8 = r8
        self.r9 = r9
        self.xmm0 = xmm0
        self.xmm1 = xmm1
        self.ret = ret

    def __repr__(self):
        return (
            f"{self.name}(rcx={self.rcx:#x} rdx={self.rdx:#x} r8={self.r8:#x} "
            f"r9={self.r9:#x} xmm0={self.xmm0:g} xmm1={self.xmm1:g}) ret={self.ret:#x}"
        )


class Emulator:
    def __init__(self, image_path=IMAGE_PATH):
        self.uc = Uc(UC_ARCH_X86, UC_MODE_64)
        self.image_path = image_path
        self.events = []  # Event list, appended by stubs
        self.stub_names = {}  # addr -> name
        self.stub_addr = {}  # name -> addr
        self.stub_handlers = {}  # name -> callable(emu, ev) -> int|None
        self.next_stub = STUB_BASE
        self.heap_next = HEAP_BASE
        self.arena_next = ARENA_BASE
        self.insn_trace = None  # set to a set() to collect executed PCs
        self.write_watch = []  # (lo, hi, label)
        self.write_log = []  # (label, addr, size, value, pc)
        self.read_watch = []
        self.read_log = []
        self.pe = pefile.PE(image_path, fast_load=False)
        self._map_image()
        self._map_runtime()
        self._install_hooks()
        self._resolve_imports()
        from crt import install as install_crt
        install_crt(self)
        from cpu import install as install_cpu
        install_cpu(self)
        from libm import install as install_libm
        install_libm(self)

    # ---- address space -------------------------------------------------
    def _map_image(self):
        pe = self.pe
        size = _align(pe.OPTIONAL_HEADER.SizeOfImage, PAGE)
        self.uc.mem_map(IMAGE_BASE, size, UC_PROT_ALL)
        hdr = pe.header[: pe.OPTIONAL_HEADER.SizeOfHeaders]
        self.uc.mem_write(IMAGE_BASE, bytes(hdr))
        for s in pe.sections:
            raw = s.get_data()
            n = min(len(raw), s.Misc_VirtualSize or len(raw))
            if n:
                self.uc.mem_write(IMAGE_BASE + s.VirtualAddress, raw[:n])
        self.image_size = size

    def _map_runtime(self):
        self.uc.mem_map(STUB_BASE, STUB_SIZE, UC_PROT_ALL)
        # every stub byte is 0xC3 (ret); the code hook does the real work
        self.uc.mem_write(STUB_BASE, b"\xc3" * STUB_SIZE)
        self.uc.mem_map(RET_SENTINEL, PAGE, UC_PROT_ALL)
        self.uc.mem_write(RET_SENTINEL, b"\xcc" * PAGE)
        self.uc.mem_map(STACK_TOP - STACK_SIZE, STACK_SIZE, UC_PROT_ALL)
        self.uc.mem_map(TEB_BASE, PAGE, UC_PROT_ALL)
        self.uc.mem_write(TEB_BASE, b"\x00" * PAGE)
        self.uc.mem_write(TEB_BASE + 0x30, struct.pack("<Q", TEB_BASE))  # gs:[0x30] self
        self.uc.reg_write(UC_X86_REG_GS_BASE, TEB_BASE)
        self.uc.mem_map(HEAP_BASE, HEAP_SIZE, UC_PROT_ALL)
        self.uc.mem_map(ARENA_BASE, ARENA_SIZE, UC_PROT_ALL)

    def _install_hooks(self):
        # Stubs always; memory and instruction hooks are added on demand so
        # plain runs stay fast (a Python callback per instruction is slow).
        self.uc.hook_add(UC_HOOK_CODE, self._on_code, begin=STUB_BASE, end=STUB_BASE + STUB_SIZE - 1)
        self._insn_hook = None
        self._write_hook = None
        self._read_hook = None
        self.unmapped = []  # (kind, addr, size, pc) for the last failed access
        self.aborted = None  # reason string when a stub aborts the run (C++ throw, etc.)
        self.uc.hook_add(UC_HOOK_MEM_READ_UNMAPPED | UC_HOOK_MEM_WRITE_UNMAPPED | UC_HOOK_MEM_FETCH_UNMAPPED, self._on_unmapped)

    def trace_insns(self, pcs):
        self.insn_trace = pcs
        if self._insn_hook is None:
            self._insn_hook = self.uc.hook_add(UC_HOOK_CODE, self._on_insn)

    def abort(self, reason):
        """Stop the current call() at the next instruction; call() returns normally."""
        self.aborted = reason
        self.uc.emu_stop()

    def _on_unmapped(self, uc, access, addr, size, value, user):
        self.unmapped.append((access, addr, size, self.reg(UC_X86_REG_RIP)))
        return False  # stop emulation; call() reports the record

    def _on_insn(self, uc, addr, size, user):
        if self.insn_trace is not None and IMAGE_BASE <= addr < IMAGE_BASE + self.image_size:
            if isinstance(self.insn_trace, set):
                self.insn_trace.add(addr)
            else:
                self.insn_trace.append(addr)

    def _on_write(self, uc, access, addr, size, value, user):
        for lo, hi, label in self.write_watch:
            if lo <= addr < hi:
                pc = self.reg(UC_X86_REG_RIP)
                self.write_log.append((label, addr, size, value, pc))
                break

    def _on_read(self, uc, access, addr, size, value, user):
        for lo, hi, label in self.read_watch:
            if lo <= addr < hi:
                pc = self.reg(UC_X86_REG_RIP)
                self.read_log.append((label, addr, size, pc))
                break

    # ---- imports and stubs ---------------------------------------------
    def _resolve_imports(self):
        """Replace import slots with stubs.

        Two sources: the import directory (covers 38 Win32 imports), and the
        IAT-like table in .rdata (import-by-name RVAs, e.g. __RTtypeid, that
        the directory does not list). The second is found from the image's
        own indirect calls: `FF 15 disp32` (call [rip+x]) and `FF 25 disp32`
        (jmp [rip+x]). A slot is only replaced when its value is an
        import-by-name RVA, so string pointers in rodata are left alone.
        """
        for entry in getattr(self.pe, "DIRECTORY_ENTRY_IMPORT", []):
            dll = entry.dll.decode(errors="replace")
            for imp in entry.imports:
                name = f"{dll}!{imp.name.decode(errors='replace')}" if imp.name else f"{dll}!ord{imp.ordinal}"
                self.uc.mem_write(imp.address, struct.pack("<Q", self.stub(name)))
        self.iat_slots = {}
        for slot_va, name in self._scan_iat_slots():
            self.iat_slots[slot_va] = name
            self.uc.mem_write(slot_va, struct.pack("<Q", self.stub(name)))

    def _scan_iat_slots(self):
        import re
        hits = {}
        for s in self.pe.sections:
            if not s.Characteristics & 0x20:  # IMAGE_SCN_CNT_CODE
                continue
            data = s.get_data()
            base_va = IMAGE_BASE + s.VirtualAddress
            for m in re.finditer(rb"\xff[\x15\x25]", data):
                off = m.start()
                if off + 6 > len(data):
                    continue
                disp = struct.unpack_from("<i", data, off + 2)[0]
                slot_va = base_va + off + 6 + disp
                if IMAGE_BASE <= slot_va < IMAGE_BASE + self.image_size:
                    hits[slot_va] = True
        out = []
        for slot_va in hits:
            v = struct.unpack("<Q", bytes(self.uc.mem_read(slot_va, 8)))[0]
            name = self._import_by_name(v)
            if name:
                out.append((slot_va, name))
        return out

    def _import_by_name(self, rva):
        """Name of an IMAGE_IMPORT_BY_NAME at RVA `rva`, or None."""
        if not (0x1000 <= rva < self.image_size):
            return None
        raw = bytes(self.uc.mem_read(IMAGE_BASE + rva + 2, 96))
        name = raw.split(b"\0", 1)[0]
        if not name or len(name) > 90:
            return None
        if not all(32 < c < 127 for c in name):
            return None
        return "IAT!" + name.decode()

    def stub(self, name, handler=None):
        """Return the stub address for a name, creating it on first use."""
        if name in self.stub_addr:
            if handler is not None:
                self.stub_handlers[name] = handler
            return self.stub_addr[name]
        addr = self.next_stub
        if addr + 16 > STUB_BASE + STUB_SIZE:
            raise RuntimeError("stub region exhausted")
        self.next_stub += 16
        self.stub_addr[name] = addr
        self.stub_names[addr] = name
        if handler is not None:
            self.stub_handlers[name] = handler
        return addr

    def _on_code(self, uc, addr, size, user):
        # Only stub entries are hooked (begin/end on hook_add), so this
        # runs only when the PC is inside the stub region.
        name = self.stub_names.get(addr)
        if name is None:
            return
        rsp = self.reg(UC_X86_REG_RSP)
        ret = struct.unpack("<Q", bytes(uc.mem_read(rsp, 8)))[0]
        ev = Event(
            name,
            self.reg(UC_X86_REG_RCX), self.reg(UC_X86_REG_RDX),
            self.reg(UC_X86_REG_R8), self.reg(UC_X86_REG_R9),
            self.xmm_double(UC_X86_REG_XMM0), self.xmm_double(UC_X86_REG_XMM1),
            ret,
        )
        self.events.append(ev)
        handler = self.stub_handlers.get(name, _default_stub)
        out = handler(self, ev)
        if out is not None:
            if isinstance(out, float):
                uc.reg_write(UC_X86_REG_XMM0, _xmm_bytes(out))
            else:
                uc.reg_write(UC_X86_REG_RAX, int(out) & (2**64 - 1))
        uc.reg_write(UC_X86_REG_RSP, rsp + 8)
        uc.reg_write(UC_X86_REG_RIP, ret)

    # ---- registers and memory helpers ----------------------------------
    def reg(self, r):
        return self.uc.reg_read(r)

    def xmm_double(self, r):
        """Low float32 of an xmm register (the name is historical; returns float32)."""
        raw = self.uc.reg_read(r)
        if isinstance(raw, int):
            raw = raw.to_bytes(16, "little")
        return struct.unpack("<f", bytes(raw[:4]))[0]

    def alloc(self, size, align=16):
        """Zeroed bump allocation on the heap (never freed)."""
        self.heap_next = _align(self.heap_next, align)
        p = self.heap_next
        self.heap_next += size
        if self.heap_next > HEAP_BASE + HEAP_SIZE:
            raise MemoryError("emulator heap exhausted")
        return p

    def arena(self, size, align=16):
        """Zeroed block for synthetic objects (Knob, Input, Output, vtables)."""
        self.arena_next = _align(self.arena_next, align)
        p = self.arena_next
        self.arena_next += size
        if self.arena_next > ARENA_BASE + ARENA_SIZE:
            raise MemoryError("emulator arena exhausted")
        return p

    def wr(self, addr, data):
        self.uc.mem_write(addr, bytes(data))

    def rd(self, addr, size):
        return bytes(self.uc.mem_read(addr, size))

    def w64(self, addr, value):
        self.wr(addr, struct.pack("<Q", value))

    def w32(self, addr, value):
        self.wr(addr, struct.pack("<I", value & 0xFFFFFFFF))

    def wf32(self, addr, value):
        self.wr(addr, struct.pack("<f", value))

    def wf64(self, addr, value):
        self.wr(addr, struct.pack("<d", value))

    def r64(self, addr):
        return struct.unpack("<Q", self.rd(addr, 8))[0]

    def r32(self, addr):
        return struct.unpack("<I", self.rd(addr, 4))[0]

    def rf32(self, addr):
        return struct.unpack("<f", self.rd(addr, 4))[0]

    def rf64(self, addr):
        return struct.unpack("<d", self.rd(addr, 8))[0]

    def fake_vtable(self, owner, slots, handler_factory=None):
        """A vtable whose every slot is a named stub: owner::vslot<k>.

        Returns the vtable address. handler_factory(k) -> handler or None.
        """
        vt = self.arena(8 * slots)
        for k in range(slots):
            name = f"{owner}::vslot{k}"
            handler = handler_factory(k) if handler_factory else None
            self.w64(vt + 8 * k, self.stub(name, handler))
        return vt

    # ---- calls into the image ------------------------------------------
    def call(self, addr, *args, floats=(), max_insns=2_000_000, timeout_us=0):
        """Win64 call. Integer args -> rcx rdx r8 r9 then stack; floats -> xmm0..3.

        Args and floats are positional across the first four slots only;
        pass extra integer arguments as later entries in `args`. Returns rax.
        """
        if len(floats) > 4:
            raise ValueError("at most 4 float args")
        sp = _align(STACK_TOP - 0x4000, 16) - 8  # call-site rsp % 16 == 8 at entry
        stack_args = args[4:]
        for i, a in enumerate(stack_args):
            self.w64(sp + 40 + 8 * i, a)
        self.w64(sp, RET_SENTINEL)
        for i, a in enumerate(args[:4]):
            self.uc.reg_write(_GPR_ARGS[i], a & (2**64 - 1))
        for i, f in enumerate(floats):
            self.uc.reg_write(_XMM_ARGS[i], int.from_bytes(_xmm_bytes(f), "little"))
        self.uc.reg_write(UC_X86_REG_RSP, sp)
        self.uc.reg_write(UC_X86_REG_RAX, 0)
        self.unmapped.clear()
        self.aborted = None
        try:
            self.uc.emu_start(addr, RET_SENTINEL, timeout_us, max_insns)
        except Exception as exc:  # UcError; keep the faulting access for the caller
            pc = self.reg(UC_X86_REG_RIP)
            where = self.unmapped[-1] if self.unmapped else None
            raise RuntimeError(f"emulation fault {exc} at pc={pc:#x} access={where}") from exc
        pc = self.reg(UC_X86_REG_RIP)
        if self.aborted is None and pc != RET_SENTINEL:
            # emu_start returns normally when it hits max_insns or the timeout
            raise RuntimeError(f"execution limit reached at pc={pc:#x} before return")
        return self.reg(UC_X86_REG_RAX)

    def call_float(self, addr, *args, floats=()):
        """Like call(), returning xmm0 as a float32."""
        self.call(addr, *args, floats=floats)
        return self.xmm_double(UC_X86_REG_XMM0)

    def watch_writes(self, lo, hi, label):
        self.write_watch.append((lo, hi, label))
        if self._write_hook is None:
            self._write_hook = self.uc.hook_add(UC_HOOK_MEM_WRITE, self._on_write)

    def watch_reads(self, lo, hi, label):
        self.read_watch.append((lo, hi, label))
        if self._read_hook is None:
            self._read_hook = self.uc.hook_add(UC_HOOK_MEM_READ, self._on_read)

    def clear_logs(self):
        self.events.clear()
        self.write_log.clear()
        self.read_log.clear()


def _default_stub(emu, ev):
    return 0


def _xmm_bytes(value):
    return struct.pack("<f", value) + b"\x00" * 12


def image_va(rva):
    return IMAGE_BASE + rva
