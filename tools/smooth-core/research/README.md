# The reverse engineering this port stands on

These are the REA's own decompilations of the soothe3 research image, taken
with Ghidra 12.1.4 through `rea decompile`, on the image the UNPACER
restores from the installed build:

```
unpacer ".../soothe3.vst3/Contents/x86_64-win/soothe3.vst3" \
    --output soothe3-restored.bin
```

The restored image is `0143f03b7c7f1e41a51896f269a64c3dfc4c804e94ce8cccbeef9c98dbbcfa64` —
byte for byte the research's single-stage-B hypothesis, so the analysis
below and the research's own dossiers describe the same bytes.

| file | procedure | why it matters |
|---|---|---|
| `rea-8ab6b0.md` | the E initializer | the per-channel spatial curve the port was missing |
| `rea-8af3d0.md` | the Gaussian helper | the spatial operator's kernel |
| `rea-8aff30.md` | the shape helper | the eight band shapes and their dispatch |
| `rea-8f03f0.md` | the projection's wrapper | copies the records, then calls the projection |
| `rea-8f78e0.md` | the weighted projection | channels × bands, one float weight each |

Each file carries the REA's evidence envelope: the subject's hash, the
provider and its version, the analysis profile, and the pseudocode.

The REA needs a 64-bit JDK 21 or newer on `JAVA_HOME`; the Ghidra provider
times out on an image this size otherwise (`REA_GHIDRA_STARTUP_TIMEOUT_MS`).

## What the curve-record operator turned out to be

`8abd70` is the operator the constructor calls between the E initializer and
the Wb updater. Its body, from the UNPACER's scoped decompilation:

```c
fVar30 = *(float *)(*(longlong *)(param_1 + 0x2c38) + 4);   // the config's detail
fVar30 = clamp(fVar30, 0, 1);
expf(fVar30 * 1.0986123 - 0.6931472);   // the same position formula as B
logf();  expf();                        // -> the scalar
/* copy the owner's O+0x6b8 grid (the common bandwidth B) */
analyst_entry_8af9e0(local_378, uVar31);
```

and `8af9e0` is a loop over the grid that calls the vector selector
`001040` once per element.

`001040` is a dispatch thunk: its table lives at `0x1821ebe78` and its index
slot at `0x1821ed508` holds `0xffffffff` -- the runtime choice is unobserved,
exactly as the research says. Taking the canonical candidate, the table's
first entry is `0x1800021e0`, which null-checks the array, checks the count,
and reaches `0x181d88060`: a broadcast fill of the array with the scalar.
So the "curve record" is a *filled* array, not a curve.

That closes the last of the research's three named frontiers in the shape
path: the per-record curve is the common bandwidth filled with the detail's
position scalar, and the shapes themselves are dispatched by `8aff30` to the
eight helpers the round-2 report already listed.

## The dispatch slot, bounded

The shape path's `001040` is one of a family of thunks that all read the same
word:

```
0x180001040  movsxd rax, dword [0x1821ed508]   ; -1 in the image
0x180001047  lea    r10, [0x1821ebe78]
0x18000104e  jmp    qword [r10 + rax*8]
```

The slot holds `0xffffffff` in the decoded image and has no writer anywhere
in it: a displacement scan of every code section for a reference to
`0x1821ed508` finds twelve readers (`movsxd`) and no store, and no `movabs`
immediate carries its address either. The store belongs to the protector's
own initialisation, and that cannot be observed here. The packed image never
returns from `LoadLibrary` without its service -- a probe that loads it with
no exception handler installed, leaving the protector's single-step control
flow alone, still hangs -- and the relinked image keeps the slot at `-1`
through `InitDll` and `GetPluginFactory`.

What the slot selects is bounded by the tables it indexes. Each thunk owns
one table for one operation, twelve entries wide, with entry 6 null and
entry 7 a one-instruction stub (`0x180001030`, `0x180001060`). The entries
are implementations of that operation, not data, so the slot is a dispatch
level and never a musical parameter.

For `001040` -- the fill of the curve record -- entries 0..5 are the same
operation at different vector widths. Each null-checks the destination,
rejects a non-positive count with `-6`, moves the scalar's bits out of
`xmm0`, and reaches a broadcast fill (`0x181d88060`, `0x181d88a80`, ...);
entry 4 (`0x180002300`) is the AVX path and splits at 2 MiB. They write the
same bytes. The port generates the record by filling it with the scalar, so
it is the result at any level, and `0x1800021e0` -- level 0 -- stays the
documented stand-in.

The kernel runner cannot execute these entries directly: they take the
scalar in `xmm0`, and the bridge passes integer and float arguments through
`rcx`/`rdx`/`r8`/`r9` only. The equivalence above is disassembly evidence,
not an executed comparison, and is recorded as such.

## The weighted projection, and what `O+0xf28` is

`8f03f0` is called from the soft and hard paths at `0x8e3f9e` and
`0x8f5d0e`, both with the same three addresses:

```
lea rcx, [r13 + 0xf28]     ; owner record set
lea rdx, [rbp + 0x3d00]    ; weights
lea r8,  [rbp + 0x4110]    ; grid
call 0x1808f03f0
```

Those addresses are **inputs**. `8f03f0` copies the 0x410-byte record set
(8 records of 0x80 plus a 0x10 trailer) from the weights and grid arrays
onto its own frame, then calls
`8f78e0(weights_copy, grid_copy, O + 0xf28 + 0x340)`. The owner set at
`O+0xf28` is only addressed, never copied or written. An earlier note called
`O+0xe08`, then `O+0xf28` the home of the arrays; both are inputs here.

`8f78e0` (0x8f78e0..0x8f7bb7, three chained `.pdata` records) is the
weighted projection itself. Per channel (count at `param_2 + 0x404`), it
gathers that channel's destination pointers from `descriptor + 0x100`, then
for every record calls the dispatched kernel at `0x1800011f0`:

```
dst = gathered[channel] + descriptor_offset[channel]*4 + record_base[i]
n   = record count                         ; the 0x410 record set's trailer
w   = *(float *)(record_pointer[i] + channel*4)   ; xmm1
```

`inc rsi` per channel and `add r15, 8` are the channel stride (the channel
count is `[r12+0x404]`, checked at `0x8f7b9e`). So the kernel receives one
float per band **per channel** — the same quantity `selected_weights`
computes in `src/weighting.rs`. The float is addressed through the array
the caller labels the grid; the caller's other array (`0x3d00`) supplies
each record's base. Both are copies: `8f03f0` passes its own stack copies,
never the owner's storage.

`0x1800011f0` is a dispatch stub on the documented selector slot
(`movsxd rax, [0x1821ed508]`, then `jmp [table + rax*8]`), so which kernel
runs is a load-time choice, not a different result. It takes the weight in
`xmm1`, which the kernel runner cannot set, so the arithmetic inside the
selected implementation stays unexecuted here; the port matches the
acceptance numbers with its own weighted blend.

The descriptor at `O+0x1268` therefore says *where* the per-channel arrays
are, not what they hold. The port owns its buffers, so the only thing to
match is `weight[band][channel]`. Groups larger than two are the part of that
law still refused: it is built upstream, in the band-record construction
(`0x61c370` only fills the records' first qword with `base + offset*4` and
advances by the aligned channel stride), not in either of these functions.

### The channel arena, and why the larger group stays refused

The float weights are not owned by the projection. `0x61c370` fills each
record's first qword with a pointer into an arena owned by the source object
at `[rbp+0x4a60]`:

```
record[i].qword0 = source.base + source.offset*4
source.offset    = align16(source.offset + stride)
```

with `stride` read from the record set's trailer (`+0x404`, the channel
count; the byte stride at `+0x408`) and the count from `+0x400`. So one band
owns `channels` floats, and the projection indexes them as
`record_pointer[i] + channel*4`. That is the whole published layout. The
values in those floats are written by whichever function owns the `0x4a60`
object; `0x8e0858` reads that slot in twenty places and never stores to it,
so the builder is a different entry point and is not identified here.

Two things follow, and both are recorded as final rather than pending:

- **The law is unreachable in the product.** The adapter is one or two
  lanes: `Engine::process_sample` clamps `active_channels` to `1..=2`, and
  `WeightBanks::build` rejects anything outside that before a record is
  read (`Err("unsupported weighting geometry")`). `selected_weights` also
  refuses `channels > 2` directly, so a caller of the helper gets an error
  instead of an index past the two-lane result. Pinning tests:
  `a_wider_channel_request_is_the_stereo_engine` and
  `a_group_wider_than_the_engine_is_an_error_not_a_panic`.
- **The two-lane law is verified, not guessed.** `weight[band][channel]` is
  a float per band per channel; for two lanes the port computes
  `[2(1-focus), 2*focus]` clamped to `[0, 1]`, which is exactly what the
  projection consumes. A third lane would need the upstream builder, and
  porting a guess for it would put unverified numbers into a stereo product
  that can never exercise them.
