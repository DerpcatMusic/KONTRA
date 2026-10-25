# 05 Stream: sample store, preload law, memory

Rule 4: voices start on time. If storage can't keep up you get a dropout,
never a late voice. Today the head length comes from warm-cache latency
measured at load. Script and CC start offsets fall outside the head. Over
budget, a binary search shrinks heads with `include_offsets = false`.
`Cursor::cold` and `render_starved` freeze voices with no upper bound. On
Vista, 100% of starts are cold (PLAYBACK_AUDIT.md).

## 1. The preload law

Kontakt's own law, decompiled (`docs/re/engine/k-sampler-source.md`, `FUN_14082a530`) and
already ported as `crates/sampler-kontakt/src/dfd_window.rs`:

```text
P = preload bytes per channel (instrument dfdChannelPreloadSize, default 61440; global override)
C = channels, F = ceil(bits/8) * C bytes per frame, off = the zone's S.Mod range in frames
d     = pos - off                   (pos = zone start in frames)
base  = P * C                       (60 KB per channel, not per sample)
if d < 0: base += F * d             (a start near the sample head has less room behind it)
total = base + off * F * 2
W     = ceil(total / 6144) * 6144   (6 KiB granularity); head frames = W / F
start = max(d, 0)                   (the window starts S.Mod before the zone start)
```

- **Preload = `base` + 2 × the zone's S.Mod range**, starting S.Mod before the zone start, exactly as `dfd_window::window` computes it.
  Nothing else extends the head.
- **Script and CC offsets are bounded by S.Mod.** KSP manual, `play_note()`: "In DFD mode,
  the sample offset is influenced by the Sample Mod (S. Mod) value of the zones. Offsets
  exceeding the zone's S. Mod setting are ignored" (since Kontakt 4.1). KONTRA does the same
  for streamed zones, so an offset the front end cannot bound never forces a whole-sample head.
  Formats without S.Mod pass their declared start-modulation range as `off`.
- **Memory bar.** The resident head bytes for an instrument are within 10% of the sum of
  Kontakt's windows for the same zones (deduplicated per sample), measured on Areia, Vista and
  ANALOG. P1.2's first attempt (base + script range, unbounded → whole sample, f32 accounting)
  measured 4.4 GB on Areia, 469 MB on Vista and 50 GB on ANALOG (2026-10-25, PluginArtifacts/v3-gates/P1.2).
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
