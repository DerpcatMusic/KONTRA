# Remaining v1 load-path advantages

Source comparison: pinned v1 `0cb7a8a0`, current v2 `94005844` (runtime equals
`6a1e031c` after the heap-trim revert). Numeric evidence is the current-source
`w8-load-attribution-resume` receipt. Its Analog pre-editor stage cell was QUIET;
the frozen-v1 cells were CONTENDED. These observations rank investigation work,
not admitted paired timing improvements. Nested translation spans are not summed.

## Ranked differences

1. **Resolve: repeated archive path walks.** V1 `src/import.rs:1038–1167`
   retains archive existence and canonical paths in load-scoped `OsString` maps.
   V2 `crates/sampler-kontakt/src/samples.rs:61–86` stats and canonicalizes the
   same archive for each unique sample name. Its source/header helpers also
   traverse archive-named ancestors. Analog has 95,624 zones; current zone/resolve
   takes 355.68 ms and subsequent source resolution 411.94 ms. V1's corresponding
   source receipt observes 50.32 ms, with different contention conditions.
   This is the largest currently actionable v1 port: reuse its path caches in
   `Samples`, retain canonical-root containment, and seed the existence checks in source
   workers. A synthetic syscall-count regression must fail before the port.

2. **Zone build: lighter representation and fewer passes.** V1
   `src/import.rs:802–911` builds a compact zone record, then batch-resolves
   distinct sample IDs. V2 `library.rs:415–480,1402–1540` keeps native object
   mirrors, lowers mappings into IR, validates that IR and later lowers prepared
   regions. Current Analog object parsing is 88.45 ms; translation is 574.64 ms,
   including the nested 355.68 ms zone/resolve span. V1 avoids intermediate IR
   and preserves fewer native semantics. Dropping v2's mappings, native mirrors,
   multi-loop behavior or validation would lose required functionality. Earlier
   immutable-chain sharing and compact playback templates are already integrated;
   the new resolver port targets repeated work without discarding those semantics.

3. **Parse: v1's warm imported-instrument cache.** V1 `src/cache.rs:1,81–107`
   can bypass parsing, resolution and imported-zone construction after validating
   dependencies. V2's persistent cache is numeric sample-header metadata only
   (`crates/sampler-kontakt/src/header_cache.rs:45–109`). The shared cold container
   path still uses the same NI reader/decryptor; current Analog container read,
   decrypt, chunk and object stages are 4.51/23.89/6.49/88.45 ms. V1 warm cache
   contains imported instrument/script data, so directly persisting that payload
   would violate the numeric-only cache requirement. Keep this warm-path advantage
   explicit; do not relabel a second OS-cache pass as a parsed-cache hit.

4. **Sample preload: v1's initial bare bank and overlap.** V1
   `src/plugin.rs:2410–2416` overlaps script setup and bare-bank construction,
   publishes playable audio before wider preload/artwork, and reuses shared sample
   sources/resident spans (`src/audio.rs:810–893`, `src/engine/bank.rs:213–272`).
   V2 source and header startup are already bounded-parallel and the production
   lazy policy admits zero eager heads in these cells: Analog preload 0.83 ms,
   Conflux 0.09 ms. Each still retains a 24 MiB page pool. Increasing eager preload
   is not a load-speed fix; pool/stream transport remains W9-owned. Further script/
   header overlap must preserve the resource-aware single-init ordering W5 owns.

5. **Art decode: v1 defers authored art after audio publication.** V1
   `src/plugin.rs:2428–2447` publishes controls without decoded pictures.
   V2 header artwork already runs asynchronously with bounded decode, 1024×512
   output and an 8 MiB cache (`src/artwork.rs`). Its streaming PNG port reduced a
   synthetic header peak from 128.30 to 9.40 MiB. Repeating that port would not
   explain Conflux's current 83.24 MiB pre-editor-to-retained-editor increase.
   Authored picture/atlas/renderer allocation attribution belongs to W3, who has
   the exact stage-harness command and counter lifetime. Live CLAP-window RSS
   remains distinct from the retained CPU harness.

## Port scope and checks

The resolver port reaches `Samples::resolve`, `source`, `cached_source`, `frames`
and `decode`, plus cloned source workers. Callers are zone and convolution-resource
translation, streamed admission, the numeric header cache and external sample
decoders. It adds no cross-load/global cache or persistent payload. The cache lifetime is one read-only library load. Each fresh
`Samples` starts fresh; failed canonicalization remains an error and canonical
paths outside the library remain rejected. Source handles, keys, cancellation,
member validation and sample decode policy remain unchanged.

The deterministic synthetic fixture resolves the same NKX member 64 times and
checks archive-path stat/readlink counts using `strace`; it does not use library
data or wall-time thresholds. Boundary and existing sample/cache fixtures protect
behavior. Non-timed tests run through `kontakto-heavy`; the all-14 timed matrix
waits for W6's direct quiet-chain handoff.

## Failing-first evidence

Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w8-v1-resolver-path-cache-20261009`.
The unmodified product at `94005844` made 64 `statx` and 64 `readlink` calls on
one archive for 64 successful resolutions. `RED.json` pins its copied fixture
binary. The syscall assertion failed before the product edit. After the port,
64 resolutions make one `statx` and one `readlink`; `GREEN.json` pins the passing
fixture. This proves the eliminated filesystem work, not elapsed load parity. The port copies
v1's existence and canonical-path memoization into the shared `Samples` path,
including decoding and frame-count callers. Worker clones retain the existence
map; they do not copy an unused canonical-path map.

The earlier loose-source allocation regression now primes the deliberately
retained archive-path cache before measuring repeated lookup allocations. Its
per-lookup budget is unchanged. This port adds only archive-path metadata;
editor RSS and whole-load timing still require the queued quiet matrix.

Validation passed through `kontakto-heavy`: the syscall regression, all three
sample-resolution integration tests, mixed clear/encrypted archive headers and
worker clones, numeric-cache rejection checks, and `cargo test --profile ci
--lib --no-run`. The root build emitted existing warnings and succeeded. This
is a source/syscall result; first sound, load-time and RSS parity are unmeasured.

Reproduction: build `cargo test --profile ci -p sampler-kontakt --test
sample_resolution_allocations --no-run`, then run `python3
tools/check-sample-path-cache.py <test-executable>`, both through `kontakto-heavy`.

NEXT: push → rebuild the scanner from the accepted chain → all-14
cold/repeat/onset/RSS after W6's direct quiet-chain handoff.
