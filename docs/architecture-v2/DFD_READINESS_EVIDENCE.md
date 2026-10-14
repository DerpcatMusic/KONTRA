# DFD attack readiness and immutable disk sources

The production V2 `Auto` policy currently publishes lazy sample heads. A loaded
editor is therefore not evidence that authored sample attacks are resident.
This differs from the initial-portion-in-RAM behavior described by
[Native Instruments' DFD documentation](https://support.native-instruments.com/support/solutions/articles/69000879768-what-is-dfd-).
These receipts identify that gap; this checkpoint does not change preload
budgets or claim a completed DFD startup policy.

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

Numeric receipts are retained privately under
`/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-asset-audit-20261014/dfd-auto`.
They include paired full process receipts, bounded decoder probes, aggregate
summary, exact coverage bounds and a binary fingerprint. No instrument source,
sample data or bank payload is included.
