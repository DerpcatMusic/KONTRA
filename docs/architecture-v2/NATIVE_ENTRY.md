# Owned preparation and independent native execution

2026-10-05; follows `ae59279`. This is the first separate application composition
root for the new core. It has no dependency on the old Engine, VM, UI, import model
or host plugin. At this checkpoint its only dependency was `sampler-core`. The later
[MIDI ingress](MIDI_INGRESS.md) adds `sampler-midi` and shared sample-offset block
processing; neither path depends on the old product.

## Implemented path

`WAV / authored PCM → Prepared → Runtime::trigger → one selected family → PCM → terminal`

`Prepared` takes ownership of PCM boxes without copying their contents, validates
finite frames/rates/regions, and compiles a bounded key-candidate index. Source order
is preserved. The first native mapping has key and velocity ranges plus fixed-pitch
sample/gain selection. Velocity is retained at canonical precision for selection,
then converted to the float audio gain domain. PCM must match the prepared rate.

`Runtime` now owns its immutable prepared plan. The previous borrowed fixture-only
constructor is removed. There is no leaked storage or self-referential lifetime
workaround. Construction/destruction remain control-side operations. This establishes
one-plan ownership. [Bounded plan replacement](PLAN_ADOPTION.md) now extends the
core with retained generations and off-audio retirement; the replacement audition
below exercises both generations through this executable.

`trigger` preflights the complete matching layer set before admitting ownership.
Capacity failure cannot sound a partial family. A valid unmapped input still creates
a no-source logical note with normal physical release and terminal delivery. Manual
source APIs remain native low-level services, not adapters to the old core.

The independent `sampler-native` executable prepares and renders offline. File IO is
outside the render call. Its WAV boundary accepts complete RIFF files containing
mono/stereo PCM16 or IEEE float32 with valid format/alignment/finite samples, up to
256 MiB. Unsupported formats fail explicitly. Output is stereo float32 WAVEFORMATEX
with a fact chunk; existing output paths are never overwritten. This is a narrow
resident import/export boundary, not a replacement for production decoding/streaming.

## Run it

Choose output paths that do not already exist:

```sh
CARGO_TARGET_DIR="$PWD/target-core" cargo run --locked --offline --release -p sampler-native -- demo /tmp/kontra-v2-demo.wav
CARGO_TARGET_DIR="$PWD/target-core" cargo run --locked --offline --release -p sampler-native -- render /tmp/kontra-v2-demo.wav /tmp/kontra-v2-copy.wav
cmp /tmp/kontra-v2-demo.wav /tmp/kontra-v2-copy.wav
```

Demo renders two seconds at 48 kHz: a prepared 440 Hz sample, sustain down at 0.25 s,
physical key-up at 0.5 s, and pedal-up at 1 s. The current demo adds a native envelope whose release ends at 1.05 s; the
original entry-slice evidence below predates that envelope. This
exercises the scheduler and lifecycle through the new executable, rather than a
legacy host adapter. `render` replays a supported input at its original sample rate.

## Validation

- 17 core unit checks, three core-only allocation/free checks and the native WAV
  boundary check pass on Rust 1.99.0 and the 1.92.0 minimum, including release on 1.99.
- Strict clippy passes for both packages/all targets. Historical KSP development
  integration still passes after updating its fixture construction; no old VM is
  linked into the native executable.
- A generated 40-region map is checked over all 128 keys and five velocities against
  an independent linear selector, including boundary precision. Layer-budget failure,
  no-source ownership, invalid preparation and candidate-budget failure are tested.
- Repeated native selection/render/EOF/release proves no callback allocations or
  frees, including owned sample retention after voices finish.
- Release executable generated 96,000 frames, with exact silence after frame 48,000;
  replay produced byte-identical WAV output. Malformed input was rejected before
  creating output; attempted overwrite failed without changing the existing file.
- FFprobe independently recognized stereo `pcm_f32le`, 48 kHz. Source and executable
  hashes plus output/evidence are retained under ignored `artifacts/architecture-v2/`.

```sh
CARGO_TARGET_DIR="$PWD/target-core" cargo test --locked --offline -p sampler-core -p sampler-native
CARGO_TARGET_DIR="$PWD/target-core" cargo +1.92.0 test --locked --offline -p sampler-core -p sampler-native
CARGO_TARGET_DIR="$PWD/target-core" cargo test --locked --offline --release -p sampler-core -p sampler-native
CARGO_TARGET_DIR="$PWD/target-core" cargo clippy --locked --offline -p sampler-core -p sampler-native --all-targets -- -D warnings
```

WAV provenance: original code written from public format facts, not copied from a
codec implementation. [Microsoft WAVEFORMATEX](https://learn.microsoft.com/en-us/windows/win32/api/mmreg/ns-mmreg-waveformatex)
was consulted for float32 encoding and extension fields; [ITU-R BS.2088-2 (11/2025)](https://www.itu.int/dms_pubrec/itu-r/rec/bs/R-REC-BS.2088-2-202511-I%21%21PDF-E.pdf)
for the non-PCM fact requirement. Neither document nor third-party source code is
redistributed. Fixtures are authored.

## Open product work

This executable is offline and fixed-pitch; it is not yet a live standalone or
CLAP/VST3 plugin. Tracking/resampling, advanced loop/envelope behavior, richer mapping and DSP,
new scripting, streaming, live plan changes and MIDI 2.0 transport remain open.
The next implementation extends native source/behavior services and the new host
composition; it must not insert a legacy-engine fallback for missing functionality.

## Scripted resident WAV audition

The native script path can now use decoded resident audio:

```sh
CARGO_TARGET_DIR="$PWD/target-core" cargo run --locked --release -p sampler-native -- script instrument.ksp sample.wav audition.wav
```

Omitting `sample.wav` still uses the authored demo sine. The source compiler targets
the decoded sample's rate; no sample-rate conversion is implied. The audition is
exactly two seconds at that rate, independently of asset length. Its input note,
pedals and AHDSR remain the documented audition controls; scripts waiting beyond
the horizon are cancelled during explicit shutdown. Every mapped key plays the
same fixed-pitch sample. This is an executable source/behavior path, not a full
instrument authoring format or production host.

Decoding, source compilation and output-format validation finish before creating
the destination. `Prepared` then owns the decoded PCM; rendering reads no source
file and borrows no frontend objects. Existing output and input paths cannot be
overwritten. The process-level check writes an authored 10 ms mono PCM16 sample,
executes the delayed KSP fixture at 44.1/48/96 kHz, and compares every output sample
against an independent envelope/velocity calculation. It verifies the fixed output
horizon, source-end silence, both channels, unchanged inputs/outputs on overwrite
attempts, and no destination on malformed source or WAV input.

`cargo test --locked -p sampler-native --test script_sample` runs that check. The
new-core score remains 90, complete and authoritative, zero errors and 119 warnings.
Local evidence uses `artifacts/architecture-v2/script-sample-*`.

## Resident plan-replacement audition

```sh
CARGO_TARGET_DIR="$PWD/target-core" cargo run --locked --release -p sampler-native -- replace FIRST.wav SECOND.wav OUTPUT.wav
```

Both resident WAVs are decoded and prepared before output creation. Their rates
must match; the command does not resample. Over a two-second window, the first
note starts at zero. At the half-second sample boundary, the driver adopts the
queued replacement before admitting a second same-key note. FIFO key-ups occur
at one and one-and-a-half seconds. Each note uses its original sample and AHDSR;
the old release tail overlaps the replacement without changing source identity.
Sources can also end naturally before their key-up.

The block driver splits its usual 256-frame buffer at the explicit adoption
boundary, drains accepted note terminals after each block, and collects retired
plans. Returned asset destruction runs outside core/MIDI rendering. Completion
requires two accepted terminals, zero logical notes and one active generation.
The offline driver performs preparation and retirement between audio operations;
separate-thread ownership is additionally checked in the core adoption tests.

The process-level test writes independent constant PCM fixtures at 44.1/48/96 kHz
and checks every output sample against the sum of two independently calculated
AHDSR curves, including both tails and final silence. Mismatched rates fail before
creating output. A one-Hz input is explicitly rejected because its half-second
boundary collides with the first note's sample. The code still uses fixed-pitch
resident playback and a fixed audition timeline; this is not live host integration.

Evidence uses `artifacts/architecture-v2/replace-*`; the authoritative new-core
score remains 90 with zero errors and 138 warnings.
