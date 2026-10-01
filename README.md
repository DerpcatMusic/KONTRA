# KONTRA

KONTRA is a sampler that loads instruments in the Kontakt format. It is
written in Rust by Matari Audio on the [MOOSE](https://github.com/Matari-Audio/moose)
plug-in framework, and runs as a CLAP or VST3 plug-in or as a standalone
application.

It is an independent learning and research project: a study of how a
large sample-based instrument is put together, from file formats and
disk streaming to scripting and effects. It is not affiliated with or
endorsed by Native Instruments, and it is not a replacement for Kontakt.
Compatibility is measured against real instruments. A preset that loads
is not proof that it sounds as it does in Kontakt.

KONTRA ships no instruments, samples, scripts or artwork. You need your
own, legally obtained libraries.

## Features

- Reads NKI instruments and NKM multis: groups, zones, key and velocity
  maps, crossfades, tuning, loops, release triggers and voice groups.
- Samples in WAV, AIFF and NCW, as loose files or members of NKX/NKR
  containers.
- Disk streaming: each sample keeps a short preload in memory and streams
  the rest, within a memory budget per instrument and per rack.
- A rack of up to 16 instruments, each with its own MIDI port and
  channel, audio output, level and pan. Four MIDI inputs and eight stereo
  outputs are declared to the host.
- A KSP script engine for performance scripts (callbacks, events and
  instrument user interfaces), with diagnostics for what it does not
  support.
- Group modulation (envelopes, controllers, script modulation), group filters and
  EQs, and the common Kontakt effects.
- A native editor: library browser with each library's own artwork,
  mapping, groups, mixer, spectrum and an on-screen keyboard.
- A command-line tool (`kontakto`) to inspect, audit and render
  instruments offline.

Missing or damaged samples are skipped and reported; KONTRA never
modifies library files.
<!-- private:start -->

### Encrypted library content

Some libraries encrypt their presets and sample containers. The
`library-access` Cargo feature reads the library's own access data from
its `.nicnt` and decrypts that content in memory. It is off by default;
without it, encrypted content fails to load with "Encrypted library
content is not supported in this build." The nightly builds of this
repository enable it. Build it into a local install with:

```sh
cargo moose install --clap --vst3 --user
cargo run --release --features standalone --bin kontakto-standalone
cargo run --release --bin kontakto -- bench-load '/path/to/instrument.nki'
```
<!-- private:end -->

## Plug-in formats

| Format | Linux | macOS | Windows |
|---|---|---|---|
| CLAP | `KONTRA.clap` | `KONTRA.clap` bundle | `KONTRA.clap` |
| VST3 | `KONTRA.vst3` folder | `KONTRA.vst3` bundle | `KONTRA.vst3` folder |
| Standalone | `kontakto-standalone` | `kontakto-standalone` | `kontakto-standalone.exe` |

## Building

You need a stable Rust toolchain (edition 2024) and `cargo-moose`, the
MOOSE build tool, installed from the same MOOSE revision that
`Cargo.toml` pins:

```sh
cargo install --locked --git https://github.com/Matari-Audio/moose \
  --rev bffa4677d0b82119d38566ce7e932dc5c463d497 cargo-moose
```

### Linux

Install the X11, OpenGL, ALSA and JACK development packages. On Debian or
Ubuntu:

```sh
sudo apt-get install build-essential pkg-config libx11-dev libx11-xcb-dev \
  libxcb1-dev libxcb-icccm4-dev libxcursor-dev libxkbcommon-dev \
  libxkbcommon-x11-dev libxrandr-dev libgl1-mesa-dev libvulkan-dev \
  libasound2-dev libjack-jackd2-dev
```

### macOS

Install the Xcode command-line tools (`xcode-select --install`).
Locally built bundles are signed ad hoc. They are not notarized, so
macOS may quarantine downloaded builds; clear that with
`xattr -dr com.apple.quarantine KONTRA.clap KONTRA.vst3`.

### Windows

Install Visual Studio Build Tools with the "Desktop development with
C++" workload.

### Build and install

```sh
cargo moose install --clap --vst3 --user     # plug-ins into your user plug-in folders
cargo moose build --clap --vst3              # bundles into target/bundles/ only
cargo run --release --features standalone --bin kontakto-standalone
cargo run --release --bin kontakto -- inspect '/path/to/instrument.nki'
cargo run --release --bin kontakto           # lists every subcommand
```

`cargo moose` builds x86_64 plug-ins for x86-64-v3 (AVX2) by default;
pass `--target-cpu baseline` for older CPUs.

### Development checks

```sh
bacon                       # compilation while editing; no tests
bacon parity                # explicitly run three focused KSP regressions
```

Both jobs use an isolated `artifacts/check` cache. The parity job uses
`cargo-nextest`; install bacon and cargo-nextest if they are missing.
Tests that need locally installed libraries remain marked `#[ignore]`.

## Nightly builds

A scheduled workflow builds CLAP, VST3 and standalone binaries for
Linux x86_64, macOS (arm64 and x86_64) and Windows x86_64 whenever there
are new commits, and publishes them as the `nightly` pre-release.
Nightlies are untested snapshots.

## License

Apache-2.0; see [LICENSE](LICENSE) and [NOTICE](NOTICE). Third-party
components and their licenses are listed in [THIRD_PARTY.md](THIRD_PARTY.md).

## Disclaimer

KONTRA is provided AS IS, without warranty of any kind, and the authors
are not liable for any damages arising from its use. Kontakt, Kontakt
Player and NKI are trademarks of Native Instruments GmbH, used here only
to describe file compatibility. Use KONTRA only with libraries you are
licensed to use, and within the terms of those licenses.


## Performance hardening branch, 2026-10-01

`codex/performance-hardening` implements priorities 1–6 from the [replacement review](audits/REPLACEMENT_REVIEW.md): shared streaming workers, positional archive reads and bounded decoded-block reuse; compact script persistence; host-block script budgets; prepared first-use callback storage; bounded loader work and cancellation; and dependency-aware parsed/resident caches. The `87a18dc` SIMD/library-access work is merged into this branch (`cd22741`), with runtime dispatch retained for newer SIMD instructions.

Four streaming workers serve all banks and instances of the same loaded plugin module. Readers and decoded blocks are bounded, and the last bank shuts workers down before plugin unload. Loading phases use at most four decode workers across instances. Cancellation is checked between import/script stages and during sample reads; it cannot interrupt arbitrary parser work or an in-progress filesystem read.

Live script work shares a block allowance across MIDI/render segments and scales with frames, sample rate and scripted parts. Oversized synchronous array operations and exhausted prepared callback storage report diagnostics. Preparation caps extra text storage at 16 MiB per runtime and 4 MiB per slot's string variables; individual callback strings are limited to 64 KiB. These are explicit compatibility limits, not permission to silently allocate on the audio thread. Cache validation uses size/mtime metadata for presets, samples/archives, resources, impulses, library metadata and searched directories; edits that preserve both size and timestamp require explicit cache clearing.

Build profiles preserve panic unwinding, which the importer and plugin boundary use. `dev` has line-table debug information with dependency debug information disabled; `debugging` restores full debug information, and `dsp-dev` uses LLVM optimization level 1. `release` uses ThinLTO and stripped symbols. `thin` and `maxperf` allow measured ThinLTO/fat-LTO comparisons; `minsize` uses optimization level `s` with unwinding retained. None forces the build machine's CPU instructions on customers.

```sh
cargo build --profile dsp-dev
# Optional Linux linker configuration; requires clang and mold on PATH:
cargo --config .cargo/fast-linux.toml build --profile thin
cargo --config .cargo/fast-linux.toml build --profile maxperf

# Plugin callback benchmark: duration is per idle/playing/tail phase.
cargo run --release --bin kontakto -- bench-host 3 8 /path/to/patch.nki --frames=64 --rate=48000
# Repeat the patch path for a multi-part workload. Reports deadline misses,
# stream/command dropouts, voice counts, CPU, RSS and script diagnostics.

# Keep normal iteration to compilation; select focused regressions explicitly:
bacon
bacon parity
```

Pinned nightly Cranelift failed a `catch_unwind` smoke test on this machine; the same source passed with LLVM. It is therefore not enabled for plugin development. `tools/pgo.sh build` now trains from an explicit `KONTRA_PGO_PRESETS` file (one preset path per line), records results, fails on requested training-load errors, and rejects cross-target training. It defaults to portable CPU code and does not install plugins unless `install` is explicitly requested. PGO gains still require comparable playback and holdout measurements; the script does not guarantee a percentage. BOLT, nightly dependency hints and size-first standard-library builds remain experiments, rather than default shipping settings. Compiler guidance: [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html), [build performance](https://doc.rust-lang.org/cargo/guide/build-performance.html), and [rustc PGO](https://doc.rust-lang.org/rustc/profile-guided-optimization.html).


The browser has separate library and preset searches, persistent custom ordering and pins, recent/vendor/name sorting, and a folder tree with breadcrumbs and keyboard navigation. Both library and preset panes render visible rows. Mouse-wheel notches move three rows; trackpad pixel scrolling retains its normal distance.

Each rack part's view menu selects **Original**, **Vectorized**, or **KONTRA**. Original uses authored coordinates, wallpaper frames and bitmap controls; Vectorized retains that layout with native faces; KONTRA reorganizes the controls. The app settings choose the default view, scale and optional vectorized wallpaper. Controls still run their KSP callbacks. This is partial legacy-KSP rendering, with unsupported widgets/resource failures diagnosed; universal Kontakt UI parity and Komplete UI remain unfinished.

`_read_persistent_var` uses the existing persistence implementation. Scripts with callbacks but no `on init` can compile. `get_voice_limit`/`set_voice_limit` now report the unavailable Time Machine Pro engine instead of aborting initialization, and `attach_level_meter` preserves valid initialization with an explicit unsupported-meter diagnostic. Those last two changes do not implement time stretching or connected meters. Large UI snapshots and FX construction run outside the editor's shared view lock. These fixes address reproduced causes; they do not certify Damage, NOVO, Metropolis Ark or the tester's Output libraries without playable copies.

The core KSP port now preserves MIDI channels through waits, generated notes,
releases and queued audio commands. It implements global `on ui_controls`
callbacks with `$NI_UI_ID`, custom control IDs, 16 custom event parameters,
indexed integer/string/real control values, widget type/identifier queries,
menu count/selected-index queries, multiline labels, and nested-loop `continue`.
Live menu rows can hide/reappear using prepared snapshot buffers; a hidden
selected row stays visible until selection changes. Modern `get_group_idx`,
`get_mod_idx` and `get_target_idx` return `$NI_NOT_FOUND`; deprecated `find_*`
retain their zero fallback. `get_engine_par_disp_ext` uses the existing value
laws without changing DSP, and `set_rpn`/`set_nrpn` reach subsequent script slots.
Array value access does not imply an XY/table renderer or indexed artwork and
automation metadata support. Broader KSP coverage remains active work; see the
[remaining compatibility gaps](audits/REPLACEMENT_REVIEW.md).

The working `kontakt-parity` branch through `824a8a7` is integrated into
`codex/performance-hardening`. The combined runtime retains bounded audio work,
channel ownership and chunked snapshots, and adds real zone counts for
`$NUM_ZONES` and prepared/recycled key-range text during playback. Scripts can
load convolution IRs at initialization; dynamic IR replacement remains pending.
