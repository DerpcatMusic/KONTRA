#!/usr/bin/env python3
"""Reference interpreter for the reader IR (see reader_lower.py).

The Rust executor in vendor/ni-file/src/kontakt/native_schema/ implements the same
semantics; this one drives layout rendering, version probing and coverage reports
without a build.

Two things are not in the stream and are therefore searched, not guessed:
  * values Kontakt validates (counts, enums) when synthesizing a sample stream;
  * branches on host state (`?` conditions): each is tried both ways.
A stream decodes when some assignment of those choices consumes it exactly.
"""
import re

M64 = (1 << 64) - 1
CURRENT = {}  # SerType -> current version, set by reader_schemas.load
OBJ = 0x1000_0000
STREAM = 0x2000_0000
CTX = 0x3000_0000
TABLE = 0x4000_0000


def parse_sexpr(text):
    toks = re.findall(r"\(|\)|[^\s()]+", text)
    pos = 0

    def rd():
        nonlocal pos
        t = toks[pos]
        pos += 1
        if t == "(":
            out = []
            while toks[pos] != ")":
                out.append(rd())
            pos += 1
            return out
        return t

    out = []
    while pos < len(toks):
        out.append(rd())
    return out


def compile_program(ops):
    instrs, labels = [], {}
    for line in ops:
        (ins,) = parse_sexpr(line)
        if ins[0] == "label":
            labels[ins[1]] = len(instrs)
        else:
            instrs.append(ins)
    return instrs, labels


class Stop(Exception):
    def __init__(self, status):
        self.status = status


def trunc(v, bits, signed):
    v &= (1 << bits) - 1
    return v - (1 << bits) if signed and v >> (bits - 1) else v


class Chooser:
    """Replays a prefix of recorded choices; later choices take their first candidate."""

    def __init__(self, prefix):
        self.prefix, self.taken, self.sizes, self.kinds, self.sites = prefix, [], [], [], []

    def pick(self, n, kind="v", site=None):
        i = len(self.taken)
        c = self.prefix[i] if i < len(self.prefix) else 0
        self.taken.append(c)
        self.sizes.append(n)
        self.kinds.append(kind)
        self.sites.append(site)
        return c


class RandomChooser(Chooser):
    """Seeded exploration: biased towards 0 and the program's own comparison constants."""

    def __init__(self, rng):
        super().__init__([])
        self.rng = rng

    def pick(self, n, kind="v", site=None):
        c = 0 if self.rng.random() < 0.25 else self.rng.randrange(n)
        self.taken.append(c)
        self.sizes.append(n)
        self.kinds.append(kind)
        self.sites.append(site)
        return c


class Stream:
    def __init__(self, data, chooser, values):
        self.data, self.chooser, self.values = data, chooser, values
        self.pos = 0
        self.out = bytearray()
        self.reads = []

    def take(self, n, kind="R"):
        if self.data is None:
            if self.pos > 1 << 14:
                raise Stop("synthetic-stream-too-long")
            if kind in "BHIQ" and n <= 8:
                v = self.values[self.chooser.pick(len(self.values))]
                b = (v & ((1 << 8 * n) - 1)).to_bytes(n, "little")
            else:
                b = bytes(n)
            self.out += b
            self.pos += n
            return b
        if self.pos + n > len(self.data):
            raise Stop("short")
        b = self.data[self.pos:self.pos + n]
        self.pos += n
        return b

    def sub(self, n):
        """A bounded nested stream over the next n bytes (decode) or n synthesized bytes."""
        if self.data is None:
            return Stream(None, self.chooser, self.values)
        if self.pos + n > len(self.data):
            raise Stop("short")
        s = Stream(self.data[self.pos:self.pos + n], self.chooser, self.values)
        self.pos += n
        return s


class Machine:
    def __init__(self, progs, readers, stream, chooser, steps=300000, cls=None):
        self.progs, self.readers, self.stream, self.chooser = progs, readers, stream, chooser
        self.mem = {}
        self.steps = steps
        self.next_sp = 0x7000_0000_0000
        self.compiled = {}
        self.globals = {"g_cls": cls, "g_curver": None}
        self.store(STREAM + 0x10, 4, 1)  # byte-order flag: 1 = no swap
        self.store(STREAM + 8, 8, STREAM + 0x100)  # inner stream object, healthy state
        self.store(STREAM + 0x100 + 0xC, 4, 1)
        self.store(CTX + 0x280, 8, TABLE)  # instrument tables: huge, so any index is in range
        self.store(CTX + 0x288, 8, TABLE + 0x380 * (1 << 20))
        self.store(CTX + 0x3c, 1, None)
        self.undef_hits = []
        self.undef_exprs = []
        self.cov = set()
        self.nobj = 0

    def prog(self, name):
        if name not in self.compiled:
            p = self.progs[name]
            self.compiled[name] = (p, *compile_program(p["ops"]))
        return self.compiled[name]

    def store(self, addr, n, v):
        if addr is None:
            return
        for i in range(n):
            if v is None:
                self.mem.pop(addr + i, None)
            else:
                self.mem[addr + i] = (v >> (8 * i)) & 0xFF

    def load(self, addr, n, signed):
        if addr is None:
            return None
        v = 0
        for i in range(n):
            b = self.mem.get(addr + i)
            if b is None:
                return None
            v |= b << (8 * i)
        return trunc(v, 8 * n, signed) if n < 8 else trunc(v, 64, True)

    def reg(self, regs, name):
        if name in regs:
            return regs[name]
        return self.globals.get(name)

    def ev(self, e, regs):
        if isinstance(e, str):
            if e == "?":
                return None
            if re.fullmatch(r"-?\d+", e):
                return int(e)
            return self.reg(regs, e)
        op = e[0]
        if op == "rd":
            return self.read(e[1])
        if op == "&&":
            a = self.ev(e[1], regs)
            if a == 0:
                return 0
            b = self.ev(e[2], regs)
            if b == 0:
                return 0
            return None if a is None or b is None else 1
        if op == "||":
            a = self.ev(e[1], regs)
            if a not in (0, None):
                return 1
            b = self.ev(e[2], regs)
            if b not in (0, None):
                return 1
            return None if a is None or b is None else 0
        if op == "sel":
            c = self.ev(e[1], regs)
            return None if c is None else self.ev(e[2] if c else e[3], regs)
        args = [self.ev(x, regs) for x in e[1:]]
        if any(a is None for a in args):
            return None
        if op.startswith("ld"):
            return self.load(args[0], int(op[2]), op.endswith("s"))
        m = re.fullmatch(r"([su])(8|16|32)", op)
        if m:
            return trunc(args[0], int(m.group(2)), m.group(1) == "s")
        a = args[0]
        if op == "!":
            return int(a == 0)
        if op == "~":
            return ~a
        if op == "neg":
            return -a
        b = args[1]
        try:
            return {
                "+": lambda: a + b, "-": lambda: a - b, "*": lambda: a * b, "&": lambda: a & b,
                "|": lambda: a | b, "^": lambda: a ^ b, "<<": lambda: a << b if 0 <= b < 64 else 0,
                ">>": lambda: a >> b if 0 <= b < 64 else (0 if a >= 0 else -1),
                "shru": lambda: (a & M64) >> b if 0 <= b < 64 else 0,
                "==": lambda: int(a == b), "!=": lambda: int(a != b), "<": lambda: int(a < b),
                "<=": lambda: int(a <= b), ">": lambda: int(a > b), ">=": lambda: int(a >= b),
                "ltu": lambda: int((a & M64) < (b & M64)), "leu": lambda: int((a & M64) <= (b & M64)),
                "gtu": lambda: int((a & M64) > (b & M64)), "geu": lambda: int((a & M64) >= (b & M64)),
                "div": lambda: int(a / b), "divu": lambda: (a & M64) // (b & M64),
                "mod": lambda: a - b * int(a / b), "modu": lambda: (a & M64) % (b & M64),
            }[op]()
        except ZeroDivisionError:
            return None

    def read(self, kind):
        s = self.stream
        start = s.pos
        if kind in "FBHIQ":
            n = {"F": 4, "B": 1, "H": 2, "I": 4, "Q": 8}[kind]
            v = int.from_bytes(s.take(n, kind if kind != "F" else "R"), "little")
            s.reads.append((kind, start, n, v))
            return v
        if kind in "SW":
            n = int.from_bytes(s.take(4, "I"), "little")
            if n > 1 << 24:
                raise Stop("string-length")
            s.take(n * (2 if kind == "W" else 1))
            s.reads.append((kind, start, s.pos - start, n))
            return None
        if kind == "O":
            return self.read_object(start)
        raise Stop("bad-read-kind")

    def read_object(self, start):
        """Nested object: u8 structured, u16 version, then (structured) three length-prefixed
        areas or (inline) the class reader's own fields. The class comes from `g_cls`."""
        s = self.stream
        flag = s.take(1, "B")[0]
        if flag > 1:
            raise Stop("throw")  # Kontakt compares the flag with the class's own
        ver = int.from_bytes(s.take(2, "H"), "little")
        cls = self.globals.get("g_cls")
        prog = self.readers.get(cls) if cls is not None else None
        if prog is not None and ver > CURRENT.get(cls, 0xFFFF):
            raise Stop("throw")  # newer than the class understands
        if flag == 0:
            if prog is None:
                s.reads.append(("O?", start, 3, ver))
                raise Stop("unframed-nested-object-unknown-class")
            s.reads.append(("O", start, 3, ver))
            self.run_nested(prog, ver)
            return None
        if s.data is None:  # synthesize: empty private/children areas, public = the body
            s.out += bytes(4)
            s.pos += 4
            body = Stream(None, self.chooser, s.values)
            if prog:
                saved, self.stream = self.stream, body
                try:
                    self.run_nested(prog, ver)
                finally:
                    self.stream = saved
            s.out += len(body.out).to_bytes(4, "little") + body.out + bytes(4)
            s.pos += 8 + len(body.out)
            s.reads.append(("O" if prog else "O*", start, s.pos - start, ver))
            return None
        areas = []
        for i in range(3):
            n = int.from_bytes(s.take(4, "I"), "little")
            if n > len(s.data) - s.pos:
                raise Stop("short")
            areas.append(s.sub(n))
        if prog:
            saved, self.stream = self.stream, areas[1]
            try:
                self.run_nested(prog, ver)
                if areas[1].pos != len(areas[1].data):
                    raise Stop("nested-trailing")
            finally:
                self.stream = saved
        s.reads.append(("O" if prog else "O*", start, s.pos - start, ver))
        return None

    def run_nested(self, prog, ver):
        self.nobj += 1
        self.call(prog, [OBJ + 0x100000 * self.nobj, STREAM, ver, CTX], 1)

    def call(self, name, args, depth=0):
        if depth > 24:
            raise Stop("recursion")
        p, instrs, labels = self.prog(name)
        regs = dict(zip(p["params"], args))
        regs["sp"] = self.next_sp
        self.next_sp += max(p["frame"], 16) + 0x1000
        pc = 0
        while pc < len(instrs):
            self.steps -= 1
            if self.steps < 0:
                raise Stop("step-limit")
            ins = instrs[pc]
            pc += 1
            op = ins[0]
            if op == "set":
                regs[ins[1]] = self.ev(ins[2], regs)
            elif op == "st":
                self.store(self.ev(ins[2], regs), int(ins[1]), self.ev(ins[3], regs))
            elif op in ("br", "brb", "brt"):
                c = self.ev(ins[1], regs)
                if c is None and op != "br":
                    c = 1 if op == "brt" else 0  # proven not to matter: fixed direction
                elif c is None:
                    self.undef_hits.append(name)
                    self.undef_exprs.append(ins[1])
                    c = self.chooser.pick(2, "b", (name, pc - 1))
                self.cov.add((name, pc, bool(c)))
                if c:
                    pc = labels[ins[2]]
            elif op == "jmp":
                pc = labels[ins[1]]
            elif op == "rdn":
                n = self.ev(ins[2], regs)
                if n is None or n < 0 or n > 1 << 26:
                    raise Stop("raw-length")
                start = self.stream.pos
                self.stream.take(n)
                self.stream.reads.append(("R", start, n, None))
            elif op == "rdbody":
                ver = self.ev(ins[1], regs)
                prog = self.readers.get(self.globals.get("g_cls"))
                if prog is None or ver is None:
                    raise Stop("nested-class-unknown")
                self.run_nested(prog, ver)
            elif op == "call":
                vals = [self.ev(a, regs) for a in ins[3]]
                regs[ins[2]] = self.call(ins[1], vals, depth + 1)
            elif op == "ret":
                return self.ev(ins[1], regs) if len(ins) > 1 else None
            elif op == "throw":
                raise Stop("throw")
            elif op == "callx":
                raise Stop("callx:" + ins[1])
            else:
                raise Stop("bad-op")
        return None


SYNTH_VALUES = [0, 1, 2, 3, 4, 5, 6, 7]


def attempt(progs, readers, name, version, data, prefix, values, cls, limit_steps=300000):
    chooser = Chooser(prefix)
    st = Stream(data, chooser, values)
    m = Machine(progs, readers, st, chooser, limit_steps, cls)
    try:
        m.call(name, [OBJ, STREAM, version, CTX])
        status = "ok"
        if data is not None and st.pos != len(data):
            status = "trailing"
    except Stop as s:
        status = s.status
    return status, st, m, chooser


def search(progs, readers, name, version, data=None, values=SYNTH_VALUES, cls=None, budget=4000, need_reads=False):
    """DFS over validated values / host-state branches.

    -> (status, stream, machine, choices_taken). `status` is "ok" on success, else the
    failure of the last attempt that got furthest.
    """
    prefix, best = [], None
    for _ in range(budget):
        status, st, m, ch = attempt(progs, readers, name, version, data, prefix, values, cls)
        if status == "ok" and not (need_reads and not st.reads):
            return status, st, m, ch.taken
        if best is None or st.pos >= best[1].pos or best[0].startswith("short"):
            best = (status, st, m, ch.taken)
        if status.startswith("callx") or status == "step-limit":
            return best
        taken, sizes = ch.taken, ch.sizes
        i = len(taken) - 1
        while i >= 0 and taken[i] + 1 >= sizes[i]:
            i -= 1
        if i < 0:
            break
        prefix = taken[:i] + [taken[i] + 1]
    return best


def constants(progs, names):
    """Integer literals in branch conditions of the programs reachable from `names`."""
    seen, todo, out = set(), list(names), set()
    while todo:
        n = todo.pop()
        if n in seen or n not in progs:
            continue
        seen.add(n)
        for op in progs[n]["ops"]:
            m = re.match(r"\(call (\w+) ", op)
            if m:
                todo.append(m.group(1))
            if op.startswith(("(br ", "(brb ", "(brt ")):
                out |= {int(x) for x in re.findall(r"(?<![\w-])(\d+)(?![\w])", op)}
    return sorted(c for c in out if c <= 255)


def explore(progs, readers, name, version, cls=None, tries=400, seed=1):
    """Random valid streams with branch coverage: -> [(coverage_gain_set, stream, machine)].

    Keeps every run that reaches a branch edge no earlier kept run reached.
    """
    import random
    rng = random.Random(seed * 7919 + version)
    values = sorted(set(SYNTH_VALUES) | set(constants(progs, [name])))
    kept, covered, idle = [], set(), 0
    for _ in range(tries):
        if idle > 40:
            break
        idle += 1
        chooser = RandomChooser(rng)
        st = Stream(None, chooser, values)
        m = Machine(progs, readers, st, chooser, 60000, cls)
        try:
            m.call(name, [OBJ, STREAM, version, CTX])
        except Stop:
            continue
        gain = m.cov - covered
        if gain and st.reads:
            covered |= m.cov
            kept.append((gain, st, m, chooser))
            idle = 0
    return kept


def host_dependent(progs, readers, name, version, values, cls, run):
    """Flip each host-state branch of a run -> {site: [(original direction, outcome)]}.

    outcome: "same" (still decodes, same reads), "diff" (decodes differently), "fail".
    """
    st, ch = run
    base = [(k, n) for k, _, n, _ in st.reads]
    out = {}
    for i in [i for i, k in enumerate(ch.kinds) if k == "b"][:48]:
        site = ch.sites[i]
        prefix = list(ch.taken)
        prefix[i] = 1 - prefix[i]
        status, st2, m2, _ = attempt(progs, readers, name, version, None, prefix, values, cls)
        if status != "ok":
            res = "fail"
        elif [(k, n) for k, _, n, _ in st2.reads] != base:
            res = "diff"
        else:
            res = "same"
        out.setdefault(site, []).append((ch.taken[i], res))
    return out
