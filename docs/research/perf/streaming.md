# Perf research lane: streaming, decode, memory layout

Status: research only. No engine change landed. The before/after measurement the lane
requires was not possible on this machine (see Blockers).

## Blockers (measured, not assumed)

- `sampler-perf` needs the real corpus. `KONTRA_KONTAKT_LIBRARIES` and
  `KONTRA_UVI_LIBRARIES` are unset and the default roots `/mnt/MAIN_STORAGE/Libraries/{Kontakt,UVI}`
  do not exist. The only `.nki` on the box is in a `/tmp` scratch dir and was not used.
- Host load average was 34 on 16 cores at run time. Wall-clock timings would be noise. The
  `perf stat -e instructions,cycles` path is still the only usable counter.

## Metric (defined, not yet measured)

CPU cost of one plain sample voice (no filter, linear or cubic interpolation, amp envelope,
stereo out, resident sample), per 64-frame block at 48 kHz, reported in µs/voice/block and
ns/frame, single thread, niced. Measure at 1, 16, 64 and 256 simultaneous voices. Every
future claim needs a before/after on this metric.

## Code facts (from `crates/sampler-kontakt/src/stream.rs` and `crates/sampler-core`)

- `stream.rs:48` read buffer for the file reader is `BUFFER = 16 << 10`. Each `Bytes::read`
  refills at most 16 KiB, so decode of compressed sources runs in 16 KiB steps.
- `stream.rs:738` head budget `DEFAULT_HEAD_BUDGET = 256 << 20`; `stream.rs:742`
  `PAGES_PER_VOICE = 3` sets the per-voice head page count. `head_frames(latency, rate, policy)`
  at `stream.rs:747` derives head size from latency and rate.
- `stream.rs:2385-2388` reader cache: `OPEN_READERS = 16`, `MAX_CACHED_READERS = 128`,
  `READER_CACHE_BYTES = 8 << 20`.
- `stream.rs:557` and `:595`: NCW reads go in `FRAMES_PER_BLOCK` units (`ncw::NcwReader`).
- `crates/sampler-core/src/resample/cubic_block.rs:9-27`: `cubic_four` computes in `f64` lanes
  and calls `std::hint::black_box` on the result inside the hot loop. That blocks vectorisation
  and is a candidate for the voice-SIMD lane, not this one.
- `crates/sampler-core/src/render.rs:836` `render_source_voice` is the per-voice entry. It does
  per-call expression, plan, filter-bank and delay lookups before any sample is read. That
  fixed overhead matters more than the read path for a resident sample.

## Sources

1. sfizz, "An Open-source SFZ Engine/Player in development" (KVR forum, developer post on
   preload): preload sets RAM used by disk streaming; larger values trade memory for
   performance; `hint_ram_loading` forces RAM load. https://www.kvraudio.com/forum/viewtopic.php?p=8155293
2. sfizz man page, `sfizz_jack(1)`: `--preload_size` is bytes preloaded in cache for samples.
   https://man.archlinux.org/man/extra/sfizz-standalone/sfizz_jack.1.en
3. HISE forum, streaming buffers: per-voice two swapping streaming buffers plus a preload
   buffer for the sample start. This is HISE's design, not sfizz's. https://forum.hise.audio/post/7
4. VSL forum post, Synchron Player preload: preload set from measured drive speed; under
   10 MB/s uses 16384 samples, faster drives go down to 2048. https://forum.vsl.co.at/post/281000
5. Spitfire Audio support, DFD preload: default 60 KB, SSD can run near 6 KB, HDD needs more.
   https://support.spitfireaudio.com/en/articles/11815484
6. Prominy, "About Products / preload": preload is per instrument or global; lower values cut
   load time and RAM. https://prominy.com/about-products/preload/
7. Native Instruments technical PDF (DFD memory): each sample gets its own preload buffer, so
   memory is roughly sample count times buffer size; streaming pool is separate.
   https://support.native-instruments.com/hc/de/article_attachments/205618585
8. Audio Imperia FAQ, "What does DFD stand for?": page title only; the expansion was not
   read in this pass.
   https://help.audioimperia.com/docs/general-information/faq/what-does-dfd-stand-for/
9. Linux `posix_fadvise(2)` man page: `WILLNEED` is a non-blocking read into page cache, may
   read less under memory pressure; `SEQUENTIAL` doubles readahead; `DONTNEED` evicts after
   fsync. https://manpages.karlosoft.com/2.78/man2/fadvise64/
10. LKML 2010 report: `WILLNEED` looked blocking on ext3, kernel 2.6.36. One user's
    observation, old kernel. https://lkml.rescloud.iu.edu/1012.0/01930.html
11. LKML 2019 patch "mm: Support madvise_willneed override by Filesystems": `madvise_willneed`
    mirrors `fadvise_willneed` and routes through filesystem hooks. https://lkml.iu.edu/hypermail/linux/kernel/1907.0/01987.html

Not yet checked: Kontakt 8 DFD xrefs via REA (`find_xrefs_to_name`, `procedure_references`,
`read_bytes`), io_uring, O_DIRECT, Windows IOCP, NCW/FLAC/WavPack decode cost. The REA search
path was not run in this pass.

## Adopt / reject so far

- Adopt for next step: measure the head-size math. Sources 4, 5, 6 say preload should scale
  with drive speed, not be fixed. Our `PAGES_PER_VOICE = 3` and 256 MiB budget are fixed.
  Test: late-read count versus head size on a slow and a fast disk.
- Reject for now: `mmap` plus `MADV_WILLNEED`. Source 9-11 give no evidence it beats a
  bounded read on sequential streaming, and it adds page-fault jitter on the audio thread.
- Reject: copying any sfizz, HISE or GPL code. Only the ideas are recorded here.

## Next steps

1. Point `KONTRA_KONTAKT_LIBRARIES` at a real corpus, then run `sampler-perf run --only
   streaming-sweep` once the box is idle. Record the metric above.
2. Add a `#[ignore]` micro-bench for `render_source_voice` on a resident sample, counting
   instructions via `perf stat`.
3. Only then prototype the head-size change in `stream.rs` with late-read and decode counters.
