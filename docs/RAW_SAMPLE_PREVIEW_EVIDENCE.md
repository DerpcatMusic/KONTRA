# Dry Mapping sample preview evidence

Reviewed 2026-10-05, based on `96dd6c8`. This change implements one common dry
source preview through Kontakt and optional UVI adapters. It changes no UVI graph
admission, modeled instrument behavior or modulation cadence.

## User behavior

Mapping retains the exact global zone index and source owner. Repeated filenames
remain distinct zones. The sample list supports group selection and paging;
selection is resolved before all rows are painted. The selected inspector shows
initial authored key/velocity/root and source identity. Play dry sample prepares
one mono/stereo source, reports real source dimensions and plays it once through
the main output/master gain. Stop, reset, source replacement, restore and editor
lifecycle cancel the matching owner. A window's old receipt cannot stop a newer
preview started by another window.

This is raw physical-source inspection. It deliberately skips instrument scripts,
round-robin selection, zone tuning, loops and effects. It does not claim live
script-selected mapping or microphone classification. No invented waveform or
meter is drawn.

## Ownership and preparation

`SelectionStore` owns the request authority and source-commit fences. Its read-only
`read`/`try_read` preserve lock semantics; public `replace` and accepted persisted
assignments cancel before replacement under the write lock. Raw writes become
crate-private. This is an experimental Rust source API change, not a stable ABI.
The existing persistence codec remains authoritative: rejected data preserves
ownership; accepted legacy/default/partial migration is still a replacement.
Capture/writeback and unrelated-slot presentation edits preserve preview.

Preparation is serialized off audio, separately from long instrument loading.
The exact retained source/rate/owner is checked before preparation and publication.
The Kontakt adapter uses the real Source/SampleReader; UVI uses the real ordered
library resource/audio path. Neither invokes an instrument Player, Worker or Lua.
A growing loose file is pinned to its checked extent before decode. Source-version
checks are sampled metadata/header checks, not cryptographic content authentication
or proof against same-version same-inode modifications.

One tagged monotonic request authority drives status. Late Ready cannot regress
Playing/Stopped or replace a newer ID. A full one-item mailbox retains one payload
off audio and retries after an actual callback pop, rather than GUI redraws.
The callback uses immutable PCM and a shared cursor; reserved retirement ownership
keeps PCM destruction off audio even with full queues, cancellation, EOF and rate
mismatch. Reset/panic only invalidate. Cancellation takes effect at the callback's
next defined observation boundary, not retroactively on already emitted audio.

## Limits

- Preview supports mono/stereo audio and integral host rates 8–192 kHz. Image
  tables and other channel layouts are explicit unsupported cases initially.
- Encoded input is capped at 16 MiB. Source/conversion/output allocations have
  separate guards; final host-rate PCM is capped at 32 MiB per payload.
- Current, ready, service-held and retired final PCM total at most 128 MiB.
  This is a final-payload bound, not a whole-feature heap/RSS bound: codec, demux,
  metadata, decoded operands and allocator scratch overlap separately.
- Equal-rate conversion preserves physical frame bits. Other rates reuse the
  common Hermite kernel with checked duration and zero padding. This cubic
  interpolation is not bandlimited or proof of native resampler equivalence.
- Bounded chunk/callback cancellation does not interrupt an indivisible file
  syscall, codec packet or synchronous encoded transform mid-operation.
- Finite preflight rejects overflowing preview mixing before changing ordinary
  audio. There is no new normalization, clipping or voice clamp.

## Functional and visual verification

All 48 selected functional checks passed before visual correction: 32 new authored
cases and 16 justified routing/restore/lifetime/public-codec regressions. They
exercise actual local decoder/audio/worker/editor code with authored source files
and metadata, including real tiny WAV/UFS member reads. These are not vendor-bank,
DAW or native-oracle tests. The first compilation caught a missing read-only
`try_read` adapter; it was added without weakening the existing lock-order test.

Compact/wide authored Mapping captures use the existing CPU paint path. The first
compact capture exposed overlapping wrapped inspector text. Adding nonshrinking
rows did not resolve it: the new baseline-containment assertion failed on the
compact selected range, despite successful compilation. The post-change run
stopped after 29 successful checks and this failure; remaining checks were not
rerun. The source cause is scroll-node text measurement without a definite width;
a caption-width successor is privately prepared, unintegrated and unverified.
The stronger assertion remains in source, exposing the outstanding defect rather
than weakening acceptance.

The UVI-disabled pre-layout source compiled and passed 29 common preview checks.
The final source has not been separately checked in that configuration, packaged
or installed. The installed checkpoint remains `96dd6c8`. The user requested an
immediate commit/push and stopped further verification.

No actual purchased-bank dry preview, current DAW interaction, native resampler
comparison, full-bank census, sustained playback or measured speedup was added.
Earlier playback/DSP/control evidence retains its original checkpoint scope.
