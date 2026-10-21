# SMOOTH input behaviour: emulation results (soothe3 restored image)

Status: **not recovered**. The harness runs the functions and the evidence is
the negative result below. No wheel step, drag scale, fine factor, reset target
or hit radius was recovered, so `input.rs` and `graph/gesture.rs` keep their
placeholder constants. Nothing in this file is a value to copy into SMOOTH.

Builds on `input-behaviour.md` (static RE). Harness:
`tools/smooth-core/research/re-tools/emulate/` (`pe_emu.py`, `crt.py`, README).

## Method

- Image: `/tmp/opencode/rea/soothe3-restored.bin`, base `0x180000000`, mapped in
  Unicorn 2.1.4 at its preferred base (no relocation needed).
- Imports: the 38 directory imports, plus 751 slots found from rip-relative
  indirect calls (`FF 15` / `FF 25`) whose value is an import-by-name RVA. The
  IAT-like table sits in `.rdata`; the import directory covers only part of it.
- Calls: Win64 ABI (rcx, rdx, r8, r9; xmm0 to xmm3 for floats).
- CRT: real malloc, calloc, free, HeapAlloc, memcpy, memset, memcmp, strcmp.
  Throw helpers (`_CxxThrowException`, `_Xbad_alloc`, ...) abort the run and
  record the reason.
- Virtual calls on synthetic objects go to named stubs, so every call and its
  arguments are logged.

## Knob pointer handler, vtable slot 3 = `fcn.180739410` (VA `0x180739410`)

Entry: `(this=rcx, a2=rdx, a3=r8)`.

Observed (with zeroed objects plus a child vtable at `this+0xda0` whose slot 3
returns a pointer to a (0.5, 0.5) float pair, and `this+0x1020` pointing to a
sub-object whose `+0x18` points to a (1.0, 1.0) block):

- Runs to `ret` in about 376 instructions. No virtual calls except the child's
  slot 3, and no writes to `this`.
- Field reads from `a2`: `+0x20` (8 bytes).
- Field reads from `a3`: `+0x0`, `+0x4`, `+0x8`, `+0xc`, `+0x120`, `+0x124`,
  `+0x128`, `+0x12c`, `+0x2d8`, `+0x2e8` (byte).
- Writes: only `a3+0x120` and `a3+0x128`.
- `this+0x1020 → +0x18 → call rdi` is an indirect call through a value we did
  not model. The run still returned.

Black-box fuzz (400 runs, random words in `a2`, `a3` and `this` at 0.3 and
0.05 density, seeds fixed):

| outcome | runs |
|---|---|
| ok, no `this` write, no event | 196 |
| fault, no `this` write | 131 |
| fault, `this` writes at 0x30–0x54 (partial, then fault) | 67 |
| fault, `this` writes, no event | 5 |
| fault, typeid path (`__RTtypeid`, `malloc`) | 1 |

No run completed with a `this` write. The value-update sink was not reached.
The writes at `this+0x30..0x54` are the Knob bounds fields (`0x4c..0x58`
belong to the hit test) written before a fault. They are not evidence of a
parameter update.

## Why it is blocked

1. Filling zero pointer slots with fake objects is wrong. It sets `[rbx+0x28]`
   nonzero, which sends the code down an RTTI branch
   (`0x180731d87`): `__RTtypeid`, `__std_type_info_name`, `malloc`, then a throw.
   Fake objects need correct type information, field by field.
2. The handler reads several `a3` fields that look like nested objects or flags
   (`+0x2d8`, `+0x2e8`). Without a real `Input` layout we cannot construct a
   valid event.
3. Branch forcing (overriding a conditional jump) was not tried. A forced path
   would not show the real formula, only that a path exists.

## Other functions

- **Knob hit test (slot 7, `fcn.18073dba0`)** runs with a packed `(x, y)` float
  pair in `rdx` and a child vtable at `this+0x2f0`. This is verified: it
  returns the child's answer and logs the virtual call. It shows no radius
  beyond the bounds, which matches `input-behaviour.md`.
- **FFT wrapper (`fcn.180001640`, the `.text 0x180001640` entry)** runs, returns
  `0` (`ippStsNoErr`) and expects the spec at `r8`, with magic `7` at `+0`,
  order at `+4` (`N = 2^order`), a flag at `+8` and a float scale at `+0x10`.
  - Impulse input gives all-ones output for every order and both flags at
    scale 1.0, so the transform is unnormalized.
  - Non-trivial input does **not** match a plain DFT, for either sign or any
    fitted scale. The spec's twiddle tables are never built, because the IPP
    init routine is not near the wrapper. The init was not located.
  - So **the forward scale is not settled**. The impulse result rules out a
    1/N normalization at scale 1.0 only.
- **Mid/side scale**: not attempted.

## What would settle the open items

1. Build the `Input` and `Context` objects from their constructors or the
   editor's event pump, not from guesses, then rerun the fuzz.
2. Find the FFT init routine (the spec writer). A correct spec makes the
   non-trivial DFT test decisive.
3. Branch-level coverage: record conditional outcomes on the slot-3 path and
   find the first branch that depends on `a2+0x20`. That is the event-kind test.
