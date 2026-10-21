"""CPUID profile for the emulator.

The soothe3 analysis path is chosen from CPU feature bits. The init routine
0x18088c5f0 runs cpuid (leaf 0, then every leaf up to the reported maximum),
stores the results in a global record, and 0x18030d310 sets the global mode
(0x26d7bb8) to 4 when feature bit 0 of that record is set and to 1 otherwise.
Unicorn's default CPU reports no such features, so the mode would be left at
0 and the analysis path would be skipped. This profile names a modern
x86-64 CPU with SSE4.2/AVX/AVX2/FMA, the class of the reference machines.

Evidence scope: an emulator assumption about the CPU, not a value read from a
plug-in run. Change PROFILE to test the other mode.
"""

from unicorn import UC_HOOK_INSN
from unicorn.x86_const import (
    UC_X86_INS_CPUID,
    UC_X86_REG_EAX,
    UC_X86_REG_EBX,
    UC_X86_REG_ECX,
    UC_X86_REG_EDX,
)

PROFILE = {
    # leaf -> (eax, ebx, ecx, edx); subleaf 0 unless the key is (leaf, sub)
    0x0: (0x20, 0x756E6547, 0x6C65746E, 0x49656E69),  # "GenuineIntel"
    0x1: (0x000906EA, 0x00100800,
          # SSE3 SSSE3 FMA CX16 SSE4.1 SSE4.2 MOVBE POPCNT OSXSAVE AVX AES
          (1 << 0) | (1 << 9) | (1 << 12) | (1 << 13) | (1 << 19) | (1 << 20)
          | (1 << 22) | (1 << 23) | (1 << 27) | (1 << 28) | (1 << 25),
          # FPU TSC CX8 CMOV MMX FXSR SSE SSE2
          (1 << 0) | (1 << 4) | (1 << 8) | (1 << 15) | (1 << 23) | (1 << 24)
          | (1 << 25) | (1 << 26)),
    (0x7, 0): (0,
               # BMI1 AVX2 BMI2 ERMS ADX SHA? (bits 3, 5, 8, 9, 19)
               (1 << 3) | (1 << 5) | (1 << 8) | (1 << 9) | (1 << 19),
               0, 0),
    0x80000000: (0x80000008, 0, 0, 0),
    0x80000001: (0, 0, (1 << 0) | (1 << 5) | (1 << 8), (1 << 11) | (1 << 29)),
}


def _leaf(eax, ecx):
    if eax == 0x7:
        return PROFILE.get((0x7, ecx), (0, 0, 0, 0))
    return PROFILE.get(eax, (0, 0, 0, 0))


def install(emu):
    """Hook CPUID on the emulator's Unicorn instance. The hook replaces the
    instruction: it writes the four result registers and does not execute cpuid."""

    def on_cpuid(uc, _user):
        eax = uc.reg_read(UC_X86_REG_EAX) & 0xFFFFFFFF
        ecx = uc.reg_read(UC_X86_REG_ECX) & 0xFFFFFFFF
        a, b, c, d = _leaf(eax, ecx)
        uc.reg_write(UC_X86_REG_EAX, a)
        uc.reg_write(UC_X86_REG_EBX, b)
        uc.reg_write(UC_X86_REG_ECX, c)
        uc.reg_write(UC_X86_REG_EDX, d)
        return 0

    emu.uc.hook_add(UC_HOOK_INSN, on_cpuid, None, 1, 0, UC_X86_INS_CPUID)
