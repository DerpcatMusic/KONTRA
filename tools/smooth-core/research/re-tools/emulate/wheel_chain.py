"""Full wheel chain: modifier event, wheel event, ring, consumer, real Knob update.

Evidence scope: isolated-function emulation (see README.md). Every step is the
image's own code. The synthetic inputs are the modifier byte, the wheel delta,
the MSVC clock stubs, the Knob's parameter record and the Output's empty map.

  1. Editor slot 34 fcn.180366f30(modifier byte)   -> ring kind 0x12
  2. Editor slot 8  fcn.180367590(dx, dy, reversed) -> ring kind 0xf
  3. UI consumer    fcn.1806f4640(owner = host+0x200) stores the state and wheel
     fields; Input = owner+0x280 (the Knob update's Input).
  4. Knob update    fcn.18073dc10(knob, ctx, Input, Output) on a REAL Knob built
     by the Knob constructor fcn.180737410.

Result (sink = fcn.1806c30c0, the Knob gesture's value call):
  one Windows notch dy = 60/256 gives +0.078125 with no modifier (divisor 300)
  and +0.0234375 with Shift (divisor 1000, Input+0xea).

Not modelled, and stubbed so the update reaches the sink (see skip_call):
  fcn.1807013a0  pending-notification insert into Output+0x120 (bookkeeping)
  fcn.180731cc0  the update's per-frame prelude (host timers and maps)
  fcn.180701710  a hash lookup on Input, reached after the sink (tail of the consumer)
The repaint gate at 0x182778030 is set to an enabled settings object.
The emulation stops after the sink, in the Output notify path (fcn.1806fae60),
which is not part of the value law.

Run with: .venv/bin/python -I wheel_chain.py [dy] [modifier]   (from this directory)
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pe_emu import Emulator, IMAGE_BASE  # noqa: E402
from input_chain import build_host, wheel_payload, EDITOR_WHEEL, CONSUMER  # noqa: E402
from unicorn import UC_HOOK_CODE  # noqa: E402
from unicorn.x86_const import UC_X86_REG_RDX, UC_X86_REG_R8, UC_X86_REG_RSP, UC_X86_REG_RAX, UC_X86_REG_RIP  # noqa: E402

MODS_SLOT34 = IMAGE_BASE + 0x366F30
KNOB_CTOR = IMAGE_BASE + 0x737410
KNOB_UPDATE = IMAGE_BASE + 0x73DC10
SINK = IMAGE_BASE + 0x6C30C0
PARAM_ID = 3


def new_output(e):
    """An Output with an empty std::map at +0x80, the only host state the wheel path touches."""
    out = e.arena(0x200)
    head = e.arena(0x40)
    e.w64(head, head)
    e.w64(head + 0x08, head)
    e.w64(head + 0x10, head)
    e.wr(head + 0x19, b'\x01')
    e.w64(out + 0x80, head)
    e.w64(out + 0x88, 0)
    return out


def backtrace(e, depth=0x800):
    """Image addresses found on the stack at the current rsp: a heuristic call chain."""
    rsp = e.reg(UC_X86_REG_RSP)
    found = []
    for off in range(0, depth, 8):
        v = int.from_bytes(bytes(e.rd(rsp + off, 8)), 'little')
        if IMAGE_BASE <= v < IMAGE_BASE + e.image_size:
            found.append(v)
    return found


def skip_call(e, addr, value=0):
    """Make a call to addr return at once with rax = value (its arguments are ignored)."""

    def cb(uc, pc, size, user):
        rsp = e.reg(UC_X86_REG_RSP)
        ret = e.r64(rsp)
        uc.reg_write(UC_X86_REG_RSP, rsp + 8)
        uc.reg_write(UC_X86_REG_RAX, value)
        uc.reg_write(UC_X86_REG_RIP, ret)

    e.uc.hook_add(UC_HOOK_CODE, cb, None, addr, addr)


def run(dy=0.234375, modifier=0x00, reversed_flag=0x00):
    e = Emulator()
    e.stub('IAT!_Query_perf_frequency', lambda emu, ev: 10_000_000)
    e.stub('IAT!_Query_perf_counter', lambda emu, ev: 1_000_000_000)
    host, editor, q, buf = build_host(e)
    owner = host + 0x200
    inp = owner + 0x280

    # The real Knob, built by its constructor (parameter id in edx).
    knob = e.arena(0x3000)
    e.call(KNOB_CTOR, knob, PARAM_ID, e.arena(0x100), max_insns=5_000_000)

    # Input state the pump keeps per frame: parameter records, the hover owner.
    params = e.arena(0x28 * 8)
    e.wf32(params + 0x28 * PARAM_ID + 4, 0.5)
    e.w64(inp + 0x000, params)
    e.w64(inp + 0x150, knob)

    # 1. modifier state, 2. wheel, both through the real Editor slots and ring.
    mods = e.arena(16)
    e.wr(mods, bytes([modifier]))
    e.call(MODS_SLOT34, editor, mods)
    e.call(CONSUMER, owner)
    payload = wheel_payload(e, 0.0, dy, reversed_flag)
    e.call(EDITOR_WHEEL, editor, 0, payload)
    e.call(CONSUMER, owner)
    print(f'Input after events: +0xb8 dx={e.rf32(inp + 0xb8)} +0xbc dy={e.rf32(inp + 0xbc)} '
          f'+0xc0 flag={e.rd(inp + 0xc0, 1)[0]:#x} +0xea(shift)={e.rd(inp + 0xea, 1)[0]:#x}')

    # 4. the real Knob update. ctx is a zeroed context; its +0x620 block is read
    #    for the scale (the wheel path does not depend on it).
    ctx = e.arena(0x800)
    e.w64(ctx + 0x620, e.arena(0x100))
    # The update first calls its embedded child at this+0xda0 (slot 2). Only the
    # call is needed here; the child's own behaviour is out of scope for the wheel.
    child = e.arena(0x10)
    e.w64(child, e.fake_vtable('Knob::child', 8))
    e.w64(knob + 0xda0, child)
    # Global at 0x182778030 points at a settings object whose first byte gates the
    # repaint hook fcn.1807ae910 (the update calls it). Set it enabled; this is the
    # default a running host has, not a value recovered from the reference.
    settings = e.arena(0x20)
    e.wr(settings, b'\x01')
    e.w64(0x182778030, settings)
    out = new_output(e)
    sink_hits = []

    def sink_cb(uc, pc, size, user):
        sink_hits.append((e.reg(UC_X86_REG_RDX), e.reg(UC_X86_REG_R8)))

    e.uc.hook_add(UC_HOOK_CODE, sink_cb, None, SINK, SINK)
    # fcn.1807013a0 is the update's pending-notification insert into Output+0x120
    # (an unordered_map keyed by the Knob). Its result is unused on the value path,
    # so it returns immediately. Bookkeeping, not the value law.
    skip_call(e, IMAGE_BASE + 0x7013A0)
    # fcn.180731cc0 is the update's first call (per-frame host prelude: timers and
    # host maps). It runs into the host's map code that is not modelled here.
    skip_call(e, IMAGE_BASE + 0x731CC0)
    try:
        e.call(KNOB_UPDATE, knob, ctx, inp, out, max_insns=20_000_000)
    except RuntimeError as exc:
        print('knob update stopped:', str(exc)[:200])
        print('return addresses on the stack (heuristic):', [
            hex(a) for a in backtrace(e)])
    for res, S in sink_hits:
        v, valid = e.rf32(res), e.rd(res + 4, 1)[0]
        print(f'sink: out.v={v:.6f} valid={valid} divisor-select Input+0xea={e.rd(inp + 0xea, 1)[0]:#x} '
              f'S+0x30={e.rf32(S + 0x30):g} S+0x34={e.rf32(S + 0x34):g} S+0xc={e.rf32(S + 0xc):g}')
    if not sink_hits:
        print('sink not reached')
    return e, knob, inp


if __name__ == '__main__':
    dy = float(sys.argv[1]) if len(sys.argv) > 1 else 0.234375
    mods = int(sys.argv[2], 0) if len(sys.argv) > 2 else 0
    run(dy=dy, modifier=mods)
