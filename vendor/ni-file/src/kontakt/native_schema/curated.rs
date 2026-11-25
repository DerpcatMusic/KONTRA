// Readers of Kontakt 8.13.1 that programs.rs lacks, one function each, lowered by
// reader_lower.py from the decompiled function (not written by hand):
//
//     python3 tools/re/lower_curated.py <kontakt8 decompile dir> <addr>...
//
// programs.rs cannot be regenerated: its Ghidra project is gone and another analysis
// does not reproduce it. A function that does not lower cleanly stays out, with a note
// here. A curated reader replaces the generated row of the same SerType and class.
//
// Each entry is checked by the census (docs/re/NI_FILE_PARITY.md) consuming its objects
// exactly, and by the round-trip and lowering tests in mod.rs and vm.rs.

use super::programs::{ProgramSrc, ReaderSrc};

pub(super) static CURATED_PROGRAMS: &[ProgramSrc] = &[
    // BParEnv public reader: vtable 0x144e749f0 slot 10 (the private reader in slot 11,
    // 0x1404f99a0, which the catalog took, is an empty stub).
    ProgramSrc {
        name: "FUN_140d0d420",
        params: &["param_1", "param_2", "param_3"],
        frame: 32,
        ops: "\
(br (!= param_3 144) L1)
(jmp L2)
(label L1)
(throw native-throw)
(label L2)
(set t1 (rd I))
(set iVar1 (s32 t1))
(br (== iVar1 0) L4)
(jmp L5)
(label L4)
(st 4 (+ param_1 8) 0)
(ret)
(label L5)
(br (== iVar1 1) L7)
(jmp L8)
(label L7)
(st 4 (+ param_1 8) 1)
(ret)
(label L8)
(br (== iVar1 2) L10)
(jmp L11)
(label L10)
(st 4 (+ param_1 8) 2)
(ret)
(label L11)
(throw native-throw)
(ret)",
    },
    // BInsertBus public reader: vtable 0x144e980c0 slot 10 (installed at +0x10 by the
    // constructor FUN_1407f7680; id 0x45 from slot 1, current version 0x12 from slot 2).
    // Its private reader (slot 11, FUN_140d095e0) checks the version and reads nothing.
    ProgramSrc {
        name: "FUN_140d0d300",
        params: &["param_1", "param_2", "param_3", "param_4"],
        frame: 32,
        ops: "\
(br (== param_3 1) L1)
(jmp L4)
(label L4)
(br (== param_3 16) L1)
(jmp L2)
(label L1)
(call FUN_140cfbaf0 t1 (param_2 (+ param_1 48) param_4))
(set uVar2 (rd F))
(st 4 (+ param_1 20) uVar2)
(set uVar2 (rd F))
(st 4 (+ param_1 24) uVar2)
(ret)
(label L2)
(br (== param_3 17) L5)
(jmp L6)
(label L5)
(call FUN_140cfbaf0 t4 (param_2 (+ param_1 48) param_4))
(set uVar2 (rd F))
(st 4 (+ param_1 20) uVar2)
(set uVar2 (rd F))
(st 4 (+ param_1 24) uVar2)
(set uVar2 (rd I))
(st 4 (+ param_1 92) uVar2)
(ret)
(label L6)
(br (== param_3 18) L8)
(jmp L9)
(label L8)
(call FUN_140cfbaf0 t8 (param_2 (+ param_1 48) param_4))
(set uVar2 (rd F))
(st 4 (+ param_1 20) uVar2)
(set uVar2 (rd F))
(st 4 (+ param_1 24) uVar2)
(set uVar2 (rd I))
(st 4 (+ param_1 92) uVar2)
(set uVar1 (rd B))
(st 1 (+ param_1 28) uVar1)
(set uVar1 (rd B))
(st 1 (+ param_1 29) uVar1)
(ret)
(label L9)
(throw native-throw)
(ret)",
    },
];

pub(super) static CURATED_READERS: &[ReaderSrc] = &[
    ReaderSrc {
        sertype: 0x07,
        class: "BParEnv",
        program: "FUN_140d0d420",
        current: 0x90,
        versions: &[(0x90, 0x90)],
        element: 0,
        status: "typed",
    },
    ReaderSrc {
        sertype: 0x45,
        class: "BInsertBus",
        program: "FUN_140d0d300",
        current: 0x12,
        versions: &[(0x01, 0x01), (0x10, 0x12)],
        element: 0,
        status: "typed",
    },
];
