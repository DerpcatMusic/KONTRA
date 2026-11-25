//! Executor for the stream IR in `programs.rs` (see tools/re/reader_lower.py).
//!
//! The IR is Kontakt 8.13.1's own reader control flow: stored-version switches, validation
//! throws and data-dependent branches are decided here by the same conditions Kontakt
//! evaluates. Values the stream does not determine (host state such as instrument tables)
//! are *undefined*; a branch on one is searched both ways and a stream decodes when some
//! assignment consumes it exactly. Reference interpreter: tools/re/ir_sim.py.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::curated::{CURATED_PRIVATE, CURATED_PROGRAMS, CURATED_READERS, CURATED_UNFRAMED};
use super::programs::{ProgramSrc, ReaderSrc, PROGRAMS, READERS};

/// The generated programs and the curated ones (curated.rs); a curated program replaces the
/// generated one of the same name.
fn all_programs() -> &'static [&'static ProgramSrc] {
    static ALL: OnceLock<Vec<&'static ProgramSrc>> = OnceLock::new();
    ALL.get_or_init(|| {
        let curated = |p: &ProgramSrc| CURATED_PROGRAMS.iter().any(|c| c.name == p.name);
        CURATED_PROGRAMS
            .iter()
            .chain(PROGRAMS.iter().filter(|p| !curated(p)))
            .collect()
    })
}

/// The generated readers, with a curated reader (curated.rs) in place of the row of the same
/// SerType and class.
fn all_readers() -> &'static [&'static ReaderSrc] {
    static ALL: OnceLock<Vec<&'static ReaderSrc>> = OnceLock::new();
    ALL.get_or_init(|| {
        let curated = |r: &ReaderSrc| {
            CURATED_READERS
                .iter()
                .any(|c| c.sertype == r.sertype && c.class == r.class)
        };
        CURATED_READERS
            .iter()
            .chain(READERS.iter().filter(|r| !curated(r)))
            .collect()
    })
}

/// Embedded bodies a reader reads through a host object's virtual reader, so the stream
/// does not name their class: (program, SerType of the body).
///
/// - `FUN_140d1d860`, BSnapshot's group list: for each group of the instrument it checks the
///   stored flag and version against that group's snapshot object and reads its body. Those
///   bodies are BGroupSnapshot (0x50, v1–v4): the hand reader (objects/snapshot.rs) reads them
///   as compact group snapshots, and every snapshot in the corpus decodes exactly with it
///   (docs/re/NI_FILE_PARITY.md).
/// - `FUN_140cf1330`, called by BZone's private reader (curated.rs) on the member at +0x510:
///   up to 16 elements, 0x28 bytes apart, present by a bit mask. BZone's constructor
///   FUN_1407face0 builds them at complete-object slot 0xA4 (+0x520 = +0x10 + 0x510) with
///   FUN_1407fb000, which installs vtable 0x144e974f0: BZoneLevelEnv (0x2D, catalog
///   vtable 0x4e974f0), whose slot 8 is its reader FUN_140d04c10 (an inline body).
/// - `FUN_140cf10e0`, LoopArray (curated.rs): up to 8 BLoop (0x05) elements, present by a
///   bit mask; their constructor FUN_1407b0490 installs vtable 0x144e91240, whose slot 1
///   returns 0x05 and slot 8 is the inline reader FUN_140cff510.
/// - `FUN_140cefcf0`, the automation list: a u32 size the array is resized to
///   (FUN_14073c3a0), a u32 count, then each element's flag, version and body. Both callers
///   pass a BHeapArray<BAutomationObject>: BGroup's private reader at +0xD0 (BGroup's
///   constructor FUN_1407f6fc0 installs its vtable at +0xE0) and FUN_140d0a2c0 at +0x1CED0
///   (complete-object slot 0x39DC). BAutomationObject is 0x01 (catalog vtable 0x4757d28:
///   slot 3 flag 0, slot 8 its inline reader FUN_140cfec70).
const BODIES: &[(&str, u16)] = &[
    ("FUN_140d1d860", 0x50),
    ("FUN_140cf1330", 0x2d),
    ("FUN_140cf10e0", 0x05),
    ("FUN_140cefcf0", 0x01),
];

/// Nested objects whose class the stream does not name, by reader call site: (program,
/// `(rd O)` sites in IR order, SerType).
///
/// - `FUN_140d04600`, BSnapshot v1 / v2 / v3: the instrument inserts and sends (0x3A
///   BParameterArraySer<BParFX, 8>), the 16 buses read in a loop (0x45 BInsertBus), then
///   one more FX rack (v2 after the five script slots, v3 after the buses). The hand reader
///   (objects/snapshot.rs) reads exactly this order, and the corpus snapshots decode with it.
/// - `FUN_140cff070`, BGroupSnapshot v1 / v2–v3 / v4 (one branch each): the group's FX rack
///   (0x3A), its source (0x0E BParSrcMode), its internal (0x3B) and external (0x3C)
///   modulator arrays, the order objects/snapshot_group.rs reads.
/// - `FUN_140d07580`, BGroup's private reader (curated.rs), versions 0x90–0x96 (seven
///   cases): each reads the member at +0x8B0 then the one at +0xB90 of the serializable
///   subobject. The constructor FUN_1407f6fc0 installs BParameterArraySer<BParFX, 8> (0x3A)
///   at complete-object slot 0x118 (+0x8C0 = +0x10 + 0x8B0) and BParSrcMode (0x0E) at slot
///   0x174 (+0xBA0).
const OBJECTS: &[(&str, &[u16], u16)] = &[
    ("FUN_140d04600", &[0, 1, 3, 4, 6, 7, 8, 10], 0x3a),
    ("FUN_140d04600", &[2, 5, 9], 0x45),
    ("FUN_140cff070", &[0, 4, 8], 0x3a),
    ("FUN_140cff070", &[1, 5, 9], 0x0e),
    ("FUN_140cff070", &[2, 6, 10], 0x3b),
    ("FUN_140cff070", &[3, 7, 11], 0x3c),
    ("FUN_140d07580", &[0, 2, 4, 6, 8, 10, 12], 0x3a),
    ("FUN_140d07580", &[1, 3, 5, 7, 9, 11, 13], 0x0e),
];

/// Whether the reader of `sertype` reads nested objects these tables name (beyond what the
/// reference interpreter tools/re/ir_sim.py models).
pub(super) fn names_nested(sertype: u16) -> bool {
    all_readers().iter().any(|r| {
        r.sertype == sertype
            && (OBJECTS.iter().any(|(at, _, _)| *at == r.program)
                || BODIES.iter().any(|(at, _)| *at == r.program))
    })
}

/// Classes whose vtable slot 8 (the body read `(rdbody ..)` calls after the flag and version)
/// is the framed read 0x140756e50: private, public and children areas, each u32-length
/// framed, dispatched to slots 11, 10 and 12. Slot 8 of each vtable, Kontakt 8.13.1:
/// BGroup 0x04 (0x144e97ab0), BParEnv 0x07 (0x144e749f0), BZone 0x2C (0x144e97560),
/// BInsertBus 0x45 (0x144e980c0), BProgramContainer 0x29 (0x144e983f0). A class not
/// listed reads its body inline, as BGroupSnapshot (0x50, slot 8 is its reader
/// 0x140cff070).
const SLOT8_FRAMED: &[u16] = &[0x04, 0x07, 0x29, 0x2c, 0x45];

/// Virtual reads the IR leaves as `(callx ..)`, proven from the vtable the constructor
/// installs: (program, read kind).
///
/// - `FUN_140cfb900` (BParSrcMode's source reader, called on BParSrcMode+0xC8) calls slot
///   0x10 of the member's vtable. The constructor at 0x140cf395b stores vtable 0x144e975e8
///   at +0xC8; its slot 0x10 is 0x140940010, which reads one u32 (0x142a78270, read_u32)
///   into the member's +0x18.
const VIRTUAL: &[(&str, char)] = &[("FUN_140cfb900", 'I')];

/// Why a stream is not (fully) decoded. Never a guess: every variant names a fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undecoded {
    /// The stream ends inside a field.
    Truncated,
    /// Kontakt's reader rejects this stream (a validation throw, e.g. an unknown enum).
    Rejected,
    /// The object decoded but bytes remain.
    Trailing(usize),
    /// The reader reaches code this build does not model (named).
    Unresolved(String),
    /// No reader accepts this SerType/version.
    NoReader,
    /// The host-state search exceeded its budget.
    Budget,
}

/// One value read from the stream.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// F f32, B u8, H u16, I u32, Q u64, S string, W wide string, O nested object, R raw bytes.
    pub kind: char,
    /// Offset in the stream handed to `decode`.
    pub offset: usize,
    pub len: usize,
    /// Integer value (the f32 bit pattern for `F`); lengths for strings; version for `O`.
    pub value: Option<u64>,
    /// Byte offset of the field in Kontakt's in-memory object, when the reader stores it
    /// there (the stable native name of a field across stored versions).
    pub slot: Option<u32>,
    /// Nesting depth (0 = the object itself).
    pub depth: u8,
}

impl Decoded {
    /// The field Kontakt stores at byte `slot` of its in-memory object (stable across stored
    /// versions, so the native name of a field). The last read wins when a reader re-reads it.
    pub fn at(&self, slot: u32) -> Option<&Field> {
        self.fields
            .iter()
            .rev()
            .find(|f| f.slot == Some(slot) && f.depth == 0)
    }

    pub fn int_at(&self, slot: u32) -> Option<u64> {
        self.at(slot).and_then(|f| f.value)
    }

    pub fn float_at(&self, slot: u32) -> Option<f32> {
        self.at(slot).and_then(Field::as_f32)
    }
}

impl Field {
    pub fn as_f32(&self) -> Option<f32> {
        (self.kind == 'F').then(|| f32::from_bits(self.value.unwrap_or(0) as u32))
    }
}

#[derive(Debug, Clone)]
pub struct Decoded {
    pub class: &'static str,
    pub fields: Vec<Field>,
    pub consumed: usize,
    /// Bytes of nested framed objects that are consumed but not decoded, by class and cause.
    pub opaque: Vec<Opaque>,
}

impl Decoded {
    /// All opaque bytes.
    pub fn opaque_bytes(&self) -> usize {
        self.opaque.iter().map(|o| o.bytes).sum()
    }
}

/// Bytes of nested objects of one class that a decode consumed without decoding, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opaque {
    /// The nested object's SerType; `None` when neither stream nor reader names it.
    pub sertype: Option<u16>,
    pub why: &'static str,
    pub bytes: usize,
}

/// Add `bytes` of `sertype` with cause `why` to `into`, one entry per class and cause.
fn hide(into: &mut Vec<Opaque>, sertype: Option<u16>, why: &'static str, bytes: usize) {
    match into
        .iter_mut()
        .find(|o| o.sertype == sertype && o.why == why)
    {
        Some(o) => o.bytes += bytes,
        None if bytes > 0 => into.push(Opaque {
            sertype,
            why,
            bytes,
        }),
        None => {}
    }
}

/// Why a nested object did not decode, as an [`Opaque`] cause.
fn cause(e: &Undecoded) -> &'static str {
    match e {
        Undecoded::Truncated => "truncated",
        Undecoded::Rejected => "rejected",
        Undecoded::Unresolved(_) => "not modelled",
        Undecoded::Budget => "search over budget",
        Undecoded::Trailing(_) => "trailing bytes",
        Undecoded::NoReader => "no reader",
    }
}

type Val = Option<(i64, u32)>;

/// Addresses are well spread already; SipHash only slows the executor down.
#[derive(Default)]
struct AddrHasher(u64);

impl std::hash::Hasher for AddrHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn write_i64(&mut self, i: i64) {
        self.0 = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

type Mem = HashMap<i64, (u8, u32), std::hash::BuildHasherDefault<AddrHasher>>;

#[derive(Debug)]
enum Un {
    Not,
    BitNot,
    Neg,
}

#[derive(Debug, Clone, Copy)]
enum Bin {
    Add,
    Sub,
    Mul,
    Div,
    DivU,
    Mod,
    ModU,
    And,
    Or,
    Xor,
    Shl,
    Shr,
    ShrU,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Ltu,
    Leu,
    Gtu,
    Geu,
}

#[derive(Debug)]
enum Ex {
    Int(i64),
    Undef,
    Reg(usize),
    Glob(usize),
    Rd(char),
    /// A nested object read; the class when the reader's call site names it ([`OBJECTS`]).
    RdObj(Option<u16>),
    Ld(u8, bool, Box<Ex>),
    Wrap(u8, bool, Box<Ex>),
    Un(Un, Box<Ex>),
    Bin(Bin, Box<Ex>, Box<Ex>),
    Sel(Box<Ex>, Box<Ex>, Box<Ex>),
    AndAnd(Box<Ex>, Box<Ex>),
    OrOr(Box<Ex>, Box<Ex>),
}

#[derive(Debug)]
enum Op {
    Set(usize, Ex),
    St(u8, Ex, Ex),
    /// Branch; Some(d) marks a host-state site proven not to matter: undefined takes direction d.
    Br(Ex, usize, Option<bool>),
    Jmp(usize),
    RdN(Ex, Ex),
    RdBody(Ex),
    Call(usize, usize, Vec<Ex>),
    Ret(Option<Ex>),
    Throw,
    CallX(String),
}

struct Program {
    params: Vec<usize>,
    nregs: usize,
    sp: usize,
    frame: i64,
    ops: Vec<Op>,
}

enum Tok {
    Atom(String),
    List(Vec<Tok>),
}

#[derive(Debug, PartialEq, Eq)]
struct IrError(String);

impl Tok {
    fn atom(&self) -> Result<&str, IrError> {
        match self {
            Self::Atom(a) => Ok(a),
            Self::List(_) => Err(IrError("expected IR atom".into())),
        }
    }

    fn list(&self) -> Result<&[Tok], IrError> {
        match self {
            Self::List(l) => Ok(l),
            Self::Atom(_) => Err(IrError("expected IR list".into())),
        }
    }

    fn form(&self) -> Result<(&str, &[Tok]), IrError> {
        let (head, args) = self
            .list()?
            .split_first()
            .ok_or_else(|| IrError("empty IR form".into()))?;
        Ok((head.atom()?, args))
    }
}

fn parse_toks(text: &str) -> Result<Vec<Tok>, IrError> {
    let mut stack: Vec<Vec<Tok>> = vec![Vec::new()];
    let mut atom = String::new();
    let flush = |atom: &mut String, stack: &mut Vec<Vec<Tok>>| -> Result<(), IrError> {
        if !atom.is_empty() {
            stack
                .last_mut()
                .ok_or_else(|| IrError("missing IR root".into()))?
                .push(Tok::Atom(std::mem::take(atom)));
        }
        Ok(())
    };
    for c in text.chars() {
        match c {
            '(' => {
                flush(&mut atom, &mut stack)?;
                stack.push(Vec::new());
            }
            ')' => {
                flush(&mut atom, &mut stack)?;
                if stack.len() == 1 {
                    return Err(IrError("unmatched IR closing parenthesis".into()));
                }
                let done = stack
                    .pop()
                    .ok_or_else(|| IrError("missing IR list".into()))?;
                stack
                    .last_mut()
                    .ok_or_else(|| IrError("missing IR root".into()))?
                    .push(Tok::List(done));
            }
            c if c.is_whitespace() => flush(&mut atom, &mut stack)?,
            c => atom.push(c),
        }
    }
    flush(&mut atom, &mut stack)?;
    if stack.len() != 1 {
        return Err(IrError("unclosed IR parenthesis".into()));
    }
    stack.pop().ok_or_else(|| IrError("missing IR root".into()))
}

struct Builder<'a> {
    regs: HashMap<String, usize>,
    labels: HashMap<String, usize>,
    progs: &'a HashMap<&'static str, usize>,
    /// The program being built and how many `(rd O)` sites it has so far.
    name: &'static str,
    objects: u16,
}

impl Builder<'_> {
    /// The next `(rd O)` site of this program, with its class when [`OBJECTS`] names it.
    fn rd_obj(&mut self) -> Ex {
        let site = self.objects;
        self.objects += 1;
        Ex::RdObj(
            OBJECTS
                .iter()
                .find(|(at, sites, _)| *at == self.name && sites.contains(&site))
                .map(|&(_, _, sertype)| sertype),
        )
    }

    /// A `(callx ..)` of this program: the read [`VIRTUAL`] proves, else a stop.
    fn callx(&mut self, why: String) -> Op {
        match VIRTUAL.iter().find(|(at, _)| *at == self.name) {
            Some(&(_, kind)) => Op::Set(self.reg("virtual read"), Ex::Rd(kind)),
            None => Op::CallX(why),
        }
    }

    fn reg(&mut self, name: &str) -> usize {
        let n = self.regs.len();
        *self.regs.entry(name.to_string()).or_insert(n)
    }

    fn ex(&mut self, t: &Tok) -> Result<Ex, IrError> {
        match t {
            Tok::Atom(a) => Ok(match a.as_str() {
                "?" => Ex::Undef,
                "g_cls" => Ex::Glob(0),
                "g_curver" => Ex::Glob(1),
                a => match a.parse::<i64>() {
                    Ok(n) => Ex::Int(n),
                    Err(_) => Ex::Reg(self.reg(a)),
                },
            }),
            Tok::List(_) => {
                let (h, args) = t.form()?;
                Ok(match (h, args) {
                    ("rd", [k]) => {
                        let kind = match k.atom()? {
                            "F" => 'F',
                            "B" => 'B',
                            "H" => 'H',
                            "I" => 'I',
                            "Q" => 'Q',
                            "S" => 'S',
                            "W" => 'W',
                            "N" => 'N',
                            "O" => return Ok(self.rd_obj()),
                            other => return Err(IrError(format!("unknown IR read kind {other}"))),
                        };
                        Ex::Rd(kind)
                    }
                    ("sel", [c, a, b]) => Ex::Sel(
                        Box::new(self.ex(c)?),
                        Box::new(self.ex(a)?),
                        Box::new(self.ex(b)?),
                    ),
                    ("&&", [a, b]) => Ex::AndAnd(Box::new(self.ex(a)?), Box::new(self.ex(b)?)),
                    ("||", [a, b]) => Ex::OrOr(Box::new(self.ex(a)?), Box::new(self.ex(b)?)),
                    ("!", [a]) => Ex::Un(Un::Not, Box::new(self.ex(a)?)),
                    ("~", [a]) => Ex::Un(Un::BitNot, Box::new(self.ex(a)?)),
                    ("neg", [a]) => Ex::Un(Un::Neg, Box::new(self.ex(a)?)),
                    ("u8" | "s8" | "u16" | "s16" | "u32" | "s32", [a]) => {
                        let bits = match h {
                            "u8" | "s8" => 8,
                            "u16" | "s16" => 16,
                            _ => 32,
                        };
                        Ex::Wrap(bits, h.starts_with('s'), Box::new(self.ex(a)?))
                    }
                    (
                        "ld1" | "ld1u" | "ld1s" | "ld2" | "ld2u" | "ld2s" | "ld4" | "ld4u" | "ld4s"
                        | "ld8" | "ld8u" | "ld8s",
                        [a],
                    ) => {
                        let width = match h {
                            "ld1" | "ld1u" | "ld1s" => 1,
                            "ld2" | "ld2u" | "ld2s" => 2,
                            "ld4" | "ld4u" | "ld4s" => 4,
                            _ => 8,
                        };
                        Ex::Ld(width, h.ends_with('s'), Box::new(self.ex(a)?))
                    }
                    (h, [a, b]) => {
                        let op = match h {
                            "+" => Bin::Add,
                            "-" => Bin::Sub,
                            "*" => Bin::Mul,
                            "div" => Bin::Div,
                            "divu" => Bin::DivU,
                            "mod" => Bin::Mod,
                            "modu" => Bin::ModU,
                            "&" => Bin::And,
                            "|" => Bin::Or,
                            "^" => Bin::Xor,
                            "<<" => Bin::Shl,
                            ">>" => Bin::Shr,
                            "shru" => Bin::ShrU,
                            "==" => Bin::Eq,
                            "!=" => Bin::Ne,
                            "<" => Bin::Lt,
                            "<=" => Bin::Le,
                            ">" => Bin::Gt,
                            ">=" => Bin::Ge,
                            "ltu" => Bin::Ltu,
                            "leu" => Bin::Leu,
                            "gtu" => Bin::Gtu,
                            "geu" => Bin::Geu,
                            other => {
                                return Err(IrError(format!("unknown IR binary operator {other}")))
                            }
                        };
                        Ex::Bin(op, Box::new(self.ex(a)?), Box::new(self.ex(b)?))
                    }
                    _ => {
                        return Err(IrError(format!(
                            "invalid IR expression {h} with {} arguments",
                            args.len()
                        )))
                    }
                })
            }
        }
    }

    fn label(&self, t: &Tok) -> Result<usize, IrError> {
        let name = t.atom()?;
        self.labels
            .get(name)
            .copied()
            .ok_or_else(|| IrError(format!("unknown IR label {name}")))
    }
}

fn build(src: &ProgramSrc, progs: &HashMap<&'static str, usize>) -> Result<Program, IrError> {
    let mut b = Builder {
        regs: HashMap::new(),
        labels: HashMap::new(),
        progs,
        name: src.name,
        objects: 0,
    };
    let params: Vec<usize> = src.params.iter().map(|p| b.reg(p)).collect();
    let sp = b.reg("sp");
    let toks = parse_toks(src.ops)?;
    let mut n = 0;
    for t in &toks {
        match t.form()? {
            ("label", [name]) => {
                let name = name.atom()?;
                if b.labels.insert(name.to_owned(), n).is_some() {
                    return Err(IrError(format!("duplicate IR label {name}")));
                }
            }
            ("label", _) => return Err(IrError("invalid IR label".into())),
            _ => n += 1,
        }
    }
    let mut ops = Vec::with_capacity(n);
    for t in &toks {
        let (h, args) = t.form()?;
        let op = match (h, args) {
            ("label", [_]) => continue,
            ("set", [r, e]) => Op::Set(b.reg(r.atom()?), b.ex(e)?),
            ("st", [width, a, v]) => {
                let width = match width.atom()? {
                    "1" => 1,
                    "2" => 2,
                    "4" => 4,
                    "8" => 8,
                    other => return Err(IrError(format!("invalid IR store width {other}"))),
                };
                Op::St(width, b.ex(a)?, b.ex(v)?)
            }
            ("br" | "brb" | "brt", [e, target]) => {
                let fixed = match h {
                    "brb" => Some(false),
                    "brt" => Some(true),
                    _ => None,
                };
                Op::Br(b.ex(e)?, b.label(target)?, fixed)
            }
            ("jmp", [target]) => Op::Jmp(b.label(target)?),
            ("rdn", [a, n]) => Op::RdN(b.ex(a)?, b.ex(n)?),
            ("rdbody", [v]) => Op::RdBody(b.ex(v)?),
            ("call", [callee, dst, args]) => {
                let name = callee.atom()?;
                let prog = b
                    .progs
                    .get(name)
                    .copied()
                    .ok_or_else(|| IrError(format!("unknown IR program {name}")))?;
                let dst = b.reg(dst.atom()?);
                let args = args
                    .list()?
                    .iter()
                    .map(|a| b.ex(a))
                    .collect::<Result<_, _>>()?;
                Op::Call(prog, dst, args)
            }
            ("ret", []) => Op::Ret(None),
            ("ret", [e]) => Op::Ret(Some(b.ex(e)?)),
            ("throw", []) => Op::Throw,
            ("throw", [why]) => {
                why.atom()?;
                Op::Throw
            }
            ("callx", [why]) => b.callx(why.atom()?.to_owned()),
            _ => {
                return Err(IrError(format!(
                    "invalid IR instruction {h} with {} arguments",
                    args.len()
                )))
            }
        };
        ops.push(op);
    }
    Ok(Program {
        params,
        nregs: b.regs.len(),
        sp,
        frame: i64::from(src.frame),
        ops,
    })
}

struct Library {
    programs: Vec<Program>,
    index: HashMap<&'static str, usize>,
}

fn library() -> Result<&'static Library, &'static IrError> {
    static LIB: OnceLock<Result<Library, IrError>> = OnceLock::new();
    LIB.get_or_init(|| {
        let index: HashMap<&'static str, usize> = all_programs()
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name, i))
            .collect();
        let programs = all_programs()
            .iter()
            .map(|p| build(p, &index).map_err(|IrError(why)| IrError(format!("{}: {why}", p.name))))
            .collect::<Result<_, _>>()?;
        Ok(Library { programs, index })
    })
    .as_ref()
}

const OBJ: i64 = 0x1000_0000;
const NESTED: i64 = 0x10_0000;
const STREAM: i64 = 0x2000_0000;
const CTX: i64 = 0x3000_0000;
const TABLE: i64 = 0x4000_0000;
const STACK: i64 = 0x7000_0000_0000;

enum Stop {
    Short,
    Throw,
    /// Path-dependent gap (unknown nested class, unknown length).
    Unresolved(String),
    /// Kontakt reads through virtual dispatch this build does not model; no other path helps.
    Virtual(String),
    Budget,
}

/// Recorded choices for the host-state search.
#[derive(Default)]
struct Chooser {
    prefix: Vec<usize>,
    taken: Vec<usize>,
    sizes: Vec<usize>,
}

impl Chooser {
    fn pick(&mut self, n: usize) -> usize {
        let i = self.taken.len();
        let c = self.prefix.get(i).copied().unwrap_or(0);
        self.taken.push(c);
        self.sizes.push(n);
        c
    }
}

struct Stream<'a> {
    data: Option<&'a [u8]>,
    out: Vec<u8>,
    pos: usize,
    base: usize,
}

impl<'a> Stream<'a> {
    fn decode(data: &'a [u8], base: usize) -> Self {
        Self {
            data: Some(data),
            out: Vec::new(),
            pos: 0,
            base,
        }
    }
    fn synth() -> Self {
        Self {
            data: None,
            out: Vec::new(),
            pos: 0,
            base: 0,
        }
    }
}

struct Machine<'a> {
    lib: &'static Library,
    stream: Stream<'a>,
    chooser: Chooser,
    values: &'a [i64],
    mem: Mem,
    fields: Vec<Field>,
    depth: u8,
    steps: u32,
    next_sp: i64,
    nobj: i64,
    globals: [Val; 2],
    opaque: Vec<Opaque>,
}

fn wrap(v: i64, bits: u8, signed: bool) -> i64 {
    let m = (1i64 << bits) - 1;
    let u = v & m;
    if signed && (u >> (bits - 1)) & 1 == 1 {
        u - (1i64 << bits)
    } else {
        u
    }
}

impl<'a> Machine<'a> {
    fn new(
        lib: &'static Library,
        stream: Stream<'a>,
        chooser: Chooser,
        values: &'a [i64],
        class: Option<u16>,
        steps: u32,
    ) -> Self {
        let mut m = Self {
            lib,
            stream,
            chooser,
            values,
            mem: Mem::default(),
            fields: Vec::new(),
            depth: 0,
            steps,
            next_sp: STACK,
            nobj: 0,
            globals: [class.map(|c| (i64::from(c), 0)), None],
            opaque: Vec::new(),
        };
        m.store(STREAM + 0x10, 4, Some((1, 0))); // byte-order flag Kontakt tests: 1 = no swap
        m.store(STREAM + 8, 8, Some((STREAM + 0x100, 0)));
        m.store(STREAM + 0x100 + 0xC, 4, Some((1, 0))); // healthy stream state
        m.store(CTX + 0x280, 8, Some((TABLE, 0))); // instrument tables: large, so any index is in range
        m.store(CTX + 0x288, 8, Some((TABLE + 0x380 * (1 << 20), 0)));
        m
    }

    fn store(&mut self, addr: i64, w: u8, v: Val) {
        for i in 0..i64::from(w) {
            match v {
                None => {
                    self.mem.remove(&(addr + i));
                }
                Some((x, src)) => {
                    self.mem
                        .insert(addr + i, (((x >> (8 * i)) & 0xff) as u8, src));
                }
            }
        }
        if let Some((_, src)) = v {
            if src != 0 && (OBJ..OBJ + NESTED).contains(&addr) {
                let f = &mut self.fields[src as usize - 1];
                if f.slot.is_none() {
                    f.slot = Some((addr - OBJ) as u32);
                }
            }
        }
    }

    fn load(&self, addr: i64, w: u8, signed: bool) -> Val {
        let mut v: u64 = 0;
        let mut src = None;
        for i in 0..i64::from(w) {
            let (b, s) = *self.mem.get(&(addr + i))?;
            v |= u64::from(b) << (8 * i);
            src = match src {
                None => Some(s),
                Some(x) if x == s => Some(x),
                _ => Some(0),
            };
        }
        let x = if w >= 8 {
            v as i64
        } else {
            wrap(v as i64, w * 8, signed)
        };
        Some((x, src.unwrap_or(0)))
    }

    fn take(&mut self, n: usize, int: bool) -> Result<Vec<u8>, Stop> {
        match self.stream.data {
            Some(d) => {
                let end = self
                    .stream
                    .pos
                    .checked_add(n)
                    .filter(|&e| e <= d.len())
                    .ok_or(Stop::Short)?;
                let b = d[self.stream.pos..end].to_vec();
                self.stream.pos = end;
                Ok(b)
            }
            None => {
                if self.stream.pos > 1 << 14 {
                    return Err(Stop::Budget);
                }
                let b = if int && n <= 8 {
                    let v = self.values[self.chooser.pick(self.values.len())];
                    v.to_le_bytes()[..n].to_vec()
                } else {
                    vec![0; n]
                };
                self.stream.out.extend_from_slice(&b);
                self.stream.pos += n;
                Ok(b)
            }
        }
    }

    fn record(&mut self, kind: char, start: usize, len: usize, value: Option<u64>) -> u32 {
        self.fields.push(Field {
            kind,
            offset: self.stream.base + start,
            len,
            value,
            slot: None,
            depth: self.depth,
        });
        self.fields.len() as u32
    }

    fn read(&mut self, kind: char) -> Result<Val, Stop> {
        let start = self.stream.pos;
        match kind {
            'F' | 'B' | 'H' | 'I' | 'Q' => {
                let n = match kind {
                    'F' | 'I' => 4,
                    'B' => 1,
                    'H' => 2,
                    _ => 8,
                };
                let b = self.take(n, kind != 'F')?;
                let mut a = [0u8; 8];
                a[..n].copy_from_slice(&b);
                let v = u64::from_le_bytes(a);
                let id = self.record(kind, start, n, Some(v));
                Ok(Some((v as i64, id)))
            }
            'S' | 'W' | 'N' => {
                let b = self.take(4, true)?;
                let len_bytes: [u8; 4] = b
                    .get(..4)
                    .and_then(|s| s.try_into().ok())
                    .ok_or(Stop::Throw)?;
                // N (FUN_140743010): a negative i32 length is a null string and reads nothing.
                if kind == 'N' && i32::from_le_bytes(len_bytes) < 0 {
                    self.record(kind, start, 4, Some(0));
                    return Ok(None);
                }
                let n = u32::from_le_bytes(len_bytes) as usize;
                // Kontakt's string reads take any length; a decoded stream bounds it. Only a
                // synthesized stream needs a cap, so a chosen length allocates nothing absurd.
                if n > 1 << 24 && self.stream.data.is_none() {
                    return Err(Stop::Throw);
                }
                self.take(n * if kind == 'W' { 2 } else { 1 }, false)?;
                self.record(kind, start, self.stream.pos - start, Some(n as u64));
                Ok(None)
            }
            'O' => self.read_object(start, None).map(|_| None),
            _ => Err(Stop::Unresolved(format!("read kind {kind}"))),
        }
    }

    fn class_reader(&self) -> Option<(usize, u16, u16)> {
        let (cls, _) = self.globals[0]?;
        self.reader_of(u16::try_from(cls).ok()?)
    }

    fn reader_of(&self, sertype: u16) -> Option<(usize, u16, u16)> {
        let r = all_readers()
            .iter()
            .find(|r| r.sertype == sertype && !r.program.is_empty())?;
        Some((*self.lib.index.get(r.program)?, r.sertype, r.current))
    }

    fn read_object(&mut self, start: usize, named: Option<u16>) -> Result<(), Stop> {
        let flag = self.take(1, true)?[0];
        if flag > 1 {
            return Err(Stop::Throw);
        }
        let ver_bytes: [u8; 2] = self
            .take(2, true)?
            .get(..2)
            .and_then(|s| s.try_into().ok())
            .ok_or(Stop::Throw)?;
        let ver = u16::from_le_bytes(ver_bytes);
        let reader = match named {
            Some(sertype) => self.reader_of(sertype),
            None => self.class_reader(),
        };
        if let Some((_, _, current)) = reader {
            if ver > current {
                return Err(Stop::Throw);
            }
        }
        if flag == 0 {
            let (prog, _, _) = reader.ok_or_else(|| {
                Stop::Unresolved("unframed nested object of unknown class".into())
            })?;
            self.record('O', start, 3, Some(u64::from(ver)));
            return self.run_named(prog, ver, named);
        }
        self.areas(start, ver, reader, named)
    }

    /// The three length-framed areas (private, public, children) of an object at `start`
    /// whose flag and version are read: Kontakt's framed read, vtable slot 8 0x140756e50.
    fn areas(
        &mut self,
        start: usize,
        ver: u16,
        reader: Option<(usize, u16, u16)>,
        named: Option<u16>,
    ) -> Result<(), Stop> {
        let id = self.record('O', start, 0, Some(u64::from(ver)));
        if let Some(data) = self.stream.data {
            let mut areas = Vec::new();
            for _ in 0..3 {
                let len_bytes: [u8; 4] = self
                    .take(4, false)?
                    .get(..4)
                    .and_then(|s| s.try_into().ok())
                    .ok_or(Stop::Throw)?;
                let n = u32::from_le_bytes(len_bytes) as usize;
                let begin = self.stream.pos;
                self.take(n, false)?;
                areas.push((begin, n));
            }
            match reader {
                None => hide(
                    &mut self.opaque,
                    named,
                    if named.is_some() {
                        "no reader"
                    } else {
                        "class not named"
                    },
                    areas.iter().map(|&(_, n)| n).sum::<usize>(),
                ),
                Some((_, sertype, _)) => {
                    let area = |(begin, n): (usize, usize)| {
                        data.get(begin..begin.saturating_add(n)).ok_or(Stop::Throw)
                    };
                    let (private, public, children) = (areas[0], areas[1], areas[2]);
                    self.framed(
                        sertype,
                        ver,
                        (area(private)?, private.0),
                        (area(public)?, public.0),
                    )?;
                    self.children(area(children)?, children.0)?;
                }
            }
        } else {
            // synthesize: empty private and children areas around the body
            self.take(4, false)?;
            let mut body = Stream::synth();
            body.base = self.stream.pos + 4;
            if let Some((prog, _, _)) = reader {
                std::mem::swap(&mut self.stream, &mut body);
                self.depth += 1;
                let r = self.run_named(prog, ver, named);
                self.depth -= 1;
                std::mem::swap(&mut self.stream, &mut body);
                r?;
            }
            self.stream
                .out
                .extend_from_slice(&(body.out.len() as u32).to_le_bytes());
            self.stream.out.extend_from_slice(&body.out);
            self.stream.out.extend_from_slice(&[0; 4]);
            self.stream.pos += 8 + body.out.len();
        }
        self.fields[id as usize - 1].len = self.stream.pos - start;
        Ok(())
    }

    /// A length-framed object of class `sertype`: its own search, as a top-level object, so
    /// host-state choices of sibling objects do not multiply; what it does not decode is
    /// opaque. Kontakt reads the private and
    /// public areas with separate readers (docs/re/NI_FILE_BINARY_RECORDS.md §2) and a class
    /// keeps its fields in one of them: the catalog reader is that one's.
    fn framed(
        &mut self,
        sertype: u16,
        ver: u16,
        private: (&[u8], usize),
        public: (&[u8], usize),
    ) -> Result<(), Stop> {
        if has_private_reader(sertype) {
            self.block(sertype, ver, true, private)?;
            return self.block(sertype, ver, false, public);
        }
        let (body, at) = if public.0.is_empty() && !private.0.is_empty() {
            private
        } else {
            hide(
                &mut self.opaque,
                Some(sertype),
                "second block",
                private.0.len(),
            );
            public
        };
        self.block(
            sertype,
            ver,
            at == private.1 && !private.0.is_empty(),
            (body, at),
        )
    }

    /// One block of a framed object, decoded with its own search; what it does not decode is
    /// opaque. Its framing bounds it, so that is opaque bytes for the census, not a stop for
    /// the parent's search (which would replay it).
    fn block(
        &mut self,
        sertype: u16,
        ver: u16,
        private: bool,
        (body, at): (&[u8], usize),
    ) -> Result<(), Stop> {
        if body.is_empty() {
            return Ok(());
        }
        match nested_decode(sertype, ver, private, body) {
            Ok(d) => self.merge(d, at),
            Err(e) => hide(&mut self.opaque, Some(sertype), cause(&e), body.len()),
        }
        Ok(())
    }

    /// A nested decode of the bytes at `at` of this stream, as fields one level deeper.
    fn merge(&mut self, d: Decoded, at: usize) {
        let (base, depth) = (self.stream.base + at, self.depth + 1);
        for o in &d.opaque {
            hide(&mut self.opaque, o.sertype, o.why, o.bytes);
        }
        self.fields.extend(d.fields.into_iter().map(|f| Field {
            offset: f.offset + base,
            depth: f.depth + depth,
            ..f
        }));
    }

    /// A framed object's child chunks (u16 SerType, u32 length, body): each decodes as its
    /// SerType names it, or counts as opaque.
    fn children(&mut self, mut bytes: &[u8], mut at: usize) -> Result<(), Stop> {
        while !bytes.is_empty() {
            let head = bytes.get(..6).ok_or(Stop::Throw)?;
            let id = u16::from_le_bytes([head[0], head[1]]);
            let n = u32::from_le_bytes([head[2], head[3], head[4], head[5]]) as usize;
            let body = bytes.get(6..6usize.saturating_add(n)).ok_or(Stop::Throw)?;
            self.chunk(id, body, at + 6);
            bytes = &bytes[6 + n..];
            at += 6 + n;
        }
        Ok(())
    }

    /// One child chunk: a failure to decode it is opaque bytes, not a stop for its parent
    /// (the parent's reader does not read it).
    fn chunk(&mut self, id: u16, body: &[u8], at: usize) {
        let opaque = body.len();
        // A record Kontakt reads from its chunk header on (curated.rs CURATED_UNFRAMED).
        if CURATED_UNFRAMED.iter().any(|r| r.sertype == id) {
            match decode_unframed(id, body) {
                Ok(d) => self.merge(d, at - 6),
                Err(e) => hide(&mut self.opaque, Some(id), cause(&e), opaque),
            }
            return;
        }
        let parsed = (|| {
            const SHORT: &str = "not object-framed";
            let (&flag, rest) = body.split_first().ok_or(SHORT)?;
            let ver = u16::from_le_bytes([*rest.first().ok_or(SHORT)?, *rest.get(1).ok_or(SHORT)?]);
            if flag > 1 || !accepts(id, ver) {
                return Err(if flag > 1 {
                    "not object-framed"
                } else {
                    "no reader"
                });
            }
            let rest = &rest[2..];
            if flag == 0 {
                return Ok((
                    ver,
                    (&[][..], at + 3),
                    (rest, at + 3),
                    (&[][..], at + 3 + rest.len()),
                ));
            }
            let mut areas = Vec::with_capacity(3);
            let mut off = 0;
            for _ in 0..3 {
                let n = rest
                    .get(off..off + 4)
                    .and_then(|b| b.try_into().ok())
                    .map(u32::from_le_bytes)
                    .ok_or(SHORT)? as usize;
                let area = (off + 4)
                    .checked_add(n)
                    .and_then(|end| rest.get(off + 4..end))
                    .ok_or(SHORT)?;
                areas.push((area, at + 3 + off + 4));
                off += 4 + n;
            }
            if off == rest.len() {
                Ok((ver, areas[0], areas[1], areas[2]))
            } else {
                Err(SHORT)
            }
        })();
        let (ver, private, public, children) = match parsed {
            Ok(p) => p,
            Err(why) => return hide(&mut self.opaque, Some(id), why, opaque),
        };
        let (fields, extra) = (self.fields.len(), self.opaque.clone());
        if self.framed(id, ver, private, public).is_err()
            || self.children(children.0, children.1).is_err()
        {
            self.fields.truncate(fields);
            self.opaque = extra;
            hide(&mut self.opaque, Some(id), "child framing", opaque);
        }
    }

    /// [`Self::run_nested`]; a reader named by its call site reads its own elements.
    fn run_named(&mut self, prog: usize, ver: u16, named: Option<u16>) -> Result<(), Stop> {
        let Some(sertype) = named else {
            return self.run_nested(prog, ver);
        };
        let element = all_readers()
            .iter()
            .find(|r| r.sertype == sertype)
            .and_then(|r| element_of(r))
            .map(|c| (i64::from(c), 0));
        let outer = std::mem::replace(&mut self.globals[0], element);
        let r = self.run_nested(prog, ver);
        self.globals[0] = outer;
        r
    }

    fn run_nested(&mut self, prog: usize, ver: u16) -> Result<(), Stop> {
        self.nobj += 1;
        let base = OBJ + NESTED * self.nobj;
        self.call(
            prog,
            &[
                Some((base, 0)),
                Some((STREAM, 0)),
                Some((i64::from(ver), 0)),
                Some((CTX, 0)),
            ],
            1,
        )?;
        Ok(())
    }

    fn ev(&mut self, e: &Ex, regs: &[Val]) -> Result<Val, Stop> {
        Ok(match e {
            Ex::Int(i) => Some((*i, 0)),
            Ex::Undef => None,
            Ex::Reg(r) => regs[*r],
            Ex::Glob(g) => self.globals[*g],
            Ex::Rd(k) => self.read(*k)?,
            Ex::RdObj(class) => {
                let start = self.stream.pos;
                self.read_object(start, *class)?;
                None
            }
            Ex::Ld(w, s, a) => match self.ev(a, regs)? {
                Some((addr, _)) => self.load(addr, *w, *s),
                None => None,
            },
            Ex::Wrap(bits, s, a) => self.ev(a, regs)?.map(|(v, src)| (wrap(v, *bits, *s), src)),
            Ex::Un(op, a) => self.ev(a, regs)?.map(|(v, _)| {
                (
                    match op {
                        Un::Not => i64::from(v == 0),
                        Un::BitNot => !v,
                        Un::Neg => v.wrapping_neg(),
                    },
                    0,
                )
            }),
            Ex::Sel(c, a, b) => match self.ev(c, regs)? {
                Some((c, _)) => self.ev(if c != 0 { a } else { b }, regs)?,
                None => None,
            },
            Ex::AndAnd(a, b) => {
                let x = self.ev(a, regs)?;
                if matches!(x, Some((0, _))) {
                    return Ok(Some((0, 0)));
                }
                let y = self.ev(b, regs)?;
                match (x, y) {
                    (_, Some((0, _))) => Some((0, 0)),
                    (Some(_), Some(_)) => Some((1, 0)),
                    _ => None,
                }
            }
            Ex::OrOr(a, b) => {
                let x = self.ev(a, regs)?;
                if matches!(x, Some((v, _)) if v != 0) {
                    return Ok(Some((1, 0)));
                }
                let y = self.ev(b, regs)?;
                match (x, y) {
                    (_, Some((v, _))) if v != 0 => Some((1, 0)),
                    (Some(_), Some(_)) => Some((0, 0)),
                    _ => None,
                }
            }
            Ex::Bin(op, a, b) => {
                let (Some((x, _)), Some((y, _))) = (self.ev(a, regs)?, self.ev(b, regs)?) else {
                    return Ok(None);
                };
                let (ux, uy) = (x as u64, y as u64);
                let v = match op {
                    Bin::Add => x.wrapping_add(y),
                    Bin::Sub => x.wrapping_sub(y),
                    Bin::Mul => x.wrapping_mul(y),
                    Bin::Div => return Ok(x.checked_div(y).map(|v| (v, 0))),
                    Bin::DivU => return Ok(ux.checked_div(uy).map(|v| (v as i64, 0))),
                    Bin::Mod => return Ok(x.checked_rem(y).map(|v| (v, 0))),
                    Bin::ModU => return Ok(ux.checked_rem(uy).map(|v| (v as i64, 0))),
                    Bin::And => x & y,
                    Bin::Or => x | y,
                    Bin::Xor => x ^ y,
                    Bin::Shl => {
                        if (0..64).contains(&y) {
                            x << y
                        } else {
                            0
                        }
                    }
                    Bin::Shr => {
                        if (0..64).contains(&y) {
                            x >> y
                        } else if x >= 0 {
                            0
                        } else {
                            -1
                        }
                    }
                    Bin::ShrU => {
                        if (0..64).contains(&y) {
                            (ux >> y) as i64
                        } else {
                            0
                        }
                    }
                    Bin::Eq => i64::from(x == y),
                    Bin::Ne => i64::from(x != y),
                    Bin::Lt => i64::from(x < y),
                    Bin::Le => i64::from(x <= y),
                    Bin::Gt => i64::from(x > y),
                    Bin::Ge => i64::from(x >= y),
                    Bin::Ltu => i64::from(ux < uy),
                    Bin::Leu => i64::from(ux <= uy),
                    Bin::Gtu => i64::from(ux > uy),
                    Bin::Geu => i64::from(ux >= uy),
                };
                Some((v, 0))
            }
        })
    }

    /// Runs `prog`; an unresolved read names the innermost program it stops in.
    fn call(&mut self, prog: usize, args: &[Val], depth: u32) -> Result<Val, Stop> {
        self.call_body(prog, args, depth).map_err(|e| match e {
            Stop::Unresolved(w) if !w.contains(" in FUN_") => {
                let at = all_programs().get(prog).map_or("?", |p| p.name);
                Stop::Unresolved(format!("{w} in {at}"))
            }
            e => e,
        })
    }

    fn call_body(&mut self, prog: usize, args: &[Val], depth: u32) -> Result<Val, Stop> {
        if depth > 24 {
            return Err(Stop::Unresolved("reader recursion".into()));
        }
        let lib = self.lib;
        let p = &lib.programs[prog];
        let mut regs: Vec<Val> = vec![None; p.nregs];
        for (r, a) in p.params.iter().zip(args) {
            regs[*r] = *a;
        }
        regs[p.sp] = Some((self.next_sp, 0));
        self.next_sp += p.frame.max(16) + 0x1000;
        let mut pc = 0;
        while pc < p.ops.len() {
            if self.steps == 0 {
                return Err(Stop::Budget);
            }
            self.steps -= 1;
            let op = &p.ops[pc];
            pc += 1;
            match op {
                Op::Set(r, e) => regs[*r] = self.ev(e, &regs)?,
                Op::St(w, a, v) => {
                    let (addr, val) = (self.ev(a, &regs)?, self.ev(v, &regs)?);
                    if let Some((addr, _)) = addr {
                        self.store(addr, *w, val);
                    }
                }
                Op::Br(c, target, fixed) => {
                    let taken = match self.ev(c, &regs)? {
                        Some((v, _)) => v != 0,
                        None => fixed.unwrap_or_else(|| self.chooser.pick(2) == 1),
                    };
                    if taken {
                        pc = *target;
                    }
                }
                Op::Jmp(t) => pc = *t,
                Op::RdN(_, n) => match self.ev(n, &regs)? {
                    Some((n, _)) if (0..1 << 26).contains(&n) => {
                        let start = self.stream.pos;
                        self.take(n as usize, false)?;
                        self.record('R', start, n as usize, None);
                    }
                    _ => return Err(Stop::Unresolved("raw read of unknown length".into())),
                },
                Op::RdBody(v) => {
                    let ver = self.ev(v, &regs)?;
                    let named = BODIES
                        .iter()
                        .find(|(at, _)| all_programs().get(prog).is_some_and(|q| q.name == *at))
                        .map(|&(_, sertype)| sertype);
                    let reader = match named {
                        Some(sertype) => self.reader_of(sertype),
                        None => self.class_reader(),
                    };
                    let (prog, sertype, _) = reader.ok_or_else(|| {
                        Stop::Unresolved("embedded object of unknown class".into())
                    })?;
                    let Some((ver, _)) = ver else {
                        return Err(Stop::Unresolved("embedded object version".into()));
                    };
                    if SLOT8_FRAMED.contains(&sertype) {
                        self.areas(self.stream.pos, ver as u16, reader, named)?;
                    } else {
                        self.run_named(prog, ver as u16, named)?;
                    }
                }
                Op::Call(q, dst, a) => {
                    let mut vals = Vec::with_capacity(a.len());
                    for e in a {
                        vals.push(self.ev(e, &regs)?);
                    }
                    regs[*dst] = self.call(*q, &vals, depth + 1)?;
                }
                Op::Ret(e) => {
                    return match e {
                        Some(e) => self.ev(e, &regs),
                        None => Ok(None),
                    }
                }
                Op::Throw => return Err(Stop::Throw),
                Op::CallX(why) => return Err(Stop::Virtual(why.clone())),
            }
        }
        Ok(None)
    }
}

/// Reader table rows for one SerType.
pub(super) fn readers_for(sertype: u16) -> impl Iterator<Item = &'static ReaderSrc> {
    all_readers()
        .iter()
        .copied()
        .filter(move |r| r.sertype == sertype)
}

/// The private-block reader curated.rs holds for `sertype`, else its catalog reader.
fn private_reader_for(sertype: u16, version: u16) -> Option<&'static ReaderSrc> {
    CURATED_PRIVATE
        .iter()
        .find(|r| {
            r.sertype == sertype && r.versions.iter().any(|&(a, b)| (a..=b).contains(&version))
        })
        .or_else(|| reader_for(sertype, version))
}

/// Whether curated.rs holds a separate private-block reader for `sertype`.
fn has_private_reader(sertype: u16) -> bool {
    CURATED_PRIVATE.iter().any(|r| r.sertype == sertype)
}

fn reader_for(sertype: u16, version: u16) -> Option<&'static ReaderSrc> {
    readers_for(sertype).find(|r| {
        !r.program.is_empty() && r.versions.iter().any(|&(a, b)| (a..=b).contains(&version))
    })
}

fn element_of(r: &ReaderSrc) -> Option<u16> {
    (r.element != 0).then_some(r.element)
}

struct Outcome {
    stop: Option<Stop>,
    fields: Vec<Field>,
    consumed: usize,
    opaque: Vec<Opaque>,
    out: Vec<u8>,
    chooser: Chooser,
}

fn attempt(
    r: &'static ReaderSrc,
    version: u16,
    stream: Stream<'_>,
    prefix: Vec<usize>,
    values: &[i64],
) -> Outcome {
    let chooser = Chooser {
        prefix,
        ..Chooser::default()
    };
    let lib = match library() {
        Ok(lib) => lib,
        Err(IrError(why)) => {
            return Outcome {
                stop: Some(Stop::Virtual(format!("bundled reader IR: {why}"))),
                fields: Vec::new(),
                consumed: 0,
                opaque: Vec::new(),
                out: Vec::new(),
                chooser,
            }
        }
    };
    let Some(&prog) = lib.index.get(r.program) else {
        return Outcome {
            stop: Some(Stop::Virtual(format!(
                "bundled reader IR: missing program {}",
                r.program
            ))),
            fields: Vec::new(),
            consumed: 0,
            opaque: Vec::new(),
            out: Vec::new(),
            chooser,
        };
    };
    // A reader does bounded work per byte it reads; a path that spins on host state does not.
    let steps = match stream.data {
        Some(d) => 4_000u32.saturating_add(
            u32::try_from(d.len())
                .unwrap_or(u32::MAX)
                .saturating_mul(600),
        ),
        None => 120_000,
    };
    let mut m = Machine::new(lib, stream, chooser, values, element_of(r), steps);
    let args = [
        Some((OBJ, 0)),
        Some((STREAM, 0)),
        Some((i64::from(version), 0)),
        Some((CTX, 0)),
    ];
    let stop = m.call(prog, &args, 0).err();
    Outcome {
        stop,
        fields: m.fields,
        consumed: m.stream.pos,
        opaque: m.opaque,
        out: m.stream.out,
        chooser: m.chooser,
    }
}

fn to_undecoded(s: Stop) -> Undecoded {
    match s {
        Stop::Short => Undecoded::Truncated,
        Stop::Throw => Undecoded::Rejected,
        Stop::Unresolved(w) | Stop::Virtual(w) => Undecoded::Unresolved(w),
        Stop::Budget => Undecoded::Budget,
    }
}

/// Next choice vector in depth-first order, or None when the search space is exhausted.
fn next_prefix(c: &Chooser) -> Option<Vec<usize>> {
    let mut i = c.taken.len();
    while i > 0 {
        i -= 1;
        if c.taken[i] + 1 < c.sizes[i] {
            let mut p = c.taken[..i].to_vec();
            p.push(c.taken[i] + 1);
            return Some(p);
        }
    }
    None
}

/// Decode `bytes` as the object Kontakt 8.13.1 stores for `sertype` at stored `version`.
///
/// Ok means Kontakt's own reader accepts exactly these bytes. The error names what stops it.
pub fn decode(sertype: u16, version: u16, bytes: &[u8]) -> Result<Decoded, Undecoded> {
    decode_inner(reader_for(sertype, version), version, bytes, true)
}

/// Decode a record that is not a serialized object (curated.rs `CURATED_UNFRAMED`), from its
/// chunk header (u16 id, u32 length) on, as Kontakt reads it.
pub fn decode_unframed(id: u16, data: &[u8]) -> Result<Decoded, Undecoded> {
    let reader = CURATED_UNFRAMED.iter().find(|r| r.sertype == id);
    let len = u32::try_from(data.len()).map_err(|_| Undecoded::Rejected)?;
    let chunk = [&id.to_le_bytes()[..], &len.to_le_bytes(), data].concat();
    decode_inner(reader, 0, &chunk, true)
}

/// [`decode`] of an object's private block: with its private reader where curated.rs has
/// one (Kontakt reads the two blocks with separate readers), else the catalog reader.
pub fn decode_private(sertype: u16, version: u16, bytes: &[u8]) -> Result<Decoded, Undecoded> {
    decode_inner(private_reader_for(sertype, version), version, bytes, true)
}

/// Like [`decode`] but the object may be followed by other data: `consumed` says how much
/// Kontakt's reader took. For readers embedded in a larger record.
pub fn decode_prefix(sertype: u16, version: u16, bytes: &[u8]) -> Result<Decoded, Undecoded> {
    decode_inner(reader_for(sertype, version), version, bytes, false)
}

type NestedKey = (u16, u16, bool, usize, usize);
/// Decode depth, and the nested decodes memoized at it.
type Memo = (u32, HashMap<NestedKey, Result<Decoded, Undecoded>>);

std::thread_local! {
    /// Nested framed objects decoded during the outermost [`decode`]: a parent's host-state
    /// search replays its stream, and the same bytes always decode the same way.
    static DECODED: std::cell::RefCell<Memo> =
        std::cell::RefCell::new((0, HashMap::new()));
}

/// [`decode`] of an object nested in the stream being decoded, memoized by where it lies.
fn nested_decode(
    sertype: u16,
    version: u16,
    private: bool,
    bytes: &[u8],
) -> Result<Decoded, Undecoded> {
    let key = (
        sertype,
        version,
        private,
        bytes.as_ptr() as usize,
        bytes.len(),
    );
    if let Some(hit) = DECODED.with(|n| n.borrow().1.get(&key).cloned()) {
        return hit;
    }
    let r = if private {
        decode_private(sertype, version, bytes)
    } else {
        decode(sertype, version, bytes)
    };
    DECODED.with(|n| n.borrow_mut().1.insert(key, r.clone()));
    r
}

fn decode_inner(
    reader: Option<&'static ReaderSrc>,
    version: u16,
    bytes: &[u8],
    exact: bool,
) -> Result<Decoded, Undecoded> {
    let reader = reader.ok_or(Undecoded::NoReader)?;
    DECODED.with(|n| n.borrow_mut().0 += 1);
    let r = decode_search(reader, version, bytes, exact);
    DECODED.with(|n| {
        let mut n = n.borrow_mut();
        n.0 -= 1;
        if n.0 == 0 {
            n.1.clear();
        }
    });
    r
}

fn decode_search(
    r: &'static ReaderSrc,
    version: u16,
    bytes: &[u8],
    exact: bool,
) -> Result<Decoded, Undecoded> {
    let mut prefix = Vec::new();
    let mut worst: Option<Undecoded> = None;
    for _ in 0..4096 {
        let o = attempt(r, version, Stream::decode(bytes, 0), prefix.clone(), &[]);
        let err = match o.stop {
            None if o.consumed == bytes.len() || !exact => {
                return Ok(Decoded {
                    class: r.class,
                    fields: o.fields,
                    consumed: o.consumed,
                    opaque: o.opaque,
                });
            }
            None => Undecoded::Trailing(bytes.len() - o.consumed),
            Some(s) => to_undecoded(s),
        };
        // The most informative failure: one that got furthest.
        if !matches!(err, Undecoded::Truncated | Undecoded::Rejected) || worst.is_none() {
            worst = Some(err);
        }
        match next_prefix(&o.chooser) {
            Some(p) => prefix = p,
            None => break,
        }
    }
    Err(worst.unwrap_or(Undecoded::Budget))
}

/// SerTypes with a reader, and for each class the stored versions it accepts and how
/// completely they decode ("typed", "host-branch", "nested", "virtual:..", "stub", ...).
#[derive(Debug, Clone)]
pub struct ReaderInfo {
    pub sertype: u16,
    pub class: &'static str,
    pub current: u16,
    pub versions: &'static [(u16, u16)],
    pub status: &'static str,
}

pub fn readers() -> Vec<ReaderInfo> {
    all_readers()
        .iter()
        .map(|r| ReaderInfo {
            sertype: r.sertype,
            class: r.class,
            current: r.current,
            versions: r.versions,
            status: r.status,
        })
        .collect()
}

/// The Kontakt class whose reader accepts `version` of `sertype` (several classes share a
/// SerType: 0x1D, 0x2A, 0x54).
pub fn class_of(sertype: u16, version: u16) -> Option<&'static str> {
    reader_for(sertype, version).map(|r| r.class)
}

/// Whether `version` of `sertype` is one Kontakt's reader accepts.
pub fn accepts(sertype: u16, version: u16) -> bool {
    reader_for(sertype, version).is_some()
}

/// Integer literals in branch conditions of the program: values worth trying for fields
/// the reader validates (enum ids, tags).
fn constants(prog: &str) -> Vec<i64> {
    let mut out = vec![0, 1, 2, 3, 4, 5, 6, 7];
    let mut seen = std::collections::HashSet::new();
    let mut todo = vec![prog];
    while let Some(n) = todo.pop() {
        if !seen.insert(n) {
            continue;
        }
        let Some(p) = all_programs().iter().find(|p| p.name == n) else {
            continue;
        };
        for line in p.ops.lines() {
            if let Some(rest) = line.strip_prefix("(call ") {
                if let Some(callee) = rest.split_whitespace().next() {
                    todo.push(callee);
                }
            }
            if line.starts_with("(br") {
                for tok in line.split(|c: char| !c.is_ascii_alphanumeric() && c != '-') {
                    if let Ok(v) = tok.parse::<i64>() {
                        if (0..=255).contains(&v) {
                            out.push(v);
                        }
                    }
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// A stream Kontakt's reader for `sertype`/`version` accepts, found by search. Used to
/// build fixtures and to check that every schema round-trips through `decode`.
pub fn synthesize(sertype: u16, version: u16) -> Result<Vec<u8>, Undecoded> {
    synthesize_all(sertype, version, 1).map(|mut v| v.swap_remove(0))
}

/// Up to `limit` accepted streams with different field sequences (every validated enum
/// value, every branch the reader takes on values it checks), in depth-first order.
pub fn synthesize_all(sertype: u16, version: u16, limit: usize) -> Result<Vec<Vec<u8>>, Undecoded> {
    let r = reader_for(sertype, version).ok_or(Undecoded::NoReader)?;
    let values = constants(r.program);
    let mut prefix = Vec::new();
    let mut last = Undecoded::Budget;
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut shapes = std::collections::HashSet::new();
    for _ in 0..3000 {
        let o = attempt(r, version, Stream::synth(), prefix.clone(), &values);
        match o.stop {
            None if !o.fields.is_empty() => {
                let shape: Vec<(char, usize)> = o.fields.iter().map(|f| (f.kind, f.len)).collect();
                if shapes.insert(shape) {
                    out.push(o.out);
                    if out.len() == limit {
                        return Ok(out);
                    }
                }
            }
            None => last = Undecoded::Unresolved("reads nothing".into()),
            Some(Stop::Virtual(w)) => {
                return if out.is_empty() {
                    Err(Undecoded::Unresolved(w))
                } else {
                    Ok(out)
                };
            }
            Some(s) => last = to_undecoded(s),
        }
        match next_prefix(&o.chooser) {
            Some(p) => prefix = p,
            None => break,
        }
    }
    if out.is_empty() {
        Err(last)
    } else {
        Ok(out)
    }
}

#[cfg(test)]
mod ir_validation_tests {
    use super::*;

    #[test]
    fn every_bundled_program_validates() {
        assert!(library().is_ok(), "{:?}", library().err());
    }

    #[test]
    fn malformed_ir_is_refused_instead_of_panicking_or_skipped() {
        let index = HashMap::from([("test", 0)]);
        for ops in [
            ")",
            "(ret",
            "()",
            "((ret))",
            "stray",
            "(set r (rd (I)))",
            "(set r (rd Z))",
            "(set r (+ 1))",
            "(set r (unknown 1 2))",
            "(set r (ld0 1))",
            "(set r (ld9 1))",
            "(st 0 0 0)",
            "(jmp missing)",
            "(br 1 missing)",
            "(label x)(label x)",
            "(call missing r ())",
            "(call test r not-a-list)",
            "(set (r) 1)",
            "(ret 1 2)",
            "(unknown)",
        ] {
            let src = ProgramSrc {
                name: "test",
                params: &[],
                frame: 0,
                ops,
            };
            assert!(build(&src, &index).is_err(), "accepted {ops}");
        }
    }

    #[test]
    fn validated_ir_preserves_control_flow_and_expression_values() {
        let index = HashMap::from([("test", 0)]);
        let src = ProgramSrc {
            name: "test", params: &["param"], frame: 32,
            ops: "(label start)(set r (+ (rd I) 2))(br (== r 3) end)(jmp start)(label end)(call test dst (r))(ret r)",
        };
        let program = build(&src, &index).unwrap();
        assert_eq!(program.ops.len(), 5);
        assert!(
            matches!(&program.ops[0], Op::Set(_, Ex::Bin(Bin::Add, a, b))
            if matches!(a.as_ref(), Ex::Rd('I')) && matches!(b.as_ref(), Ex::Int(2)))
        );
        assert!(matches!(program.ops[1], Op::Br(_, 3, None)));
        assert!(matches!(program.ops[2], Op::Jmp(0)));
        assert!(matches!(&program.ops[3], Op::Call(0, _, a) if a.len() == 1));
        assert!(matches!(program.ops[4], Op::Ret(Some(Ex::Reg(_)))));
        let src = ProgramSrc {
            name: "test",
            params: &[],
            frame: 0,
            ops: "(throw native-throw)",
        };
        assert!(matches!(
            build(&src, &index).unwrap().ops.as_slice(),
            [Op::Throw]
        ));
    }
}

#[cfg(test)]
mod catalog_equivalence_tests {
    use super::*;

    // Test-only oracle of the pre-remediation lowering rules. It is used only
    // on retained, valid catalog programs. Structural errors fail the test;
    // the production parser remains the sole fallible validation path.
    fn legacy_ex(b: &mut Builder<'_>, t: &Tok) -> Ex {
        match t {
            Tok::Atom(a) => match a.as_str() {
                "?" => Ex::Undef,
                "g_cls" => Ex::Glob(0),
                "g_curver" => Ex::Glob(1),
                a if a.parse::<i64>().is_ok() => Ex::Int(a.parse().unwrap()),
                a => Ex::Reg(b.reg(a)),
            },
            Tok::List(l) => {
                let h = l[0].atom().unwrap();
                let a = |i: usize, b: &mut Builder<'_>| Box::new(legacy_ex(b, &l[i]));
                match h {
                    "rd" if l[1].atom().unwrap() == "O" => b.rd_obj(),
                    "rd" => Ex::Rd(l[1].atom().unwrap().chars().next().unwrap()),
                    "sel" => Ex::Sel(a(1, b), a(2, b), a(3, b)),
                    "&&" => Ex::AndAnd(a(1, b), a(2, b)),
                    "||" => Ex::OrOr(a(1, b), a(2, b)),
                    "!" => Ex::Un(Un::Not, a(1, b)),
                    "~" => Ex::Un(Un::BitNot, a(1, b)),
                    "neg" => Ex::Un(Un::Neg, a(1, b)),
                    "u8" | "s8" | "u16" | "s16" | "u32" | "s32" => {
                        Ex::Wrap(h[1..].parse().unwrap(), h.starts_with('s'), a(1, b))
                    }
                    h if h.starts_with("ld") => {
                        Ex::Ld(h[2..3].parse().unwrap(), h.ends_with('s'), a(1, b))
                    }
                    h => {
                        let op = match h {
                            "+" => Bin::Add,
                            "-" => Bin::Sub,
                            "*" => Bin::Mul,
                            "div" => Bin::Div,
                            "divu" => Bin::DivU,
                            "mod" => Bin::Mod,
                            "modu" => Bin::ModU,
                            "&" => Bin::And,
                            "|" => Bin::Or,
                            "^" => Bin::Xor,
                            "<<" => Bin::Shl,
                            ">>" => Bin::Shr,
                            "shru" => Bin::ShrU,
                            "==" => Bin::Eq,
                            "!=" => Bin::Ne,
                            "<" => Bin::Lt,
                            "<=" => Bin::Le,
                            ">" => Bin::Gt,
                            ">=" => Bin::Ge,
                            "ltu" => Bin::Ltu,
                            "leu" => Bin::Leu,
                            "gtu" => Bin::Gtu,
                            "geu" => Bin::Geu,
                            _ => panic!("invalid catalog expression {h}"),
                        };
                        Ex::Bin(op, a(1, b), a(2, b))
                    }
                }
            }
        }
    }

    fn legacy_build(src: &ProgramSrc, progs: &HashMap<&'static str, usize>) -> Program {
        let mut b = Builder {
            regs: HashMap::new(),
            labels: HashMap::new(),
            progs,
            name: src.name,
            objects: 0,
        };
        let params = src.params.iter().map(|p| b.reg(p)).collect();
        let sp = b.reg("sp");
        let toks: Vec<_> = src
            .ops
            .lines()
            .flat_map(|line| parse_toks(line).unwrap())
            .collect();
        let mut n = 0;
        for t in &toks {
            let (head, args) = t.form().unwrap();
            if head == "label" {
                b.labels.insert(args[0].atom().unwrap().to_owned(), n);
            } else {
                n += 1;
            }
        }
        let mut ops = Vec::with_capacity(n);
        for t in &toks {
            let l = t.list().unwrap();
            let atom = |i: usize| l[i].atom().unwrap().to_owned();
            match l[0].atom().unwrap() {
                "label" => {}
                "set" => {
                    let r = b.reg(&atom(1));
                    ops.push(Op::Set(r, legacy_ex(&mut b, &l[2])));
                }
                "st" => ops.push(Op::St(
                    atom(1).parse().unwrap(),
                    legacy_ex(&mut b, &l[2]),
                    legacy_ex(&mut b, &l[3]),
                )),
                h @ ("br" | "brb" | "brt") => {
                    let target = b.labels[&atom(2)];
                    let fixed = match h {
                        "brb" => Some(false),
                        "brt" => Some(true),
                        _ => None,
                    };
                    ops.push(Op::Br(legacy_ex(&mut b, &l[1]), target, fixed));
                }
                "jmp" => ops.push(Op::Jmp(b.labels[&atom(1)])),
                "rdn" => ops.push(Op::RdN(legacy_ex(&mut b, &l[1]), legacy_ex(&mut b, &l[2]))),
                "rdbody" => ops.push(Op::RdBody(legacy_ex(&mut b, &l[1]))),
                "call" => {
                    let prog = b.progs[atom(1).as_str()];
                    let dst = b.reg(&atom(2));
                    let args = l[3]
                        .list()
                        .unwrap()
                        .iter()
                        .map(|a| legacy_ex(&mut b, a))
                        .collect();
                    ops.push(Op::Call(prog, dst, args));
                }
                "ret" => ops.push(Op::Ret(l.get(1).map(|e| legacy_ex(&mut b, e)))),
                "throw" => ops.push(Op::Throw),
                "callx" => {
                    let op = b.callx(atom(1));
                    ops.push(op);
                }
                h => panic!("invalid catalog instruction {h}"),
            }
        }
        Program {
            params,
            nregs: b.regs.len(),
            sp,
            frame: i64::from(src.frame),
            ops,
        }
    }

    #[test]
    fn every_retained_program_builds_and_matches_legacy_lowering() {
        let index: HashMap<_, _> = all_programs()
            .iter()
            .enumerate()
            .map(|(i, p)| (p.name, i))
            .collect();
        let mut checked = 0;
        for src in all_programs() {
            let current = build(src, &index).unwrap_or_else(|e| panic!("{}: {e:?}", src.name));
            let old = legacy_build(src, &index);
            assert_eq!(current.params, old.params, "{} params", src.name);
            assert_eq!(current.nregs, old.nregs, "{} registers", src.name);
            assert_eq!(current.sp, old.sp, "{} stack register", src.name);
            assert_eq!(current.frame, old.frame, "{} frame", src.name);
            // Debug renders every operation and nested expression, including
            // resolved jump labels, call IDs, register IDs and signed widths.
            assert_eq!(
                format!("{:?}", current.ops),
                format!("{:?}", old.ops),
                "{} ops",
                src.name
            );
            checked += 1;
        }
        assert_eq!(checked, all_programs().len());
        assert!(checked > 0);
    }

    #[test]
    fn all_supported_load_spellings_preserve_width_and_signedness() {
        let index = HashMap::new();
        let mut b = Builder {
            regs: HashMap::new(),
            labels: HashMap::new(),
            progs: &index,
            name: "",
            objects: 0,
        };
        for width in [1, 2, 4, 8] {
            for suffix in ["", "u", "s"] {
                let text = format!("(ld{width}{suffix} 42)");
                let toks = parse_toks(&text).unwrap();
                let ex = b.ex(&toks[0]).unwrap();
                assert!(matches!(&ex, Ex::Ld(w, signed, a) if *w == width
                    && *signed == (suffix == "s") && matches!(a.as_ref(), Ex::Int(42))));
                assert_eq!(
                    format!("{ex:?}"),
                    format!("{:?}", legacy_ex(&mut b, &toks[0]))
                );
            }
        }
    }
}
