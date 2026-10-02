#!/usr/bin/env python3
"""Developer-only @component subset; no production UI or Kontakt parity claim."""
import argparse
from dataclasses import dataclass
from pathlib import Path
import re


@dataclass
class Token:
    text: str
    line: int


def tokens(source):
    if len(source.encode()) > 1024 * 1024:
        raise ValueError("module exceeds 1 MiB")
    result, at, line = [], 0, 1
    while at < len(source):
        start = at
        long = re.match(r"(?:--)?\[(=*)\[", source[at:])
        if long:
            end = source.find("]" + long[1] + "]", at + len(long[0]))
            if end < 0:
                raise ValueError(f"line {line}: unclosed long string/comment")
            at = end + len(long[1]) + 2
        elif source.startswith("--", at):
            at = source.find("\n", at)
            if at < 0:
                at = len(source)
        elif source[at] in "\"'":
            quote = source[at]
            at += 1
            while at < len(source) and source[at] != quote:
                at += 2 if source[at] == "\\" else 1
            if at >= len(source):
                raise ValueError(f"line {line}: unclosed string")
            at += 1
        else:
            match = re.match(r"[A-Za-z_]\w*|[^\S\n]+|.", source[at:])
            at += len(match[0]) if match else 1
        text = source[start:at]
        result.append(Token(text, line))
        line += text.count("\n")
    return result


class Frontend:
    def __init__(self, source):
        self.ts, self.at, self.nodes, self.depth = tokens(source), 0, 0, 0

    def skip(self):
        while self.at < len(self.ts) and (
            self.ts[self.at].text.isspace() or self.ts[self.at].text.startswith("--")
        ):
            self.at += 1

    def require(self, expected):
        self.skip()
        if self.at == len(self.ts) or self.ts[self.at].text != expected:
            line = self.ts[min(self.at, len(self.ts) - 1)].line
            raise ValueError(f"line {line}: expected {expected!r}")
        self.at += 1

    def expression(self, closing):
        out, delimiters, blocks = [], [], 0
        while self.at < len(self.ts):
            t = self.ts[self.at].text
            if not delimiters and not blocks and t in [closing, ",", ";", "\n"]:
                break
            if t == "@":
                out.append(self.component())
                continue
            if t in ["(", "{", "["]:
                delimiters.append({"(": ")", "{": "}", "[": "]"}[t])
            elif t in [")", "}", "]"]:
                if not delimiters or delimiters.pop() != t:
                    raise ValueError(f"line {self.ts[self.at].line}: unbalanced expression")
            elif t in ["function", "if", "do", "repeat"]:
                blocks += 1
            elif t in ["end", "until"]:
                blocks -= 1
                if blocks < 0:
                    raise ValueError(f"line {self.ts[self.at].line}: unexpected {t}")
            out.append(t)
            self.at += 1
        if delimiters or blocks or not "".join(out).strip():
            raise ValueError("incomplete component expression")
        return "".join(out).strip()

    def fields(self, closing, children):
        props, content, seen = [], [], set()
        while True:
            self.skip()
            if self.at == len(self.ts):
                raise ValueError(f"unclosed component, expected {closing}")
            if self.ts[self.at].text == closing:
                self.at += 1
                return props, content
            if self.ts[self.at].text in [",", ";"]:
                self.at += 1
                continue
            name, start = self.ts[self.at].text, self.at
            self.at += 1
            self.skip()
            named = (self.at < len(self.ts) and re.fullmatch(r"[A-Za-z_]\w*", name)
                     and self.ts[self.at].text == "=")
            if named:
                if name in seen:
                    raise ValueError(f"duplicate component property {name}")
                seen.add(name)
                self.at += 1
                props.append(f"{name}={self.expression(closing)}")
            elif children:
                self.at = start
                content.append(self.expression(closing))
            else:
                raise ValueError(f"line {self.ts[start].line}: modifier requires named arguments")

    def component(self):
        self.depth += 1
        try:
            if self.depth > 64:
                raise ValueError("component nesting exceeds 64")
            return self._component()
        finally:
            self.depth -= 1

    def _component(self):
        self.require("@")
        self.skip()
        if self.at == len(self.ts):
            raise ValueError("expected component name")
        name = self.ts[self.at].text
        self.at += 1
        while self.at < len(self.ts) and self.ts[self.at].text == ".":
            if self.at + 1 == len(self.ts):
                raise ValueError("incomplete component name")
            name += "." + self.ts[self.at + 1].text
            self.at += 2
        if not re.fullmatch(r"[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*", name):
            raise ValueError("invalid component name")
        self.require("{")
        props, children = self.fields("}", True)
        self.nodes += 1
        if self.nodes > 4096:
            raise ValueError("component count exceeds 4096")
        expression = f"native_component({name},{{{','.join(props)}}},{{{','.join(children)}}})"
        while True:
            checkpoint = self.at
            self.skip()
            if self.at == len(self.ts) or self.ts[self.at].text != ".":
                self.at = checkpoint
                return expression
            self.at += 1
            if self.at == len(self.ts):
                raise ValueError("expected modifier name")
            modifier = self.ts[self.at].text
            self.at += 1
            self.require("(")
            props, _ = self.fields(")", False)
            expression = f'native_modifier({expression},"{modifier}",{{{",".join(props)}}})'

    def lower(self):
        out = []
        while self.at < len(self.ts):
            if self.ts[self.at].text == "@":
                out.append(self.component())
            else:
                out.append(self.ts[self.at].text)
                self.at += 1
        return "".join(out)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("resources", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    source_root, target_root = args.resources.resolve(), args.output.resolve()
    if source_root.is_relative_to(target_root) or target_root.is_relative_to(source_root):
        raise ValueError("lowered output must be separate from source resources")
    count, size = 0, 0
    for source in sorted(args.resources.rglob("*.nui")):
        if source.is_symlink():
            raise ValueError("symlinked resource is unavailable")
        count += 1
        size += source.stat().st_size
        if count > 256 or size > 8 * 1024 * 1024:
            raise ValueError("module bundle exceeds developer probe limits")
        target = args.output / source.relative_to(args.resources)
        target.parent.mkdir(parents=True, exist_ok=True)
        try:
            target.write_text(Frontend(source.read_text()).lower())
        except ValueError as error:
            raise ValueError(f"{source}: {error}") from error
    print(f"lowered_modules={count} production_runtime=false", flush=True)


if __name__ == "__main__":
    main()
