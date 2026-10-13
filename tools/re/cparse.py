#!/usr/bin/env python3
"""Parser for the subset of C that Ghidra's decompiler prints.

Used by reader_lower.py. Produces tuples:

  expressions  ('num', value, ctype) ('var', name) ('un', op, e) ('bin', op, a, b)
               ('cast', ctype, e) ('index', e, i) ('call', callee, [args])
               ('assign', op, lhs, rhs) ('tern', c, a, b) ('incdec', op, pre, e)
  statements   ('block', [s]) ('if', c, then, else) ('while', c, body)
               ('do', body, c) ('for', init, c, step, body) ('switch', e, [s])
               ('case', value) ('default',) ('label', name) ('goto', name)
               ('return', e) ('break',) ('continue',) ('expr', e)

A ctype is (base, pointer_depth) with base a Ghidra type word such as
`undefined4`, `longlong`, `uint`.
"""
import re
import sys

TOKEN = re.compile(
    r"""(?P<ws>\s+|/\*.*?\*/|//[^\n]*)
    |(?P<num>0[xX][0-9a-fA-F]+[uUlL]*|\d+\.\d*(?:[eE][-+]?\d+)?[fF]?|\d+[uUlL]*)
    |(?P<id>[A-Za-z_][A-Za-z_0-9]*(?:\._\d+_\d+_)?)
    |(?P<str>"(?:\\.|[^"\\])*")
    |(?P<chr>'(?:\\x[0-9a-fA-F]+|\\.|[^'\\])')
    |(?P<op>>>=|<<=|\+\+|--|->|<=|>=|==|!=|&&|\|\||<<|>>|[-+*/%&|^]=|[-+*/%&|^~!<>=?:;,.(){}\[\]])""",
    re.S | re.X,
)
TYPE_WORDS = {
    "int", "uint", "short", "ushort", "char", "byte", "bool", "float", "double", "longlong",
    "ulonglong", "long", "ulong", "code", "void", "float10", "size_t", "unsigned", "signed",
    "int3", "int5", "int6", "int7", "uint3", "uint5", "uint6", "uint7", "BOOL", "wchar_t",
}


def is_type_word(w):
    return w in TYPE_WORDS or re.fullmatch(r"undefined\d*", w) is not None or w == "struct"


def tokenize(text):
    out, pos = [], 0
    while pos < len(text):
        m = TOKEN.match(text, pos)
        if not m:
            raise SyntaxError(f"bad character {text[pos:pos+20]!r}")
        pos = m.end()
        if m.lastgroup != "ws":
            out.append((m.lastgroup, m.group()))
    return out


class Parser:
    def __init__(self, toks):
        self.t, self.i = toks, 0
        self.decls = {}  # name -> (ctype, array_len|None)

    def peek(self, k=0):
        return self.t[self.i + k][1] if self.i + k < len(self.t) else None

    def kind(self, k=0):
        return self.t[self.i + k][0] if self.i + k < len(self.t) else None

    def eat(self, s=None):
        tok = self.t[self.i]
        if s is not None and tok[1] != s:
            raise SyntaxError(f"expected {s!r}, got {tok[1]!r} at {self.i}: {' '.join(x[1] for x in self.t[max(0,self.i-8):self.i+8])}")
        self.i += 1
        return tok[1]

    # ---- types -------------------------------------------------------------
    def type_at(self, k=0):
        """If a type begins at offset k, return (ctype, tokens_consumed)."""
        j, words = k, []
        while self.kind(j) == "id" and is_type_word(self.peek(j)):
            words.append(self.peek(j))
            j += 1
        if not words:
            return None
        if words[0] == "struct" and self.kind(j) == "id":
            words.append(self.peek(j)); j += 1
        depth = 0
        while self.peek(j) == "*":
            depth += 1
            j += 1
        base = " ".join(w for w in words if w not in ("unsigned", "signed", "struct")) or "int"
        if "unsigned" in words:
            base = "u" + base
        return (base, depth), j - k

    # ---- expressions -------------------------------------------------------
    def expr(self):
        e = self.assign()
        while self.peek() == "," and self.kind() == "op":
            self.eat()
            e = ("comma", e, self.assign())
        return e

    def assign(self):
        lhs = self.tern()
        if self.peek() in ("=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>="):
            op = self.eat()
            return ("assign", op, lhs, self.assign())
        return lhs

    def tern(self):
        c = self.binary(0)
        if self.peek() == "?":
            self.eat()
            a = self.assign()
            self.eat(":")
            b = self.tern()
            return ("tern", c, a, b)
        return c

    LEVELS = [["||"], ["&&"], ["|"], ["^"], ["&"], ["==", "!="], ["<", "<=", ">", ">="],
              ["<<", ">>"], ["+", "-"], ["*", "/", "%"]]

    def binary(self, lvl):
        if lvl == len(self.LEVELS):
            return self.unary()
        lhs = self.binary(lvl + 1)
        while self.peek() in self.LEVELS[lvl] and self.kind() == "op":
            op = self.eat()
            lhs = ("bin", op, lhs, self.binary(lvl + 1))
        return lhs

    def unary(self):
        p = self.peek()
        if self.kind() == "op" and p in ("!", "~", "-", "+", "*", "&"):
            self.eat()
            return ("un", p, self.unary())
        if self.kind() == "op" and p in ("++", "--"):
            self.eat()
            return ("incdec", p, True, self.unary())
        if p == "(":
            ty = self.type_at(1)
            if ty and self.peek(1 + ty[1]) == ")":
                self.eat()
                self.i += ty[1]
                self.eat(")")
                return ("cast", ty[0], self.unary())
        if p == "sizeof":
            self.eat()
            self.eat("(")
            depth = 1
            while depth:
                depth += {"(": 1, ")": -1}.get(self.eat(), 0)
            return ("num", 8, ("int", 0))
        return self.postfix()

    def postfix(self):
        e = self.primary()
        while True:
            p = self.peek()
            if p == "(" and self.kind() == "op":
                self.eat()
                args = []
                while self.peek() != ")":
                    args.append(self.assign())
                    if self.peek() == ",":
                        self.eat()
                self.eat(")")
                e = ("call", e, args)
            elif p == "[":
                self.eat()
                idx = self.expr()
                self.eat("]")
                e = ("index", e, idx)
            elif p in ("++", "--") and self.kind() == "op":
                self.eat()
                e = ("incdec", p, False, e)
            elif p == "." and self.kind() == "op":
                self.eat()
                e = ("var", self.eat())  # member access on a local: opaque name
            elif p == "->" and self.kind() == "op":
                self.eat()
                e = ("var", self.eat())
            else:
                return e

    def primary(self):
        k, v = self.t[self.i]
        if k == "num":
            self.i += 1
            return parse_number(v)
        if k == "chr":
            self.i += 1
            body = v[1:-1]
            n = int(body[2:], 16) if body.startswith("\\x") else (ord(body[1]) if body.startswith("\\") and len(body) == 2 else ord(body))
            return ("num", n, ("char", 0))
        if k == "str":
            self.i += 1
            return ("var", "$str")
        if k == "id":
            self.i += 1
            return ("var", v)
        if v == "(":
            self.eat()
            e = self.expr()
            self.eat(")")
            return e
        raise SyntaxError(f"unexpected {v!r} at {self.i}: {' '.join(x[1] for x in self.t[max(0,self.i-8):self.i+8])}")

    # ---- statements --------------------------------------------------------
    def block(self):
        self.eat("{")
        body = []
        while self.peek() != "}":
            s = self.stmt()
            if s is not None:
                body.append(s)
        self.eat("}")
        return ("block", body)

    def decl(self):
        """`type name[N], *other;` at the top of a function."""
        ty = self.type_at()
        if not ty:
            return False
        ctype, n = ty
        j = self.i + n
        # a declaration is type followed by an identifier then [ ; , [ ]
        if self.kind(n) != "id" or self.peek(n + 1) not in (";", ",", "["):
            return False
        self.i += n
        while True:
            depth = 0
            while self.peek() == "*":
                self.eat()
                depth += 1
            name = self.eat()
            length = None
            if self.peek() == "[":
                self.eat()
                length = self.expr()
                self.eat("]")
            self.decls[name] = ((ctype[0], ctype[1] + depth), length)
            if self.peek() == ",":
                self.eat()
                continue
            self.eat(";")
            return True

    def stmt(self):
        p, k = self.peek(), self.kind()
        if p == "{":
            return self.block()
        if p == ";":
            self.eat()
            return None
        if k == "id":
            if p == "if":
                self.eat(); self.eat("(")
                c = self.expr()
                self.eat(")")
                then = self.stmt()
                other = None
                if self.peek() == "else":
                    self.eat()
                    other = self.stmt()
                return ("if", c, then, other)
            if p == "while":
                self.eat(); self.eat("(")
                c = self.expr()
                self.eat(")")
                return ("while", c, self.stmt())
            if p == "do":
                self.eat()
                body = self.stmt()
                self.eat("while"); self.eat("(")
                c = self.expr()
                self.eat(")"); self.eat(";")
                return ("do", body, c)
            if p == "for":
                self.eat(); self.eat("(")
                init = None if self.peek() == ";" else self.expr()
                self.eat(";")
                c = None if self.peek() == ";" else self.expr()
                self.eat(";")
                step = None if self.peek() == ")" else self.expr()
                self.eat(")")
                return ("for", init, c, step, self.stmt())
            if p == "switch":
                self.eat(); self.eat("(")
                e = self.expr()
                self.eat(")")
                return ("switch", e, self.block()[1])
            if p == "case":
                self.eat()
                v = self.expr()
                self.eat(":")
                return ("case", v)
            if p == "default" and self.peek(1) == ":":
                self.eat(); self.eat(":")
                return ("default",)
            if p == "goto":
                self.eat()
                name = self.eat()
                self.eat(";")
                return ("goto", name)
            if p == "return":
                self.eat()
                e = None if self.peek() == ";" else self.expr()
                self.eat(";")
                return ("return", e)
            if p == "break":
                self.eat(); self.eat(";")
                return ("break",)
            if p == "continue":
                self.eat(); self.eat(";")
                return ("continue",)
            if self.peek(1) == ":" and self.kind(1) == "op" and not is_type_word(p):
                self.eat(); self.eat(":")
                return ("label", p)
            if self.decl():
                return None
        e = self.expr()
        self.eat(";")
        return ("expr", e)


def parse_number(v):
    low = v.lower()
    if "." in low or (low.endswith("f") and not low.startswith("0x")):
        return ("fnum", float(low.rstrip("f")), ("float", 0))
    suffix = re.search(r"[ul]*$", low).group()
    digits = low[: len(low) - len(suffix)]
    n = int(digits, 0)
    unsigned = "u" in suffix
    wide = "l" in suffix or n > 0xFFFFFFFF
    if not unsigned and not wide and digits.startswith("0x") and n > 0x7FFFFFFF:
        unsigned = True
    base = ("ulonglong" if unsigned else "longlong") if wide else ("uint" if unsigned else "int")
    return ("num", n, (base, 0))


def parse_functions(text):
    """Yield (name, address_hex, params[(ctype, name)], body, decls) for every function."""
    sections = re.split(r"^// ===== depth \d+ (FUN_[0-9a-f]+) @ ([0-9a-f]+)\s*$", text, flags=re.M)
    for i in range(1, len(sections), 3):
        name, addr, body = sections[i], sections[i + 1], sections[i + 2]
        m = re.search(r"\b" + name + r"\(([^)]*)\)\s*\{", body)
        if not m:
            continue
        params = []
        for part in m.group(1).split(","):
            part = part.strip()
            if not part or part == "void":
                continue
            toks = tokenize(part)
            nm = toks[-1][1]
            base = [t[1] for t in toks[:-1] if t[1] != "*"]
            params.append(((" ".join(base), sum(1 for t in toks if t[1] == "*")), nm))
        src = body[m.end() - 1:]
        try:
            p = Parser(tokenize(src))
            blk = p.block()
        except (SyntaxError, IndexError) as e:
            yield name, addr, params, ("error", str(e)), {}
            continue
        yield name, addr, params, blk, p.decls


if __name__ == "__main__":
    bad = 0
    for path in sys.argv[1:]:
        n = 0
        for name, addr, params, body, decls in parse_functions(open(path, errors="replace").read()):
            n += 1
            if body[0] == "error":
                bad += 1
                print("PARSE ERROR", name, body[1][:300])
        print(path, n, "functions")
    sys.exit(1 if bad else 0)
