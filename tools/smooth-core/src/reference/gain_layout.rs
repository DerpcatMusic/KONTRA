//! Field layout of the gain body `0x8e04c0` (research notes section 20).
//!
//! This is a map, not a translation. Every constant below is a byte offset that the body
//! reads, taken from `research/re-tools/emulate/gain_state_listing.json` (one process block,
//! 48 kHz stand-in, identity band state, four body entries). The roles are **not known**
//! unless the comment says so. Offsets that the stand-in never reads are absent: the body
//! may read more fields under non-identity parameters, so this list is a lower bound.
//!
//! Nothing here is wired into `dsp.rs`; the translation (section 20.4 of the notes) will
//! take these offsets as its input struct once roles are decoded.

/// Fields of the DSP object (`rcx` at entry, `[obj + off]`).
pub mod obj {
    /// u8, gate. `cmp byte [rcx+0x2bc8], 0` at `0x8e04e7`; stand-in value 1.
    pub const GATE_U8: usize = 0x2bc8;
    /// u64, pointer to the config record (see [`cfg`]). Read at `0x8e054c`, `0x8e056b`.
    pub const CFG_PTR: usize = 0x2c38;
    /// u64 counts compared with each other and with 2 (`0x8e05b1`, `0x8e05bc`); stand-in 2, 2.
    pub const COUNT_A: usize = 0x48;
    pub const COUNT_B: usize = 0x50;
    /// u8 flag; stand-in 0. Reads at `0x8e08c3`, `0x8e2e9b`.
    pub const FLAG_U8: usize = 0x58;
    /// u64 pointers to heap arrays (stand-in values in `0x404b....`..`0x404c....`).
    pub const PTR_68: usize = 0x68;
    pub const PTR_70: usize = 0x70;
    pub const PTR_118: usize = 0x118;
    pub const PTR_2C8: usize = 0x2c8;
    pub const PTR_3E8: usize = 0x3e8;
    pub const PTR_8F8: usize = 0x8f8;
    pub const PTR_900: usize = 0x900;
    pub const PTR_A18: usize = 0xa18;
    pub const PTR_A20: usize = 0xa20;
    pub const PTR_BC8: usize = 0xbc8;
    pub const PTR_C58: usize = 0xc58;
    /// u64 at `0x10` (`0x8e46b6`, stand-in pointer into the image) and u64 at `0x40`
    /// (`0x8e11da`, `0x8e1290`, stand-in value 1). Roles open.
    pub const U64_10: usize = 0x10;
    pub const U64_40: usize = 0x40;

    /// Paired u32 records: (first, second). Stand-in values are (1, 0x100), (1, 0x101), (2, 0x101).
    /// The second word is 0x100 or 0x101 in every pair, which suggests a band or channel
    /// index plus a flag, but this is not decoded.
    pub const PAIR_198: (usize, usize) = (0x198, 0x1a0);
    pub const PAIR_348: (usize, usize) = (0x348, 0x350);
    pub const PAIR_468: (usize, usize) = (0x468, 0x470);
    pub const PAIR_978: (usize, usize) = (0x978, 0x980);
    pub const PAIR_A98: (usize, usize) = (0xa98, 0xaa0);
    pub const PAIR_C48: (usize, usize) = (0xc48, 0xc50);
    pub const PAIR_CD8: (usize, usize) = (0xcd8, 0xce0);
}

/// Fields of the config record reached through [`obj::CFG_PTR`] (stand-in address `0x200208d4`).
/// Values are the stand-in's; they are not plug-in defaults.
pub mod cfg {
    /// u32, stand-in 0 (`0x8e0572`).
    pub const U32_0: usize = 0x00;
    /// f32, stand-in 0.5 (`0x8e0599`).
    pub const F32_4: usize = 0x04;
    /// f32, stand-in 1.0 (`0x8e058c`).
    pub const F32_28: usize = 0x28;
    /// f32, stand-in 0.5 (`0x8e05a7`).
    pub const F32_3C: usize = 0x3c;
    /// f32, stand-in 60.0 (`0x8e057e`); reads as the `0x42700000` pattern.
    pub const F32_4C: usize = 0x4c;
    /// u8, stand-in 0 (`0x8e0561`).
    pub const U8_52: usize = 0x52;
}

/// Argument registers at entry (`0x8e04c0`), per the notes section 19: `rcx` = this,
/// `rdx` = channel-pointer table (copied to `rdi`), `r8` = second pointer (copied to `rbx`),
/// `r9` = third pointer (non-null in the stand-in; its table is read at offsets 0..0xf8).
pub mod entry {
    pub const THIS: &str = "rcx";
    pub const TABLE_A: &str = "rdx";
    pub const TABLE_B: &str = "r8";
    pub const OPT: &str = "r9";
}
