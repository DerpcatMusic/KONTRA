# 05 Stream: sample store, preload law, memory

Rule 4: voices start on time. If storage can't keep up you get a dropout,
never a late voice. Today the head length comes from warm-cache latency
measured at load. Script and CC start offsets fall outside the head. Over
budget, a binary search shrinks heads with `include_offsets = false`.
`Cursor::cold` and `render_starved` freeze voices with no upper bound. On
Vista, 100% of starts are cold (PLAYBACK_AUDIT.md).

## 1. The preload law

```rust
pub struct PreloadLaw {
    pub base: Frames,          // Kontakt DFD default: 60 KB of source data per sample (all channels) → 61440 / bytes_per_frame
    pub cover_start_mod: bool, // always true: the zone's whole start-offset range is resident
    pub min: Frames, pub max: Frames,
}
```

- **Preload = `base` + the zone's full reachable start range.** The start
  range is the union of zone `S.Mod` or `playPos` ranges, every script
  `play_note` offset the KSP or Lua sema can bound, and every CC/RTC route
  to sample start (Areia's CC113, Vista's KeyPosition). A start range the
  front end can't bound is a load diagnostic plus a whole-sample preload
  for that zone. Kontakt does the same.
- **The budget is never solved by shrinking heads below the law.** Over
  budget, the loader reports which zones exceed it and drops whole
  articulations or mic positions *the user chose to purge*. KONTRA never
  silently truncates.
- **Layout.** Preloads are stored in the source format (i16/i24 packed,
  mono or stereo as recorded), not as f32 stereo scaled by `max_step`. The
  voice decodes on read. The ×16 `max_step` factor is replaced by the
  actual maximum pitch step of the zone, from the plan.

## 2. The store

```rust
pub struct Store {                 // process-wide, one per plugin binary (shared by all instances)
    blobs: ContentMap<BlobKey, Blob>,   // key = (file identity, offset, len, format) → dedup across instances
    pages: PageCache,                   // fixed-size pages (64 KB), clock eviction
    io: IoService,                      // N worker threads, priority queue by deadline
}
pub struct Blob { preload: Arc<[u8]>, file: FileRef, frames: Frames, fmt: SampleFmt }
```

- **Content-keyed and process-wide.** Two instances of the same library
  share one preload, and so do two patches that share samples. Today each
  load holds its own copy.
- **Deadline-ordered I/O.** A voice asks for the page it needs after the
  preload, plus a lookahead, tagged with the sample time it is needed by.
  The `IoService` serves the earliest deadline first. Reads are
  `pread`/`io_uring` where available, with no mmap on the audio path.
- **Two stream buffers per voice**, HISE-style. The voice reads preload,
  then buffer A while B fills, then swaps. Buffer size comes from the law
  (block-independent).
- **The audio side is lock-free.** The engine holds a `StreamClient`: a
  read-only page table (`Arc` swapped on growth) plus an SPSC request ring
  per engine. It never blocks and never allocates.

## 3. When data is late

The voice outputs silence for the missing frames. It counts
`Stats.dropout_frames` and the stall position, and keeps its *timeline*:
the cursor advances, so the note doesn't start late or drift. There is
no `cold`, `holding_onset`, `start_runway` or onset cohort. Contract C2
(10-TESTING.md) asserts zero dropouts at the reference disk speed, and
contract C1 asserts every voice's first frame lands at its scheduled
sample.

## 4. Formats

`kontra-stream` decodes from a `SampleFmt` and reads through a `Source`
trait that each format crate's reader implements: NCW, monolith slices,
UVI UFS, WAV/FLAC/AIFF. It doesn't know Kontakt or UVI. Today `sampler-uvi`
depends on `sampler-kontakt` for `StreamPolicy`, `Streamed` and
`AssetSource`; that dependency goes away.

Decrypting and decompressing readers live in `format-*`. The store calls
them on I/O threads and caches *decoded* pages, so NCW decoding happens
once per page.

## 5. Memory targets

| Measure | Today | Target |
|---|---|---|
| Preload bytes per stereo 24-bit zone, default law | f32 stereo × max_step 16 head | i24 packed × (60 KB + start range) |
| Same library in two instances | 2× | 1× |
| Areia full load vs 1.5 GB budget | exceeded, heads shrunk to 611 frames | under budget at full law (measure in phase 3) |
