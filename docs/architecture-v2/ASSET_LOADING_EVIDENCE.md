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

## Diagnostic scope

With `shots` enabled, `AssetProbe` records a bounded history of actual requests:
asset index where known, capped local path, kind, authority, outcome, codec,
selected frame/count, target, window, encoded/prepared bytes, prepared dimensions,
read/decode/total elapsed milliseconds, and strip-cache hit. Source and worker
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
835 NKI/NKM files and 21 resource containers. Ten libraries expose their artwork
through containers. Morphology Evolved also has loose resources: 103 PNGs,
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

The exact whole-strip memory/fallback pixel fixture and request-outcome/cache-hit
fixture pass. Broader asset/resolver checks and the owned metadata timing fixture
are being recorded before this slice is marked complete. The ignored metadata
profile decodes no program/sample payloads and labels reconstructed legacy clone
costs separately from current cold/warm measurements.
