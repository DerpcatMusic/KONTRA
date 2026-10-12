"""Bounded Kontakt 8.13.1 original-byte binding/fade checks; never launches a host."""
import csv
import hashlib
import json
import math
import struct
from pathlib import Path

import capstone
import pefile
from unicorn import Uc, UC_ARCH_X86, UC_MODE_64
from unicorn.x86_const import (
    UC_X86_REG_RAX, UC_X86_REG_RBX, UC_X86_REG_RCX, UC_X86_REG_RDX,
    UC_X86_REG_R8, UC_X86_REG_R9, UC_X86_REG_RDI, UC_X86_REG_RSI,
    UC_X86_REG_R14, UC_X86_REG_RSP, UC_X86_REG_RIP, UC_X86_REG_MXCSR,
    UC_X86_REG_XMM0, UC_X86_REG_XMM1, UC_X86_REG_XMM2,
    UC_X86_REG_XMM7, UC_X86_REG_XMM8, UC_X86_REG_XMM9,
)

ROOT = Path(__file__).resolve().parents[3]
CORPUS = ROOT / "artifacts/engine-analysis-2026-10-07/kontakt-engine"
PIN = "0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8"
meta = json.loads((CORPUS / "pe.json").read_text())
raw = Path(meta["path"]).read_bytes()
assert meta["sha256"] == hashlib.sha256(raw).hexdigest() == PIN
pe = pefile.PE(data=raw, fast_load=True)
base = pe.OPTIONAL_HEADER.ImageBase
cpu = Uc(UC_ARCH_X86, UC_MODE_64)
cpu.mem_map(base, (pe.OPTIONAL_HEADER.SizeOfImage + 4095) & ~4095)
cpu.mem_write(base, pe.get_memory_mapped_image())
OBJ, DESC, OUT, STACK, STOP, ENGINE, GLOBAL = [0x2000000 + i * 0x10000 for i in range(7)]
for addr in [OBJ, DESC, OUT, STACK, STOP, ENGINE, GLOBAL]:
    cpu.mem_map(addr, 0x10000 if addr != GLOBAL else 0x30000)
cpu.mem_write(STOP, b"\xcc")


def put(addr, fmt, value):
    cpu.mem_write(addr, struct.pack("<" + fmt, value))


def get(addr, fmt):
    return struct.unpack("<" + fmt, cpu.mem_read(addr, struct.calcsize(fmt)))[0]


def f32(value):
    return struct.unpack("<f", struct.pack("<f", value))[0]


def xmm(reg, fmt, value):
    cpu.reg_write(reg, int.from_bytes(struct.pack("<" + fmt, value), "little"))


def xmm_get(reg, fmt):
    return struct.unpack("<" + fmt, cpu.reg_read(reg).to_bytes(16, "little")[:struct.calcsize(fmt)])[0]


def run(start, end=STOP):
    rsp = STACK + 0xff00 - 8
    put(rsp, "Q", STOP)
    cpu.reg_write(UC_X86_REG_RSP, rsp)
    cpu.reg_write(UC_X86_REG_MXCSR, 0x1f80)
    cpu.emu_start(start, end, timeout=1000000, count=500000)
    assert cpu.reg_read(UC_X86_REG_RIP) == end, hex(start)


# The actual engine factory, not the editor preview. Stop at constructor entry.
factory = []
for enum, expected in [(5, 0x1407c2d10), (6, 0x1407c2110)]:
    for targets in [1, 2, 16]:
        put(DESC + 0x558, "I", enum)
        put(DESC + 0x308, "I", targets)
        cpu.reg_write(UC_X86_REG_RDX, DESC)
        cpu.reg_write(UC_X86_REG_RBX, base)
        cpu.reg_write(UC_X86_REG_R14, OBJ)
        target_table = base + (0x9787f8 if enum == 5 else 0x978838)
        branch = get(target_table + (targets - 1) * 4, "I") + base
        call = list(capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64).disasm(pe.get_data(branch - base, 16), branch))[1]
        entry = int(call.op_str, 16)
        if targets == 1:
            assert entry == expected
        run(0x140974c6a, entry)
        factory.append(dict(enum=enum, targets=targets, constructor=hex(entry)))

# Vtable identity comes from native RTTI inventory; the kernel is shared across counts.
with (CORPUS / "vtable-methods.csv").open() as stream:
    digital = [r for r in csv.DictReader(stream) if "BLfoMultiDigitalWithTargets<" in r["type"] and r["slot"] == "6"]
assert len(digital) == 16 and {r["entry_va"] for r in digital} == {"0x140b078d0"}

# The reader's active-note byte depends on note value, not the packed flag.
threshold = get(0x144737708, "f")
next_note = struct.unpack("<f", struct.pack("<I", get(0x144737708, "I") + 1))[0]
sync_checks = []
for note in [-1., 0., threshold, next_note, 1 / 24]:
    for flag in [0, 1]:
        put(DESC + 0x18, "f", note)
        cpu.reg_write(UC_X86_REG_RDI, DESC)
        cpu.reg_write(UC_X86_REG_RAX, flag)
        run(0x140ab5ddc, 0x140ab5df6)
        active = get(DESC + 8, "B")
        assert active == int(f32(note) > threshold) and get(DESC + 9, "B") == flag
        sync_checks.append(dict(note=f32(note), packed_flag=flag, active=active))

# Execute the saved-field load, native vtable lookup and unsynced getter together.
delays = [0., .001, .5, 1., 32 / 48, 10., 100., 1000., 9999.]
saved_delay_checks = []
delay_checks = []
for ms in delays:
    put(DESC + 0x55c, "f", ms)
    put(DESC + 0x5a8, "Q", 0x144e74260)
    put(DESC + 0x5b0, "B", 0)
    for tempo_source in ["transport", "fallback"]:
        put(ENGINE + 8, "Q", GLOBAL)
        put(GLOBAL + 0x11c00, "Q", GLOBAL + 0x1000 if tempo_source == "transport" else 0)
        put(GLOBAL + 0x1008, "f", 120.)
        put(GLOBAL + 0x2d7f8, "Q", GLOBAL + 0x2000)
        put(GLOBAL + 0x3188, "i", 120000)
        cpu.reg_write(UC_X86_REG_RDI, DESC)
        cpu.reg_write(UC_X86_REG_RSI, ENGINE)
        xmm(UC_X86_REG_XMM8, "f", .001)
        run(0x140977d3a, 0x140977d85)
        assert cpu.reg_read(UC_X86_REG_R8) == 0x14072b380
        assert xmm_get(UC_X86_REG_XMM0, "f") == f32(ms)
        saved_delay_checks.append(dict(ms=f32(ms), tempo_source=tempo_source,
                                      getter=hex(cpu.reg_read(UC_X86_REG_R8))))

    for rate in [44100., 48000., 96000.]:
        put(ENGINE + 8, "Q", GLOBAL)
        put(GLOBAL + 0x27d00, "f", rate)
        put(OUT, "Q", OBJ)
        put(OBJ, "Q", 0x144729c08)
        cpu.reg_write(UC_X86_REG_R14, OUT)
        cpu.reg_write(UC_X86_REG_RSI, ENGINE)
        xmm(UC_X86_REG_XMM7, "f", .03125)
        run(0x140976457, 0x140976471)
        control_rate = xmm_get(UC_X86_REG_XMM1, "f")
        assert control_rate == f32(rate / 32)
        cpu.reg_write(UC_X86_REG_RCX, OBJ)
        run(0x1405ed630)
        assert get(OBJ + 0x14, "f") == control_rate

        cpu.reg_write(UC_X86_REG_RBX, OBJ)
        xmm(UC_X86_REG_XMM0, "f", ms)
        xmm(UC_X86_REG_XMM8, "f", .001)
        xmm(UC_X86_REG_XMM9, "d", 1.)
        run(0x140977d85, 0x140977db1)
        count = math.trunc(f32(f32(f32(ms) * control_rate) * f32(.001)))
        assert get(OBJ + 0xf8, "i") == count
        assert xmm_get(UC_X86_REG_XMM0, "d") == 4.333333200878572
        exponent = xmm_get(UC_X86_REG_XMM1, "d")
        assert exponent == (1 / count if count else math.inf)
        delay_checks.append(dict(ms=f32(ms), rate=rate, control_rate=control_rate, count=count))

# Real saved Hz setter: increment is the f32 product, promoted to f64.
frequency_checks = []
for rate in [44100., 48000., 96000.]:
    inverse = f32(1 / (rate / 32))
    put(OBJ + 0xc0, "f", inverse)
    for hz in [.01, .2, 1., 3.7, 20., 212.1]:
        cpu.reg_write(UC_X86_REG_RCX, OBJ)
        xmm(UC_X86_REG_XMM1, "f", hz)
        run(0x14095ecd0)
        assert get(OBJ + 0xb8, "d") == f32(f32(hz) * inverse)
        frequency_checks.append(dict(rate=rate, hz=f32(hz), increment=get(OBJ + 0xb8, "d")))

# Saved v73 flag and phase copy in the actual engine setup.
for flag in [0, 1]:
    put(DESC + 0x568, "f", .125)
    put(DESC + 0x56d, "B", flag)
    cpu.reg_write(UC_X86_REG_RDI, DESC)
    cpu.reg_write(UC_X86_REG_RBX, OBJ)
    run(0x140977dc5, 0x140977df3)
    assert get(OBJ + 0xe8, "d") == .125 and get(OBJ + 0xe5, "B") == flag

# Preserve non-unit mix values and the saved-to-runtime component order.
weights = [.3, -.4, .5, -.6, .7]
for index, weight in enumerate(weights):
    put(DESC + 0x5e0 + 4 * index, "f", weight)
put(DESC + 0x558, "I", 6)
put(DESC + 0x56c, "B", 1)
run(0x140977e45, 0x140977eb4)
assert [get(OBJ + offset, "f") for offset in [0x128, 0x134, 0x130, 0x12c, 0x138]] == [f32(w) for w in weights]
assert get(OBJ + 0x13c, "B") == 1


def waveform(frames):
    cpu.reg_write(UC_X86_REG_RCX, OBJ)
    cpu.reg_write(UC_X86_REG_RDX, OUT)
    cpu.reg_write(UC_X86_REG_R8, frames)
    run(0x140b078d0)
    return list(struct.unpack("<" + "f" * frames, cpu.mem_read(OUT, frames * 4)))


def init_wave():
    cpu.mem_write(OBJ, bytes(0x300))
    put(OBJ + 0xe8, "d", .125)
    put(OBJ + 0xb8, "d", .0007)
    put(OBJ + 0x128, "f", .3)
    put(OBJ + 0x100, "f", .5)
    put(OBJ + 0x13c, "B", 1)
    cpu.reg_write(UC_X86_REG_RCX, OBJ)
    run(0x1405f45a0)


# Compare the native fade to the same native waveform without fade; no sine oracle.
fade_checks = []
for count in [1, 2, 5, 24, 100]:
    for flag in [0, 1]:
        frames = count + 3
        init_wave()
        plain = waveform(frames)
        init_wave()
        factor = f32(math.pow(4.333333200878572, 1 / count))
        put(OBJ + 0xf4, "f", factor)
        put(OBJ + 0xf8, "i", count)
        put(OBJ + 0xfc, "i", count)
        put(OBJ + 0xe5, "B", flag)
        actual = waveform(frames)
        accumulator = f32(.3)
        expected = []
        for i, value in enumerate(plain):
            if i < count:
                expected.append(f32(f32(accumulator - f32(.3)) * value))
                lo, hi = (f32(.3), f32(1.3)) if flag else (0., 1.)
                accumulator = min(hi, max(lo, f32(accumulator * factor)))
            else:
                expected.append(value)
        assert actual == expected, (count, flag, max(abs(a-b) for a,b in zip(actual, expected)))
        assert get(OBJ + 0xfc, "i") == 0 and get(OBJ + 0xf0, "f") == accumulator
        fade_checks.append(dict(count=count, flag=flag, values=actual, accumulator=accumulator, max_error=0.))

receipt = dict(
    binary_sha256=PIN, image_base=hex(base), version="8.13.1 standalone",
    helper_stubs=[], factory=factory, sync_checks=sync_checks,
    saved_delay_checks=saved_delay_checks, delay_checks=delay_checks,
    frequency_checks=frequency_checks, fade_checks=fade_checks,
    fade_values=sum(len(c["values"]) for c in fade_checks),
    pow_scope="Count/exponent/native pow-call arguments verified; CRT pow itself was not emulated. Fade receives an authored f32 multiplier.",
    scope="Saved unsynced scalar binding and original waveform fade only; no host, whole engine setup, transport sync, or generic source scheduling certification.",
)
Path(__file__).with_name("native-binding.json").write_text(json.dumps(receipt, indent=2) + "\n")
disassembler = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_64)
spans = [
    (0x140974c6a, 0x140974c86), (0x140975131, 0x140975158),
    (0x140975220, 0x140975247), (0x1407c2110, 0x1407c21c0),
    (0x14097411c, 0x140974124), (0x140976457, 0x140976473),
    (0x14072b380, 0x14072b39e), (0x140a74bf0, 0x140a74bfa),
    (0x140ab5ddc, 0x140ab5df6), (0x140977d3a, 0x140977d85),
    (0x14097741e, 0x140977427), (0x140977442, 0x14097744b),
    (0x140977d73, 0x140977de0), (0x140977de0, 0x140977df3),
    (0x140977e45, 0x140977eb9),
]
lines = ["; Original instructions; Kontakt 8.13.1 standalone SHA256 " + PIN]
for start, end in spans:
    lines.append("\n; Span " + hex(start) + ".." + hex(end))
    for ins in disassembler.disasm(pe.get_data(start - base, end - start), start):
        lines.append(f"{ins.address:x}  {ins.bytes.hex():24} {ins.mnemonic} {ins.op_str}")
Path(__file__).with_name("native-binding.asm").write_text("\n".join(lines) + "\n")
print("PASS", len(factory), "factory paths;", len(sync_checks), "sync guards;", len(saved_delay_checks), "saved-delay bindings;", len(delay_checks), "delay counts;", len(frequency_checks), "Hz setters;", receipt["fade_values"], "bit-exact fade values")
