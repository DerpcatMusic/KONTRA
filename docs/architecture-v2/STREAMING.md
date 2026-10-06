# Native sample residency and streaming

V2-11 is in progress. Resident and paged assets now render through the same native
source/DSP path. The bounded cache/worker protocol, initial source readiness and
observable source failure, a seekable WAV decoder and explicit bounded demand
servicing are executable. Starvation fades/recovery and distinct offline preparation are
still open; this is not yet a production disk-streaming service.

## Asset identity

`Pcm::asset_id()` identifies one immutable decoded asset revision within the process.
Cloning a PCM handle across views or prepared plans preserves its optional sample
buffer and identity. `Pcm::streamed(rate, frames)` admits metadata without allocating
sample storage; `frame_count()` and `resident_frames()` distinguish the two cases. Constructing another revision assigns a new, non-reused identity,
even when its current samples happen to be equal. These are runtime identities, not
persistent filenames or content hashes. Content deduplication belongs on control;
rendering does not hash samples, compare paths or increment shared references.

The page cache must key by immutable asset identity plus decoded page position,
not a note, source cursor or replaceable plan-local sample index. Decode/analysis
changes require a new asset revision. Workers must retain or resolve admitted asset
metadata independently of a stale musical request; a demand record owns no asset.

## Current source demand service

`Runtime::visit_voice_demand` predicts the current voice's source ranges over an
explicit output-frame horizon. Each `SampleDemand` names the immutable asset,
half-open decoded-frame interval and first-use engine deadline. It includes future
admitted source starts within the exclusive horizon, source/envelope/choke completion,
current expression pitch, muted source continuity and interpolation support. Both
crossfade legs are reported as separate ranges, so a distant partner does not imply
loading everything between it and the primary range.

Rendering and prediction share source-address resolution. Forward, reverse,
wrap/reflected/finite-loop traversal and release exit rules retain their integer
positions and fractional recurrence. The sinc support grows with the current ratio;
out-of-view zero padding creates no storage request. The renderer's existing PCM
math and kernel coefficients are unchanged.

Queries copy cursor/envelope state; they do not consume playback, commands or time.
They do not execute pending callbacks or speculate about future input/automation.
Clients settle due events before prediction and refresh after pitch/release changes.
A stopped visitor returns `false` immediately to report incomplete demand; queue
capacity is not reported as successful readiness. Ranges can repeat across output
frames or voices; the resource service must deduplicate pages and retain their earliest
required deadline. Within one snapshot, overlapping interpolation windows resolve only newly entered
virtual source positions, retaining the first deadline of their shared guards.
Fractional phase and envelope advancement still follow the exact render recurrence.
The configured horizon bounds traversal work; this is not yet a measured production
cache scheduling policy.

The next implementation must provide bounded page/storage admission, owner-stamped
worker messages, eviction/retirement without audio-thread destruction, exact paged
reads, visible not-ready/underrun outcomes and distinct live/offline contracts.
It must integrate these services into actual rendering before V2-11 can close.

## Evidence and references

`tests/demand.rs` checks exact forward/reverse deadlines, stopped visitors without
state mutation, future starts, muted one-shot bounds, half/double/16× source ratios,
zero guards and two-sided crossfade/release topology under the heap guard.
Existing source/resampling fixtures continue to verify independently expected PCM.
The shared-PCM plan test also checks shared versus freshly constructed asset IDs.
Logs use `artifacts/source-demand-*`; none establish disk-streamed playback yet.

Reviewed pinned sfizz
[`FilePool.cpp:92–129,286–321`](https://github.com/sfztools/sfizz/blob/f5c6e29f23b8057867c08e88f5f6ac6738baa30b/src/sfizz/FilePool.cpp#L92)
and its BSD-2-Clause header. It separates preload offsets from asynchronous filling
and publishes filled-frame progress. Native streaming must additionally account for
arbitrary traversal windows and explicit immutable page ownership. No source was
copied or SFZ frontend work resumed.


## Bounded decoded-page exchange

`StreamCache::new(pages)` prepares exactly that many 4,096-frame stereo buffers and
three bounded SPSC channels. The audio cache owns page slots; one worker coordinator
serializes requests/results and can distribute owned `DecodeJob`s to executors.
No file API or decoder runs in this kernel. Job ranges include the short final page.
Workers fill/validate pages and return an explicit success or decode failure.
Rejected completions retain the job and its result for retry or off-audio disposal.

Cache keys use immutable asset identity and page index. Page readiness is explicit:
missing, pending, ready or failed. A new request reserves its slot and queue capacity
before replacing an old entry. Protect all current demand in an epoch before admitting
replacement requests, so voice visitation order cannot evict another needed page.
Unprotected entries are eligible in last-use order. A full cache/queue reports capacity;
it never steals a protected page, waits or silently claims that data is ready.

The worker coalesces superseded slot requests and selects the earliest deadline.
A priority update does not launch an already-dispatched job again. Each admission
has a non-reused request sequence; a late completion for a reused or invalidated slot
returns its buffer instead of publishing stale data. `invalidate` permits explicit
failed-page retry. A failed decode never publishes partial/nonfinite sample data.

Audio-side eviction, invalidation and discarded/failed completions return buffers
through the recycle queue. Worker reuse preserves the allocation; cache operations
never allocate, free or perform an Arc update. Endpoint/cache/job destruction belongs
off audio. A job deliberately dropped by its worker removes that buffer from the
pool; production coordinators must complete failed/cancelled jobs rather than drop
them. Resident pages remain readable after worker disconnect; misses/pending requests
report the disconnection. Disconnection is not permission for audio-side destruction.

`tests/stream_cache.rs` checks exact buffer-pointer reuse, shared assets, protected
pages, short pages, deadline updates, queue/cache pressure, stale completion after
slot reuse, failed-page retry, nonfinite samples, foreign worker rejection and
endpoint shutdown. An actual worker thread publishes a page that remains owned by
the cache after that worker exits. Cache transfer paths run under heap guards.
Native/MSRV, strict Clippy and root boundary checks pass; targeted release checks
also cover the cache protocol (`artifacts/stream-cache-*`). This is not yet evidence
of streamed source playback or storage-latency tolerance.

## Paged native rendering and initial admission

`Runtime::with_stream_cache` transfers the cache during control-side setup. The
current caller services requests/completions through `stream_cache_mut`; rendering
only borrows it. It performs no decoding, waiting, page destruction or shared-handle
cloning. Paged and resident reads use one generic source traversal/resampler and
processor path, with contiguous-span acceleration for either storage kind.
Out-of-view padding is zero; an unavailable physical frame is a distinct failure.
A filtered output frame commits neither cursor, envelope nor output until every
required guard and crossfade leg is available.

Attack/release selection preflights the complete selected layer set before publishing
sources or sequence decisions. The first frame's actual pitch, source offset,
interpolation guards and crossfade legs must be ready, even for muted sources.
`Error::NotReady` rejects that admission without partial layers. This is an initial
frame guarantee only: worker horizons must still cover subsequent reads and later
pitch/release changes. Scheduled manual sources can lose readiness before start;
rendering checks actual reads again.

The current live failure contract stops the unavailable source and increments
`Runtime::stream_underruns()` once. It never replays delayed samples or pauses musical
time. Existing voice DSP drains its declared zero-input tail across callback
boundaries; host key pairing remains owned until the real note-off. Bus tails retain
their existing separate ownership. **A dry source currently cuts at the miss**;
preventive fades and recovery policy remain required before production readiness.
Muted sources retain virtual advancement; their demand still includes needed data
for a later unmute.

`tests/paged_render.rs` compares paged/resident PCM exactly across five source rates,
both directions, wrap/reflected/crossfade loops, release and differing block
partitions. It also checks atomic missing-guard rejection, host pairing after a live
miss, and one-time DSP-tail drainage, with allocation/deallocation guards. Logs:
`artifacts/paged-render-*`. These checks do not establish storage-latency tolerance
or Kontakt/Falcon performance parity.

## Seekable file decode boundary

The native WAV reader now shares one seekable RIFF parser/range decoder between
resident loading and worker page filling. Opening reads bounded chunk metadata and
skips audio bodies; `read_frames` seeks directly to a validated half-open range and
uses fixed page-sized scratch storage. Existing PCM16/float32 mono/stereo support
is unchanged. Unknown padded chunks are skipped; malformed sizes, duplicate format
or data chunks, partial frames, nonfinite samples and truncated reads fail explicitly.
The resident loader retains its 256 MiB input limit; page decoding does not allocate
a full decoded asset. A retained source owns one streamed asset revision; source
bytes must remain immutable while that revision is used.

The native file-worker fixture opens a real temporary WAV, fills owned cache jobs
on another thread, then compares native paged rendering against authored samples,
including the short last page and EOF. A counted seekable reader verifies opening
reads no audio and range reads consume only their requested bytes. Logs:
`artifacts/stream-wave-*`. This is the first concrete worker decoder, not yet a
multicodec registry or disk-latency/overload acceptance test.

## Live demand servicing

`Runtime::service_streaming(horizon_frames)` polls at most cache-capacity completions,
starts a residency epoch, protects demand for **all** live sources, then requests pages
with their first-use deadlines. It keeps the original plan's asset identity, handles
future admitted starts inside the horizon, and skips resident assets and already
failed sources. It does not advance musical time, execute callbacks, decode or wait.
Protecting cached ranges uses the sorted asset/page index rather than scanning every
cache slot for each range.

`Ok(true)` means all snapshot demand is resident; `Ok(false)` includes pending/failed
pages, whose status remains inspectable. Explicit queue/cache errors retain already
accepted requests and report incomplete service. Failed decodes require explicit
invalidation before retry. The cache remains attached after errors. Missing cold
onsets still require caller preparation; this service predicts admitted voices only.
Callers requery after events, pitch/release changes and before a chosen render horizon;
it does not guess callbacks that may create new sources inside a future block.

Tests refill an eleven-page source through three cache slots, compare exact resident
output at fractional, unity and 16× rates in both directions, and verify each further
page is decoded only once. A capacity fixture protects two instruments before any
replacement and checks both remain playable after rejection. The overlap planner's
first-use deadlines are compared with exhaustive per-frame/per-tap traversal across
loop shapes, fractional phases, direction and release. Logs: `artifacts/stream-service-*`.
These establish bounded ownership and rendering correctness, not a disk deadline SLA
or competitor performance result.
