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
| Physical dynamic PCM | separate quota using that same configured limit; packed heads and full buffers share admission through staging, current/old-voice revisions and returned retired payloads |
| Queued revision bytes | separately bounded by the same configured byte limit until replies return |
| Decode jobs in flight | one |
| Script-side request metadata | authored target inventory (at most 65,536) plus 129 maximum transport/worker credits; 8 MiB total paths, 4 KiB per path |
| File metadata | 1 MiB aggregate, 65,536 chunks, 4,096 markers/loops |

`SampleControl::physical_budget()` supplies the worker's physical authority, and
`Runtime::sample_physical_budget_status()` reports held/peak dynamic payload bytes.
The original head-budget method names remain aliases. A physical lease is reserved
before each packed or full-buffer allocation and follows the immutable storage
until it is actually freed on control.
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

`ResidentBuffer` reserves checked full PCM bytes before allocation and decoding;
its immutable `Pcm` generation retains the same lease through clones, sounding
voices, purge guards and returned replies. Non-finite decode, source failure,
cancellation and failed publication release staging on control. Existing caller-owned
full buffers can be admitted without copying at submission; their earlier caller
allocation is outside this contract until admission succeeds. Foreign authorities
are rejected. Budget-bound full assets use `set_budgeted_resident_loader`; raw
whole-buffer callbacks are explicitly incompatible because they allocate before
admission. Legacy unbudgeted static reload behavior is unchanged.

This physical quota covers dynamic packed heads and full PCM, excluding octave
levels, page pools, encoded-reader workspaces and process RSS.

Initial authored `Program.Streaming=0` or `SamplePlayer.AllowStreaming=0` now
requires full resident PCM. Typed `Asset.residency` retains the strongest
requirement of every shared source user, including a bypassed player sharing an
audible asset. Missing flags retain the catalog's streaming-enabled default.
Static metadata descriptors remain unready until one-shot full-buffer publication;
full buffers and ordinary packed attacks share the configured static physical
quota. Full buffers reserve before allocation and decode, all preparation finishes
before part publication, and a cold fallback retains mandatory RAM sources. An
insufficient mandatory quota fails explicitly. `report.head_bytes` remains packed
storage; `required_resident_assets/bytes` reports full bodies separately.
`resident_non_head_bytes_outside(&authority)` permits exact single-charge accounting.

The same policy parser controls dynamic requests even when a stream service is
bound. Required full bodies remain resident; ordinary requests keep streamed tails.
Accepted initialization writes to Program Streaming or SamplePlayer AllowStreaming
refresh the worker recipes from the final bounded initialization XML before any
source/control service binds. Existing queued tasks retain their identity, order
and cancellation state. Attribute consumers participate in the initialization
read ledger, so an actually consumed residency write is not mislabeled unbound.
Five real dynamic-load cases cover both directions of each flag and a remaining
oscillator veto, including an earlier canceled request and independent PCM values.

The host's synthetic Part and its placeholder connections carry no admitted XML
identity. They cannot fold edits into a same-numbered node of a wrapped or
unwrapped program, nor supply impulse/sample/meter ownership. Part Streaming
writes are explicitly reported unavailable; unsupported changed live Program and
oscillator residency flags retain committed readback and emit diagnostics.
Program/oscillator **live parameter writes** and Part inheritance still need their
own verified transactions and remain reported unsupported. The native manual says
that a Sample oscillator can require memory playback despite Program Streaming;
initial XML policy support does not establish live-write or Part-policy parity.
[UVI oscillator residency policy](https://manual.uvi.net/falcon/en/oscillators/)

The worker currently reads an 8,192-frame onset and bounded loop endpoint legs, with guards for the engine's maximum supported interpolation reach. Packed storage reduces suitable mono/16-bit heads without changing decoded values. This dynamic head rule is separate from the separate initial static stream policy; it is not evidence that the latter is free of cold-onset starvation. Dense-burst/backlog sizing and real policy deadlines require their own receipts.

A deterministic inline/offline player without a stream registry can request bounded resident preparation; it has no audio-thread file access. Production bank/loose loading supplies the stream registry and keeps large source bodies on disk.

## Metadata and supported scope

Current dynamic targets are `SamplePlayer` oscillators, stereo/mono WAV, AIFF, and FLAC. Other oscillator kinds, multi-channel routing beyond stereo, unknown fractional loops, and unverified vendor geometry fail explicitly rather than receiving a fabricated successful task. A full-range dormant region compiles its dimensionless pitch first; after binding, individual notes outside the engine's real rate/pitch range receive `SourcePitch`, while an unbound player receives `SourceUnavailable`.

File root/fine tune describe `sampleInfo`; they do not replace the compiled oscillator root/tune. Standard file start/end/loop geometry applies unless explicit authored `PlaybackOptions` wins. UVI uses the first WAV sampler loop, treats type 2 as forward, and ignores finite play count. WAV inclusive ends become exclusive core bounds; AIFF sustain markers are already exclusive. FLAC foreign `riff`/`aiff` metadata is read; non-one-shot foreign `acid` can loop the full file. Conflicting unverified loop precedence is an explicit failure. [UVI file metadata laws](https://lua.uvi.net/_sample_metadata.html)

Unknown vendor sampler data remains unresolved. Complete explicit playback can still determine the sound: the worker then reports the unresolved file geometry, preserves known file root/tune, and omits unknown geometry from readback. It never guesses private bytes. The metadata reader honors declared RIFF/FORM scope instead of inheriting the audio decoder's legacy RIFF extent repair; malformed source metadata is therefore an explicit diagnostic.

After a committed reply, `sampleInfo` reads current numeric file metadata with the documented field names and millisecond units. `Runtime::sample_target_asset` borrows the actual committed source, including initially dormant targets; `Prepared::region_asset` remains a static-plan query. Existing plugin waveform publication still needs an explicit current-source consumer; a static prepared asset must not be presented as evidence of the new revision. [Oscillator sampleInfo schema](https://lua.uvi.net/class_oscillator.html)

## Independent fixtures and evidence

- Core `sample_revision`: five fixtures cover old/new render equivalence against independent runtimes, rate/window/loop retention, cancellation, queue/reply/slot/byte pressure, stale plan rejection, control-only payload destruction, more than 64 distinct targets, and dormant/per-key pitch diagnostics. Commit/render/retirement paths run under an allocator/deallocator guard.
- UVI `sample_loading` and `resource_tasks`: four fixtures cover actual resident decode/commit, callback identity and one delivery, polling before/after publication, no-callback loading, missing sources, and cancellation.
- Core `resident_budget`: four fixtures cover pre-allocation rejection, exact limits/overflow/non-finite data, full/head composition, full old/new render bytes, retained Pcm clones and replies, cancellation, foreign authority and caller-buffer adoption. Three private resident generation fixtures and one guarded async purge/reload fixture cover retained readers, reload pressure, incompatible callbacks, changed authority, and purge during staging.
- Core `dynamic_head_budget`: three fixtures independently cover simultaneous old/new sounding sources, exact sample bytes, canceled payload replies, foreign authority rejection, and quota retained until control actually drops returned payloads. Audio commit/render/release/collection runs under allocation/deallocation guards.
- UVI worker quota tests: zero/one-byte-short/exact caps exercise actual streamed and full-resident preparation. Invalid geometry is rejected before allocation; cancellation immediately after a read and decoder/source failure prevent publication and release staging.
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

The normal, visible first-layer `LayerNext` button reproduces **1,746 requests**.
The first complete interaction receipt (`ao-layer-next-dynamic-source-evidence.json`)
records 1,746 failures, zero successful commits, zero changed source identities
and zero dynamic payload bytes. A bounded first-error probe identified a path
resolution failure before allocation: the host had replaced the raw relative
sample path with a single module-relative path, discarding the other read contexts.
The failed receipt is retained, rather than overwritten by a successful rerun.

`SamplePath` now carries the raw path and the calling module directory to the
same admitted source authority. Bank lookup tries the available native contexts
in order: bank-relative as given, script folder, preset folder, then that bank's
`Samples` folder. Explicit rooted paths and `$Volume` references retain one exact
authority; different banks, invalid volumes and escaping roots fail. Unknown
multi/original-load folders and host CWD are not fabricated. Loose loading uses
the known script/program contexts and preserves explicitly authored absolute
paths. Only typed absence advances to another candidate: ambiguity, corruption,
permission errors and changed-source failures remain errors. Channel bundles
resolve wholly within one context. Exact map lookups and same-directory lossless
extension alternatives avoid per-request scans of the bank member inventory;
there is no dynamic basename search. This follows the documented read-context
law, with unavailable contexts kept explicit. [UVI file paths](https://lua.uvi.net/print.html)

The first real after-fix gesture completed **1,746 successful owner acknowledgments,
zero failures/cancellations/pending tasks**, and exactly 1,746 changed target source
identities. All 6,984 authored targets remained covered. New sources retained
streamed bodies; their physical heads held/peaked at 28,785,138 bytes against the
separate 256 MiB dynamic quota. Eight typed meter requests resolved to eight exact
owners, without establishing native display laws. This receipt is a source-service
correctness result: it played no MIDI notes, and compiler/external CPU contention
prevents a speed/deadline claim. The first-success numeric summary remains
separate from the initial zero-request and complete failed receipts.

Task timing uses fixed histograms and saturating count/total/max counters for
all worker jobs: metadata queue wait, resolution including headers, exact path
lookup, reader open, file metadata, preparation/decode, ready-to-submit delay,
commit acknowledgment and total task lifetime. Substages overlap their parent
resolution span, and queue/ack spans overlap across tasks; summing them is not
instrument wall time or CPU time. Fixed histograms retain later jobs after the
example's first 64 detailed observations fill. The example can compare bounded
installed resident-head windows against independently reopened registered readers,
including header geometry, without saving sample payloads or rendered audio.

The final map-based receipt (`ao-layer-next-resolved-source-evidence.json`) again
completed all 1,746 tasks successfully and replaced exactly 1,746 source identities.
It independently reopened every changed source: **4,656 resident windows / 1,191,936
frames matched bit for bit**, with zero header mismatches, missing registrations,
read failures or truncated verification. Both numeric FNV64 receipts were
`2777464607603900197`. The comparisons cover bounded resident windows and header
identity; they do not compare every source frame or native rendering.

| All 1,746 jobs, measured wall spans | Total | Maximum per job |
| --- | ---: | ---: |
| Exact path lookup (5,238 candidate roots) | 12.30 ms | 68.79 µs |
| Reader admission/open | 478.86 ms | 29.04 ms |
| File metadata | 14.84 ms | 55.27 µs |
| Resolution including open/metadata | 506.44 ms | 29.09 ms |
| Preparation/decode/packing/registration | 293.46 ms | 449.43 µs |
| Metadata queue wait (overlapping task lifetimes) | 19,884.06 s | 21.45 s |
| Commit acknowledgment wait (overlapping tasks) | 516.01 s | 714.49 ms |

The complete process took 32.09 seconds, including initialization and a 603.09 ms
independent readback pass. Every process observation saw compiler activity; OS
cache state was uncontrolled. Neither this duration nor the earlier 75.89-second
scan-based successful run is an isolated speed comparison. The stage clocks
instead expose that source preparation spans account for much less wall time
than the serialized task queue/acknowledgment path under these conditions. Owner
callback/UI/command scheduling attribution was not enabled in that receipt. No notes were
played, so zero reported stream underruns is not a disk-deadline receipt.

A separate final owner-clock receipt (`ao-layer-next-owner-phase-evidence.json`)
uses the same normal visible gesture, after the initialization-recipe and synthetic
Part-identity fixes. All **1,746 tasks again succeeded after owner acknowledgment**,
with zero failed, canceled or pending work; 1,746 source identities changed and all
6,984 targets remained covered. The independent check again matched 4,656 windows /
1,191,936 frames and both hashes above, without truncation or missing registrations.
Dynamic physical held/peak bytes remained 28,785,138. No init/runtime script faults
were recorded. These are source-service and bounded PCM receipts, not native audio
or sounding-voice parity.

The owner-clock snapshots bracket the gesture and task settlement, excluding startup
and subsequent independent PCM verification. Their 25.307-second window contains
2,026 owner turns, 1,746 deferred task callbacks and 104 UI publications. Those
publications exported 1,055,080 widgets in total. The largest measured owner cost
is the repeated full interface export:

| Owner snapshot delta | Calls | Wall time | Owner thread CPU |
| --- | ---: | ---: | ---: |
| UI publication | 104 | 24,019.44 ms | 23,733.58 ms |
| Interface export, nested in publication | 104 | 23,851.48 ms | 23,567.82 ms |
| Face equality, nested in publication | 104 | 1.93 ms | 1.83 ms |
| All advance work | 2,027 | 41.06 ms | 39.37 ms |
| Sample poll, nested in advance | 2,027 | 16.24 ms | 14.76 ms |

The export and equality rows are nested substages and must not be added to UI
publication. There were no command-queue-full retries. The normal owner ran without
a seeded audit barrier; diagnostic clocks are opt-in and absent from ordinary plugin
builds. A source manifest records the frozen helper's Rust/Cargo inputs and binary
SHA256 `ec30d253dfdc4e2cbc00b417455abdbca78d7ed1b27a9120e6b492bbb9e45d83`.

Across the 1,746 worker jobs, resolution including headers totaled 593.67 ms and
preparation/decode totaled 299.58 ms. The newly measured completed-worker-to-owner
delivery span totaled **22.926 seconds** (mean 13.13 ms, maximum 287.03 ms).
Commit-submit-to-owner-ack spans totaled 523.88 seconds across overlapping tasks
(maximum 793.03 ms); they include transport, core commit and owner scheduling,
not just acknowledgment computation. Metadata queue spans likewise overlap:
22,607.87 seconds summed, maximum 23.90 seconds. Repeated full UI export now has
direct thread-CPU attribution as the dominant measured owner bottleneck; it is
not inferred from aggregate task wait alone.

The complete process took 33.145 seconds. Compiler activity appeared in all 65
process observations, so this is not an isolated speed comparison or a deadline
margin result. The original failed, zero-request, successful pre-clock and final
owner-clock receipts remain separate; no vendor script, artwork or PCM was saved.

Independent path fixtures cover nested-module raw context, exact ordered roots,
explicit volumes, ASCII case folding with unchanged Unicode names, extension
ambiguity, escape rejection, whole-bundle roots, and absence-only fallback.
Readback fixtures cover mono/stereo 16/24/float packed storage, holes and invalid
bounds without partial output mutation, immutable generation pinning, retained
leases and allocation/deallocation guards. These do not establish whole-file or
native audio equivalence for an installed instrument.

Static cancelable wrappers preserve `LoadError::Canceled` through UVI staging;
the noncancelable APIs remain compatible wrappers. Cancellation is checked during
source resolution, around impulse preparation and between bounded 4,096-frame
full/head reads. Source failures and canceled staging cannot become successful
cold fallbacks. A filesystem operation itself may still block until it returns.
