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
cargo moose install --clap --vst3 --user --features library-access
cargo run --release --features standalone,library-access --bin kontakto-standalone
cargo run --release --features library-access --bin kontakto -- bench-load '/path/to/instrument.nki'
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

### Tests

```sh
cargo test --release
```

Tests that need locally installed libraries are marked `#[ignore]`.

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
