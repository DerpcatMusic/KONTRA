# Original Ladder gain and audio reference

This reference supports comparison with the existing HQ-off kernel. It does not
add HQ processing, change parameter admission, or clear an importer diagnostic.
Three native float32 associations now match all 117 retained HQ-off PCM cases
bit for bit. The initial 84 failures and all eight experimental combinations
remain in private evidence. Gain targets, parameter domains and quality admission
are unchanged.

## Reference identity and recovery

Kontakt 8.13.1 `Kontakt8.exe`, 178,386,944 bytes, SHA-256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
No native application, original entry point, library sample or licensing path was
executed. The probes execute selected unchanged DSP instructions with authored
inputs and explicitly initialized synthetic core state.

UNPACER 0.3.6 could not package the gain setter because that address has no
on-disk unwind record. A direct full-PE REA attempt reached its 75-second bound
without a result. Both failures remain in private evidence. REA 6.5.0/Ghidra
12.1.4 then analyzed a caller-selected 160-byte original gain slice through its
explicit raw-x64 loader. Its input digest matches the original source region;
the reported function covers 154 bytes and 32 instructions. External constants
and the lookup table lie outside that slice and are resolved independently from
the complete original PE by the numeric probe. Raw-slice analysis does not prove
whole-PE function ownership or recover a runnable host.

The original filter-record reader `0x140d00db0` was separately recovered through
UNPACER/REA with 1,880 analyzed body bytes verified against the original PE and
zero synthetic body bytes. Its selected unwind spans 1,930 bytes; a terminal
call's inferred no-return behavior remains a provider limitation.

## Gain and quality controls

Original setter `0x140b05c70`, parameter zero, tail-calls gain leaf `0x140b05320`.
Within the current signed normalized gain domain `[-1, 1]`, it stores `12*x` dB
and derives linear amplitude by interpolating the generated table at
`f32(1200 + f32(119.58940887451172*x))`. The original instructions use separate
float32 multiply and add operations. Sixty cases cover this setter with both
the generated table and an authored irregular table, including inputs outside
the currently admitted domain. The wider cases document the native leaf rather
than widen product admission.

Six additional cases identify the quality setter's strict `x > 0.5` threshold.
Original prepare `0x140b06180` selects internal rate factors one and two and
configures the original resamplers used by audio `0x140b00b80`.

The v0x92 reader places its Ladder-specific byte at raw-record offset `0x28`;
v0x95 also reads a separate byte at `0x29`. The Ladder branch stores its leading
float at `0x14`, cutoff at `0x0c` and resonance at `0x10`. These record offsets
are distinct from DSP-core offsets. Its ownership was subsequently traced separately through the original typed
22..33 apply branch: `+0x28` reaches setter parameter 3 and therefore selects HQ
when nonzero; `+0x29` instead reaches the separate cutoff-path byte `+0x49`.
The core prepares a changed HQ request at its next audio entry. This ownership
proof does not implement HQ: the current IR and `LadderSettings` have no
represented HQ mode, and this change does not alter importer diagnostics.

## Original audio captures

The retained grid has 564 complete stereo captures of 1,024 frames each:

- 44.1, 48 and 96 kHz; HQ off and on.
- Gain `-1`, zero and `+1`; three cutoffs and three resonances.
- Small and hot impulses plus authored noise followed by zero input.
- All twelve output selectors and explicit zero-input cases.

All captured samples are finite. Six rate/quality configurations also produce
identical original PCM across host partitions of 1, 7, 31, 64, 127 and 256 frames
when the adapter follows the observed outer caller's at-most-32-frame core calls.
An earlier direct 127-frame core call failed because the core fixes its internal
chunk size once per invocation; that invalid-call receipt remains separate.
The corrected adapter changes no target instructions.

The table initializer substitutes 2,400 CRT `pow` calls. HQ coefficient setup
additionally substitutes CRT `sin`, `expf` and `exp`, plus memory operations.
HQ-off setup needs no helpers after table initialization. Original HQ-off audio
uses only `memcpy`; HQ-on audio calls no imported helper in these captures. No
filter, nonlinear stage, control clock or resampling DSP helper was substituted.
The Windows CRT's last-bit math results are not established by these substitutes.

## Product comparison and remaining scope

The compact test fixture selects 81 HQ-off LP4 rate/knob combinations and 36
selector/rate cases. Each retains 24 sample checkpoints and a complete 1,024-frame
stereo PCM checksum; authored inputs are reproduced from exact float32 bits.
A separate test checks nine literal gain-target values, all bit exact.

The initial product comparison failed 84 of 117 complete PCM hashes, with maximum
retained-checkpoint absolute error `2.592802e-6`. Test-only variants attributed the
differences without changing constants, using a zero-change variant that also
had to reproduce every production sample exactly:

| Associations changed | Hash failures / 117 | Maximum checkpoint error |
| --- | ---: | ---: |
| None | 84 | `2.592802e-6` |
| Double each history before its addition | 81 | `2.7567148e-6` |
| Form `q * 4.16` before both feedback products | 84 | `3.3676624e-6` |
| Both history and feedback | 12 | `7.450581e-9` |
| Both, plus native final weighted-sum grouping | 0 | 0 |

The remaining twelve cases were HP2, HP3, HP4 and Notch at each of the three
sample rates. Original `0x140b01092..0x140b01125` doubles the histories before
addition, `0x140b010c4` forms the feedback factor first, and
`0x140b012a9..0x140b012bf` groups stage 1 with the clipped input in the final
sum. Production now follows exactly those three associations. Coefficient laws,
casts, FMA behavior, control clocks and parameter codecs were not changed.

The optimized `ci`/`x86-64-v3` Ladder module passes 9/9 tests, including the strict
native PCM grid and existing held controls, reset and partition oracles. The
same binary passes the full core library: 487 passed, zero failed, 11 ignored;
this count includes those nine Ladder tests. The duplicate experimental kernel
was removed after retaining its source, eight-way results and binary hashes.
The relevant runtime integrations also pass: buses 17, DSP controls 6, DSP chains
8, and voice modulation 24 with one ignored benchmark. The complete validation
count is 542 passed and 12 ignored; the nine Ladder tests are a subset, not an
extra count. The integration source map remained unchanged through the run.
These are correctness results, not performance measurements.

The separate field-consumer probe covers 72 synthetic already-parsed records and
six original quality state/rate transitions in 91 bounded calls/fragments. It
executes no serialized reader, factory or native host. Its zero-input transitions
establish quality-state ownership, not changing-quality audio parity.

The static PCM captures do not establish original factory lifetime, public engine
parameter binding, reset/bypass scheduling, live wrapper control cadence or
actual-library equivalence. HQ-on captures are a reference for future
work, not support in the current kernel. Diagnostic wall time is not a CPU result.

Private numeric data, helper-call records, original byte bounds and failures are
under `/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-native-ladder-20261015`.
Vendor code and the full PCM grid are not committed.
