# Multichannel: CURRENT13 read-only design review

Base: `13c044d6f9f80576507521a2dc6521e9aad7d9c5` (not historical source3665).
Scope: Kontakt decode → intermediate representation (IR) → stream → render,
including convolution impulse responses. Product implementation unchanged.
All authored Rust is **NOT_RUN / NOT_COMPILED**. Native behavior **UNKNOWN**.
CPU **AND** RAM versus both reference products: **UNACHIEVED**.

## Primary requirement, not a project claim

The archived unversioned NI manual lists **multichannel WAV and AIFF** separately
from mono/stereo in its Audio file formats table:

- URL: `https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/file-formats#audio-file-formats`
- Archive: coordinator `handoff/official-docs/kontakt-formats.html`
- SHA-256: `46e22eebeb2e1ab7f3799cedbf5011a37209c80896f907e15e981f133dea15de`
- Archive retrieval: `2026-10-10T02:13:01.037857+00:00`.

The Convolution section says it “fully supports a multi-channel signal flow,
allowing you to use surround impulse responses,” permits third-party WAV
impulses, and displays channel count, bit depth and sample rate:

- URL: `https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference#convolution`
- Archive: coordinator `handoff/official-docs/kontakt-effects.html`
- SHA-256: `f903ae5b1807ff273324c4f8237174f39f83dc0343536b51d73b477031313644`
- Archive retrieval: `2026-10-10T03:52:51.683112+00:00`.

Both HTML hashes independently verified. No shared archive/index was modified.
These specifications establish a support gap. They do **not** establish a
maximum channel count, channel order, WAV-mask interpretation, AIFF surround
layout, downmix weights, or a convolution input/output matrix. No mapping law
or installed Kontakt 8.13.1 equivalence is inferred from them.

## Exact current source and callers

Paths/lines below are at the pinned base; navigation used `rg` because this
clean checkout has no `graft/`. The root checkout's untracked AGENTS instructed
that fallback. The historical `src/engine` layout is not CURRENT13.

| Seam | CURRENT13 source | Actual behavior |
| --- | --- | --- |
| Full decode | `crates/sampler-kontakt/src/samples.rs:782-803` `decode` | RIFF and FORM call `wav`; NCW explicitly selects 0 and `min(channels,2)-1`. |
| PCM frame conversion | same file `Wav::frame:831-861`, `wav:864-887` | Advances by the complete interleaved stride but emits only channels 0/1. Mono repeats channel 0. No mixing. |
| WAV admission | same file `wav_layout:889-945` | Positive channel count; supported sample encoding; no upper channel bound. Extensible tag reads subformat tag but does not retain channel mask. |
| AIFF admission | same file `aiff_reader:952-1059` | Positive count; PCM/AIFC NONE/twos/sowt/fl32/FL32; uses COMM frame count and SSND extent; no upper channel bound. |
| Random access | `crates/sampler-kontakt/src/stream.rs:166-226` `SampleReader::open_counted`, `read:251-297` | WAV and FORM parse into `Codec::Wav`; `Wav::frame` loses the same channels. NCW read uses channel 0/1 too. |
| AIFF mono/stereo error | `crates/sampler-kontakt/src/pcm.rs:69-74,139-144` | Symphonia `Reader::open` / `decode_next` reject >2. Only located production caller is `stream.rs:210`. |
| Resident load | `crates/sampler-kontakt/src/load.rs:172-215` `load_read` | `Samples::decode` → boxed stereo frames → `Pcm::new`. No channel metadata reaches `finish_kept`. |
| Stream admission | `load.rs:277-308`, `stream.rs:550-580` | Header cache or resolved Source → `Streamer::open`; cached `source.header()` can skip `source.open()` entirely. |
| Sample IR | `crates/sampler-ir/src/lib.rs:357-364` `Asset`; Kontakt raw `kontakt.rs:162` | Asset has no PCM channel/layout field. Raw `num_channels` exists but is not a preserved decoded PCM contract. |
| PCM/page storage | `crates/sampler-core/src/lib.rs:182`, `prepare.rs:23-26,74-83`; `stream.rs` | `Frame = [f32;2]`; resident and streamed PCM/page buffers are stereo. |
| Render | `crates/sampler-core/src/render.rs:140-174`, `src/sound/v2.rs:1803` | `Runtime::render` / `render_split` accept stereo Frame slices. Multiple stereo output buses do not preserve extra channels of one source. |
| IR loading | `crates/sampler-kontakt/src/library.rs:531-542` | `resolve_impulse` → `Samples::decode` → stereo `(rate,frames)` closure. Loss occurs before shaping. |
| IR shaping | `crates/sampler-kontakt/src/effects.rs:749,1173-1274` | `Decoded=(u32,Vec<[f32;2]>)`; shapes two vectors; stores stereo `sampler_ir::Impulse`. |
| IR restore | `crates/sampler-kontakt/src/cache.rs:167-172` | Re-decodes impulse recipe source through `Samples::decode`; failure is a cache miss. |
| IR representation/DSP | `crates/sampler-ir/src/lib.rs:1551-1565`; core `lower.rs:1987`; `dsp/convolution.rs:317-378` | Left/right vectors become two independent convolvers. Not a surround or cross-channel convolution matrix. |

**Correction to the historical premise:** ordinary CURRENT13 AIFF/AIFC PCM is
silently truncated, just like WAV. In `open_counted`, `pcm` is true for FORM;
the earlier `if pcm` either returns `Ok(Self { ... Codec::Wav ... })` or returns
an error through `?`. Therefore the later `if head.starts_with(b"FORM")`
Symphonia branch is unreachable for FORM input. Its mono/stereo error must not
be described as the active AIFF format policy.

For six-channel input whose first two channels are silent and later channels
contain audio, current frame selection produces silence. It is neither a
surround renderer nor an intentional downmix. Missing channel metadata means
no downstream unsupported warning can recover that distinction.

## Minimum safe staged fix — design only, not applied

### Stage 1: stop silent data loss without changing product structure

This is a capability/admission correction, **not multichannel parity**. Reject
valid >2-channel assets before allocating decoded PCM or publishing streamed
assets. Keep zero-channel / malformed encoding / truncated extent failures
separate. The diagnostic should identify source path, declared count and
format, e.g. `unsupported 6-channel AIFF: this renderer supports mono/stereo;
no channels were discarded`. Existing String/LoadError/io error interfaces
can carry it; no new dependency or public type is needed.

Required small spans for a separately authorized implementation:

1. Add >2 channel rejection in `wav_layout` and `aiff_reader`, after each
   existing format-validity check. Both resident PCM and random-access PCM
   reuse those parsers. Do not change frame count units or mono duplication.
2. Add the same capability rejection after NCW reader/header validation in
   `samples::decode` and `SampleReader::open_counted`, before NCW decoding or
   codec publication. The NCW reader validates nonzero channels, not <=2
   (`vendor/ncw/src/header.rs:48`; reader allocates channel vectors). The NCW
   writer only supports 1/2, so do not manufacture a multichannel bank or use
   that writer as an acceptance oracle. NCW support itself is listed by NI;
   an exact vendor multichannel NCW contract is not established here.
3. Close the numeric-header fast path. The minimum conservative local choice
   is for Kontakt `impl AssetSource for Source::header` to return `None`, so
   `Streamer::open` actually opens/validates every source at admission even
   on an existing header-cache hit. Do not change custom/UVI AssetSource
   implementations. Old `Source.header=(rate,frames)` carries **no channel
   count** and cannot prove admission. A cache MAGIC bump alone is not enough
   unless all writers/readers guarantee the new channel policy. Re-enabling
   the fast path needs a versioned, policy-validated channel header later.
4. Preserve existing IR error propagation: `effects::chain_with:1054-1080`
   reports an IR decode error as scoped slot `impulse response` / `NotModeled`
   and omits that convolution. `library.rs:552-554` retains those notes in
   `instrument.unsupported`; it does not abort the whole instrument. Cache
   recipe failure must remain a miss and take the same visible cold path.
   Never substitute a unit impulse or a guessed first stereo pair.

Removing the dead Symphonia branch is optional cleanup, not required for this
stage. Do not change `Frame`, public `Decoded`, IR schema, engine buffers,
output topology, build manifests, targets or wrappers in stage 1. Disabling
numeric-header reuse may add initialization I/O; it is outside the audio
callback. No CPU/RAM improvement is claimed. Failures should happen at load,
not after a voice has begun streaming. General/custom source contracts remain
outside this Kontakt-only stage.

Stage 1 acceptance must cover old numeric caches, warm/cold impulse recipes,
loose samples and bounded monolith members, not just public `decode` calls.
The tiny supplied Rust tests deliberately do not claim that integration scope
is complete. A guard patch lacking cached-admission and IR reporting checks
is not ready to integrate.

### Stage 2: actual preservation (blocked by the no-structural-change scope)

Retain channel count and layout with all PCM channels, from decoder through IR
and source identities/cache keys, packed PCM, streamed pages and render buses.
Choose a bounded channel count and preallocate off audio; do not add per-frame
allocation/locking/I/O. Define channel routing explicitly; raw ordinal channels
are not automatically speaker positions. WAV masks and AIFF layouts need a
verified policy. Define surround IR input/output matrix semantics before DSP
changes. Only an explicit user-selected conversion may downmix, with a visible
loss report. None of these policies are specified by the two cited sections.

A decoder-only `Vec<Vec<f32>>` change would still drop channels at `Pcm::new`
and stereo IR construction. Splitting one sample into independent zones would
also change voice/loop/modulation/steal semantics. Neither is a minimum safe
fix. Full preservation cannot be completed in this bounded source-only task.

## Supplied evidence and synthetic sources

- `tools/check_multichannel_current13.py`: stdlib-only source pin/hash checks,
  archived primary hash checks and 72 in-memory synthetic fixture checks.
  Formats: PCM16 WAV, extensible PCM16 WAV, float WAV, PCM16 AIFF, AIFC sowt,
  AIFC fl32; channels 1/2/3/4/6/8; unique markers and tail-only markers.
  Checks FORM/RIFF extents, frame stride, payload values, rate fields and
  demonstrates why first-two selection erases tail-only content. This is an
  independent fixture/model check, **not execution of product decoding**.
- `audits/fixtures/multichannel_admission.rs`: unwired, dependency-free fixture
  builder plus three stage-one acceptance tests using existing public Kontakt
  interfaces. Tests require reject-at-decode/open for 3/4/6/8 WAV/AIFF, preserve
  mono/stereo markers at nonsequential offsets, and refuse truncated extents.
  Rejection test is expected to fail CURRENT13; no product target is added.
  Neither test execution nor type checking was performed.
- Direct installed `rustfmt` 1.98.1, edition 2024: syntax/format check only.
- Python/static result: PASS; fixture case digest
  `0a1a91685bb60e24931a5c88851c461f331656cde64d0bf4a04c23936c3a4785`.

Still missing: implemented guard, compiler/type checks, Rust test execution,
NCW multichannel fixtures, warmed header-cache admission test, archive-member
multichannel fixture, IR unsupported-slot/cached recipe test, streamed page
boundary/loop tests, surround output comparison and native parity/performance.
No real bank, application, host, build/test executable, install or publication
was used. No automatic merge. Root checkout, old ledger and all manifests
remain untouched by this worker.
