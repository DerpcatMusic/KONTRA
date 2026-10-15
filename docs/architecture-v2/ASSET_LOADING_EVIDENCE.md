# Shared asset loading evidence, 2026-10-14

This review follows the shipping resource authorities and UI preparation worker.
The installed-library inventory is directory metadata; it does not certify that
every program initializes, every picture decodes, or a rendered interface matches
Kontakt or Falcon. Per-request evidence is available to the local offline inspector
through its optional production-renderer feature. Vendor payloads stay in memory.

## Resolution and ownership

| Request | Authority and resolution | Failure semantics |
| --- | --- | --- |
| Kontakt legacy picture, sidecar, bitmap font, file font | `Resources` selects the instrument's library, indexes the nearest loose `Resources` tree, then tries NKR before NICNT. Explicit selected-program resolution honors the saved BProgram/F0 resource-container reference. | Invalid paths, same-tree case aliases, archive ambiguity, failed indexing, oversized reads and read/decode errors remain errors. Other libraries cannot supply a missing file. |
| Kontakt NativeUI source/font/image | NativeUI loads the selected program's resource package on `native-resources`; subsequent images load on `native-art`. `.nui` sources and fonts have a cumulative 16 MiB budget before font parsing. | Missing modules or unreadable fonts fail the authored package. An unavailable NativeUI surface cannot be counted as a successful empty legacy render. |
| UVI bank artwork/file font | The admitted bank owns the request. Candidate paths include program-relative, bank-root and script/package-relative forms, in their existing order. A unique suffix is the final fallback. Explicit `$Bank.ufs/` references must name this bank. | Exact or suffix collisions are ambiguous. A failed bank admission cannot fall through to loose resources or bundled fonts. |
| UVI loose artwork/file font | Canonicalized path inside the loose program directory, with bounded reads. | Absolute, parent, unresolved volume and escaping symlink paths are rejected. Loose filesystem spelling remains exact; bank lookups use a folded index. No cross-library search is added. |
| Explicit `Assistant-Regular.ttf` | The existing authored UVI host-font fallback, only after its own authority reports absence. | Reports identify `uvi-host-font`; it is not reported as a recovered bank font, and no other family/style is substituted. |

`read_with_origin_result` uses the same single bounded payload read as
`read_result`; it adds successful provenance, not a second probe. Origins are
`kontakt-loose`, `kontakt-nkr`, `kontakt-nicnt`, `uvi-loose`, `uvi-bank`, and
`uvi-host-font`. Failed lookups retain their authority and typed reason.

Production `Face` passes its selected program into `Assets::prepare_for_program`.
The worker identity includes that program, so changing programs in the same NKM
replaces the resource worker and its caches. Owner-resolution failure is terminal;
another program's loose tree is not used to manufacture success. Generic
`Assets::prepare` remains available for fixtures without a selected-program owner.

## Decode and cache path

KSP/UVI initialization emits authored assets, frame metadata, placement and font
references into UI IR. `Preparation` derives the actually wanted frame, target
device size and crop window from the selected page, visibility and values. It
queues those requests to `authored-art`; discovery, encoded reads and decoding
happen there, rather than during UI frame construction. NativeUI uses its own
asynchronous package/image workers over the same resource and decode service.

The shared decoder dispatches PNG, JPEG, WebP and SVG by bytes. Unknown dispatch
formats are reported as `unsupported`. A recognized codec that cannot decode,
exceeds an existing codec bound, or disagrees with authored frame geometry is
`decode_failed`; that outcome does not prove corruption alone. Bitmap fonts also
require the documented 256 red delimiter positions and valid single-frame sidecar.
Their malformed bitmap and absent metadata outcomes remain distinct from an
absent image. Font parsing reports success or a decode failure.

PNG frames use streamed scanlines and the existing fractional-coverage,
premultiplied-alpha reduction oracle. Nonstreamed codecs bound their transient
atlas; decoded frame output is bounded at eight million pixels. The frame-worker
LRU admits at most 64 MiB and settles rejected requests without a decode loop.
The whole-strip LRU retains at most 48 MiB, and NativeUI's image cache retains
at most 48 MiB and 4096 entries. These budgets describe their respective caches,
not the entire process or combined rack memory.

## Corrections established in this branch

* Whole PNG strips previously allocated every output frame and reduction row
  before the strip LRU checked its budget. Preflight now checks total output,
  all reduction-row scratch and frame-structure storage before allocating frames.
  A strip that cannot fit falls back to the bounded requested-frame path. Encoded
  bytes and codec scratch have their separate existing limits.
* Successful whole-strip decodes previously bypassed the scan decode counters.
  They now count once, while later frame cache hits count as cache hits with no
  encoded read or decode. The request receipt preserves the actual frame pixels.
* A UVI UI source previously also discovered Kontakt resources that it could
  never use. That second authority is no longer constructed. Selected Kontakt
  NativeUI construction also avoids the duplicate preliminary `Source::of` walk.
  Kontakt container discovery now streams a child directory and retains only
  NKR/NICNT paths, avoiding a temporary sorted copy of every sample filename.
* UVI's recent-bank cache previously deep-cloned every directory member/footer/name
  and the entire lookup map into each reader, and duplicated both during first
  admission. Readers now share immutable `Arc` storage. Rewritten bank identity
  still invalidates that namespace. Fault-injection tests mutate private copies.
* A UVI suffix miss previously allocated lowercase strings for every bank member.
  It now searches the shared, once-folded lookup keys. Duplicate/case-colliding
  paths remain ambiguous rather than selecting an arbitrary member.
* Kontakt loose discovery previously stopped silently after 65536 entries or
  32 levels and subsequently called an unindexed request missing. Incomplete
  discovery now reports a fixed limit/read failure for unresolved requests;
  already indexed files remain readable. Length/NUL validation applies directly
  at the provider boundary as well as the UI boundary.
* Sparse strip preparation previously used a linear value fraction even when
  the painter used authored UVI exponential/power mappers. Preparation now calls
  the same mapper and frame helpers; fixed authored frame overrides retain
  precedence. Kontakt six-state buttons prepare all normal, held and hovered
  on/off frames because transient pointer state is absent from parameter values.
* Unix UFS reads now use exact positioned reads instead of a separate seek/read
  pair; other targets retain their prior path. Header traversal uses a fixed
  twelve-byte stack buffer instead of allocating one small vector per record.
  The [Rust FileExt contract](https://doc.rust-lang.org/std/os/unix/fs/trait.FileExt.html#method.read_exact_at)
  specifies exact offset reads, unchanged cursor, interrupted-read handling and
  early EOF. The fixture checks bytes, cursor and EOF explicitly.

## Metadata cache and measured storage bottleneck

The UVI process cache and persisted cache have different scopes. Recent readers
share immutable directory/index Arcs; a same-process reopen does not deserialize
metadata. A later process can load `kontra/v2-ufs-directories` from the ordinary
platform cache directory. This product cache stores typed record/member/folder
descriptors, names, paths, links, warnings and their bounded metadata suffixes
(at most twelve bytes for a member and four for a folder). It stores no resource
body, script, sample, image, font or derived metadata/content access key.

Its signature includes namespace digest, format/schema version, automatic
parser/cache/codec source digest, source path, captured identity, size, nanosecond
mtime and bounded first/last 4096-byte digests. Unix adds device/inode and ctime;
other targets require an available captured creation generation. The bounded
fingerprint is not a full-file content hash or protection against adversarial
middle edits preserving every stamp. Entry checksum and source identity checks
run before acceptance and again before publication. Invalid, stale, wrong-key,
unreadable or oversized entries fall back to the source parser.

Cache entries are limited to 64 MiB; the binary decoder checks lengths and counts
before allocation with a 128 MiB aggregate allocation budget. Validation checks
the contiguous record chain, resource ranges/protection, metadata tags/suffix
lengths, child/parent links and paths rebuilt from those links. The local cache
budget is 256 MiB. Private same-directory temporary files publish by atomic rename;
partial temporary files are never read as entries. These operations occur on the
existing loading worker. `KONTRA_SCAN_ACTIVE` bypasses both lookup and store;
scans do not discover the product cache directory. First-ever indexing still
requires the source scan.

The owned Augmented Orchestra bank is 18,443,085,298 bytes, with 140,136 records,
68,299 members and 596 folders on an `ntfs3` mount. The initial process-cold
metadata receipt was 19,335 ms; same-process Arc reopens were 0.014–0.022 ms.
Reconstructed prior deep clones took 5.59–18.48 ms; this is an isolated clone
mechanism comparison, not an archived executable baseline.

An opt-in stage receipt attributed 63,260 ms of a 65,472 ms directory decode to
140,136 twelve-byte record-header reads. Name transforms totaled about 15 ms;
casefold index construction was 11.6 ms. Visible retained vector/string capacity
was 36,020,220 bytes, excluding allocator overhead and peak temporary maps.
Performance-event sampling was unavailable in this session; phase clocks and
Linux process read counters provide the attribution. Clocks are elapsed time,
include waits and nesting, and must not be added as independent CPU work.

| Header experiment | Header reads | Requested header/adjacent bytes | Interpretation |
| --- | ---: | ---: | --- |
| Exact direct ranges | 140,136 | 1,681,632 | Default; no lookahead amplification |
| Full aligned 512-byte window | 111,955 | 57,320,946 | Exact directory parity; later OS-warm run |
| Full aligned 4096-byte window | 74,996 | 307,181,042 | Exact parity; excessive byte amplification |
| Prior-short-record gated 4096-byte window | 137,489 | 278,942,726 | Failed locality hypothesis: short metadata interleaves with long records |

No buffered window is enabled by default. `KONTRA_UFS_HEADER_WINDOW_BYTES` is a
bounded diagnostic experiment, not a measured cold-start recommendation. Standard
[Rust buffering documentation](https://doc.rust-lang.org/std/io/struct.BufReader.html)
motivates amortizing repeated small reads; real record ordering and bytes decide
whether that approach is useful for this container. An isolated same-process
alternating warm header test read identical 1,681,632-byte ranges each time:
seek/read took 128.1–135.7 ms; positioned reads took 84.8–86.9 ms. This establishes
the syscall-mechanism improvement, not a cold-storage throughput claim.

The final persisted-cache fixture compared every directory field against the
source parser, including metadata suffixes and warnings. Population source scan
took 8667.8 ms; validated store took 109.0 ms; five fresh-owner cache loads took
104.0–109.5 ms. A separate fresh-process load-only receipt took 105.1 ms and
performed no record-chain scan. The typed entry was 17,794,273 bytes. An earlier
population run took 45,219.7 ms with 443,502,592 Linux physical read bytes; its
cache loads took 93.4–110.5 ms. Process-cold, OS-warm, NTFS storage latency,
instrumentation and concurrent build activity are distinct; no OS cache was
flushed, so these population times are not controlled before/after speedups.

Numeric receipts are retained under
`/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-asset-audit-20261014`.
The explicit owned-bank test cache stays in dev-drive scratch and is not part of
the retained evidence or repository.

## Diagnostic scope

With `shots` enabled, `AssetProbe` records a bounded history of actual requests:
asset index where known, capped local path, kind, authority, outcome, codec,
selected frame/count, target, window, encoded/prepared bytes, prepared dimensions,
read/decode/total elapsed milliseconds, and strip-cache hit. Actual worker
requests also report queue and decode-permit wait separately; direct/native
requests leave these unmeasured fields null. Source and worker
histories are capped at 512 records with explicit dropped counts. NativeUI's
font and image history uses the same bound. Production builds without `shots`
have no per-request trace storage or timing clocks.

Aggregate discovery, read and decode timing/counts remain available when traces
drop. Discovery is separate from request duration; first resource reads can include
lazy archive indexing. Decode timing includes codec-lock waits. These are inclusive
wall-clock times, not on-core CPU time, and must not be summed into a frame budget.
Frame LRU hits can reuse an existing completion without another worker request;
absence of a new trace is not a new missing-file finding. Histories across pages
and Bitmap/Vector presentations are not manifests of all declared assets.

The offline renderer may write numeric local reports and present ephemeral PNG
witnesses. It must separately identify unrequested assets, pending work, failed
admission, NativeUI initialization failures and unsupported services. An authored
control binding, a present image, successful CPU paint and native gesture/audio
fidelity are different claims.

## Installed hierarchy observation

The fresh filesystem-only inventory found all eleven local Kontakt libraries,
835 NKI/NKM files and 21 resource containers. Ten libraries have resource
containers and no loose `Resources` tree. Morphology Evolved also has loose resources: 103 PNGs,
58 TXT sidecars and 30 WAV files. This demonstrates why a general loader must
keep both archive and loose-file paths; it does not establish all member outcomes.

Receipt: `/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-asset-audit-20261014/installed-kontakt-resource-hierarchy.json`.
Historical Pacific/Vista missing pictures and the rejected Vista NICNT remain
documented in `docs/audit-2026-10-10/UI_LOADING_393.md`. The directory census alone
does not replace fresh per-request evidence for those instruments.

## Dynamic sample replacement boundary

The repaired Lua task callback contract does not implement `loadSample`.
That API still produces a failed/unavailable resource task. Existing UVI
`Bank::decode_resource` and `Bank::stream_source` can resolve and decode an owned
audio resource, but no dynamic source-replacement publication connects the request
to future voice starts. Substituting only a PCM buffer while retaining the old
frame length, rate, root, loops or stream identity would be incorrect.

A future implementation needs the authored program/bank origin, bounded worker
queue and cache, cancellation/generation identity, a complete immutable source
revision, and success/failure completion through the native Task contract. Existing
voices must retain their original source revision; future starts receive the new
one only after its metadata and backing source are validated. Resolution, payload
read/decryption, header parse, source initialization and publication need separate
timing receipts. This review does not claim that replacement is implemented.

## Validation receipts

Focused production UI asset checks: 18 passed, one explicit installed-inventory
ignore. Kontakt resource units: 5 passed. Kontakt integration with the actual
installed library root: 3 passed, two explicit Conflux fixture ignores; Afflatus
NICNT and encrypted Solo NKR image payloads ran in memory and passed. UVI bank
units: 9 passed, one metadata profile ignore. UFS/cache units: 9 passed, four
explicit numeric-corpus/profile ignores. Owned AO window and full-directory
persisted parity profiles passed; the fresh-process cache-hit receipt passed.

Gold fixtures cover preallocation budget/fallback pixels, premultiplied alpha,
nonlinear/fixed strip frames, six button states, selected-owner failure
settlement, missing/unsupported/decode/geometry/font outcomes and cache hits.
Cache fixtures cover bounded roundtrip, every truncation boundary, excessive
counts, checksummed invalid ranges/paths/parents, corruption, oversized entries,
wrong namespace, replacement/rename, bounded fingerprint changes, partial
unpublished files, exact source fallback and scanner bypass. Their temporary
files remain in dev-drive scratch. These checks certify their scopes; they do
not certify every installed program or native gesture/audio parity.
