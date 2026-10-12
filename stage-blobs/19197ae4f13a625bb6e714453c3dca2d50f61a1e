# Engine CPU, 2026-10-01

Ryzen 7 7800X3D (Zen 4: AVX-512 F/BW/VL/VBMI, 512-bit ops double-pumped, 1 MiB L2), `--release --features library-access`. The host ran other builds throughout, so cycles and wall time are noisy: instructions per block (`perf stat` inside `bench-host`) are the stable measure, and cycle claims come from `perf record -e cycles:u` sample counts on the audio thread or from minimum-of-N micro-benchmarks.

- **Mega Brass** is `bench-host 10 16` on Afflatus Chapter II Brass: about 732 voices, 226 of them audible and all filtered.
- **Areia** is the 16 Vlns + 10 Vls Col Legno patch, with 205 voices.
- **Synthetic** is `bench <voices> 24 <layers>`.

Every kept change is bit-exact, except the filter dots regrouping, which stays inside the existing -120 dBFS shared-filter test (see below).

## Kept

| Technique | Where | Measure | Before | After |
|---|---|---|---|---|
| I16/I24 accumulate as flat AVX2 intrinsics | `Pcm::accumulate_avx2` | synthetic 10k x 10, instructions | 106.8 G | 93.5 G |
| + AVX-512 accumulate | `Pcm::accumulate_avx512` | synthetic 10k x 10, instructions | 93.5 G | 84.5 G |
| `#[inline(always)]` on `Voice::plan`, `Lanes::add`, `VoiceFilter::follow`/`hold` | engine | Mega Brass, instructions/block | 9.33 M (outlined) | 9.18 M |
| Envelope stop edge: bisect the level once per stage, then step 8 frames at a time without per-frame stop tests | `Envelope::edge`, `affine_until` | Mega Brass, instructions/block | 9.180 M | 8.907 M |
| AVX-512 cubic mix, 16 frames per pass | `mix_avx512` | Mega Brass, instructions/block | 8.907 M | 8.42 M |
| AVX-512 cubic mix | `mix_speed`, step 0.53 / 1.87 | ns per call | 125 / 135 | 117 / 126 |
| A muted voice rests its filter once, not every block | `Voice::render` | L1-miss samples, render path | 16.1 k | 12.5 k |
| A lone filtered voice sums into its run inside the AVX2 dots pass | `LaneFilter::dots_out` | Mega Brass, instructions/block | 8.097 M | 7.908 M |
| Lone-voice filter dots in 4 interleaved chains (was 1 add chain per channel, latency-bound) | `LaneFilter::dots_out_body` | `dots_out` cycles samples | 14.3 k | 10.6 k |
| AVX-512 VBMI packed decoder: byte permute, variable shifts, lane prefix sums | `integrate_avx512` | `decode_speed`, per 132-frame window | 178 ns | 130 ns |
| AVX-512 VBMI packed decoder | `integrate_avx512` | Mega Brass / Areia, instructions/block | 7.833 / 1.920 M | 7.596 / 1.834 M |

The filter dots regrouping only changes how the sum is rounded. `shared_filters_match_voices_filtered_alone_within_120_db` keeps its maximum error at -132.45 dB, the same as before. The test does see these dots: scaling them by 1 + 1e-5 fails it at -110.7 dB.

## Dropped

| Technique | Result | Why dropped |
|---|---|---|
| Fat LTO + `codegen-units = 1` | synthetic +1.1% instructions; Mega Brass flat; cycles within noise | No win, and 1.7 GB of artifacts |
| `panic = "abort"` | Not possible | `import.rs` uses `catch_unwind`, and the vendored CLAP/VST3 firewall catches panics |
| Hot-first `#[repr(C)]` Voice layout | L2/L3 miss counts unchanged | Muted voices already touch few lines once their filter rests |
| Generic flat I24 loop (no intrinsics) | 197 G instructions, against 107 G | LLVM scalarized it |
| PGO, linker, build profiles | Not measured here | Build settings belong to the coordinator |

## Totals (merged `kontakt-parity` baseline against this branch)

| | Before | After |
|---|---|---|
| Mega Brass, instructions/block (mean) | 8.915 M | 7.548 M (-15%) |
| Mega Brass, instructions/block (p99) | 15.9 M | 13.8 M |
| Mega Brass, cycles per voice-frame | about 8.5 | about 7.7 (noisy) |
| Areia, instructions/block (mean) | 2.011 M | 1.833 M (-9%) |
| Areia, instructions/block (p99) | 3.61 M | 3.34 M |
| Synthetic 1k, instructions | 13.80 G | 10.09 G |
| Synthetic 1k, best time | 0.115 s | 0.100 s |
| Synthetic 10k x 10, instructions | 107.96 G | 80.95 G |
| Synthetic 10k x 10, best time | 1.042 s | 0.912 s |

`audit-libraries`: 8 of 8 libraries ok, 0 underruns (`LIBRARIES.md`).

## What is left

- `Engine::render` is memory-bound. Its hot lines are `LaneFilter::begin` reading each voice's filter sections, the `Lanes::add` table probe, and the plan's f64 end division.
- The voice array (1128 B per voice, about 1.1 MB) is larger than L2.
- Packed decode still decodes each block from its start. A window that starts mid-block pays for the skipped frames.
