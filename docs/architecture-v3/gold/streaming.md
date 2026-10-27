# Gold standard: disk streaming and memory

Scope: preload and head sizing, what stays resident, RAM format of heads, page cache and I/O,
late data, offline bounce, start offsets, sharing, load time. Evidence tags: [doc] vendor or project
documentation, [source] third-party source, [binary] string or decompile evidence, [receipt] a
PluginArtifacts run, [code] KONTRA source, [inferred] reasoning from the above. Libraries are cited only
where a file or receipt shows the behaviour; no library was played.

## 0. Corrections to the brief

- **"8192 resident zones for Areia 16 Violins" is not supported by any artifact I could find.** The only
  Areia receipt (`PluginArtifacts/v3-gates/P1.2/r12/areia.txt`, instrument `Areia - 03 16 Violins - Spiccato
  Fast.nki`) admits **528 assets, 106,235,712 payload bytes** (`budget_bytes` 1,547,541,888). 17,705,952
  requested frames × 6 bytes = 106,235,712 exactly, so the admitted bytes are stereo i24 packed frames.
  The zone count is not in that receipt. The `docs/architecture-v3/05-STREAM.md` figures ("Areia 1,056 of
  49,152 zones resident", "Kontakt ~104 MB for 1,056 zones") also have no receipt I could open. Treat both as
  unverified until an `ir_json` / `nki_tree` dump of the group `purged` flags and zone counts is attached.
- **"Kontakt keeps decoded PCM" is not what the manual says.** Kontakt can save samples as its own
  lossless NCW codec, which it "decompress[es] on-the-fly from memory" to cut the footprint
  [doc: KONTAKT_Manual.pdf, "Save as" section, p.~ line 4598–4604 of `pdftotext -layout`]. KONTRA's
  head is decoded packed PCM ([code] `crates/engine/stream/src/file.rs:17-96`, `NcwSource`). That is a
  real gap, see §5.

## 1. Kontakt 8

### 1.1 What the user sees ([doc], KONTAKT_Manual.pdf)

- **DFD (Direct From Disk) tab**: streams from disk and "only load[s] a small section of the sample in RAM"
  for groups whose Source Module is in DFD mode. "DFD Preload Buffer Size" is the size of each buffered
  portion; "If any Group in DFD mode causes drop-outs … try increasing this parameter" (manual, DFD Tab).
- **Background loading / "Allow instant playback for samples which are not loaded yet"**: plays notes
  during background load, with a documented risk of flam-like glitches. Off = hold until loaded.
- **Purge menu** (per instrument): usage-driven. "reset markers" clears per-sample "used" flags, play,
  then "update sample pool" unloads unflagged samples and reloads purged ones; "purge all", "reload all".
  Manual warns that round robin and random triggers break under purge. Usage is per **sample**, not per group.
- **Memory meter**: "Sample memory: total amount of allocated sample memory" (manual, Classic view).

### 1.2 What the KSP manual says ([doc], KSP_Manual.pdf)

- `purge_group(<group>, <mode>)`: 0 unloads, 1 reloads; only in `on ui_control`, `on ui_controls`,
  `on persistence_changed`; async via `$NI_ASYNC_ID` / `on async_complete`.
- `get_purge_state(<group>)`: 0 purged, 1 loaded.
- **S.Mod rule** (`play_note`, KSP manual p. at line 3277): "In DFD mode, the sample offset is dependent on
  the Sample Mod (S. Mod) value of the respective zones … Sample offset value greater than the zone's S. Mod
  setting will be ignored and no sample offset will be applied." So an offset cannot force a larger head.
- `$ZONE_PAR_SAMPLE_START_MOD` (KSP line ~12904): "the sample start modulation range … the S.Mod
  parameter in Kontakt's Wave Editor."
- `set_voice_limit()` note: "Time Machine Pro uses its own memory allocation that is separate from memory
  used by loaded samples" (KSP manual, around line 4461).

### 1.3 The head law, as recovered ([doc] + [code] + [binary])

`docs/re/engine/k-sampler-source.md` (G2) recovers, for a DFD-mode group only:
`P` = `dfdChannelPreloadSize` in bytes per channel, default 61440 (decompile constant `0xf000`; 723 of 734
surveyed programs carry it, [doc] `k-sampler-source.md` line 121). Window `W` rounds up to 6144-byte
grains; `base = P*C`, extended by `F*d` when the start sits near the head; `total = base + off*F*2`, where
`off` is the zone's S.Mod range in frames. The arithmetic is ported in `crates/sampler-kontakt/src/dfd_window.rs`
and mirrored in `crates/engine/engine/src/prepare.rs:164-187` [code]. Status: unit high, formula medium
(the channel argument is not fully traced).

What this implies:
- A DFD head is **60 KB per channel plus two S.Mod ranges**, not a fraction of the sample.
- Groups **not** in DFD mode (`source mode != 4`, see the same doc) are loaded whole. This is the case the
  "resident zones" arithmetic must separate from streamed zones.

### 1.4 Stored form

- Heads and full samples are kept in the source format (i16/i24 packed) when the sample is not NCW
  [inferred from receipts: i24 stereo = 6 bytes/frame, see §0].
- Kontakt's own NCW ("proprietary lossless codec, 30–50%") is kept compressed and decoded per read
  [doc, Kontakt manual "Save as" section]. The manual says this improves streaming access and footprint.
- Not verified: whether DFD heads from NCW sources are compressed in RAM. A probe is needed: load one
  NCW-saved library, compare Kontakt's Memory meter with and without the compressed box checked.

### 1.5 Scheduling and late data ([doc] + [binary])

- The manual describes drop-outs as the expected failure of a too-small DFD buffer. No playback timeline
  guarantee is documented.
- Kontakt binary strings include `dfdChannelPreloadSize`, `overrideInstPreloadSize`, `overrideInstDFD`,
  `streamingBufferSize`, `numStreamingVoices`, `loadSamplesInBackground` [binary: `strings` on
  `Kontakt8.exe`, 4 matches]. The thread count and read policy are not recovered.
- Recovered decompile reference: `docs/re/engine/KONTAKT_CPU_ARCHITECTURE.md` line 72 lists the same
  options at `0140910000.c:7645`, `0140920000.c:8258`.

## 2. UVI Workstation / UVIScript

- **[doc] none found online.** WebSearch returned nothing on UVI streaming or UVIScript; UVI's own docs
  were not reachable in this session. The local `/home/derpcat/Documents/UVI/Falcon/UVIScript` directory is
  empty (0 files).
- **[binary] `UVIWorkstationx64.exe` strings**: `$PlugsoundPro.ufs`, `'%1' UFS not found`, `Allow Streaming`,
  `AllowStreaming`, `acid disk based`. So UFS containers and a per-sample streaming flag exist. The
  preload size and policy are not recovered from strings.
- **[receipt]** KONTRA's UVI path (`sampler-uvi`) runs the same store as Kontakt. Its cross-check tests
  (`normal_uvi_checked_pcm_streams_retain_original_ordinals_…` in the P1.2 patch) compare streamed and
  resident audio, not UVI's own memory.
- **[library]** `/data/main/Libraries/UVI/*` (Augmented Orchestra 18 GB, VWinds Clarinets 2.2 GB, Double
  Reeds 1.7 GB, Flutes 1.6 GB) are `.ufs` containers; read format pending (not studied beyond names).
  Scripting: not read.

## 3. HISE

- **[doc] docs.hise.audio, Sampler Settings**: "Preload Size … default 8192 samples … If the preload size is set
  to -1, the whole sample will be loaded into memory." "Buffer Size … default 4096 … two streaming buffers
  which are swapped, one for disk reads and one feeding audio" (WebSearch excerpt; original page
  `docs.hise.audio/working-with-hise/workspaces/sampler-workspace/sample-settings.html`).
- **[source] `hi_streaming/StreamingSampler.h:237-240`**: `PRELOAD_SIZE 8192`, `BUFFER_SIZE_FOR_STREAM_BUFFERS 4096`.
- **[source] `StreamingSamplerSound.cpp:186-208`**: `internalPreloadSize = preloadSize + sampleStartMod`
  (fixed, not a function of the sample rate or the zone's own offset range). Entire sample when the value
  exceeds `HISE_LOAD_ENTIRE_SAMPLE_THRESHHOLD` (default `INT_MAX`, `StreamingSampler.h:271-272`, so
  effectively never). `jmax(..., 2048)` floor.
- **[source] `StreamingSamplerSound.cpp:322-326`**: `getActualPreloadSize` counts `int16` (monolith) or `float`
  (streamed) bytes per sample. Storage is float or `int16` by `hlac::HiseSampleBuffer` (`hi_lac/hlac/SampleBuffer.h:209-240`).
  HLAC is HISE's compression; `CompressionHelpers.h` exists but its use on the preload path was not traced.
- **[source] `SampleThreadPool.cpp:85-192`**: a single lock-free FIFO (`moodycamel` queue), waits 500 ms when
  idle, error text `"HDD overflow"`. No deadline ordering. Not EDF.
- **Late data**: `readFromDisk` clears the buffer and reads what it can (`StreamingSamplerSound.cpp:1245-1290`);
  the rest stays zero. No timeline hold is seen. [inferred] silent dropout, timeline kept by the caller.
- **Load time**: preloads are read during note-on setup and in the thread pool at load. Not measured.

## 4. sfizz

- **[source] `src/sfizz/Config.h:29-36`**: `preloadSize 8192`, `loadInRam false`, `numBackgroundThreads 4`,
  `fileClearingPeriod 5` seconds.
- **[source] `FilePool.cpp:310-333`**: `framesToLoad = min(frames, maxOffset + preloadSize)` unless `loadInRam`.
  So the head is 8192 frames **plus** the maximum start offset seen for that file. Note the offset is
  per-file, taken from the largest offset any region asks for, not per zone.
- **[source] `FilePool.cpp:288-290`**: preloaded files are keyed by `FileId` (path, reverse flag). Two regions
  share one buffer. Cross-instance sharing is not visible from this file.
- **[source] `FilePool.h:80-86`**: `getData()` returns the streamed buffer when `availableFrames` is greater
  than the preloaded size, otherwise the preloaded one. A voice never waits.
- **[source] `FilePool.cpp:667-702`, `Status {Preloaded, Streaming, Done, GarbageCollecting}`**: idle files
  are reclaimed after `fileClearingPeriod`. Garbage collection waits for `readerCount == 0`.
- **[doc]** sfzformat.com opcodes: `offset` is "the offset used to play the sample", `end`, `delay`,
  `count`, `load_mode`/`load_start`/`load_end` (no description on the page), `sample_quality`. No
  preload or memory text on the page (WebFetch of `sfzformat.com/opcodes/`). The SFZ spec does not
  define a preload size; that is a player choice.

## 5. KONTRA now

| Aspect | Where | What it does |
|---|---|---|
| Head law | `crates/engine/engine/src/prepare.rs:164-187` `window()` | Kontakt-style law for **every** loaded zone, including non-DFD zones. `preload` from `instrument.preload`. |
| Which samples load | `prepare.rs:86-91` `load_samples` | Only groups with `!g.purged` at load. Heads for each sample = union of its zones' windows. |
| Store | `crates/engine/stream/src/store.rs:28-37, 136-170` | Process-wide, 64 MiB streamed page cache, 4096-entry request ring. Pinned pages keyed by `BlobKey` (file#offset, fmt), shared through `Weak`. |
| Page size | `crates/engine/stream/src/sample.rs:12` | 1024 frames. Kontakt's grain is 6144 bytes, so 1024 i24 stereo = 6144 bytes. Matches. |
| Format of heads | `file.rs:17-96` | Decoded packed PCM (i16/i24/f32). NCW is decoded at page read, `NcwSource`. Compressed heads are not kept. |
| Decode on read | `sample.rs:85-112` `read_page` | Pinned: decode from bytes. Streamed: cache slot. Missing: silence, `missing` count. |
| I/O | `store.rs:351-395` `serve` | One thread, `BinaryHeap` ordered by `(due, sample, page)`. `pread` through `Region::read_at`. No io_uring, no mmap. |
| Due times | `voice/src/lib.rs:22, 135-142` | Lookahead fixed at `LOOKAHEAD = 8192` output frames. All pages asked in one block get `due = now` (block start). EDF is therefore not per page. |
| Eviction | `store.rs:432-444` | Second-chance clock over slots. Pinned pages are not in the clock; they stay while any sample holds them. |
| Late data | `sample.rs:61-72`, `voice/lib.rs` | Silence for missing frames; cursor advances (timeline kept). Counted in `missing_frames()`. No wait, no fade. |
| Offsets | `prepare.rs:164-187` | `start_mod` (S.Mod) is used in the window. A start offset beyond S.Mod is not bounded here at `play_note` time (see §6). |
| Purge | `prepare.rs:86-91`; purge doc `docs/re/KONTAKT_PURGE_RESIDENCY.md` | Group-level purge. The doc's per-sample usage flags are not in `prepare.rs`. |

Documentation drift: `docs/architecture-v3/05-STREAM.md` §3 says "There is no `cold`, `holding_onset`…" but
`voice` still has a `missing` count; the rule is consistent. §1 "Memory bar" numbers are not in the
code.

### 5.1 Measurements on record ([receipt], PluginArtifacts/v3-gates/P1.2)

| Library | Instrument | Assets | Admitted payload | Budget | Note |
|---|---|---|---|---|---|
| Areia | 16 Violins Spiccato Fast | 528 | 106.2 MB | 1,547,541,888 B | `full_possible` phase |
| Vista | 4 Violas | 1,636 | 314.8 MB | same | |
| ANALOG STRINGS | | 138 | 159.4 MB | same | |

The phases in the receipts are all `full_possible`, which means the attempt that admits the whole set.
The `r6` receipt (`P1.2/r6/areia.txt`) reports 13 identical `stream_preload_attempt` records, so the
measured numbers are stable across retries. An older round (`lib-areia-before.txt`) shows a failing
attempt at `rss_kb 3,969,888` (about 3.8 GiB RSS, `hwm_kb`) with `admitted_payload_bytes` 1,547,541,888 at
the cap. The before/after receipts (`lib-areia-before/after.txt`) show RSS peak 288,284 KB after `translate_ksp_init`.

Note the receipt's own caveat: `"scope":"control-only candidate; requested frames are not decoded-byte counters;
failed attempts publish nothing"`. So these are control-plane numbers, not a whole-process RSS measurement.

### 5.2 Kontakt versus KONTRA, what can and cannot be compared

- Kontakt's per-zone head (60 KB/ch + 2×S.Mod) vs KONTRA's admitted bytes (above): not compared yet. The
  P1.2 gate compares "resident head bytes" to "sum of Kontakt windows", and its own numbers (Round 6 in
  05-STREAM.md) say Vista 314.8 vs 313.3 MB, Areia 106.2 vs 103.8 MB, ANALOG 159.4 vs 159.3 MB. Those
  Kontakt figures are the doc's, and no receipt in PluginArtifacts I could open backs them. Mark them
  [doc, unverified].

## 6. Gaps, ranked by impact

1. **Compressed resident heads (memory, high).** KONTRA keeps decoded i24 (6 bytes/frame). Kontakt keeps
   NCW compressed and decodes on read (manual: 30–50%). Expected saving 2× on NCW-sourced libraries.
   Measure: Areia/Vista/ANALOG RSS with NCW vs decoded heads on the same zones.
2. **Head law is applied to every zone, not only streamed ones (memory, high).** `prepare.rs:86-91` loads
   a head for **non-DFD** zones too, while Kontakt loads the whole sample for non-DFD groups. The head
   could be larger or smaller than Kontakt's resident set. Measure: count of resident bytes per
   library split by source mode 4 vs others.
3. **No per-page deadline (dropouts, high).** `voice/lib.rs:142` gives every page in a block `due = now`.
   The I/O thread serves in `(due, sample, page)` order, so the first page asked for a voice that starts
   later is served first only by sample id. Measure: `missing_frames` under a throttled disk with 16
   simultaneous 24-bit voices.
4. **Late data is silence with no fade (dropouts, medium).** KONTRA plays silence and keeps the timeline,
   which is a deliberate choice (05-STREAM §3). The Kontakt manual suggests a bigger buffer, which is the
   same knob. Missing: a quality-of-service policy (fade-in on resume, or hold the note).
5. **Start offsets are not bounded at play time (dropouts, medium).** KSP `play_note` ignores an offset larger than
   S.Mod in DFD mode (KSP manual line 3277). KONTRA's head does include S.Mod, but there is no equivalent
   guard. Verify `play_note` / `sample_offset` callers before assuming it is bounded.
6. **Single I/O thread, no io_uring / readahead (dropouts, medium).** `store.rs:351` is one thread. Kontakt
   has `numStreamingVoices` / `streamingBufferSize` options (binary strings). sfizz uses 4 background
   threads, HISE a 500-ms polling pool.
7. **No cross-instance sharing key beyond `BlobKey` (memory, low-medium).** `BlobKey = file#offset, fmt`
   dedups within one `Store`. Two plugin instances in one host process share the `Store` only if they
   share the `StreamClient` (check `kontra` plugin wiring; not verified here).
8. **Purge is group-level, not per-sample (memory, medium).** Kontakt's manual purges by usage flag per sample.
   KONTRA's `purge_group` releases whole groups.
9. **Load time (medium, unmeasured).** `load_samples` opens all samples in parallel, then `load_many` reads pages
   on all cores. There is no receipt for load wall time on Areia, Vista or ANALOG beyond the AUDIT
   stages in `lib-*.txt`. `translate_ksp_init` takes about 340 ms on Areia and 5 ms on Vista (receipt).

## 7. Gold standard (format-neutral, implementable)

**Head policy.** A zone's head is the window it needs, sized from the law, and never less than the
largest of: (a) the measured disk latency × sample rate (not a fixed 60 KB), (b) its S.Mod range plus one
block. Samples of non-streamed groups are whole and resident. Streamed zones get heads from the same law
only for their own frames.

**Resident form.** Store heads compressed (NCW-class lossless, or FLAC-class for other formats) and decode
per block into a small per-voice ring. Decoded PCM is a cache, not the store. Budget check: compressed
bytes × 2 ≤ Kontakt's decoded windows for the same zones (the P6.1c target in 05-STREAM is ≤60%).

**Store.** One process-wide store keyed by `(container identity, offset, length, codec)`. Pages are 1024
frames. Pinned pages and streamed pages share one eviction budget with an explicit total
(bytes, not slot count). Overflow is an error reported with the zone name (`budget exceeded: zone X needs
N MB, budget M MB`); no silent truncation.

**I/O.** A pool of `N` (default 2, up to physical cores) reader threads reading `pread` or io_uring
(Linux) / overlapped read (Windows). Requests carry a deadline in output frames equal to the sample time the
page is needed by. Earliest-deadline-first. Readahead size = max(LOOKAHEAD, disk latency × rate). Per-page
deadlines, not per-block.

**Late data.** Output silence for missing frames, timeline kept, `missing_frames` counted per voice and
per group, and a diagnostic that names the group. An optional `hold` policy (per instrument, default off)
lets a voice wait up to a bounded time and then fade in, never a silent start.

**Start offsets.** Clamp `play_note` offset to the zone's S.Mod range in DFD mode (match the KSP manual,
since scripts depend on it). For non-DFD samples, a start offset is a plain seek and needs no head growth.

**Offline bounce.** `Store::settle` (exists) is the only wait; it is allowed offline only. Bounce must give
the same frames as realtime (the P1.2 test does this).

**Sharing.** Heads and full samples are shared by blob identity across instances in one process. A
second instance with the same blob costs 0 bytes.

**Measurement gates.** (1) Resident bytes per library ≤ 60% of Kontakt windows, measured on Areia,
Vista, ANALOG with the same zones (RSS and admitted bytes, both). (2) Zero `missing_frames` on C2 with a
disk throttled to the reference speed. (3) First-frame lands at the scheduled sample on C1. (4)
Load wall time within 1.2× of Kontakt on the same instrument (measure with the AUDIT stages).

## 8. Open items for the coordinator

- Attach a per-library zone-count dump (`ir_json`) to resolve the 8192 / 1,056 disagreement.
- Measure Kontakt's Memory meter for one NCW library with and without the compressed box (§1.4).
- UVI streaming policy: find UVI's own docs or a UFS spec. The local UVIScript folder is empty.
- Parse `MODO DRUM *.pak` and `EZDrummer` and `Ample` containers before saying how they are streamed. Not done.
- Dolce, Chorus, Pacific, Shreddage, Una Corda, Afflatus, Morphology: no receipt yet. Their
  folder sizes (Kontakt: 3.2–109 GB) are the reason the store must be bounded.
