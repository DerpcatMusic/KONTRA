#!/usr/bin/env python3
"""UVI effects load through their one `UviModule` table (06-FORMATS §2).

#155 declared a `UviModule` per script-addressable UVI insert, but the load
path kept a second hand-written builder per effect (`dual_delay`,
`spark_verb`, `flanger`, `phasor`, `wave_shaper`, `diode_clipper`, `drive`,
`shelves`, `maximizer`) and `decode_model` re-listed every attribute with
its default. This codemod:

1. turns each module's `attrs: &["Name", ...]` into `Attr` rows carrying the
   default `decode_model` gave it (`Attr::num("Name", default)` or
   `Attr::flag("Name")`), then deletes those `decode_model` arms;
2. moves the builder files under `program/models/` (`git mv`, history kept),
   puts each module's static where its builder was (the RE notes above stay
   with it) and deletes the builder fns and their private helpers;
3. points those files' unit tests at the shared load path.

The generic load (`UviModule::load`), the load-only diagnostics, and the
`built`/`moves` call sites are hand-wired after this; the compiler lists them.
Run from plugins/kontra. Prints the production-line count before and after.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path("crates/format/uvi/src/program")
MODELS = ROOT / "models.rs"

# builder file (old path) -> new path, statics it receives, items it loses.
MOVES = {
    "delay.rs": ("models/delay.rs", ["DUAL_DELAY"], ["fn dual_delay", "fn rotation", "fn width", "fn mul", "const MAX_S"]),
    "reverb.rs": ("models/reverb.rs", ["SPARK_VERB"], ["fn spark_verb", "const SPEED_OF_SOUND"]),
    "routing/sweep.rs": ("models/sweep.rs", ["FLANGER", "PHASOR"], ["fn flanger", "fn phasor", "fn rate"]),
    "routing/shaper.rs": ("models/shaper.rs", ["WAVE_SHAPER", "DIODE_CLIPPER"], ["fn wave_shaper", "fn diode_clipper", "fn gain_of", "fn unsupported"]),
}

# Test call sites of the deleted builders -> the shared load path.
TEST_CALLS = [
    (r"\bdual_delay\(", "load_one("),
    (r"\bspark_verb\(", "load_one("),
    (r"\bflanger\(Params::of\((\w+)\)\)", r"load_one(\1, &mut Notes::default())"),
    (r"\bphasor\(Params::of\((\w+)\), ", r"load_one(\1, "),
    (r"\bwave_shaper\(Params::of\(([^()]*\(\))\), ", r"UviModule::load(\1, "),
]


def production_lines() -> int:
    total = 0
    for p in pathlib.Path("crates/format/uvi/src").rglob("*.rs"):
        if p.name == "tests.rs":
            continue
        lines = p.read_text().splitlines()
        n = len(lines)
        for i, line in enumerate(lines):
            if line == "#[cfg(test)]" and i + 1 < n and re.match(r"mod \w+ \{", lines[i + 1]):
                n = i
                break
        total += n
    return total


def item_span(text: str, head: str) -> tuple[int, int] | None:
    """Span of the top-level item whose line starts with `head` (after any
    visibility), with the `///` and `#[...]` lines above it and the blank
    line after it."""
    m = re.search(rf"(?m)^(?:pub(?:\([a-z]+\))? )?{re.escape(head)}" + (r"\b" if head[-1].isalnum() else ""), text)
    if not m:
        return None
    start = m.start()
    while True:  # doc comments and attributes above
        prev = text.rfind("\n", 0, start - 1) + 1
        line = text[prev:start - 1] if start > 0 else ""
        if start > 0 and (line.startswith("///") or line.startswith("#[")):
            start = prev
        else:
            break
    i, depth, opened = m.end(), 0, False
    while i < len(text):
        c = text[i]
        if c == '"':
            i = text.index('"', i + 1) + 1 if text[i - 1] != "'" else i + 1
            while text[i - 2] == "\\":
                i = text.index('"', i) + 1
            continue
        if c == "{":
            depth, opened = depth + 1, True
        elif c == "}":
            depth -= 1
        elif c == ";" and depth == 0:
            i += 1
            break
        i += 1
        if opened and depth == 0:
            if text[i:i + 1] == ";":
                i += 1
            break
    end = i
    while text[end:end + 1] == "\n":
        end += 1
    return start, end


def defaults(models: str) -> dict[str, list[tuple[str, str]]]:
    """Per tag, `decode_model`'s saved reads: (name, default or 'flag')."""
    out = {}
    for arm in re.finditer(r'(?ms)^        "(\w+)"(?: if .+?)? => \{\n(.*?)^        \}', models):
        reads = re.findall(r'(?:bool_val\("(\w+)"\)|val\("(\w+)", ([-\d._]+)\))', arm.group(2))
        if reads:
            out[arm.group(1)] = [(b or v, "flag" if b else d) for b, v, d in reads]
    return out


def attr_rows(reads, attrs, held) -> str:
    by_name = dict(reads)
    rows = []
    for name in attrs:
        d = by_name.get(name, "0.0")  # Drive's Mode is held at 0 by its guard
        if d == "flag":
            row = f'Attr::flag("{name}")'
        else:
            d = d if "." in d else d + ".0"
            row = f'Attr::num("{name}", {d})'
        if name in held:
            row += ".held()"
        rows.append(f"        {row},\n")
    return "    attrs: &[\n" + "".join(rows) + "    ],"


def main() -> None:
    before = production_lines()
    if (ROOT / "models/delay.rs").exists():
        print("uvi-mapping-tables: already applied")
        return
    models = MODELS.read_text()
    table = defaults(models)

    # 1. attrs -> Attr rows.
    def rows(m: re.Match) -> str:
        tag, body = m.group(1), m.group(2)
        names = re.findall(r'"(\w+)"', body)
        held = {"NormalizePower"} if tag == "SampledReverb" else set()
        return f'    tag: "{tag}",\n' + attr_rows(table.get(tag, []), names, held)

    models = re.sub(r'(?s)    tag: "(\w+)",\n    attrs: &\[(.*?)\],', lambda m: rows(m) if m.group(2).strip() else m.group(0), models)

    # 2. decode_model keeps only the DigitalEq arm (its knobs are per chunk).
    for tag in table:
        if tag == "DigitalEq":
            continue
        span = re.search(rf'(?ms)^        "{tag}"(?: if .+?)? => \{{\n.*?^        \}}\n', models)
        if span:
            models = models[: span.start()] + models[span.end():]

    # 3. statics move to their builder's file, replacing its fns.
    (ROOT / "models").mkdir(exist_ok=True)
    for old, (new, statics, dead) in MOVES.items():
        subprocess.run(["git", "mv", str(ROOT / old), str(ROOT / new)], check=True)
        path = ROOT / new
        text = path.read_text()
        moved = []
        for s in statics:
            span = item_span(models, f"static {s}:")
            moved.append(models[span[0]:span[1]])
            models = models[: span[0]] + models[span[1]:]
        first = item_span(text, dead[0])
        insert_at = first[0]
        text = text[:insert_at] + "".join(moved) + text[insert_at:]
        for d in dead:
            span = item_span(text, d)
            if span:
                text = text[: span[0]] + text[span[1]:]
        for pattern, repl in TEST_CALLS:
            text = re.sub(pattern, repl, text)
        path.write_text(text)

    for decl, owner in [("mod delay;\n", ROOT.parent / "program.rs"), ("mod reverb;\n", ROOT.parent / "program.rs"),
                        ("mod shaper;\n", ROOT / "routing.rs"), ("mod sweep;\n", ROOT / "routing.rs")]:
        owner.write_text(owner.read_text().replace(decl, "", 1))
    ignored = ROOT / "ignored.rs"  # scans every program source for quoted names
    text = ignored.read_text()
    for old, (new, _, _) in MOVES.items():
        text = text.replace(f'include_str!("{old}")', f'include_str!("{new}")')
    ignored.write_text(text)
    models = models.replace("use super::{flag, num};\n", "use super::{flag, num};\n\nmod delay;\nmod reverb;\nmod shaper;\nmod sweep;\n\npub use delay::DUAL_DELAY;\npub use reverb::SPARK_VERB;\npub use shaper::{DIODE_CLIPPER, WAVE_SHAPER};\npub use sweep::{FLANGER, PHASOR};\n", 1)
    MODELS.write_text(re.sub(r"\n{3,}", "\n\n", models))
    print(f"uvi-mapping-tables: format/uvi production lines {before} -> {production_lines()}")


if __name__ == "__main__":
    sys.exit(main())
