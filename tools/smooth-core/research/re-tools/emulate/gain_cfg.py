"""Basic-block CFG of the gain body 0x8e04c0..0x8e4be2 (section 20 of ../../re-latency-fft-scale.md).

Two sources, kept apart in the output:
  static  capstone linear sweep of the body range; leaders are the entry, every direct jcc/jmp
          target inside the range, and every instruction after a jcc/jmp/ret. Edges: jcc gives
          taken + fallthrough, jmp gives its target, ret gives exit, call falls through.
          Indirect jumps are listed as `indirect` (no edges invented).
  traced  one emulated process block (48 kHz stand-in, same setup as gain_chain_probe.py) with
          every executed instruction recorded in order. A block is `executed` if any of its
          instructions ran; an edge is `observed` if the trace moved from the last instruction
          of the source block to the first instruction of the target block.

Output (written next to this file, or to argv[1]):
  gain_body_cfg.json  nodes and edges with both static and traced flags
  gain_body_cfg.dot   the same graph for graphviz

Evidence scope: isolated emulation, stand-in state (identity band, no solo), so blocks that only
run for non-identity parameters are `static` only. Compiler output is flattened: the traced
order is the check, not the sweep.

Run with: python -I gain_cfg.py [out_dir]
"""

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from capstone import Cs, CS_ARCH_X86, CS_MODE_64  # noqa: E402
from capstone.x86 import X86_OP_IMM  # noqa: E402

from pe_emu import IMAGE_BASE, Emulator, image_va  # noqa: E402
from process_lib import PROCESS_THUNK, Session, alloc_frames  # noqa: E402

LO = 0x8E04C0
HI = 0x8E4BE2  # end of the body as recorded in section 19 (about 17.5 KB)
N = 512
JCC_JMP_RET = ("jmp", "ret")


def sweep(emu):
    md = Cs(CS_ARCH_X86, CS_MODE_64)
    md.detail = True
    raw = emu.rd(image_va(LO), HI - LO + 64)
    insns = []
    for ins in md.disasm(raw, LO + IMAGE_BASE):
        rva = ins.address - IMAGE_BASE
        if rva >= HI:
            break
        insns.append(ins)
    return insns


def build(insns):
    by_rva = {ins.address - IMAGE_BASE: ins for ins in insns}
    leaders = {LO}
    indirect = []
    for ins in insns:
        rva = ins.address - IMAGE_BASE
        if ins.mnemonic.startswith("j"):
            op = ins.operands[0] if ins.operands else None
            if op is not None and op.type == X86_OP_IMM:
                tgt = op.imm - IMAGE_BASE
                if LO <= tgt < HI:
                    leaders.add(tgt)
            else:
                indirect.append(rva)
            nxt = rva + ins.size
            if nxt < HI:
                leaders.add(nxt)
        elif ins.mnemonic == "ret":
            nxt = rva + ins.size
            if nxt < HI:
                leaders.add(nxt)
    leaders = {l for l in leaders if l in by_rva}
    starts = sorted(leaders)
    blocks = {}
    for i, s in enumerate(starts):
        end = starts[i + 1] if i + 1 < len(starts) else HI
        body = []
        r = s
        while r < end and r in by_rva:
            body.append(by_rva[r])
            r += by_rva[r].size
        blocks[s] = {"start": s, "end": r, "insns": body}
    for s, b in blocks.items():
        last = b["insns"][-1]
        lrva = last.address - IMAGE_BASE
        succ, kind = [], "fall"
        if last.mnemonic == "ret":
            kind = "ret"
        elif last.mnemonic == "jmp":
            op = last.operands[0]
            if op.type == X86_OP_IMM:
                succ.append(op.imm - IMAGE_BASE)
                kind = "jmp"
            else:
                kind = "indirect"
        elif last.mnemonic.startswith("j"):
            op = last.operands[0]
            if op.type == X86_OP_IMM:
                succ.append(op.imm - IMAGE_BASE)
            kind = "jcc"
            if b["end"] in blocks:
                succ.append(b["end"])
        elif b["end"] in blocks:
            succ.append(b["end"])
        b["kind"] = kind
        b["succ"] = [t for t in succ if t in blocks]
        b["lrva"] = lrva
    return blocks


def trace_block(sess, blocks):
    emu = sess.emu
    path = []
    emu.trace_insns(path)
    tin, _ = alloc_frames(emu, 2, N, [[0.0] * N] * 2)
    tout, _ = alloc_frames(emu, 2, N, [[0.0] * N, [0.0] * N])
    emu.call(PROCESS_THUNK, sess.wrapper, tout, tin, 0, N, max_insns=400_000_000)
    emu.trace_insns(None)
    return [a - IMAGE_BASE for a in path if LO <= a - IMAGE_BASE < HI]


def main():
    out_dir = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).parent
    emu = Emulator()
    insns = sweep(emu)
    blocks = build(insns)
    sess = Session(48000)
    sess.prepare()
    seq = trace_block(sess, blocks)

    def block_of(rva):
        best = None
        for s in blocks:
            if s <= rva and (best is None or s > best):
                best = s
        return best

    executed = {}
    for rva in seq:
        b = block_of(rva)
        if b is not None:
            executed[b] = executed.get(b, 0) + 1
    observed = set()
    for a, b in zip(seq, seq[1:]):
        ba, bb = block_of(a), block_of(b)
        if ba is None or bb is None or ba == bb:
            continue
        last = blocks[ba]["insns"][-1].address - IMAGE_BASE
        if a == last:
            observed.add((ba, bb))

    nodes = []
    edges = []
    for s in sorted(blocks):
        b = blocks[s]
        nodes.append({
            "id": f"{s:#x}",
            "end": f"{b['end']:#x}",
            "insns": len(b["insns"]),
            "kind": b["kind"],
            "traced_count": executed.get(s, 0),
        })
        for t in b["succ"]:
            edges.append({
                "from": f"{s:#x}",
                "to": f"{t:#x}",
                "static": True,
                "observed": (s, t) in observed,
            })
    for s, t in sorted(observed):
        if not any(e["from"] == f"{s:#x}" and e["to"] == f"{t:#x}" for e in edges):
            edges.append({"from": f"{s:#x}", "to": f"{t:#x}", "static": False, "observed": True})

    indirect = [b["lrva"] for b in blocks.values() if b["kind"] == "indirect"]
    summary = {
        "range": [f"{LO:#x}", f"{HI:#x}"],
        "instructions": len(insns),
        "blocks": len(blocks),
        "blocks_traced": sum(1 for s in blocks if executed.get(s)),
        "instructions_traced": len(seq),
        "indirect_jumps": [f"{r:#x}" for r in indirect],
        "untraced_blocks_are_static_only": True,
    }
    (out_dir / "gain_body_cfg.json").write_text(json.dumps({"summary": summary, "nodes": nodes, "edges": edges}, indent=1))
    lines = ["digraph gain_body {", "  node [shape=box fontname=monospace];"]
    for nd in nodes:
        style = "" if nd["traced_count"] else ' style=dashed'
        lines.append(f'  "{nd["id"]}" [label="{nd["id"]}\\n{nd["insns"]} insns\\nkind={nd["kind"]} hits={nd["traced_count"]}"{style}];')
    for e in edges:
        style = "" if e["observed"] else ' style=dashed'
        lines.append(f'  "{e["from"]}" -> "{e["to"]}"{style};')
    lines.append("}")
    (out_dir / "gain_body_cfg.dot").write_text("\n".join(lines) + "\n")
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
