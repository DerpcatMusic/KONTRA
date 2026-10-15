# DFD attack readiness and immutable disk sources

The current V2 `Auto` policy prepares resident attacks before part publication,
with an explicit capacity fallback and a 256 MiB per-stream quota. The chronological
receipts below explain the change from lazy heads and the remaining dense-tail
underruns. Complete attack coverage does not establish tail-stream deadline safety.
The intended initial-portion-in-RAM behavior is described in
[Native Instruments' DFD documentation](https://support.native-instruments.com/support/solutions/articles/69000879768-what-is-dfd-).

## What is measured

`examples/dfd_readiness.rs` calls the real `V2Loader` with its default `Auto`
request, then installs its real part in `V2Core`. Eight physical note IDs at
keys 60–67, velocity 0.8 and CC1/11 values 110/127 exercise the installed Afflatus
4 Horns KS program. Each burst renders 120 paced host blocks. The second burst
follows exact chokes and a short service interval; round-robin alternatives and
release/tail state make it **a different burst, not a fully warm repeat**.

Audio remains in RAM. Receipts contain only numeric counters, frame counts,
durations and process-local asset IDs. Fresh processes reset decoded source
heads/readers, but OS/storage caches remain uncontrolled. No cache eviction or
`drop_caches` command was used. No rustc/cargo process was visible immediately
before or after the paired window. This is a diagnostic observation, not a
pinned, repeated latency benchmark. The optional audit and off-callback snapshot
scans also add work, so callback maxima are not a baseline DSP speed claim.

The optional `KONTRA_STREAM_AUDIT` clocks each page request and records at most
512 detailed jobs **per decoder**. All-job counts, totals, maxima and fixed log2
histograms continue after detailed records fill. Queue wait, opening, combined
seek/read/decode, and publication are distinct stages. Combined decoder reads
are not presented as separate physical disk and codec timings. The core's
ordinary streaming underrun counter does not include deferred cold starts.

## Actual Auto observations

| Observation | 64-frame host | 256-frame host |
|---|---:|---:|
| Initial sample assets with any head | 0 / 15,008 | 0 / 15,008 |
| Initial packed heads | 0 B | 0 B |
| Configured head cap | 8 MiB | 8 MiB |
| Page pool | 96 MiB | 96 MiB |
| First nonzero block offset | 64 output frames | 256 output frames |
| First-burst cold starts | 126 | 119 |
| First-burst ordinary underruns | 0 | 0 |
| First-burst callback deadline misses | 1 / 120 | 5 / 120 |
| Second-burst callback deadline misses | 5 / 120 | 13 / 120 |
| Guarded audio alloc/dealloc calls | 0 | 0 |
| Completed decoder jobs, both bursts | 480 | 1,052 |
| Queue wait p95 / maximum | 2,551 / 6,043 us | 3,224 / 6,174 us |
| Seek/read/decode p95 / maximum | 254 / 984 us | 132 / 23,223 us |
| Open maximum | 109 us | 1,168 us |
| Failed jobs / reply backpressure | 0 / 0 | 0 / 0 |

The first host block is silent while onset sources arrive. The reported first
nonzero offset is block-granular, not the exact first audible sample. These
results separate attack delay from an ordinary underrun; they do not attribute
every callback deadline miss to storage. Dense queue wait is substantially
larger than a typical isolated decode. The existing single-read p95 head sizing
therefore does not by itself establish dense burst readiness.

## Exact coverage and memory bounds

`Pcm::resident_covers(range)` checks an exact immutable resident generation.
`Streamer::head_readiness` checks every planned authored start range, including
offset and reverse starts. A nonzero head or a temporary cache page is not
treated as complete permanent attack coverage. The snapshot is control-side;
a later explicit purge may change it. Dynamic revisions have separately
admitted heads and are outside this initial-plan coverage report.

The additional receipt measured 17,059 merged start ranges across 15,008
mapped sample assets: 14,390,363 source frames. Their decoded stereo-f32 size is
115,122,904 B. Even the narrowest supported packing, mono 16-bit, needs at least
28,780,726 B, so the fixed 8 MiB cap cannot cover this complete mapping. Packing
is lossless; no channels or authored mappings were removed to meet a budget.
After the first burst, only 64 assets and 139 ranges were covered (~706 kB).

Saved group mutes exclude none of these assets. A static key/velocity filter
for this chord leaves 532 asset alternatives, 767 ranges and 5,169,440 decoded
bytes. This is an upper bound over those assets' authored ranges, including
round-robin, articulation, trigger and controller alternatives. It does not
infer on-note Lua mutations or identify the exact native first-burst selection.
Using only the observed 64 assets as a startup preload recipe would be a
preset/chord-specific omission and is not implemented.

The required Auto policy decision remains explicit: budget-checked eager coverage
of the prepared playable mapping, or truthful partial/cold readiness with a
defined insufficient-budget response. Blindly turning off `lazy` is unsafe:
the eager loader at the time of the first receipt did not enforce the general
head cap, only mandatory wavetable residency. Increasing a global cache alone
does not resolve that law.

The eager loader now validates checked frame/byte bounds, rejects an impossible
mono-16 minimum before reading, then prepares and packs each source's complete
ranges off audio. It checks the exact packed total against the cap before any
initial head publication. An upper decoded-size bound never rejects a losslessly
packed head that fits. Over-cap, short/changed metadata and cancellation discard
the staged batch; a typed `HeadBudgetExceeded` identifies capacity separately
from source errors. Reads check cancellation between source-page chunks.
`PreparedRanges` retains validated packed data, avoiding a second packing pass.
An in-flight cold reload superseded by KSP purge discards its prepared head and
preserves the original no-publication contract.

The explicit 128 MiB eager experiment subsequently prepared every one of those
15,008 assets and 17,059 ranges, with **86,342,178 B** of packed attack payloads
(82.34 MiB). Its 14,499,834,880 B full sample set remained streamed, with a
separate 96 MiB page pool. This was an explicit production Kontakt-loader policy
override in `examples/dfd_eager.rs`; default V2 Auto was unchanged.
The initial sequential preparation took 16.817 s (17.375 s total load), read
432,115,712 physical bytes, and reached 201,440 KiB process high-water RSS.
The verified source volume is NVMe/ntfs3; these timings describe the complete
filesystem/codec/packing path, not an assumed mechanical disk. OS caches were
uncontrolled, so subsequent warm-process timings cannot be compared as a clean
cold-storage speedup.

That receipt's zero read-latency estimate came from a separate bug: persistent
numeric header hits skipped the bounded physical latency probes. Cached headers
now still probe up to the configured number of sources, validate their numeric
metadata, and measure open plus first-page reads. `latency_samples == 0` explicitly
means unmeasured. A header cache hit is no longer presented as a known zero-latency
disk.

Startup head preparation now uses a worker count clamped to 1–8 (shared with
the tail-decoder policy), a bounded result channel, and exact atomic packed-byte
admission. Source reads/packing finish before the complete initial batch is
published. Any source error, budget failure or cancellation discards that batch.
Reader initialization, combined seek/read/decode, validation/packing, startup
batch wait and publication have separate optional probes. All-job histograms
continue beyond the first 64 detailed jobs per worker. Reader initialization
includes its encoded header/buffer reads; it is not presented as pure OS-open
latency. The then-current prepare-before-admit path can transiently hold one
unadmitted prepared buffer per worker; it is not a total-RSS cap. The subsequent
generation-budget/preallocation work must close that ownership gap.

The first same-binary one/four-worker stage pair is **contended and unsuitable
for a speedup claim**. An external DSP test was observed at roughly six CPU
cores, the first process incurred about 444 MB physical reads and the next only
40 kB, and measured latency changed the head geometry. Both runs covered all
15,008 assets. The first measured 912 output head frames, 123,336,036 packed
bytes and 22.603 s preparation; the second measured 618 frames, 87,473,304 packed
bytes and 212 ms. The first worker's summed initialization/read-decode/pack
times were 16.627 s / 5.587 s / 261 ms; the four workers' sums were 312 / 177 /
146 ms. These sums identify where work occurred and preserve cache/adaptation
differences; dividing the wall times would misattribute those differences to
parallelism. A clean repeated comparison with equal geometry remains needed.

## Generation-owned packed-head quota

The later `HeadBudget` closes the prepare-before-admit gap for packed payloads.
Every page first analyzes the immutable decoded frames for exact lossless width,
reserves those bytes atomically, then allocates/encodes the packed payload. The
same lease follows the prepared, published, and retired immutable generation.
Dropping the last audio read guard does not destroy storage or release quota;
control-side snapshot collection does both. Current, staged, and retired packed
head payloads therefore share one cap across startup workers, cold reloads, and
explicit KSP residency reloads. `Streamer::head_budget_status()` reports held and
peak bytes, including retired storage, separately from visible current coverage.

Decoder staging is separately bounded to one 4,096-frame stereo workspace per
head worker (32 KiB; 1–8 workers), plus the codec/source reader workspace and
range metadata. Workers reuse that page between source jobs. Explicit-residency
recipes share one further control-side page; cold reloads use one page per batch. There is no complete decoded attack-range staging buffer.
Packing no longer creates a temporary f32 sample copy. The quota covers requested
packed PCM payload bytes; it is not a cap on allocator overhead, process RSS,
the independent page pool, octave levels, encoded readers, or full-resident PCM.
Dynamic sample revisions retain their separate revision admission budget.

Initial budget binding serializes with publication. It can adopt an existing
current immutable head without copying it, after reserving its exact bytes.
Unaccounted retired generations must first be reclaimed. A recipe prepared
before binding cannot publish uncharged payloads afterwards; wrong-budget and
wrong-asset publications fail without changing the current source.

`StreamPolicy::allow_cold_fallback` defaults to false. When explicitly enabled,
only a typed `HeadBudgetExceeded` from eager preparation permits retrying cold
starts over the same admitted sources and worker endpoint. No failed staged
batch is published. Mandatory wavetable ranges are prepared again and must fit;
source/metadata failures, cancellation, and mandatory-residency failures remain
errors. `StreamReport::head_fallback` retains the typed reason, and exact coverage
continues to distinguish cold/partial heads from complete readiness. The default
production V2 Auto policy has not been changed by this hook. Faster GUI publication
and a complete-head policy still require a separately measured product decision.

A canceled or KSP-purged in-flight reload now contributes zero committed reloads
rather than the former attempted-job count. Source bytes stay purged. Exact
packed admission also allows lawful mono heads that the earlier conservative
8-bytes-per-frame estimate skipped, while retaining the same configured cap.

## Worker ownership and source changes

Completion queue backpressure now retries only publication. The worker retains
the already decoded job and its exact success/failure result. It does not reopen
or decode that page again. A fixture injects three publication rejections for
both valid PCM and invalid-source failure, then verifies one open, one decode,
four publication attempts, and the exact final result/bytes.

Kontakt loose/monolith sources and UVI loose sources now share a frozen
`FileIdentity` admission stamp. On Unix the stamp includes
device, inode, high-resolution change/modify times and length. Every reopened
reader checks the **opened handle**, and physical refill checks it again before
decrypting/using those bytes. Replacing a path after reader eviction cannot
silently bind new bytes to an old sample revision. Same-size rewrites with
restored mtime, inode replacement, removal, truncation and change after open are
covered. A still-open old handle may supply its unchanged original bytes or
fail explicitly; it cannot publish replacement bytes as the old revision.
Kontakt shares one stamp per physical archive, validates before and after
directory parsing, and checks retained archive handles as well as reopened loose
files. No additional file descriptor is retained per sample. Header-cache hits
still bind the current admitted file identity. Out-of-file/overflowing member
ranges fail before decoding. The non-Unix creation/modify/length fallback has
weaker rewrite detection; equivalent Windows by-handle identity/change-time
validation is not verified. Strong mtime-restored rewrite fixtures are explicitly
Unix fixtures and are not presented as Windows verification.

UVI streaming also rejects an undeclared total frame count instead of silently
scanning the entire encoded sample to construct a supposedly bounded header.

## Verification and receipt location

Focused verification passed: exact offset/reverse readiness 1/1, injected
completion pressure 1/1 (success and failure subcases), and UVI streamed identity
and decoder parity 4/4. The whole Kontakt stream suite passed 20 tests (one
pre-existing ignored stress test), including zero/exact/over-cap, multiple ranges,
short source, cancellation before/during reads and the unchanged in-flight KSP
purge fixture. After sharing source identity, Kontakt stream tests passed 21
(one ignored), file-identity tests 2/2, and UVI stream tests 4/4. Core packed-publication tests passed 2/2. The optimized production
readiness example compiled before the subsequent budget change.
Parallel preparation then passed the complete stream suite (23 tests, one
ignored), including independently generated distinct-asset PCM in both playback
directions and 1/4/32 requested-worker limits. Zero/exact/over-cap admission was
also rerun with both one and four requested workers; failure publishes no asset
from the staged batch. The actual 40,000-frame dynamic streamed-source fixture
passed after shared admission authority was finalized.
The generation lease then passed five integration tests (including heap-guarded
retained-reader destruction, zero/exact/over-cap admission, concurrent staging,
wrong-budget/stale builders, exact allocation sizing, and independently generated
PCM across 4,096-frame boundaries with forward/reverse wrap/ping-pong loops).
Four core publication tests passed, including explicit-residency budget-pressure
retry and cancellation before publication. Kontakt streaming passed 25 active
tests (one pre-existing ignored), including typed opt-in fallback and mandatory
residency. The earlier 16-asset parallel preparation oracle was strengthened to
use immediate triggers, a stream cache, live-voice and nonzero-PCM assertions;
all requested 1/4/32-worker cases passed exact resident comparisons. UVI's actual
40,000-frame streamed dynamic-source fixture and all four source/identity tests
passed again. These are correctness receipts, not deadline-margin claims.

Numeric receipts are retained privately under
`/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-asset-audit-20261014/dfd-auto`.
They include paired full process receipts, bounded decoder probes, aggregate
summary, exact coverage bounds and a binary fingerprint. No instrument source,
sample data or bank payload is included.

## Default policy revision after the above measurements

The product Auto policy now requests eager resident attack preparation with a
256 MiB **per initial stream** physical-payload cap, replacing lazy 8 MiB admission.
Packed heads and mandatory full-resident base PCM share that authority.
It uses the existing bounded parallel loader workers; audio never reads files or
waits for preloading. All authored alternatives remain in the head plan. This
cap is not total process RAM: the page pool, metadata, worker scratch, dynamic
sample revisions, octave levels, encoded readers and other parts are separate allocations.
The initial source load must finish preloading before its part is published.

If exact lossless packed heads exceed the cap, the typed capacity fallback
releases the entire staged attack batch before retrying mandatory wavetable
ranges over the same immutable sources/quota. Decode/storage/cancellation errors
remain errors. Mandatory full-resident sources must also fit and cannot fall back
to streaming. Fallback is retained in the load report and support diagnostics,
and the report panel explicitly says sample attacks may wait for disk. Such a
load is not claimed to have complete attack readiness. The startup report records
planned/covered ranges and packed bytes/budget; it excludes later dynamic loads.

A small authored UVI fixture checks actual production Auto loading, exact attack
coverage, nonzero audio in the first callback and zero cold starts/underruns.
A separate physical accounting fixture retains old heads through an audio guard,
stages new heads, and verifies both consume quota until control-side collection;
its audio guard release performs no allocation/deallocation.

The 17-second cold head-open observation above is still a limitation. This policy
revision is not a cold-load speed claim or an early GUI publication implementation.
The UI inspector remains independently usable without loading the plugin binary.
The real-bank receipts below retain both attack successes and tail failures;
they do not establish universal SSD deadline acceptance. Mandatory documented UVI
Program.Streaming/AllowStreaming residency, shared startup full-PCM/head leases,
and separate dynamic revision leases now have focused correctness checks. Live
residency-flag writes, aggregate multi-part admission and dense-tail margins remain
open. Startup diagnostics report required-resident assets/bytes, packed heads and
combined physically held payload separately.

The first real-bank trial of this revision used a 128 MiB ceiling. With 23
compiler observations, a 16.614 ms read-probe p95 produced 1,406 base head
frames and exceeded the exact packed quota: all ordinary attacks fell back,
121 cold starts remained, and first signal appeared one 64-frame block late.
The next warmer 256-frame trial (10 compiler observations) measured 115 us
probe p95, 614 head frames and 87,059,856 packed bytes; all 17,059 ranges across
15,008 assets were ready, first signal offset was zero, and cold starts,
underruns and guarded heap calls stayed zero. These are different adaptive
geometries and uncontrolled cache conditions, **not an A/B speedup**.
The bounded Auto ceiling was increased to 256 MiB to accommodate the observed
latency-driven geometry rather than suppressing or clipping it. The subsequent
256 MiB receipt is below. Explicit fallback remains essential for larger plans
or slower storage; 256 MiB is not a universal readiness guarantee.

### Actual 256 MiB Auto receipt: attack success, tail failure retained

The 64-frame host run covered all 17,047 planned ranges across 15,008 assets,
using 206,391,132 packed bytes. It loaded in 5,626.73 ms and had first signal at
block offset zero, zero cold starts, ordinary underruns, callback deadline misses
and guarded heap calls. Compiler activity appeared in 21 monitoring observations.

The following warmer 256-frame run covered all 17,059 ranges with 86,859,594
packed bytes and loaded in 535.516 ms. First signal again had offset zero, with
zero cold starts, callback deadline misses and guarded heap calls. However,
**24 ordinary stream underruns appeared at block 2**, output offset 512 frames
(~10.67 ms), around the first head-to-tail transition. The 4,856 decoder jobs
reported no failures or reply backpressure; maximum queue wait was 12,386 us
and maximum combined seek/read/decode was 24,927 us. Compiler activity appeared
in eight observations. Those wall times include storage waits and scheduling;
they do not identify pure codec CPU or disk latency.

The second burst reached 160 deferred cold starts despite unchanged complete
static head coverage, while ordinary underruns stayed at 24. The static recipe
therefore does not prove every script-authored dynamic onset is covered. Exact
voice/page/deadline correlation remains necessary. The warm/cold and geometry
differences preclude a speedup claim. This evidence explicitly fails a claim of
universally safe SSD streaming.

The [compact numeric receipt](receipts/dfd-auto-attacks-20261015.json) retains both ceilings.
Immutable full artifacts: `dfd-auto-eager-256cap-evidence.json`, per-host JSON
and decoder stderr under the shared diagnostic artifact directory. The older
128 MiB failure and successful warmer receipt remain preserved separately.

### Follow-up demand and worker CPU audit

A later fixed-binary witness (`b4f584c4d59a43a01c33345cb33bf5230f4a12602dd78b4044b46cc34f781d5e`)
adds a bounded current-voice/page snapshot after selected callbacks and opt-in
Linux worker thread CPU clocks. The query never polls completions, advances
voices or changes requests; it is a current-state query, not historical proof of
an exact missed read. A separate fixture verifies pending-versus-head readiness,
visitor bounds, no heap work, unchanged clock and exact continuation PCM.

In this run the first burst had complete attack coverage, zero cold starts,
ordinary underruns, heap calls and callback deadline misses. The prior 24-underrun
failure remains valid and is not erased by this non-reproduction. Ten compiler
observations were again present (an external `pure_va_dispersion_core` build).
The second burst still counted 160 cold starts. Its first post-callback snapshot
contained 114 cold voices with current source demand outside resident heads;
these pages were pending. Thus unchanged static coverage coexists with actual
uncovered starts; no blanket claim of complete dynamic onset coverage is made.

The slowest retained page read took 19,607 us elapsed but 68 us thread CPU;
worker total was 19,609 us elapsed and 70 us CPU. Other ~18.9 ms reads consumed
33–52 us CPU. These stalls are predominantly off-CPU. This separates compute
from waiting/descheduling, without distinguishing filesystem/device waits from
scheduler delay. Maximum queue wait reached 53,951 us. All 792 jobs had CPU
clocks, zero decode failures and no reply backpressure. Per-job probes and full
numeric snapshots remain private diagnostic artifacts.

A separate source audit found that static head planning omitted the explicitly
authored `Playback.start_range` despite active SampleStart routes. Planning now
covers the full admitted offset interval in both directions, with runtime-u32
and source bounds and saturating arithmetic; unused ranges consume no extra
head storage. An independent full-resident versus actual streamed fixture proves
exact first-callback audio for modulated forward/reverse starts without waiting
for tail pages. This closes a generalized known-bound omission. Arbitrary
script `play_note` offsets still have no invented static bound. The real-bank
cold-start cause remains unassigned until its script-offset identity is joined.
