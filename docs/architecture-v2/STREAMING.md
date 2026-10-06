# Native sample residency and streaming

V2-11 is in progress. Playback still uses resident PCM. The first executable pieces
are immutable asset identity, demand prediction and bounded decoded-page ownership.
The cache/worker protocol is executable; paged rendering, actual decoder integration,
onset readiness and starvation policies are not complete.

## Asset identity

`Pcm::asset_id()` identifies one immutable decoded asset revision within the process.
Cloning a PCM handle across views or prepared plans preserves both the sample buffer
and this identity. Constructing another revision assigns a new, non-reused identity,
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
required deadline. The configured horizon bounds traversal work; this is not yet a
measured cache scheduling policy.

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
