# Conflux C8–C10 source DSP boundary, 2026-10-15

C8 (`wavetableInharmonic`), C9 (`wavetableModAmount`) and C10
(`wavetableModFrequency`) remain omitted. The current tree lacks both their
live source destinations and the native source processing they control. This
audit adds evidence and a calibration boundary, not a guessed oscillator or
new production admission.

## Current implementation

`sampler-ir::Wavetable` contains position, onset phase, two form amounts and
two form selectors. Its modulation destination enum has only Position,
Form1 and Form2. `sampler-core/src/lower.rs::wavetable_lanes` binds those
three amounts and two selectors to physical engine addresses. The Kontakt
translator diagnoses the other destinations instead of constructing routes.

`sampler-kontakt/src/wavetable.rs::admitted` rejects enabled inharmonic mode,
nonzero auxiliary modulation type, randomized phase and disabled key tracking.
The file codec preserves their metadata, which is different from executing
those modes. The existing phase-form leaf has 490 original-instruction vectors;
it does not establish the complete oscillator, auxiliary waveform, amount law,
frequency units, mode order, reset or smoothing clocks.

There is a separate **quality gap**: the file codec admits saved quality
ordinals 1 through 4, but `admitted` does not carry quality into IR or select a
quality-specific kernel. `sampler-core/src/wavetable.rs` always uses its cubic
cycle readout and linear table morph. The four source quality settings therefore
cannot be called equivalent merely because the source imports. NI describes
four interpolation choices with different accuracy/aliasing and CPU costs.
[Kontakt source-module reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view).
No quality gate or kernel was changed in this audit.

## Native identity and execution evidence

Read-only target: Kontakt8.exe, SHA-256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
All addresses below refer to that image. A raw byte/PE scan and bounded Ghidra
queries use the existing Kontakt8131 project with no reanalysis or host launch.
The queries use dev-drive scratch, logs and locks. Several project function
bodies are single-address placeholders; recovered decompilation is useful
control-flow evidence, not a complete analyzed-body/callgraph census.

| Observation | Native address or value | What it establishes |
| --- | --- | --- |
| `$ENGINE_PAR_WT_MOD_AMOUNT` string | `0x144e7eac8` | Native token exists; current public KSP source-module list omits it |
| Its symbol registration | `0x14045c7d4..0x14045c7f3`, identity `0x1de` / 478 | Registration identity, not a source-DSP ordinal or normalized law |
| WT_MOD_TUNE registration | `0x14045c7af..0x14045c7ce`, identity `0x1dc` / 476 | Separate registry identity |
| WT_INHARMONIC_MODE registration | `0x14045c71b..0x14045c73a`, identity `0x1d8` / 472 | Separate registry identity |
| Saved target-name registration | `0x140a6d5c2..0x140a6d605` | Inharmonic / amount / frequency have target IDs 14 / 15 / 16 in another registry |
| KSP command builders | `0x1407ad260` / `0x1407aad30` | Construct set/get commands; these are not oscillator consumers |
| Statement executor | `0x140798900`, cases `0x39` / `0x3a` | Calls setter `0x140789c30` / getter `0x140785620` with evaluated arguments |
| Setter ID translation | Runtime table `0x14a5ec800` | KSP identities map to another parameter namespace before dispatch |
| General typed setter dispatch | `0x1408bdd60` | Downstream dispatch to trace; a call alone does not identify the WT consumer |
| `BWavetableMachine` RTTI / vtable | Descriptor `0x14a3bbe78`, locator `0x1498c4b38`, vtable `0x144fd0c90` | Identifies the native machine class independently of UI display classes |
| Native machine preparation | `0x140ce5fa0` | Resets retained clock/history fields and computes rate-dependent values; not the FM/inharmonic render law |

The current `engine_parameter_names.rs` omits WT_MOD_AMOUNT while the KSP
parity ledger labels that name Approximate. This is a concrete catalog/ledger
mismatch to repair separately with a reached-callback regression and truthful
unsupported-consumer reporting. Neither adding a name nor accepting its stored
value would make C9 supported. The [official KSP list](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters#instrument--source-and-amplifier-module)
documents the related WT modes and tuning units; its omission is not evidence
that the token is absent from this native image.

Generic native modulation helpers classify target IDs 14–16 as additive, but
that does not establish the source parameter's physical scaling, post-combination
clamp, time interpolation or audio-rate consumer. NI describes inharmonic
control as detuning a second wavetable oscillator and auxiliary tuning in
semitone, ratio and fixed-Hz modes. Those descriptions do not specify an exact
frequency/phase/ring-modulation kernel. The tuning-unit domain must be measured
independently of the mode selector and modulation-depth domain.

## Smallest next calibration

First trace the translated WT parameter through the typed dispatcher into its
actual source-field setter/getter. Capture the registry translation, accepted
input range, physical stored value and rejected-write atomicity with a bounded
original-byte harness before selecting a Rust law. Do not initialize the BSS
mapping to an assumed ID or substitute a sine oscillator/math helper to make
the probe run. Any unimplemented import or unknown required ownership stops
that probe and remains an explicit gap.

Then isolate **one** deterministic mode and waveform with authored 2048-frame
table data, fixed phase, zero randomness, no effects or modulation routes, and
one voice. Record native output plus both oscillator phases before and after a
held amount or tune edit. Compare whole-block and partitioned execution at the
same sample rate, including an edit at a non-block boundary. Start with bypass
and zero amount, then one nonzero amount/tuning value; add other modes only
after this state/clock boundary is understood. Inharmonic disabled/enabled must
also retain its own second-oscillator state and onset/reset evidence. A scalar
setter result alone cannot promote the audio engine or the live C8–C10 routes.

The native constructor, field lifetimes and exact audio consumer still need
verification before this held-voice harness can be claimed as executed. No new
native audio calibration or production source kernel was completed here.

A smaller original-byte **preparation-only** probe has now executed
`0x140ce5fa0`, including its original native callee `0x141a3f2c0`, on detached
zeroed machine storage. Eight sample rates, each repeated on fresh storage,
produce identical complete state images. Only CRT `memset` is serviced (16
calls total); no native DSP/math helper is substituted. The observed prepared
fields are:

| Rate Hz | Integer at `+0x102a8` | Float at `+0x102ac` |
| ---: | ---: | ---: |
| 8,000 / 11,025 / 22,050 / 44,100 | 1 | 0.03125 |
| 48,000 | 2 | 0.015625 |
| 88,200 / 96,000 | 3 | 0.010416666977107525 |
| 192,000 | 6 | 0.0052083334885537624 |

The original preparation uses the f32 constant
`3.125000148429535e-5`, rounds its rate product by adding f32 0.5 before integer
conversion, clamps the integer to at least one, and stores its reciprocal
after multiplication by 32. These are measured fields, **not an assigned
inharmonic/FM smoothing or render cadence**. Their consuming state transitions
must still be traced before porting a clock.

The parameter-map initializer is `0x140780c80`, reached by the read-only table
xref at `0x140468504`. Its strict original-byte probe stops on an unmapped read
at `0x14098f94a`: the engine metadata owner has not been initialized. No table
entries are guessed and no helper is skipped to obtain a passing result. The
typed setter decompilation also leaves the jump table at `0x1408c483d`
unresolved. Those limitations prevent a claimed C8–C10 setter/domain result.

Direct machine configuration is recovered at `0x140cdaf50`. It clamps three
source-block floats (`+0x6c`, `+0x68`, `+0xb4`) to [0,1], forwards `+0x84` and
`+0xbc` without a clamp into machine fields `+0x10178` and `+0x10194`, and
converts `+0xc0` with helper `0x140ce73f0` into `+0x1019c`. These source-block
offsets still need independent reader/parameter-name correlation; physical
labels are not assigned from proximity alone.

Two smaller consumers are now available for the next exact native probe.
`0x140ce6b00` stores mode at `+0x1018c`, clears nine mix-weight fields beginning
at `+0x1027c`, and sets the selected one/two weights for modes 1–9, marking an
update at `+0x102a0/+0x102b0`. `0x140ce73f0` branches on unit field `+0x101a0`:
one branch forwards the input, one rounds a signed ratio mapping, and one uses
an exponential polynomial. Recovering these branches does not establish their
full source/host normalization or oscillator output. The auxiliary waveform
setter `0x141a3f0e0` stores its selector only. Numeric leaf execution and the
audio consumers remain separate gates; none was ported to production here.

The detached original-byte leaf probe now executes 12 mode writes, including
0, 1–9, 10 and -1. It verifies the exact write footprint: the mode, nine
weights, update flag and update counter; other bytes remain unchanged. Mode 3
sets weight cells 0 and 2 together; modes 1/2/4/5/6/7/8/9 set cells
0/1/6/7/3/4/5/8 respectively. This is a measured state selector, not proof of
the corresponding oscillator output.

The tuning leaf returns for all eight finite inputs in units 0, 2 and 3:
unit 0 preserves input bits including negative zero, unit 3 returns zero, and
unit 2 produces the following selected outputs. All returned cases leave
machine storage unchanged. The eight unit-1 cases explicitly stop at imported
`roundf`; no math implementation is substituted to manufacture native vectors.

| Unit 2 raw input | Native output |
| ---: | ---: |
| -1 | 0.000025000294044730254 |
| 0 | 0.4999934136867523 |
| 0.5 | 70.7110595703125 |
| 1 | 10000.3408203125 |

A separate original configuration probe executes 36 finite raw-field cases
through `0x140cdaf50`, after native preparation. The three clamp lanes map -1
to 0 and 2 to 1, preserving negative zero. Raw fields `+0x84/+0xbc` instead
preserve all tested -1..2 input bits. Unit-2 conversion of `+0xc0` matches the
leaf above. The concrete optional-unit branch is supplied directly; no virtual
getter is invented. Configuration calls no imports and preserves an opaque
128-byte sentinel region at `+0x101e0..+0x10260`. The region's preservation is
not labeled oscillator-phase continuity without consumer proof.

Finally, original dispatcher `0x140cddeb0` is observed on eight combinations
of raw selector `+0x10180` (0–3) and flag `+0x1017c` (0/1), stopping at the
first instruction of each selected kernel. It selects eight distinct entries:
flag 0 selects `0x140b2d8d0/0x140b09e30/0x140b0e1f0/0x140b127b0`; flag 1
selects `0x140b305a0/0x140b0cbc0/0x140b11160/0x140b15780`. No audio kernel is
executed or replaced by this dispatch probe. Together these findings identify
concrete native branches for later calibration; they do not settle host value
normalization, serialized ordinal conversion, complete processing or C8–C10
parity.

Raw local receipts live under
`/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-cpu-20261014/`:
`wavetable-native-string-audit.json`, `wavetable-native-string-xrefs.json`,
`wavetable-native-registration.asm`, `wavetable-native-ksp-near-candidates.txt`,
`wavetable-native-ksp-statements.txt`, `wavetable-native-engine-dispatchers.txt`
and `wavetable-native-rtti-vtables.json`. Failed/unresolved candidate probes are
retained alongside these files. No native binary, decompiled body or library
payload is committed with this document.
Preparation probe source, numeric output and log are
`wavetable-native-prepare-probe.py/.json/.log`; the explicit failed map probe is
`wavetable-native-map-probe.py/.log`. The initializer/typed-dispatch slices are
`wavetable-native-parameter-map-and-source-setter.txt` and
`wavetable-native-typed-dispatch.txt`.
Direct machine/helper slices are `wavetable-native-machine-consumers.txt` and
`wavetable-native-auxiliary-helpers.txt`.
The later probes are `wavetable-native-leaf-probe.py/.json/.log`,
`wavetable-native-config-probe.py/.json/.log` and
`wavetable-native-dispatch-probe.py/.json/.log`.
