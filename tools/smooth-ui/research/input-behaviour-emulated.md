# SMOOTH input behaviour: emulation results (soothe3 restored image)

Status: **partly recovered (round 2)**. The wheel message path is now run end
to end by the real image code: the Editor push, the ring, the pop, the kind
dispatch, and the consumer's stores into the UI object. Still **not recovered**:
how the stored wheel fields reach the `Input` the components read, the value law
(wheel step, fine factor, drag scale, reset target, hit radius). So
`input.rs` and `graph/gesture.rs` keep their placeholder constants. Nothing below
the round-2 section is a value to copy into SMOOTH, except the layouts and
addresses, which are verified.

Repro: `tools/smooth-core/research/re-tools/emulate/input_chain.py` (run with
`python -I` from that directory, venv with unicorn, capstone and pefile).

## Round 2: real wheel pipeline (verified by emulation)

Method: the image is run as-is (Unicorn, base 0x180000000). Only imports are
stubbed. The MSVC clock (`_Query_perf_frequency` = 10 MHz, `_Query_perf_counter`
= fixed value) is a stub because the consumer checks the frequency against
`0x989680` and divides by it (`fcn.1803312c0`). Nothing is faked on the path.

### 1. Editor wheel push (`kali::Editor` slot 8 = `fcn.180367590`)

- Reads `*(editor+0x1d0)` = host. Payload read from the event `r8`:
  `float dx @+0`, `float dy @+4`, `uint8 flags @+8`.
- Pushes kind `0xf` to `host+0x300` via `fcn.180331100`.
- Verified: push with `(1.5, -2.0, 0x05)` lands as kind `0xf`, `dx=1.5`,
  `dy=-2.0`, `flags=0x05`.

### 2. Ring (`fcn.180331100` push, `fcn.1806f8a60` try_pop)

Vyukov-style bounded MPMC ring. Queue object `q`:

| offset | field |
|---|---|
| `+0x00` | capacity (divisor for `pos / cap`) |
| `+0x08` | cell buffer |
| `+0x40` | enqueue position (`lock cmpxchg` in push) |
| `+0x80` | dequeue position (`lock cmpxchg` in try_pop) |

Cell (`0x80` bytes): `seq @+0` (free = `2*lap`, published = `2*lap+1`,
consumed = `2*lap+2`), `kind @+8` (dword), payload `@+0x10` (copied with
`fcn.1806f06d0`/`fcn.180330fd0`).

`try_pop(q, out)`: `out+0` = kind (dword), `out+8` = payload. Returns `al`.

### 3. Who pops the wheel ring

The UI pump `fcn.1806f5ae0` (no direct callers; reached through a
`std::function`) runs with `this = r14 = host+0x200`, so the wheel ring is
`this+0x100`:

1. `fcn.1806f4280` pops `[this+0x6b0]+0x140` (second ring).
2. `fcn.1806f4640` pops `this+0x100` (the wheel ring). Its kind switch is
   a jump table of 7 entries at VA `0x1806f4d04` (kind `0xc..0x12`, base-relative
   dwords): `0xc -> 0x1806f46cf`, `0xd -> 0x1806f473e`, `0xe -> 0x1806f496a`,
   `0xf -> 0x1806f4bf8`, `0x10 -> default`, `0x11 -> 0x1806f499d`,
   `0x12 -> 0x1806f4b35`.
3. `fcn.1806f4d20` pops a queue at `+0x1c0` of another object (not traced).

The `0xf` case (`0x1806f4bf8`) stores, with pcs confirmed by the write trace:

| pc | store | value (verified) |
|---|---|---|
| `0x1806f4c2f` | `this+0x338` (float) | `dx` |
| `0x1806f4c43` | `this+0x33c` (float) | `dy` |
| `0x1806f4c4b` | `this+0x340` (byte) | `flags` |

Then it calls `fcn.1803312c0` (QueryPerformanceFrequency and counter, scaled
to 100 ns units) and stores the 64-bit value read through its returned pointer
at `this+0x720`. This is consistent with an event timestamp; the reader is not
checked.

Kind `0xf` is **not** handled by the other pop loop `fcn.180849620`
(it pops `r13+0x200` and its jump table sends `0xc..0xf` to the default case).

### 4. Where the Input comes from (partly)

After the pops, the pump calls the root component through its vtable:
`call [vt+8]` on `[this+0x6b8]` with `rdx = [this+0xb8]` (the Context object),
`r8 = this+0x280` (the **Input** object), `r9 = this+0x4c8` (the Output).
So the UI-side Input is at `this+0x280` = `host+0x480`, and the Context is a
separate object (the Knob reads its float at `+0x620`).

The wheel fields at `this+0x338..0x340` are not copied into `this+0x280` in
the pump's straight-line code after the consumer (I read that block). The copy
happens elsewhere; not found. The static
search (scalar and block-copy accesses to `+0x330..0x340`) gives many
candidates; none is confirmed.

### 5. Knob update (slot 2 = `fcn.18073dc10`, `update(ctx, input, output)`)

Registers: `rdi = this` (Knob), `rsi = Context`, `rbx = Input`, `r14 = Output`.
It calls the child update at `this+0xda0` (vtable slot 2), reads `Context +0x620`
(float), and reads Input pointer fields at `+0x150/+0x158/+0x160/+0x168`
(compared with component pointers; their roles are not confirmed). Knob fields
`+0x2e8` (int) and `+0x2ec` (bool) are read and written here; meaning not
confirmed.
No value write from the wheel is visible in this function. The value step is
therefore not here, or the Input's wheel fields are read by a function that
calls slot 2 with the value already computed.

## What is still open (ordered)

1. Find the reader that copies `this+0x338..0x340` (and `+0x720`) into the
   Input at `this+0x280`. Next step: run the pump `fcn.1806f5ae0` in the
   emulator with read watches on `this+0x330..0x348`, with a UI object built
   field by field (`+0x6b0` ring, `+0x6b8` root vtable, `+0xb8` Context, `+0x1c0`
   queue). Each fault names the missing field.
2. The Knob pointer handler (slot 3, `fcn.180739410`): its body is split into
   several `.pdata` fragments (the 34-byte entry at the start is only the
   prologue). Trace it by executed blocks, not by linear disassembly.
3. Value law: the wheel step, drag scale and fine factor come from the Knob
   range/skew fields (set by its constructor) and the parameter table. Not
   recovered.

Nothing from round 2 changes a SMOOTH constant yet: the wheel step is still the
placeholder. The verified layouts and addresses above are what the next round
builds on.

---

# Round 1 (superseded): fake-struct fuzzing


Status: **not recovered**. The harness runs the functions and the evidence is
the negative result below. No wheel step, drag scale, fine factor, reset target
or hit radius was recovered, so `input.rs` and `graph/gesture.rs` keep their
placeholder constants. Nothing in this file is a value to copy into SMOOTH.

Builds on `input-behaviour.md` (static RE). That file is on branch `smooth/re-ui-input` (PR #93), not on `main`, so it is not in this tree. Harness:
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
  returns the child answer and logs the virtual call. The static notes (PR #93) report no radius
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
