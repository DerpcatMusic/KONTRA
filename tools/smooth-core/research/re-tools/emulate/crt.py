"""Real implementations for the CRT, heap and memory imports the soothe3 code uses.

Everything else stays a logging no-op (returns 0). Throwing helpers abort the
run: the function under test took a C++ exception path we did not model, and
the caller reports it (Emulator.aborted) instead of continuing on garbage.
"""

_THROW = (
    "IAT!_CxxThrowException",
    "IAT!_invalid_parameter_noinfo_noreturn",
    "IAT!?_Xbad_alloc@std@@YAXXZ",
    "IAT!?_Xbad_function_call@std@@YAXXZ",
    "IAT!?_Xinvalid_argument@std@@YAXPEBD@Z",
    "IAT!?_Xlength_error@std@@YAXPEBD@Z",
    "IAT!?_Throw_Cpp_error@std@@YAXH@Z",
)


def _abort_on_throw(name):
    def handler(emu, ev):
        emu.abort(f"{name} called from {ev.ret:#x}")
    return handler


def _malloc(emu, ev):
    size = ev.rcx or 1
    return emu.alloc(size)


def _calloc(emu, ev):
    return emu.alloc(max(1, ev.rcx * ev.rdx))  # arena memory is already zero


def _realloc(emu, ev):
    new = emu.alloc(max(1, ev.rdx))
    old = ev.rcx
    if old:
        # the old size is unknown; copy a bounded prefix
        emu.wr(new, emu.rd(old, min(ev.rdx, 4096)))
    return new


def _memcpy(emu, ev):
    if ev.r8:
        emu.wr(ev.rcx, emu.rd(ev.rdx, ev.r8))
    return ev.rcx


def _memset(emu, ev):
    if ev.r8:
        emu.wr(ev.rcx, bytes([ev.rdx & 0xFF]) * ev.r8)
    return ev.rcx


def _memcmp(emu, ev):
    a = emu.rd(ev.rcx, ev.r8) if ev.r8 else b""
    b = emu.rd(ev.rdx, ev.r8) if ev.r8 else b""
    return (a > b) - (a < b) & 0xFFFFFFFF


def _strcmp(emu, ev):
    a = _cstr(emu, ev.rcx)
    b = _cstr(emu, ev.rdx)
    return (a > b) - (a < b) & 0xFFFFFFFF


def _cstr(emu, addr, limit=4096):
    out = bytearray()
    while len(out) < limit:
        c = emu.rd(addr + len(out), 1)[0]
        if c == 0:
            break
        out.append(c)
    return bytes(out)


def install(emu):
    """Attach the real CRT/heap handlers to an Emulator's stub table."""
    alloc = {
        "IAT!malloc": _malloc,
        "IAT!_aligned_malloc": lambda emu, ev: emu.alloc(ev.rcx or 1, max(16, ev.rdx)),
        "IAT!calloc": _calloc,
        "IAT!realloc": _realloc,
        "IAT!free": lambda emu, ev: 0,
        "IAT!_aligned_free": lambda emu, ev: 0,
        "KERNEL32.dll!HeapAlloc": lambda emu, ev: emu.alloc(max(1, ev.r8)),  # dwBytes is arg 3
        "KERNEL32.dll!HeapFree": lambda emu, ev: 1,
        "IAT!??2@YAPEAX_K@Z": _malloc,
        "IAT!??3@YAXPEAX@Z": lambda emu, ev: 0,  # operator delete(void*)
        "IAT!_callnewh": lambda emu, ev: 0,  # no new_handler: fail the allocation
        "IAT!memcpy": _memcpy,
        "IAT!memmove": _memcpy,
        "IAT!memset": _memset,
        "IAT!memcmp": _memcmp,
        "IAT!strcmp": _strcmp,
    }
    for name, handler in alloc.items():
        emu.stub(name, handler)
    for name in _THROW:
        emu.stub(name, _abort_on_throw(name))
    return emu
