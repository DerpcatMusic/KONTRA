# Kontakt modulator runtime evidence

Research artifact for KONTRA wave 2. This is evidence, not an implementation. Native
addresses refer to Kontakt 8.13.1 standalone image and private Ghidra project. The
serializable `BPar*` catalog entries identify payload readers only; runtime classes are
separate (`BGlide`, `BEnvFollowA`, `BArp32Step`).

## Reader/version inventory

Catalog (`docs/re/kontakt-catalog/reader_schemas.tsv`) currently reports:

| Payload | ser type | native version | saved versions/layouts | reader |
|---|---:|---:|---|---:|
| `BParEnvF` | `0x0A` | `0x60` | v0x60: `FFFF` | `0x140cffac0` |
| `BParGlide` | `0x0B` | `0x51` | v0x50 `IFF`, v0x51 `IFFFFB` | `0x140d034d0` |
| `BParArp` | `0x09` | `0x81` | v0x80 `IB?I?B?E?F?F?`; v0x81 extended | `0x140cff920` |
| `BParGroupDynamics` | `0x4A` | `0x10` | v0x10 `F?` | `0x140d035f0` |

These layouts are serialization facts only. No processing semantics should be inferred
from their vtables.

## Runtime classes and entry points

From `vtable-methods.csv`:

| Runtime class | vtable | destructor | processing candidate | other class-specific slots |
|---|---:|---:|---:|---|
| `BGlide` | `0x4e94be8` | `0x14081dc50` | slot 6 `0x140ceb090` | slot 10 `0x1405e9600`; slots 15+ common/state methods |
| `BEnvFollowA` | `0x4f1a148` | `0x14081d6c0` | slot 6 `0x140ae3d80` | slot 15 `0x14094ce20`, slot 19 `0x140838b00` |
| `BArp32Step` | `0x4f18ff8` | `0x14081cba0` | slot 6 `0x140a768e0` | slots 7/8/19 and 22/23/24 are class-specific |

The cached `vtable-methods.csv` lists class-specific addresses. The slot-6 functions
are the strongest processing candidates because they accept an instance, float buffer
(or event/output buffer), and count. Native decompilation was obtained from a private
copy with serialized access using `flock`.

## BGlide: observed processing structure

`FUN_140ceb090(instance, float* buffer, uint count)` is a stateful one-pole linear
ramp with a finite zero tail. The relevant fields are `state = *(float*)(p+0xb8)`,
`step = *(float*)(p+0xbc)`, and `scale = *(float*)(p+0xc0)`. For each output sample,
when the state has not passed zero in the direction of `step`, it writes
`out[i] = scale * state`, then updates `state = state + step`. Once a nonzero state
has crossed through zero (`step < 0 && state < 0`, or `step > 0 && state > 0` after
the update), the remaining output is zeroed with `memset`. A zero state emits one
zero sample before the crossing test can terminate the remainder. The body is manually
unrolled in groups of four, with a scalar tail; this is an implementation optimization,
not a 32-frame control quantum.

The exact finite-ramp law is therefore established for the native function:
`y[n] = scale * (state0 + n*step)` until the first direction-consistent nonzero
crossing, followed by zeros; all arithmetic is native float in the decompiled body.
The initialization/units of `state`, `step`, and `scale`, and the meaning of the
separate slot-10/state methods, remain open. `FUN_1405f44a0` clears byte `+0xa1`;
`FUN_1405e9600` returns zero.

## BEnvFollowA: observed processing structure

`FUN_140ae3d80(instance, uint32* output, int count)` is a pure buffer copy. It copies
`count` 32-bit values from the instance's pointer at `+0xe4` to the caller's output,
using groups of four and a scalar tail. No rectification, attack/release coefficient,
or detector update occurs in this slot-6 body. `FUN_14094ce20` clears the state/pointer
region (`+0xb4` onward through the observed fields), and `FUN_140838b00` copies the
4-byte fields at `+0xb4` and `+0xb8` from another instance. The detector or producer
that fills `+0xe4` has not been identified; do not substitute a generic envelope
follower. Exact source signal, smoothing law, cadence, and reset policy remain open.

## BArp32Step: observed processing structure

`FUN_140a768e0(instance, uint* output, int count)` emits table values. Let `L =
(int)*(int*)(p+0xd8)`, `table = (uint*)(p+0xe0)`, and `phase = *(double*)(p+0xb0)`.
With no external modulation pointer at `+0xc8`, each sample is
`out[i] = table[(int)phase]`, then `phase += step` where `step = *(double*)(p+0xb8)`.
If `phase >= L`, free-running mode (`byte +0xdc == 0`) wraps by subtracting `L`;
otherwise one-shot mode sets `step = 0` and clamps the phase to `L-1`. After the
block, the phase receives an additional correction proportional to `*(int*)(p+0x5c)`,
a native global constant, and `step`; its exact unit/intent is open.

With an external float pointer at `+0xc8`, each input is clamped to [0,1], transformed
by a native affine/range mapping and a fractional quadratic term, quantized to an
integer table step, then multiplied by `*(float*)(p+0xc0)` and used as the phase
increment. The decompiled expression has the shape
`step = float_trunc((A(input) - (frac-frac²)*B) * C) * scale`, where A/B/C are
native constants and `frac` is the fractional part of A(input); exact constants need
a constant-read extraction. In a separate mode (`byte +0xdd != 0`),
`FUN_140a76c40(instance, sample_index)` is called before table lookup, so step
mutation/sequence generation is delegated there. Slots 22/23/24
(`0x140968140`, `0x1409619a0`, `0x1409685d0`) set loop mode, table length, and one
table entry respectively. Step duration, clock source, retrigger/free-run behavior,
and exact external modulation mapping remain partly open.

## Group dynamics

`BParGroupDynamics` is present in the serialization catalog (`0x4A`, version `0x10`,
reader `0x140d035f0`) but no `BGroupDynamics` runtime class appears in the supplied
vtable ledger. Therefore no processing law is established here. Treat group dynamics as
payload-only until its runtime owner is identified; do not equate catalog class with a
processor.

## Evidence limits / next queries

The native query project rejects hidden path components; successful queries used
`RE_PROJECT_DIR=/tmp/kontakto-wave2-modulators-ghidra` (symlink to the shared private
project) and `flock /home/derpcat/.cache/kontakto-wave2/modulators-private-query.lock`.
Next useful queries are `BGlide` slot-10/advance helper, complete slot-6 bodies, and
`BEnvFollowA`/`BArp32Step` class-specific slots in one serialized batch. Runtime class
identity is stronger than serialization identity, but vtable membership alone does not
prove audio semantics.
