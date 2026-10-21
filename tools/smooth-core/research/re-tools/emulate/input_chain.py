"""Run the real soothe3 wheel path in the emulator: Editor slot 8 push -> ring -> consumer.

Evidence scope: isolated-function emulation of the restored PE image (see README).
No Wine, no host. Each step calls the real image code, so the queue, the message
copy and the consumer's stores are what the binary does for these inputs.

Chain (VAs in the restored image, base 0x180000000):
  kali::Editor slot 8 (wheel)  fcn.180367590 : pushes kind 0xf to host+0x300
  ring push                    fcn.180331100 : Vyukov MPMC, cell 0x80, seq/kind/payload
  ring pop                     fcn.1806f8a60 : try_pop(q, out) -> kind @out+0, payload @out+8
  wheel consumer               fcn.1806f4640 : owner = host+0x200, pops owner+0x100
                               (= host+0x300); kind 0xf case stores dx @+0x338,
                               dy @+0x33c, flags byte @+0x340 (owner fields).
Run with: .venv/bin/python -I input_chain.py   (from this directory)
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pe_emu import Emulator, IMAGE_BASE  # noqa: E402

EDITOR_WHEEL = IMAGE_BASE + 0x367590
PUSH = IMAGE_BASE + 0x331100
TRY_POP = IMAGE_BASE + 0x6F8A60
CONSUMER = IMAGE_BASE + 0x6F4640
CAP, CELL = 8, 0x80


def build_host(e):
    """Host object with the wheel ring at +0x300 (capacity, buffer, zero positions)."""
    host = e.arena(0x800)
    editor = e.arena(0x400)
    e.w64(editor + 0x1D0, host)           # editor -> host (slot 8 reads *(editor+0x1d0))
    q = host + 0x300
    buf = e.arena(CAP * CELL)
    e.w64(q + 0x00, CAP)                  # [q]      capacity (div operand)
    e.w64(q + 0x08, buf)                  # [q+8]    cell buffer
    return host, editor, q, buf           # enqueue at q+0x40, dequeue at q+0x80 (start 0)


def wheel_payload(e, dx, dy, flags):
    p = e.arena(0x40)
    e.wf32(p + 0, dx)
    e.wf32(p + 4, dy)
    e.wr(p + 8, bytes([flags]))           # JUCE-style flags byte
    return p


def main():
    e = Emulator()
    # MSVC STL clock used by the wheel stamp (fcn.1803312c0): 10 MHz frequency, monotonic counter.
    e.stub('IAT!_Query_perf_frequency', lambda emu, ev: 10_000_000)
    e.stub('IAT!_Query_perf_counter', lambda emu, ev: 1_000_000_000)
    host, editor, q, buf = build_host(e)

    # 1. real Editor wheel push
    p = wheel_payload(e, 1.5, -2.0, 0x05)
    e.call(EDITOR_WHEEL, editor, 0, p)
    seq = e.r64(buf)
    kind = e.r32(buf + 8)
    print(f'push: cell0 seq={seq} (expect 1) kind={kind:#x} (expect 0xf) '
          f'dx={e.rf32(buf + 0x10)} dy={e.rf32(buf + 0x14)} flags={e.r32(buf + 0x18) & 0xff:#x}')

    # 2. real try_pop into an out message
    out = e.arena(0x100)
    got = e.call(TRY_POP, q, out)
    print(f'pop : ok={got & 0xff} kind={e.r32(out):#x} '
          f'dx={e.rf32(out + 8)} dy={e.rf32(out + 0xc)} flags={e.r32(out + 0x10) & 0xff:#x}')

    # 3. real wheel consumer on a fresh push; owner = host+0x200 so owner+0x100 == q
    owner = host + 0x200
    e.call(EDITOR_WHEEL, editor, 0, p)
    e.watch_writes(owner + 0x338, owner + 0x344, 'wheel')
    e.clear_logs()
    try:
        e.call(CONSUMER, owner)
    except RuntimeError as exc:
        print('consumer stopped:', exc)
    print(f'consumer: owner+0x338 dx={e.rf32(owner + 0x338)} '
          f'owner+0x33c dy={e.rf32(owner + 0x33c)} owner+0x340 flags={e.rd(owner + 0x340, 1)[0]:#x}')
    for label, addr, size, value, pc in e.write_log[:8]:
        print(f'  write {label} addr={addr - owner:#x}(owner) size={size} value={value:#x} from pc={pc:#x}')
    print('stub calls after the wheel store:', [str(ev) for ev in e.events][:6])


if __name__ == '__main__':
    main()
