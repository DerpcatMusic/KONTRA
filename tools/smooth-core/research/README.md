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
| `rea-8f03f0.md` | the weighted reconstruction | the band weights' assembly |
| `rea-8f78e0.md` | the projection | the weighted projection |

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
