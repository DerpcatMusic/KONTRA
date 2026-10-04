# Experimental initial PCM cache

The Worker has an optional owned PCM cache for initial static audio resources. It is disabled by default; only `KONTRA_UVI_STATIC_PCM_CACHE=1` opts in. Public Library and CLI loaders, dynamic Lua resource requests, and browser catalog caching retain their existing paths. This cache does not provide activation, new decryption, DSP admission, or instant UI loading.

## Ownership and acceptance

One replaceable local cache slot stores the existing owned I16, I24, or F32 sample planes. Accepted samples retain source channels, RIFF metadata, loops, alias identities, and shared `Arc<Sample>` relationships. There is no mapped PCM or new audio storage variant: later truncation of the disposable cache cannot invalidate accepted samples.

Eligibility is limited to nonempty initial audio plans using mode-0 records. Images, wavetable imports, protected sample records, and banks larger than 512 MiB use the original loader. Individual encoded/decoded source bounds, a 512 MiB aggregate PCM bound, metadata bounds, contiguous payload geometry, finite float samples, checksums, source identities, and program fingerprint are checked before publication. A contract fingerprint includes the decoder, storage, library, generator, UFS, crypto, audio, and dependency sources.

The bank is fingerprinted before and after cache reading or initial decoding. Unix cache and fingerprint inputs use nonblocking open followed by regular-file validation on the opened handle. Cache failures release partial cached PCM before original decoding. Typed cancellation stops loading; original decoder errors retain their failure path. Publication uses a private temporary file and atomic rename. The disposable cache does not promise crash durability.

Worker progress distinguishes loaded aliases, actual codec decodes, resident PCM, and cache hit/miss. Mapping dimensions and the cache result publish together after the existing stop check. Cache use changes resource preparation only; it grants no audio readiness or destination authority.

Later source also binds each alias to the opened Library header and the ordered
member read parameters: record identity, byte offset, encoded size and mode.
The exact existing resource resolver selects those members; record IDs alone
cannot substitute a different member from a public snapshot. This closes a
source-binding gap where an unchanged pathname hash and record IDs could accept
PCM produced from a different snapshot byte range. Mode-0 keys are neither
needed nor included. Invalid snapshot ranges fall back to the original reader,
whose failure remains primary. Old cache metadata without the required digest
is a miss; source-contract invalidation also remains active.

## Completed evidence before the CPU restriction

An earlier owned-cache candidate restored all 158,605,863 sample scalar bit patterns in the actual owned Clarinet resource set, with 196 aliases and 317,220,848 resident PCM bytes. Sample metadata, sharing, UI state, and saved state matched the original loading path. Nine malformed or stale cache cases fell back to original decoding. A partial rejection released cached PCM before original decoding; 93,099 additional live allocation bytes remained at decoder entry rather than a second complete PCM payload.

Three warm-load pairs under concurrent system load measured median loading-thread CPU time of 3,104.199 ms for original decoding versus 569.807 ms for the owned cache. Median wall time was 9,525.485 versus 3,455.024 ms, with substantial variation. These are bounded initial-resource measurements under load, not clean whole-program benchmarks or instant-loading guarantees.

The earlier mapped-cache prototype was rejected after external truncation caused SIGBUS. Its timing results do not describe this owned implementation.

## Verification still pending

The final regular-file/FIFO handling, empty-plan eligibility, Worker hit/miss activity, cancellation without readiness, original codec first-cause ordering, and combined source/UI integration have not been compiled or executed after the CPU restriction. Static review found the integration preserves existing Mapping ownership and invalidates the cached activity snapshot when publishing cache status. Prior candidate tests do not verify this final revision.

The header/member provenance revision also remains uncompiled and unrun. Its
authored tiny WAV fixture checks a genuine original decode and cache hit before
changing only a snapshot offset, then compares fallback PCM and original error
chains. It includes a duplicate-ID decoy and physical-size rejection. These
prepared assertions are not completed evidence. Cache planning still has no
owner-stop checks between its bounded alias iterations; that cancellation gap
is separate from provenance binding.

The installed `bb60218` checkpoint is unchanged and does not include this cache. Do not enable it by default or claim complete loading parity from the receipts above.
