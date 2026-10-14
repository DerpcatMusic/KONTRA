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

## Worker ownership and source changes

Completion queue backpressure now retries only publication. The worker retains
the already decoded job and its exact success/failure result. It does not reopen
or decode that page again. A fixture injects three publication rejections for
both valid PCM and invalid-source failure, then verifies one open, one decode,
four publication attempts, and the exact final result/bytes.

UVI loose sources now freeze identity at admission. On Unix the stamp includes
device, inode, high-resolution change/modify times and length. Every reopened
reader checks the **opened handle**, and physical refill checks it again before
decrypting/using those bytes. Replacing a path after reader eviction cannot
silently bind new bytes to an old sample revision. Same-size rewrites with
restored mtime, inode replacement, removal, truncation and change after open are
covered. A still-open old handle may supply its unchanged original bytes or
fail explicitly; it cannot publish replacement bytes as the old revision.
The non-Unix creation/modify/length fallback has weaker rewrite detection;
equivalent Windows by-handle identity/change-time validation is not verified.
Kontakt source admission still needs the same general stamp law at this point.

UVI streaming also rejects an undeclared total frame count instead of silently
scanning the entire encoded sample to construct a supposedly bounded header.

## Verification and receipt location

Focused verification passed: exact offset/reverse readiness 1/1, injected
completion pressure 1/1 (success and failure subcases), and UVI streamed identity
and decoder parity 4/4. The whole Kontakt stream suite passed 20 tests (one
pre-existing ignored stress test), including zero/exact/over-cap, multiple ranges,
short source, cancellation before/during reads and the unchanged in-flight KSP
purge fixture. Core packed-publication tests passed 2/2. The optimized production
readiness example compiled before the subsequent budget change.

Numeric receipts are retained privately under
`/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-asset-audit-20261014/dfd-auto`.
They include paired full process receipts, bounded decoder probes, aggregate
summary, exact coverage bounds and a binary fingerprint. No instrument source,
sample data or bank payload is included.
