#!/usr/bin/env python3
"""Lower Kontakt 8.13.1's decompiled serializable readers to a small stream IR.

The IR keeps the readers' own control flow, so every stored version, every
validation throw and every data-dependent branch is decided at decode time by the
same conditions Kontakt evaluates; nothing here guesses a layout.

IR (one s-expression per line, a program per decompiled function):

  (set r e)  (st N addr e)  (br e L)  (jmp L)  (label L)  (call prog dst (args))
  (rdn dest n)  (ret [e])  (throw why)  (callx why)

  e: integer | ? (undefined) | register | (op a b) | (ld1u|ld1s|...|ld8 a)
     (u8|s8|u16|s16|u32|s32 a) | (rd F|B|H|I|Q|S|W|O) | (sel c a b)
  ops: + - * div divu mod modu & | ^ << >> shru == != < <= > >= ltu leu gtu geu && || ! ~ neg

Reads: F f32 bits, B u8, H u16, I u32, Q u64, S length-prefixed string, W UTF-16
string, O nested framed object (u8 structured, u16 version, three u32-length areas).
Stack primitives are identified by their decompiled bodies (see PRIMS).
"""
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from cparse import parse_functions  # noqa: E402

PRIMS = {
    "FUN_142a780f0": "F", "FUN_142a780c0": "B", "FUN_142a78220": "H", "FUN_142a78270": "I",
    "FUN_142a78400": "Q", "FUN_142a78330": "S", "FUN_14093ff90": "S", "FUN_140743010": "S",
    "FUN_142a784b0": "W", "FUN_14093d6c0": "O",
}

# Argument index of the stream for primitives that do not take it first.
PRIM_STREAM_ARG = {"FUN_140743010": 1}

# Functions whose decompilation is wrong or allocator noise; semantics from their bodies.
OVERRIDES = {
    # u16 tag that must equal param_3, then a raw u32 (decompiler lost the register).
    "FUN_140d17430": ["(set tag (rd H))", "(br (== tag param_3) Lok)", "(throw tag-mismatch)",
                      "(label Lok)", "(set v (rd I))", "(ret v)"],
}

SIZE = {"char": 1, "byte": 1, "bool": 1, "uchar": 1, "short": 2, "ushort": 2, "int": 4, "uint": 4,
        "long": 4, "ulong": 4, "longlong": 8, "ulonglong": 8, "code": 8, "void": 1, "float": 4,
        "double": 8, "float10": 8, "size_t": 8, "int3": 3, "uint3": 3, "int5": 5, "int6": 6, "int7": 7,
        "uint5": 5, "uint6": 6, "uint7": 7, "BOOL": 4, "wchar_t": 2}
SIGNED = {"char", "short", "int", "long", "longlong", "int3", "int5", "int6", "int7", "BOOL"}


def base_ty(base):
    """(kind, width) of a non-pointer Ghidra type word."""
    m = re.fullmatch(r"undefined(\d*)", base)
    if m:
        return ("u", int(m.group(1) or 1))
    if base in ("float", "double"):
        return ("f", SIZE[base])
    return ("s" if base in SIGNED else "u", SIZE.get(base, 8))


def ctype_ty(ct):
    base, depth = ct
    if depth:
        return ("p", 8, ctype_ty((base, depth - 1)) if depth > 1 else base_ty(base))
    return base_ty(base)


def width(ty):
    return ty[1]


def esz(ty):
    return ty[2][1] if ty[0] == "p" and ty[2][0] != "p" else 8


class Ex:
    __slots__ = ("text", "ty", "const")

    def __init__(self, text, ty=("s", 4), const=None):
        self.text, self.ty, self.const = text, ty, const


UNDEF = lambda ty=("u", 8): Ex("?", ty)  # noqa: E731
CONCAT = re.compile(r"CONCAT(\d)(\d)$")
SUBX = re.compile(r"SUB(\d)(\d)$")
EXT = re.compile(r"[SZ]EXT(\d)(\d)$")
VTABLE_SLOT = re.compile(r"^\*\*\(code \*\*\)\(\*(\w+) \+ (0x[0-9a-f]+|\d+)\)$")
# Pure lookups over Kontakt's runtime registries (name -> id maps built at startup).
OPAQUE = {"FUN_140886530", "_guard_check_icall"}
VTABLE_STREAM = re.compile(r"\+ 8\)\s*\+ (0x[0-9a-f]+|\d+)\)$")


def show(e):
    """Cheap canonical text of a parsed expression for pattern matching."""
    k = e[0]
    if k == "var":
        return e[1]
    if k == "num":
        return hex(e[1]) if e[1] > 9 else str(e[1])
    if k == "un":
        return e[1] + show(e[2])
    if k == "bin":
        return f"({show(e[2])} {e[1]} {show(e[3])})"
    if k == "cast":
        return f"({e[1][0]} {'*' * e[1][1]}){show(e[2])}"
    if k == "call":
        return f"{show(e[1])}({','.join(show(a) for a in e[2])})"
    if k == "index":
        return f"{show(e[1])}[{show(e[2])}]"
    return k


def is_alias(e, names):
    """A copy of the stream: a variable, possibly cast or offset (never a load from it)."""
    if e[0] == "var":
        return e[1] in names
    if e[0] == "cast":
        return is_alias(e[2], names)
    if e[0] == "bin" and e[1] in "+-":
        return is_alias(e[2], names)
    return False


def mentions(e, names):
    if isinstance(e, tuple):
        if e and e[0] == "var" and e[1] in names:
            return True
        return any(mentions(x, names) for x in e[1:])
    if isinstance(e, list):
        return any(mentions(x, names) for x in e)
    return False


class Lowerer:
    def __init__(self, fn, funcs, streams):
        self.fn, self.funcs, self.streams = fn, funcs, streams
        self.name, self.addr, self.params, self.body, self.decls = fn
        self.ops = []
        self.nlabel = self.ntmp = 0
        self.vars = {}
        for ct, nm in self.params:
            self.vars[nm] = ctype_ty(ct)
        self.arrays = {}
        self.memvars = {}
        self.frame = 0
        for nm, (ct, length) in self.decls.items():
            ty = ctype_ty(ct)
            if length is not None:
                n = self.const_of(length) or 1
                self.arrays[nm] = self.frame
                self.frame += (n * (esz(("p", 8, ty)) if ty[0] != "p" else 8) + 15) & ~15
                self.vars[nm] = ("p", 8, ty)
            else:
                self.vars[nm] = ty
        self.addressed = set()
        self.find_addressed(self.body)
        for nm in sorted(self.addressed):
            if nm in self.vars and nm not in self.arrays:
                self.memvars[nm] = self.frame
                self.frame += 16
        self.stream_vars = set(streams.get(self.name, ()))
        self.labels = {}
        self.vslots = {}
        self.breaks, self.continues = [], []
        self.aliases_pass()

    # ---- analysis ----------------------------------------------------------
    def const_of(self, e):
        return e[1] if e[0] == "num" else None

    def find_addressed(self, node):
        if isinstance(node, tuple):
            if node[0] == "un" and node[1] == "&" and node[2][0] == "var":
                self.addressed.add(node[2][1])
            for x in node[1:]:
                self.find_addressed(x)
        elif isinstance(node, list):
            for x in node:
                self.find_addressed(x)

    def aliases_pass(self):
        """Locals assigned from stream-carrying expressions carry the stream too."""
        changed = True
        while changed:
            changed = False

            def walk(n):
                nonlocal changed
                if isinstance(n, tuple):
                    if n[0] == "assign" and n[1] == "=" and n[2][0] == "var" and n[2][1] not in self.stream_vars \
                            and is_alias(n[3], self.stream_vars):
                        self.stream_vars.add(n[2][1])
                        changed = True
                    for x in n[1:]:
                        walk(x)
                elif isinstance(n, list):
                    for x in n:
                        walk(x)

            walk(self.body)

    # ---- emit helpers ------------------------------------------------------
    def emit(self, s):
        self.ops.append(s)

    def tmp(self):
        self.ntmp += 1
        return f"t{self.ntmp}"

    def label(self):
        self.nlabel += 1
        return f"L{self.nlabel}"

    def user_label(self, name):
        return self.labels.setdefault(name, "U" + re.sub(r"\W", "_", name))

    # ---- types / conversion ------------------------------------------------
    def wrap(self, text, ty):
        if ty[0] == "p" or ty[1] >= 8 or ty[0] == "f":
            return text
        return f"({ty[0]}{ty[1] * 8} {text})" if ty[1] in (1, 2, 4) else text

    def conv(self, e, ty):
        if e.ty == ty or e.text == "?":
            return Ex(e.text, ty, e.const)
        if e.const is not None and ty[0] != "f":
            return Ex(str(wrap_const(e.const, ty)), ty, wrap_const(e.const, ty))
        if ty[0] == "f" or e.ty[0] == "f":
            return UNDEF(ty)
        narrower = ty[1] < e.ty[1] or (ty[1] == e.ty[1] and ty[0] != e.ty[0]) or (ty[1] > e.ty[1] and e.ty[0] == "s" and ty[0] == "u" and ty[1] < 8)
        return Ex(self.wrap(e.text, ty) if narrower else e.text, ty)

    def promote(self, a, b):
        if a.ty[0] == "f" or b.ty[0] == "f":
            return None
        w = max(4, a.ty[1], b.ty[1])
        uns = (a.ty[0] != "s" and a.ty[1] == w) or (b.ty[0] != "s" and b.ty[1] == w)
        return ("u" if uns else "s", w)

    # ---- expressions -------------------------------------------------------
    def var_ref(self, name):
        if name in self.arrays:
            return Ex(f"(+ sp {self.arrays[name]})", self.vars[name])
        if name in self.memvars:
            ty = self.vars[name]
            return self.load(Ex(f"(+ sp {self.memvars[name]})", ("u", 8)), ty)
        if name in self.vars:
            ty = self.vars[name]
            return UNDEF(ty) if ty[0] == "f" else Ex(name, ty)
        return UNDEF()

    def load(self, addr, ty):
        if ty[0] == "f":
            return UNDEF(ty)
        w = ty[1] if ty[0] != "p" else 8
        if w not in (1, 2, 4, 8):
            return UNDEF(ty)
        sign = "s" if ty[0] == "s" and w < 8 else "u"
        return Ex(f"(ld{w}{sign if w < 8 else ''} {addr.text})", ty)

    def ex(self, e):
        k = e[0]
        if k == "num":
            ty = ctype_ty(e[2])
            return Ex(str(e[1]), ty, e[1])
        if k == "fnum":
            return UNDEF(("f", 4))
        if k == "var":
            return self.var_ref(e[1])
        if k == "comma":
            self.ex(e[1])
            return self.ex(e[2])
        if k == "cast":
            return self.cast(e[1], self.ex(e[2]))
        if k == "un":
            return self.unary(e)
        if k == "bin":
            return self.binary(e)
        if k == "index":
            ptr = self.ex(e[1])
            idx = self.ex(e[2])
            return self.deref(self.add_ptr(ptr, idx, 1))
        if k == "assign":
            return self.assign(e)
        if k == "incdec":
            _, op, pre, tgt = e
            before = self.ex(tgt)
            t = self.tmp()
            self.emit(f"(set {t} {before.text})")
            old = Ex(t, before.ty)
            new = self.arith("+" if op == "++" else "-", old, Ex("1", ("s", 4), 1))
            self.store(tgt, new)
            return new if pre else old
        if k == "tern":
            c, a, b = self.ex(e[1]), self.ex(e[2]), self.ex(e[3])
            if c.const is not None:
                return a if c.const else b
            ty = a.ty if a.ty == b.ty else (self.promote(a, b) or a.ty)
            return Ex(f"(sel {c.text} {self.conv(a, ty).text} {self.conv(b, ty).text})", ty)
        if k == "call":
            return self.call(e)
        raise ValueError(f"expr {k}")

    def cast(self, ct, x):
        ty = ctype_ty(ct)
        if ty[0] == "p":
            return Ex(x.text, ty, x.const)
        if ct[0] in ("code", "void"):
            return Ex(x.text, ("u", 8), x.const)
        return self.conv(x, ty) if x.ty[0] != "p" else self.conv(Ex(x.text, ("u", 8), x.const), ty)

    def deref(self, p):
        if p.ty[0] != "p":
            return UNDEF()
        return self.load(p, p.ty[2])

    def unary(self, e):
        op, a = e[1], e[2]
        if op == "*":
            if a[0] == "cast" and a[1][1] >= 1:
                inner = self.ex(a[2])
                ct = (a[1][0], a[1][1] - 1)
                return self.load(Ex(inner.text, ("u", 8)), ctype_ty(ct))
            return self.deref(self.ex(a))
        if op == "&":
            tgt = a
            if tgt[0] == "var" and tgt[1] in self.memvars:
                return Ex(f"(+ sp {self.memvars[tgt[1]]})", ("p", 8, self.vars[tgt[1]]))
            if tgt[0] == "var" and tgt[1] in self.arrays:
                return self.var_ref(tgt[1])
            if tgt[0] == "index":
                return self.add_ptr(self.ex(tgt[1]), self.ex(tgt[2]), 1)
            if tgt[0] == "un" and tgt[1] == "*":
                return self.ex(tgt[2])
            return UNDEF()
        x = self.ex(a)
        if x.ty[0] == "f":
            return UNDEF(("f", 4)) if op in "-+" else UNDEF(("s", 4))
        if op == "!":
            return Ex(f"(! {x.text})", ("s", 4), None if x.const is None else int(not x.const))
        ty = self.promote(x, Ex("0", ("s", 4))) if x.ty[0] != "p" else ("u", 8)
        x = self.conv(x, ty) if x.ty[0] != "p" else x
        if op == "-":
            return Ex(self.wrap(f"(neg {x.text})", ty), ty, None if x.const is None else wrap_const(-x.const, ty))
        if op == "~":
            return Ex(self.wrap(f"(~ {x.text})", ty), ty, None if x.const is None else wrap_const(~x.const, ty))
        return x

    def add_ptr(self, p, i, sign):
        if p.ty[0] == "p":
            size = esz(p.ty)
            off = i.text if size == 1 else f"(* {i.text} {size})"
            return Ex(f"(+ {p.text} {off})" if sign > 0 else f"(- {p.text} {off})", p.ty)
        return UNDEF()

    OPS = {"+": "+", "-": "-", "*": "*", "&": "&", "|": "|", "^": "^", "<<": "<<"}
    CMP = {"==": "==", "!=": "!=", "<": "<", "<=": "<=", ">": ">", ">=": ">="}
    UCMP = {"<": "ltu", "<=": "leu", ">": "gtu", ">=": "geu", "==": "==", "!=": "!="}

    def binary(self, e):
        op = e[1]
        if op in ("&&", "||"):
            mark = len(self.ops)
            a = self.ex(e[2])
            mark_b = len(self.ops)
            b = self.ex(e[3])
            if len(self.ops) == mark_b:  # pure right side: no control flow needed
                return Ex(f"({op} {a.text} {b.text})", ("s", 4))
            del self.ops[mark:]
            t, tl, fl, end = self.tmp(), self.label(), self.label(), self.label()
            self.cond(e, tl, fl)
            self.emit(f"(label {tl})")
            self.emit(f"(set {t} 1)")
            self.emit(f"(jmp {end})")
            self.emit(f"(label {fl})")
            self.emit(f"(set {t} 0)")
            self.emit(f"(label {end})")
            return Ex(t, ("s", 4))
        return self.arith(op, self.ex(e[2]), self.ex(e[3]))

    def arith(self, op, a, b):
        if a.ty[0] == "p" or b.ty[0] == "p":
            if op == "+" and a.ty[0] == "p" and b.ty[0] != "p":
                return self.add_ptr(a, b, 1)
            if op == "+" and b.ty[0] == "p" and a.ty[0] != "p":
                return self.add_ptr(b, a, 1)
            if op == "-" and a.ty[0] == "p" and b.ty[0] != "p":
                return self.add_ptr(a, b, -1)
            if op == "-" and a.ty[0] == "p" and b.ty[0] == "p":
                size = esz(a.ty)
                d = f"(- {a.text} {b.text})"
                return Ex(d if size == 1 else f"(div {d} {size})", ("s", 8))
            if op in self.CMP:
                return Ex(f"({self.UCMP[op]} {a.text} {b.text})", ("s", 4))
            a = Ex(a.text, ("u", 8), a.const) if a.ty[0] == "p" else a
            b = Ex(b.text, ("u", 8), b.const) if b.ty[0] == "p" else b
        ty = self.promote(a, b)
        if ty is None or a.text == "?" or b.text == "?":
            if op in self.CMP or op in ("&&", "||"):
                return UNDEF(("s", 4))
            return UNDEF(a.ty if a.ty[0] == "f" else b.ty)
        if op in ("<<", ">>"):
            ty = self.promote(a, Ex("0", ("s", 4)))
            x, y = self.conv(a, ty), b
            name = "<<" if op == "<<" else (">>" if ty[0] == "s" else "shru")
            return self.fold(name, x, y, ty)
        x, y = self.conv(a, ty), self.conv(b, ty)
        if op in self.CMP:
            name = self.UCMP[op] if ty[0] == "u" else self.CMP[op]
            return self.fold(name, x, y, ("s", 4), cmp=True)
        if op in ("/", "%"):
            name = {"/": "div", "%": "mod"}[op] + ("u" if ty[0] == "u" else "")
            return self.fold(name, x, y, ty)
        return self.fold(self.OPS[op], x, y, ty)

    def fold(self, name, x, y, ty, cmp=False):
        if x.const is not None and y.const is not None:
            v = pyop(name, x.const, y.const)
            if v is not None:
                v = v if cmp else wrap_const(v, ty)
                return Ex(str(v), ty, v)
        text = f"({name} {x.text} {y.text})"
        return Ex(text if cmp else self.wrap(text, ty), ty)

    # ---- stores / assignment ----------------------------------------------
    VSLOT_LOAD = re.compile(r"^\*\(code \*\*\)\(\*\(longlong \*\)(.+) \+ (0x[0-9a-f]+|\d+)\)$")

    def assign(self, e):
        _, op, lhs, rhs = e
        if op == "=" and lhs[0] == "var":
            m = self.VSLOT_LOAD.match(show(rhs))
            if m:  # function pointer fetched from an object's vtable: remember the slot
                self.vslots[lhs[1]] = int(m.group(2), 0)
        if op != "=":
            cur = self.ex(lhs)
            val = self.arith(op[:-1], cur, self.ex(rhs))
        else:
            val = self.ex(rhs)
        return self.store(lhs, val)

    def store(self, lhs, val):
        k = lhs[0]
        if k == "var":
            nm = lhs[1]
            if nm in self.memvars:
                ty = self.vars[nm]
                self.emit(f"(st {max(1, min(ty[1], 8)) if ty[0] != 'p' else 8} (+ sp {self.memvars[nm]}) {val.text})")
            else:
                ty = self.vars.get(nm)
                if ty is None:
                    return val
                if ty[0] == "f":
                    self.emit(f"(set {nm} ?)")
                else:
                    self.emit(f"(set {nm} {self.conv(val, ty).text if ty[0] != 'p' else val.text})")
            return val
        if k == "un" and lhs[1] == "*":
            tgt = lhs[2]
            if tgt[0] == "cast" and tgt[1][1] >= 1:
                addr = self.ex(tgt[2])
                ty = ctype_ty((tgt[1][0], tgt[1][1] - 1))
            else:
                p = self.ex(tgt)
                addr, ty = p, (p.ty[2] if p.ty[0] == "p" else ("u", 8))
            self.emit_store(addr.text, ty, val)
            return val
        if k == "index":
            p = self.add_ptr(self.ex(lhs[1]), self.ex(lhs[2]), 1)
            if p.ty[0] == "p":
                self.emit_store(p.text, p.ty[2], val)
            return val
        if k == "cast":  # assignment through a cast lvalue
            return self.store(lhs[2], val)
        return val

    def emit_store(self, addr, ty, val):
        w = ty[1] if ty[0] != "p" else 8
        if ty[0] == "f" or w not in (1, 2, 4, 8) or val.text == "?" and False:
            self.emit(f"(st {w if w in (1, 2, 4, 8) else 8} {addr} ?)")
        else:
            self.emit(f"(st {w} {addr} {val.text})")

    # ---- calls -------------------------------------------------------------
    def call(self, e):
        callee, args = e[1], e[2]
        if callee[0] == "var":
            nm = callee[1]
            if nm in PRIMS:
                t = self.tmp()
                kind = PRIMS[nm]
                self.emit(f"(set {t} (rd {kind}))")
                ty = {"F": ("u", 4), "B": ("u", 1), "H": ("u", 2), "I": ("u", 4), "Q": ("u", 8)}.get(kind, ("u", 8))
                return Ex(t if kind in "FBHIQ" else "?", ty)
            if CONCAT.match(nm) and len(args) == 2:
                m = CONCAT.match(nm)
                hi, lo = self.ex(args[0]), self.ex(args[1])
                lb = int(m.group(2))
                tot = int(m.group(1)) + lb
                ty = ("u", 8 if tot > 4 else 4)
                lo_m = f"(& {lo.text} {(1 << (8 * lb)) - 1})"
                return Ex(f"(| (<< {hi.text} {8 * lb}) {lo_m})", ty)
            m = SUBX.match(nm)
            if m and len(args) == 2:  # SUBmn(x, byte offset): n low bytes at that offset
                x, off = self.ex(args[0]), self.ex(args[1])
                n = int(m.group(1)) if False else int(m.group(2))
                ty = ("u", n if n in (1, 2, 4, 8) else 8)
                shifted = f"(shru {x.text} {8 * off.const})" if off.const else x.text
                return Ex(f"(& {shifted} {(1 << 8 * n) - 1})", ty)
            m = EXT.match(nm)
            if m and len(args) == 1:
                x = self.ex(args[0])
                n = int(m.group(2))
                return Ex(x.text, ("u" if nm[0] == "Z" else "s", n if n in (1, 2, 4, 8) else 8))
            if nm == "_CxxThrowException" or nm.endswith("noreturn") or nm in ("abort", "terminate"):
                self.emit("(throw native-throw)")
                return UNDEF()
            if nm in OPAQUE:
                return UNDEF()
            if nm in self.funcs:
                return self.user_call(nm, args)
            if any(mentions(a, self.stream_vars) for a in args):
                self.emit(f"(callx unread-helper-{nm})")
                return UNDEF()
            return UNDEF()  # allocator, string ops, logging: no stream effect
        txt = show(callee)
        slot = VTABLE_SLOT.match(txt)
        if callee[0] == "un" and callee[1] == "*" and callee[2][0] == "var" and callee[2][1] in self.vslots:
            slot = re.match(r"(.*)\+ (.*)", f"x + {self.vslots[callee[2][1]]}")  # slot offset only
        if slot and int(slot.group(2), 0) in (8, 0x10) and len(args) == 1:
            # SerType / current version of the object being deserialized
            return Ex("g_cls" if int(slot.group(2), 0) == 8 else "g_curver", ("u", 4))
        if slot and int(slot.group(2), 0) == 0x40 and len(args) >= 3:
            # body read of an embedded object: read(stream, version, ctx) on a known class
            ver = self.ex(args[2])
            self.emit(f"(rdbody {ver.text})")
            return UNDEF()
        if VTABLE_STREAM.search(txt) and len(args) >= 3:
            if txt.endswith("+ 0x10)"):
                dest, n = self.ex(args[1]), self.ex(args[2])
                self.emit(f"(rdn {dest.text} {n.text})")
                return UNDEF()
            self.emit(f"(callx stream-vtable-{VTABLE_STREAM.search(txt).group(1)})")
            return UNDEF()
        if any(mentions(a, self.stream_vars) for a in args):
            self.emit("(callx virtual-call-on-stream)")
        return UNDEF()

    def user_call(self, nm, args):
        callee = self.funcs[nm]
        vals = []
        for i, a in enumerate(args):
            x = self.ex(a)
            if i < len(callee[2]):
                ty = ctype_ty(callee[2][i][0])
                x = self.conv(x, ty) if ty[0] not in ("p", "f") and x.ty[0] != "p" else x
            vals.append(x.text)
        t = self.tmp()
        self.emit(f"(call {nm} {t} ({' '.join(vals)}))")
        return Ex(t, ctype_ty((self.ret_type(nm), 0)) if False else ("u", 8))

    def ret_type(self, nm):
        return "undefined8"

    # ---- control flow ------------------------------------------------------
    def cond(self, e, t, f):
        if e[0] == "bin" and e[1] == "&&":
            mid = self.label()
            self.cond(e[2], mid, f)
            self.emit(f"(label {mid})")
            return self.cond(e[3], t, f)
        if e[0] == "bin" and e[1] == "||":
            mid = self.label()
            self.cond(e[2], t, mid)
            self.emit(f"(label {mid})")
            return self.cond(e[3], t, f)
        if e[0] == "un" and e[1] == "!":
            return self.cond(e[2], f, t)
        x = self.ex(e)
        if x.const is not None:
            return self.emit(f"(jmp {t if x.const else f})")
        self.emit(f"(br {x.text} {t})")
        self.emit(f"(jmp {f})")

    def stmts(self, body):
        for s in body:
            self.stmt(s)

    def stmt(self, s):
        k = s[0]
        if k == "block":
            return self.stmts(s[1])
        if k == "expr":
            return self.ex(s[1]) and None
        if k == "if":
            t, f, end = self.label(), self.label(), self.label()
            self.cond(s[1], t, f)
            self.emit(f"(label {t})")
            self.stmt(s[2])
            if s[3] is not None:
                self.emit(f"(jmp {end})")
            self.emit(f"(label {f})")
            if s[3] is not None:
                self.stmt(s[3])
                self.emit(f"(label {end})")
            return
        if k == "while":
            head, body, end = self.label(), self.label(), self.label()
            self.emit(f"(label {head})")
            self.cond(s[1], body, end)
            self.emit(f"(label {body})")
            self.loop_body(s[2], head, end)
            self.emit(f"(jmp {head})")
            self.emit(f"(label {end})")
            return
        if k == "do":
            body, test, end = self.label(), self.label(), self.label()
            self.emit(f"(label {body})")
            self.loop_body(s[1], test, end)
            self.emit(f"(label {test})")
            self.cond(s[2], body, end)
            self.emit(f"(label {end})")
            return
        if k == "for":
            _, init, c, step, bd = s
            if init is not None:
                self.ex(init)
            head, body, cont, end = self.label(), self.label(), self.label(), self.label()
            self.emit(f"(label {head})")
            if c is not None:
                self.cond(c, body, end)
            self.emit(f"(label {body})")
            self.loop_body(bd, cont, end)
            self.emit(f"(label {cont})")
            if step is not None:
                self.ex(step)
            self.emit(f"(jmp {head})")
            self.emit(f"(label {end})")
            return
        if k == "switch":
            return self.switch(s)
        if k == "label":
            return self.emit(f"(label {self.user_label(s[1])})")
        if k == "goto":
            if s[1] not in self.all_labels:
                return self.emit(f"(callx goto-{s[1]})")
            return self.emit(f"(jmp {self.user_label(s[1])})")
        if k == "return":
            if s[1] is None:
                return self.emit("(ret)")
            x = self.ex(s[1])
            return self.emit(f"(ret {x.text})")
        if k == "break":
            return self.emit(f"(jmp {self.breaks[-1]})")
        if k == "continue":
            return self.emit(f"(jmp {self.continues[-1]})")
        raise ValueError(f"stmt {k}")

    def loop_body(self, body, cont, end):
        self.breaks.append(end)
        self.continues.append(cont)
        self.stmt(body)
        self.breaks.pop()
        self.continues.pop()

    def switch(self, s):
        x = self.ex(s[1])
        t = self.tmp()
        self.emit(f"(set {t} {x.text})")
        end = self.label()
        labels, default = [], None
        for st in s[2]:
            if st[0] == "case":
                lab = self.label()
                labels.append(lab)
                v = self.ex(st[1])
                self.emit(f"(br (== {t} {v.text}) {lab})")
            elif st[0] == "default":
                default = self.label()
                labels.append(default)
        self.emit(f"(jmp {default or end})")
        self.breaks.append(end)
        i = 0
        for st in s[2]:
            if st[0] in ("case", "default"):
                self.emit(f"(label {labels[i]})")
                i += 1
            else:
                self.stmt(st)
        self.breaks.pop()
        self.emit(f"(label {end})")

    def devirtualize(self, node):
        """`if (v == FUN_x) { FUN_x(); } else { (*v)(args); }` is profile-guided inlining of
        a virtual call: the guess is the object's own implementation, so keep the direct
        branch (with the arguments the else branch shows)."""
        if isinstance(node, list):
            return [self.devirtualize(x) for x in node]
        if not isinstance(node, tuple):
            return node
        if node[0] == "if" and node[1][0] == "bin" and node[1][1] == "==" and node[1][2][0] == "var" \
                and node[1][3][0] == "var" and node[1][3][1].startswith("FUN_") and node[1][2][1] in self.vptr_vars:
            then, other = node[2], node[3]
            fn = node[1][3][1]
            real = self.find_call(other, node[1][2][1])
            return self.devirtualize(self.fill_args(then, fn, real[2] if real else None))
        return tuple(self.devirtualize(x) if isinstance(x, (tuple, list)) else x for x in node)

    def find_call(self, node, var):
        if isinstance(node, tuple):
            if node[0] == "call" and node[1] == ("un", "*", ("var", var)):
                return node
            for x in node[1:]:
                r = self.find_call(x, var)
                if r:
                    return r
        elif isinstance(node, list):
            for x in node:
                r = self.find_call(x, var)
                if r:
                    return r
        return None

    def fill_args(self, node, fn, args):
        if isinstance(node, list):
            return [self.fill_args(x, fn, args) for x in node]
        if not isinstance(node, tuple):
            return node
        if node[0] == "call" and node[1] == ("var", fn) and not node[2] and args:
            return ("call", node[1], list(args))
        return tuple(self.fill_args(x, fn, args) if isinstance(x, (tuple, list)) else x for x in node)

    def run(self):
        self.all_labels = set()
        self.vptr_vars = set()

        def vars_loaded(n):
            if isinstance(n, tuple):
                if n[0] == "assign" and n[1] == "=" and n[2][0] == "var" and self.VSLOT_LOAD.match(show(n[3])):
                    self.vptr_vars.add(n[2][1])
                for x in n[1:]:
                    vars_loaded(x)
            elif isinstance(n, list):
                for x in n:
                    vars_loaded(x)

        vars_loaded(self.body)
        self.body = self.devirtualize(self.body)

        def labels(n):
            if isinstance(n, tuple):
                if n[0] == "label":
                    self.all_labels.add(n[1])
                for x in n[1:]:
                    labels(x)
            elif isinstance(n, list):
                for x in n:
                    labels(x)

        labels(self.body)
        self.stmt(self.body)
        self.emit("(ret)")
        return self.ops, self.frame


def wrap_const(v, ty):
    if ty[0] == "p" or ty[0] == "f":
        return v
    bits = ty[1] * 8
    v &= (1 << bits) - 1
    if ty[0] == "s" and v >> (bits - 1):
        v -= 1 << bits
    return v


def pyop(name, a, b):
    try:
        return {
            "+": lambda: a + b, "-": lambda: a - b, "*": lambda: a * b, "&": lambda: a & b,
            "|": lambda: a | b, "^": lambda: a ^ b, "<<": lambda: a << b, ">>": lambda: a >> b,
            "shru": lambda: (a & 0xFFFFFFFFFFFFFFFF) >> b, "==": lambda: int(a == b),
            "!=": lambda: int(a != b), "<": lambda: int(a < b), "<=": lambda: int(a <= b),
            ">": lambda: int(a > b), ">=": lambda: int(a >= b), "ltu": lambda: int(a < b),
            "leu": lambda: int(a <= b), "gtu": lambda: int(a > b), "geu": lambda: int(a >= b),
            "div": lambda: int(a / b), "divu": lambda: a // b, "mod": lambda: a - b * int(a / b),
            "modu": lambda: a % b,
        }[name]()
    except (ZeroDivisionError, KeyError, OverflowError):
        return None


def load_functions(paths):
    funcs = {}
    for p in paths:
        for name, addr, params, body, decls in parse_functions(Path(p).read_text(errors="replace")):
            if body[0] != "error":
                funcs[name] = (name, addr, params, body, decls)
            else:
                print(f"parse error {name}: {body[1][:100]}", file=sys.stderr)
    return funcs


def stream_params(funcs):
    """Parameters that carry the stream: passed to a primitive or a stream-taking callee."""
    res = {n: set() for n in funcs}
    changed = True
    while changed:
        changed = False
        for name, (_, _, params, body, _) in funcs.items():
            pnames = [p[1] for p in params]

            def walk(n):
                nonlocal changed
                if isinstance(n, tuple):
                    if n[0] == "call":
                        callee, args = n[1], n[2]
                        sp = None
                        if callee[0] == "var" and callee[1] in PRIMS:
                            sp = {PRIM_STREAM_ARG.get(callee[1], 0)}
                        elif callee[0] == "var" and callee[1] in res:
                            sp = {i for i, (_, pn) in enumerate(funcs[callee[1]][2]) if pn in res[callee[1]]}
                        elif callee[0] != "var" and VTABLE_STREAM.search(show(callee)):
                            for a in args[:1]:
                                for v in vars_in(a):
                                    if v in pnames and v not in res[name]:
                                        res[name].add(v)
                                        changed = True
                            m = re.findall(r"\((\w+) \+ 8\)", show(callee))
                            for v in m:
                                if v in pnames and v not in res[name]:
                                    res[name].add(v)
                                    changed = True
                        for i in sp or ():
                            if i < len(args):
                                for v in vars_in(args[i]):
                                    if v in pnames and v not in res[name]:
                                        res[name].add(v)
                                        changed = True
                    for x in n[1:]:
                        walk(x)
                elif isinstance(n, list):
                    for x in n:
                        walk(x)

            walk(body)
    return res


def vars_in(e):
    if isinstance(e, tuple):
        if e[0] == "var":
            return [e[1]]
        return [v for x in e[1:] for v in vars_in(x)]
    if isinstance(e, list):
        return [v for x in e for v in vars_in(x)]
    return []


def optimize(ops):
    """Fold `(set tN e)(set v tN)` copies and drop register writes nobody reads."""
    uses = {}
    for o in ops:
        for w in set(re.findall(r"[A-Za-z_]\w*", re.sub(r"^\(set \w+", "(set", o))):
            uses[w] = uses.get(w, 0) + 1
    out = []
    for op in ops:
        m = re.fullmatch(r"\(set (\w+) (t\d+)\)", op)
        if m and out and out[-1].startswith(f"(set {m.group(2)} ") and uses.get(m.group(2), 0) == 1:
            out[-1] = f"(set {m.group(1)} {out[-1][len(f'(set {m.group(2)} '):]}"
            continue
        out.append(op)
    return out


# Non-reading helpers whose effect on later branches is real: parameter-id remapping and
# "is this module type X" predicates. Everything else that never touches the stream
# (allocators, containers, string copies) is replaced by an opaque no-op.
SEMANTIC = {"FUN_140cf4c80", "FUN_14096bda0", "FUN_14096bdc0", "FUN_14096da90"}


def consumes_stream(progs):
    reads = {}

    def walk(n, seen):
        if n in reads:
            return reads[n]
        if n in seen or n not in progs:
            return False
        r = False
        for o in progs[n]["ops"]:
            if o.startswith(("(rdn", "(rdbody")) or "(rd " in o:
                r = True
                break
            m = re.match(r"\(call (\w+) ", o)
            if m and walk(m.group(1), seen | {n}):
                r = True
                break
        reads[n] = r
        return r

    return {n for n in progs if walk(n, frozenset())}


def prune(progs, keep):
    live = consumes_stream(progs) | SEMANTIC | set(keep)
    for n, p in progs.items():
        if n not in live:
            p["ops"] = ["(ret ?)"]
            p["frame"] = 0
    return progs


def lower_all(funcs, keep=()):
    streams = stream_params(funcs)
    out = {}
    for name, fn in funcs.items():
        if name in OVERRIDES:
            out[name] = {"params": [p[1] for p in fn[2]], "ops": OVERRIDES[name], "frame": 0, "addr": fn[1]}
            continue
        if name in PRIMS:
            continue
        low = Lowerer(fn, funcs, streams)
        ops, frame = low.run()
        ops = optimize(ops)
        out[name] = {"params": [p[1] for p in fn[2]], "ops": ops, "frame": frame, "addr": fn[1]}
    return prune(out, keep)


if __name__ == "__main__":
    funcs = load_functions(sys.argv[1:])
    progs = lower_all(funcs)
    print(len(progs), "programs", file=sys.stderr)
    for n, p in progs.items():
        print(f"== {n} {p['params']} frame={p['frame']}")
        print("\n".join(p["ops"]))
