//! Executor for the stream IR in `programs.rs` (see tools/re/reader_lower.py).
//!
//! The IR is Kontakt 8.13.1's own reader control flow: stored-version switches, validation
//! throws and data-dependent branches are decided here by the same conditions Kontakt
//! evaluates. Values the stream does not determine (host state such as instrument tables)
//! are *undefined*; a branch on one is searched both ways and a stream decodes when some
//! assignment consumes it exactly. Reference interpreter: tools/re/ir_sim.py.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::programs::{ProgramSrc, ReaderSrc, PROGRAMS, READERS};

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
        self.fields.iter().rev().find(|f| f.slot == Some(slot) && f.depth == 0)
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
}

type Val = Option<(i64, u32)>;

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

fn parse_toks(text: &str) -> Vec<Tok> {
    let mut stack: Vec<Vec<Tok>> = vec![Vec::new()];
    let mut atom = String::new();
    let flush = |atom: &mut String, stack: &mut Vec<Vec<Tok>>| {
        if !atom.is_empty() {
            stack.last_mut().unwrap().push(Tok::Atom(std::mem::take(atom)));
        }
    };
    for c in text.chars() {
        match c {
            '(' => {
                flush(&mut atom, &mut stack);
                stack.push(Vec::new());
            }
            ')' => {
                flush(&mut atom, &mut stack);
                let done = stack.pop().unwrap();
                stack.last_mut().unwrap().push(Tok::List(done));
            }
            c if c.is_whitespace() => flush(&mut atom, &mut stack),
            c => atom.push(c),
        }
    }
    flush(&mut atom, &mut stack);
    stack.pop().unwrap()
}

struct Builder<'a> {
    regs: HashMap<String, usize>,
    labels: HashMap<String, usize>,
    progs: &'a HashMap<&'static str, usize>,
}

impl Builder<'_> {
    fn reg(&mut self, name: &str) -> usize {
        let n = self.regs.len();
        *self.regs.entry(name.to_string()).or_insert(n)
    }

    fn ex(&mut self, t: &Tok) -> Ex {
        match t {
            Tok::Atom(a) => match a.as_str() {
                "?" => Ex::Undef,
                "g_cls" => Ex::Glob(0),
                "g_curver" => Ex::Glob(1),
                a if a.parse::<i64>().is_ok() => Ex::Int(a.parse().unwrap()),
                a => Ex::Reg(self.reg(a)),
            },
            Tok::List(l) => {
                let Tok::Atom(head) = &l[0] else { unreachable!("IR head") };
                let h = head.as_str();
                let a = |i: usize, me: &mut Self| Box::new(me.ex(&l[i]));
                match h {
                    "rd" => {
                        let Tok::Atom(k) = &l[1] else { unreachable!() };
                        Ex::Rd(k.chars().next().unwrap())
                    }
                    "sel" => Ex::Sel(a(1, self), a(2, self), a(3, self)),
                    "&&" => Ex::AndAnd(a(1, self), a(2, self)),
                    "||" => Ex::OrOr(a(1, self), a(2, self)),
                    "!" => Ex::Un(Un::Not, a(1, self)),
                    "~" => Ex::Un(Un::BitNot, a(1, self)),
                    "neg" => Ex::Un(Un::Neg, a(1, self)),
                    "u8" | "s8" | "u16" | "s16" | "u32" | "s32" => {
                        let bits: u8 = h[1..].parse().unwrap();
                        Ex::Wrap(bits, h.starts_with('s'), a(1, self))
                    }
                    h if h.starts_with("ld") => {
                        let w: u8 = h[2..3].parse().unwrap();
                        Ex::Ld(w, h.ends_with('s'), a(1, self))
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
                            other => unreachable!("IR operator {other}"),
                        };
                        Ex::Bin(op, a(1, self), a(2, self))
                    }
                }
            }
        }
    }
}

fn build(src: &ProgramSrc, progs: &HashMap<&'static str, usize>) -> Program {
    let mut b = Builder { regs: HashMap::new(), labels: HashMap::new(), progs };
    let params: Vec<usize> = src.params.iter().map(|p| b.reg(p)).collect();
    let sp = b.reg("sp");
    // First pass: label positions.
    let toks: Vec<Tok> = src.ops.lines().flat_map(parse_toks).collect();
    let mut n = 0;
    for t in &toks {
        let Tok::List(l) = t else { continue };
        let Tok::Atom(h) = &l[0] else { continue };
        if h == "label" {
            let Tok::Atom(name) = &l[1] else { continue };
            b.labels.insert(name.clone(), n);
        } else {
            n += 1;
        }
    }
    let mut ops = Vec::with_capacity(n);
    for t in &toks {
        let Tok::List(l) = t else { continue };
        let Tok::Atom(h) = &l[0] else { continue };
        let atom = |i: usize| -> String {
            match &l[i] {
                Tok::Atom(a) => a.clone(),
                _ => unreachable!(),
            }
        };
        match h.as_str() {
            "label" => {}
            "set" => {
                let r = b.reg(&atom(1));
                ops.push(Op::Set(r, b.ex(&l[2])));
            }
            "st" => ops.push(Op::St(atom(1).parse().unwrap(), b.ex(&l[2]), b.ex(&l[3]))),
            "br" | "brb" | "brt" => {
                let target = b.labels[&atom(2)];
                let fixed = match h.as_str() {
                    "brb" => Some(false),
                    "brt" => Some(true),
                    _ => None,
                };
                ops.push(Op::Br(b.ex(&l[1]), target, fixed));
            }
            "jmp" => ops.push(Op::Jmp(b.labels[&atom(1)])),
            "rdn" => ops.push(Op::RdN(b.ex(&l[1]), b.ex(&l[2]))),
            "rdbody" => ops.push(Op::RdBody(b.ex(&l[1]))),
            "call" => {
                let prog = b.progs[atom(1).as_str()];
                let dst = b.reg(&atom(2));
                let Tok::List(args) = &l[3] else { unreachable!() };
                let args = args.iter().map(|a| b.ex(a)).collect();
                ops.push(Op::Call(prog, dst, args));
            }
            "ret" => ops.push(Op::Ret(l.get(1).map(|e| b.ex(e)))),
            "throw" => ops.push(Op::Throw),
            "callx" => ops.push(Op::CallX(atom(1))),
            other => unreachable!("IR op {other}"),
        }
    }
    Program { params, nregs: b.regs.len(), sp, frame: i64::from(src.frame), ops }
}

struct Library {
    programs: Vec<Program>,
    index: HashMap<&'static str, usize>,
}

fn library() -> &'static Library {
    static LIB: OnceLock<Library> = OnceLock::new();
    LIB.get_or_init(|| {
        let index: HashMap<&'static str, usize> = PROGRAMS.iter().enumerate().map(|(i, p)| (p.name, i)).collect();
        let programs = PROGRAMS.iter().map(|p| build(p, &index)).collect();
        Library { programs, index }
    })
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
        Self { data: Some(data), out: Vec::new(), pos: 0, base }
    }
    fn synth() -> Self {
        Self { data: None, out: Vec::new(), pos: 0, base: 0 }
    }
}

struct Machine<'a> {
    lib: &'static Library,
    stream: Stream<'a>,
    chooser: Chooser,
    values: &'a [i64],
    mem: HashMap<i64, (u8, u32)>,
    fields: Vec<Field>,
    depth: u8,
    steps: u32,
    next_sp: i64,
    nobj: i64,
    globals: [Val; 2],
}

fn wrap(v: i64, bits: u8, signed: bool) -> i64 {
    let m = (1i64 << bits) - 1;
    let u = v & m;
    if signed && (u >> (bits - 1)) & 1 == 1 { u - (1i64 << bits) } else { u }
}

impl<'a> Machine<'a> {
    fn new(stream: Stream<'a>, chooser: Chooser, values: &'a [i64], class: Option<u16>) -> Self {
        let mut m = Self {
            lib: library(),
            stream,
            chooser,
            values,
            mem: HashMap::new(),
            fields: Vec::new(),
            depth: 0,
            steps: 400_000,
            next_sp: STACK,
            nobj: 0,
            globals: [class.map(|c| (i64::from(c), 0)), None],
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
                    self.mem.insert(addr + i, (((x >> (8 * i)) & 0xff) as u8, src));
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
                let end = self.stream.pos.checked_add(n).filter(|&e| e <= d.len()).ok_or(Stop::Short)?;
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
            'S' | 'W' => {
                let b = self.take(4, true)?;
                let n = u32::from_le_bytes(b[..4].try_into().unwrap()) as usize;
                if n > 1 << 24 {
                    return Err(Stop::Throw);
                }
                self.take(n * if kind == 'W' { 2 } else { 1 }, false)?;
                self.record(kind, start, self.stream.pos - start, Some(n as u64));
                Ok(None)
            }
            'O' => self.read_object(start).map(|_| None),
            _ => Err(Stop::Unresolved(format!("read kind {kind}"))),
        }
    }

    fn class_reader(&self) -> Option<(usize, u16, u16)> {
        let (cls, _) = self.globals[0]?;
        let r = READERS.iter().find(|r| i64::from(r.sertype) == cls && !r.program.is_empty())?;
        Some((*self.lib.index.get(r.program)?, r.sertype, r.current))
    }

    fn read_object(&mut self, start: usize) -> Result<(), Stop> {
        let flag = self.take(1, true)?[0];
        if flag > 1 {
            return Err(Stop::Throw);
        }
        let ver = u16::from_le_bytes(self.take(2, true)?[..2].try_into().unwrap());
        let reader = self.class_reader();
        if let Some((_, _, current)) = reader {
            if ver > current {
                return Err(Stop::Throw);
            }
        }
        if flag == 0 {
            let (prog, _, _) = reader.ok_or_else(|| Stop::Unresolved("unframed nested object of unknown class".into()))?;
            self.record('O', start, 3, Some(u64::from(ver)));
            return self.run_nested(prog, ver);
        }
        let id = self.record('O', start, 0, Some(u64::from(ver)));
        if self.stream.data.is_none() {
            // synthesize: empty private and children areas around the body
            self.take(4, false)?;
            let mut body = Stream::synth();
            body.base = self.stream.pos + 4;
            if let Some((prog, _, _)) = reader {
                std::mem::swap(&mut self.stream, &mut body);
                self.depth += 1;
                let r = self.run_nested(prog, ver);
                self.depth -= 1;
                std::mem::swap(&mut self.stream, &mut body);
                r?;
            }
            self.stream.out.extend_from_slice(&(body.out.len() as u32).to_le_bytes());
            self.stream.out.extend_from_slice(&body.out);
            self.stream.out.extend_from_slice(&[0; 4]);
            self.stream.pos += 8 + body.out.len();
        } else {
            let data = self.stream.data.unwrap();
            let mut areas = Vec::new();
            for _ in 0..3 {
                let n = u32::from_le_bytes(self.take(4, false)?[..4].try_into().unwrap()) as usize;
                let begin = self.stream.pos;
                self.take(n, false)?;
                areas.push((begin, n));
            }
            if let Some((prog, _, _)) = reader {
                let (begin, n) = areas[1];
                let sub = Stream::decode(&data[begin..begin + n], self.stream.base + begin);
                let mut sub = sub;
                std::mem::swap(&mut self.stream, &mut sub);
                self.depth += 1;
                let r = self.run_nested(prog, ver);
                self.depth -= 1;
                let finished = self.stream.pos == n;
                std::mem::swap(&mut self.stream, &mut sub);
                r?;
                if !finished {
                    return Err(Stop::Unresolved("nested object left bytes unread".into()));
                }
            }
        }
        self.fields[id as usize - 1].len = self.stream.pos - start;
        Ok(())
    }

    fn run_nested(&mut self, prog: usize, ver: u16) -> Result<(), Stop> {
        self.nobj += 1;
        let base = OBJ + NESTED * self.nobj;
        self.call(prog, &[Some((base, 0)), Some((STREAM, 0)), Some((i64::from(ver), 0)), Some((CTX, 0))], 1)?;
        Ok(())
    }

    fn ev(&mut self, e: &Ex, regs: &[Val]) -> Result<Val, Stop> {
        Ok(match e {
            Ex::Int(i) => Some((*i, 0)),
            Ex::Undef => None,
            Ex::Reg(r) => regs[*r],
            Ex::Glob(g) => self.globals[*g],
            Ex::Rd(k) => self.read(*k)?,
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
                    Bin::Shl => if (0..64).contains(&y) { x << y } else { 0 },
                    Bin::Shr => if (0..64).contains(&y) { x >> y } else if x >= 0 { 0 } else { -1 },
                    Bin::ShrU => if (0..64).contains(&y) { (ux >> y) as i64 } else { 0 },
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

    fn call(&mut self, prog: usize, args: &[Val], depth: u32) -> Result<Val, Stop> {
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
                    let (prog, _, _) = self
                        .class_reader()
                        .ok_or_else(|| Stop::Unresolved("embedded object of unknown class".into()))?;
                    let Some((ver, _)) = ver else { return Err(Stop::Unresolved("embedded object version".into())) };
                    self.run_nested(prog, ver as u16)?;
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
    READERS.iter().filter(move |r| r.sertype == sertype)
}

fn reader_for(sertype: u16, version: u16) -> Option<&'static ReaderSrc> {
    readers_for(sertype).find(|r| !r.program.is_empty() && r.versions.iter().any(|&(a, b)| (a..=b).contains(&version)))
}

fn element_of(r: &ReaderSrc) -> Option<u16> {
    (r.element != 0).then_some(r.element)
}

struct Outcome {
    stop: Option<Stop>,
    fields: Vec<Field>,
    consumed: usize,
    out: Vec<u8>,
    chooser: Chooser,
}

fn attempt(r: &'static ReaderSrc, version: u16, stream: Stream<'_>, prefix: Vec<usize>, values: &[i64]) -> Outcome {
    let lib = library();
    let prog = lib.index[r.program];
    let chooser = Chooser { prefix, ..Chooser::default() };
    let mut m = Machine::new(stream, chooser, values, element_of(r));
    let args = [Some((OBJ, 0)), Some((STREAM, 0)), Some((i64::from(version), 0)), Some((CTX, 0))];
    let stop = m.call(prog, &args, 0).err();
    Outcome { stop, fields: m.fields, consumed: m.stream.pos, out: m.stream.out, chooser: m.chooser }
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
    decode_inner(sertype, version, bytes, true)
}

/// Like [`decode`] but the object may be followed by other data: `consumed` says how much
/// Kontakt's reader took. For readers embedded in a larger record.
pub fn decode_prefix(sertype: u16, version: u16, bytes: &[u8]) -> Result<Decoded, Undecoded> {
    decode_inner(sertype, version, bytes, false)
}

fn decode_inner(sertype: u16, version: u16, bytes: &[u8], exact: bool) -> Result<Decoded, Undecoded> {
    let r = reader_for(sertype, version).ok_or(Undecoded::NoReader)?;
    let mut prefix = Vec::new();
    let mut worst: Option<Undecoded> = None;
    for _ in 0..4096 {
        let o = attempt(r, version, Stream::decode(bytes, 0), prefix.clone(), &[]);
        let err = match o.stop {
            None if o.consumed == bytes.len() || !exact => {
                return Ok(Decoded { class: r.class, fields: o.fields, consumed: o.consumed });
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
    READERS
        .iter()
        .map(|r| ReaderInfo { sertype: r.sertype, class: r.class, current: r.current, versions: r.versions, status: r.status })
        .collect()
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
        let Some(p) = PROGRAMS.iter().find(|p| p.name == n) else { continue };
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
    let r = reader_for(sertype, version).ok_or(Undecoded::NoReader)?;
    let values = constants(r.program);
    let mut prefix = Vec::new();
    let mut last = Undecoded::Budget;
    for _ in 0..3000 {
        let o = attempt(r, version, Stream::synth(), prefix.clone(), &values);
        match o.stop {
            None if !o.fields.is_empty() => return Ok(o.out),
            None => last = Undecoded::Unresolved("reads nothing".into()),
            Some(Stop::Virtual(w)) => return Err(Undecoded::Unresolved(w)),
            Some(s) => last = to_undecoded(s),
        }
        match next_prefix(&o.chooser) {
            Some(p) => prefix = p,
            None => break,
        }
    }
    Err(last)
}
