#!/usr/bin/env python3
"""trace.py KONTAKT.nka OURS.nka -- the rows of a probe page's trace that differ.

A trace is the page's !log array as save_array_str writes it: `init,<what>,<value>` reads, then
`uc,<control>,<value>,<shift>,<alt>,<ctrl>` per ui_control, `ucs,<id>` and `pc`. Init reads are
matched by name; ui_control rows as runs of one control (see `runs`), aligned in order.
"""
import sys


def rows(path):
    lines = [l.strip() for l in open(path, encoding="latin-1").read().replace("\r", "").splitlines()]
    return [l for l in lines[1:] if l]


def runs(rows):
    """Callback rows as runs of one control: (control, modifiers, last value, count). A drag
    calls back once per motion event, as many as each host's pointer sends, so a run is
    compared by where it ends."""
    out = []
    for r in rows:
        if not r.startswith("uc,"):
            continue
        name, rest = r[3:].split(",", 1)
        value, mods = rest.rsplit(",", 3)[0], ",".join(rest.rsplit(",", 3)[1:])
        if out and out[-1][:2] == (name, mods):
            out[-1] = (name, mods, value, out[-1][3] + 1)
        else:
            out.append((name, mods, value, 1))
    return out


def main():
    k, o = rows(sys.argv[1]), rows(sys.argv[2])
    ki = {r.rsplit(",", 1)[0]: r.rsplit(",", 1)[1] for r in k if r.startswith("init,")}
    oi = {r.rsplit(",", 1)[0]: r.rsplit(",", 1)[1] for r in o if r.startswith("init,")}
    bad = 0
    for name in ki.keys() | oi.keys():
        if ki.get(name) != oi.get(name):
            print(f"{name[5:]:24} kontakt {ki.get(name, '-'):>8}  ours {oi.get(name, '-'):>8}")
            bad += 1
    kr, orr = runs(k), runs(o)
    import difflib
    sm = difflib.SequenceMatcher(a=[r[:2] for r in kr], b=[r[:2] for r in orr], autojunk=False)
    for tag, a0, a1, b0, b1 in sm.get_opcodes():
        if tag == "equal":
            for x, y in zip(kr[a0:a1], orr[b0:b1]):
                if x[2] != y[2]:
                    print(f"{x[0]:10} kontakt {x[2]:28} ({x[3]:3}x)  ours {y[2]:28} ({y[3]:3}x)")
                    bad += 1
            continue
        for x in kr[a0:a1]:
            print(f"{x[0]:10} kontakt {x[2]:28} ({x[3]:3}x)  ours -")
            bad += 1
        for y in orr[b0:b1]:
            print(f"{y[0]:10} kontakt {'-':28}         ours {y[2]:28} ({y[3]:3}x)")
            bad += 1
    print(f"{bad} rows differ")


if __name__ == "__main__":
    main()
