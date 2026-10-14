# Dynamic sample source revisions

`loadSample` now resolves and prepares a real source on `uvi-sample-load`, publishes a bounded source transaction, and completes its task after the audio runtime commits the new binding and the script owner receives its reply. The producer belongs to one script owner. The callback adopts already prepared transactions and borrows immutable PCM. File I/O, decoding, allocation, registry locks, Lua work, and source destruction remain off audio.

This implements the documented callback/no-callback contract: a callback receives the same task userdata returned by the call, while a call without a callback still loads and returns nil. Completion callbacks resume on the Lua owner. Decoder completion alone cannot set success. [UVI asynchronous API](https://lua.uvi.net/group___async.html)

## Identity and lifetime

The IR carries an explicit optional source and a target sidecar from authored XML node ID to final zone references. Empty, missing, and unreadable initial players retain complete dormant compiled regions: key/velocity selection, pitch/root/tune, envelope, modulation, chains, groups, and buses. No fabricated file or PCM index represents absence.

Each generation allocates an arena on control. Future region bindings select a revision slot. Every admitted voice pins its slot and copies its own cursor. Replacement changes future admissions; sounding and already admitted scheduled voices retain their original source ID, rate, full length, playback window, loops, direction, envelope, and cursor. Old and new sources can coexist within the same plan. Render, parallel render, stretch metadata, stream requests, and demand prediction borrow the pinned source.

A transaction validates target, plan, sequence, output rate, geometry, arena capacity, and byte capacity before changing any binding. Cancellation and commit race through one atomic compare/exchange: cancellation that wins prevents mutation; cancellation after commit begins cannot revoke a committed source. Success remains unobservable until the control reply is accepted. Plan adoption rejects old producers; dropping/reloading a script cancels its outstanding tasks. Decoded or retired ownership always returns through a reply, including failure paths.

## Streaming and bounds

Production UVI loads hand the existing stream cache's decoder registry to the source worker. Dynamic registrations use a separate bounded map, preserving the immutable initial registry. Only decoder/control workers lock that map. The revision retains its registration; retirement removes it on control after the last old voice releases it. Decoder readers already in flight own their own validated origin.

The worker opens the actual bank/disk authority, reads bounded metadata headers, reuses that opened audio reader, and prepares a streamed PCM descriptor with bounded onset/loop heads. It does not fully decode a production sample as a fallback. The same decoder workers and numeric AssetIds serve subsequent pages. Bank bytes are decrypted in memory. Loose and bank snapshot checks fail changed/truncated sources instead of mixing revisions.

Current limits are explicit:

| Boundary | Limit |
| --- | --- |
| Authored targets | 65,536 |
| Arena slots | authored targets + 64 by normal lowering, checked maximum 65,600 |
| Incoming / reply transactions | caller-selected, maximum 64 each |
| Retired revisions per reply | 64, independent of total arena slots |
| Logical resident revision heads / PCM | 256 MiB with normal lowering |
| Physical dynamic packed heads | separate quota using that same configured limit; covers worker staging, current/old-voice revisions and returned retired payloads |
| Queued revision bytes | separately bounded by the same configured byte limit until replies return |
| Decode jobs in flight | one |
| Script-side request metadata | authored target inventory (at most 65,536) plus 129 maximum transport/worker credits; 8 MiB total paths, 4 KiB per path |
| File metadata | 1 MiB aggregate, 65,536 chunks, 4,096 markers/loops |

`SampleControl::head_budget()` supplies the worker's physical authority, and
`Runtime::sample_head_budget_status()` reports held/peak dynamic packed bytes.
The physical lease is reserved before each packed allocation and follows the
immutable head generation until its storage is actually freed on control.
Logical arena removal, task acknowledgement, cancellation, or dropping the last
audio reader cannot release it prematurely. Foreign streamed-budget identities
are rejected before submission. This quota is independent of the initial static
Streamer's head budget; 1,746 dynamic targets are not folded into a static
8 MiB cap. An active source and its replacement both count while they coexist.

One worker reuses a 4,096-frame decode workspace and packs bounded pages; it does
not build a complete decoded attack buffer. Metadata requests can queue during
`onInit`, before the production services bind. They are indexed by authored target
and FIFO request ID. Superseded pending metadata is removed before admission,
returns its path credits, and still delivers its original task's canceled callback.
The fixed payload transport and one-worker limits remain unchanged.

The physical quota above covers **streamed packed heads**, not full-resident
revision buffers, page pools, encoded-reader workspaces, or process RSS. Full-resident
revision decode/arena/queue limits remain separate logical bounds; they are not yet
one generation-owned physical quota across retained replies. Likewise, authored
`SamplePlayer.AllowStreaming=false` and Program/Part streaming policy still need
explicit translation and bounded full-resident admission. The native manual says
that a Sample oscillator can require memory playback despite Program Streaming;
current streaming success is not proof of that residency policy's parity.
[UVI oscillator residency policy](https://manual.uvi.net/falcon/en/oscillators/)

The worker currently reads an 8,192-frame onset and bounded loop endpoint legs, with guards for the engine's maximum supported interpolation reach. Packed storage reduces suitable mono/16-bit heads without changing decoded values. This dynamic head rule is separate from the existing static `HeadChoice::Auto` lazy policy; it is not evidence that the latter is free of cold-onset starvation. Dense-burst/backlog sizing and real policy deadlines require their own receipts.

A deterministic inline/offline player without a stream registry can request bounded resident preparation; it has no audio-thread file access. Production bank/loose loading supplies the stream registry and keeps large source bodies on disk.

## Metadata and supported scope

Current dynamic targets are `SamplePlayer` oscillators, stereo/mono WAV, AIFF, and FLAC. Other oscillator kinds, multi-channel routing beyond stereo, unknown fractional loops, and unverified vendor geometry fail explicitly rather than receiving a fabricated successful task. A full-range dormant region compiles its dimensionless pitch first; after binding, individual notes outside the engine's real rate/pitch range receive `SourcePitch`, while an unbound player receives `SourceUnavailable`.

File root/fine tune describe `sampleInfo`; they do not replace the compiled oscillator root/tune. Standard file start/end/loop geometry applies unless explicit authored `PlaybackOptions` wins. UVI uses the first WAV sampler loop, treats type 2 as forward, and ignores finite play count. WAV inclusive ends become exclusive core bounds; AIFF sustain markers are already exclusive. FLAC foreign `riff`/`aiff` metadata is read; non-one-shot foreign `acid` can loop the full file. Conflicting unverified loop precedence is an explicit failure. [UVI file metadata laws](https://lua.uvi.net/_sample_metadata.html)

Unknown vendor sampler data remains unresolved. Complete explicit playback can still determine the sound: the worker then reports the unresolved file geometry, preserves known file root/tune, and omits unknown geometry from readback. It never guesses private bytes. The metadata reader honors declared RIFF/FORM scope instead of inheriting the audio decoder's legacy RIFF extent repair; malformed source metadata is therefore an explicit diagnostic.

After a committed reply, `sampleInfo` reads current numeric file metadata with the documented field names and millisecond units. `Runtime::sample_target_asset` borrows the actual committed source, including initially dormant targets; `Prepared::region_asset` remains a static-plan query. Existing plugin waveform publication still needs an explicit current-source consumer; a static prepared asset must not be presented as evidence of the new revision. [Oscillator sampleInfo schema](https://lua.uvi.net/class_oscillator.html)

## Independent fixtures and evidence

- Core `sample_revision`: five fixtures cover old/new render equivalence against independent runtimes, rate/window/loop retention, cancellation, queue/reply/slot/byte pressure, stale plan rejection, control-only payload destruction, more than 64 distinct targets, and dormant/per-key pitch diagnostics. Commit/render/retirement paths run under an allocator/deallocator guard.
- UVI `sample_loading` and `resource_tasks`: four fixtures cover actual resident decode/commit, callback identity and one delivery, polling before/after publication, no-callback loading, missing sources, and cancellation.
- Core `dynamic_head_budget`: three fixtures independently cover simultaneous old/new sounding sources, exact sample bytes, canceled payload replies, foreign authority rejection, and quota retained until control actually drops returned payloads. Audio commit/render/release/collection runs under allocation/deallocation guards.
- UVI worker quota tests: zero/one-byte-short/exact caps exercise the actual source worker, and cancellation immediately after a source read prevents publication and releases staging.
- UVI `dynamic_initialization`: 1,746 distinct authored players queue real requests before binding, then every worker result reaches a real audio commit and the original success callback. A second fixture supersedes one target 1,024 times, receives all 1,023 canceled callbacks and the final successful commit, and checks physical held/peak bytes. Both use authored loose WAV sources; they do not prove an installed bank's metadata/codec coverage.
- UVI `dynamic_streamed`: a genuine attached production script thread loads an initially empty player from an authored loose WAV without any Lua wait coroutine. The owner wakes for decode and commit replies. All 40,000 frames match an independently loaded resident UVI plan bit for bit; source data remains streamed and the fixture records zero stream underruns after explicit offline page readiness.
- Kontakt `dynamic_registry_pages`: old/new AssetIds read their exact registered page bytes; duplicate identity is refused, original registry sharing is preserved, and registrations retire independently.
- Eight metadata fixtures cover WAV/AIFF/FLAC loop laws, declared scopes, malformed/bounded inputs, foreign acid, explicit-over-unknown geometry, and a streamed reader that skips a 4 MiB audio payload while reading less than 256 metadata/header bytes.

These are correctness and ownership receipts. The explicitly waited offline page test does not measure real-time disk deadline margin, cold OS cache, dense chords, or a DAW's static Auto-head policy. Native wavetable/granular replacement and private UVI sampler payloads remain unsupported.

## Installed-bank source-service witness

The opt-in `dynamic_source_witness` example uses ordinary promotion and script
attachment, a real source worker/registry, core render commits, and bounded owner
status requests. `SampleLoadStatus` separates requested, successful-after-ack,
failed, canceled and still-pending tasks; pending is further split into metadata,
worker, ready and submitted work. The synthetic 1,746-target fixtures assert this
conservation before service binding and after all commits. This control query
can wait for its owner and must never run on the audio callback.

A bounded fresh-load witness of the originally diagnosed Augmented Orchestra
Pizzicato 2 program completed with **zero dynamic requests**. All 6,984 authored
targets resolved to 1,238 existing source identities, no identity changed, and
the dynamic physical quota remained at zero. This is a negative initialization
witness, not evidence that installed-bank `loadSample` requests succeeded. The
numeric receipt is retained as `ao-production-dynamic-source-evidence.json` in
the private asset-audit artifacts. Compilation overlapped this run; its duration
is not a performance result. No script, image, sample or rendered audio was saved.

Read-only structural call-site inspection places the two `loadSample` calls in
`Layer:load`; the layer selection and saved-state loading paths reference layer
loading. The example can discover authored control metadata and apply one
explicit normal control edit. An unavailable or unidentified UI trigger must not
be replaced by invoking an internal vendor Lua function. The fresh-load zero
request receipt remains separate from any subsequent interaction witness.
