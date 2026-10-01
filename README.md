# KONTRA

KONTRA is a Rust sampler for instruments that use the Kontakt file format. It runs as a CLAP or VST3 plug-in and as a standalone application, using the MOOSE plug-in framework.

KONTRA is an independent project. It is not affiliated with or endorsed by the owner of Kontakt, and it is not a replacement for Kontakt. Loading a preset does not mean that its controls, scripts, routing, or sound match Kontakt.

KONTRA ships no instrument libraries or sample collections. Use libraries you are licensed to use, under their license terms.

## Compatibility and known gaps

**Full** means verified end to end for the named behavior. **Partial** means only a subset is implemented or exercised. **Unsupported** means the feature is not implemented. **Untested** means there is not enough reference or host testing to make a claim. No broad compatibility area below is marked full.

| Area | Status | Implemented | Known limits |
|---|---|---|---|
| Preset parsing | **Partial** | Reads NKI instruments and NKM multis, including common groups, zones, key and velocity maps, loops, tuning, release triggers, and voice groups. | Unknown or unsupported structures may be skipped or rejected; parsing is not proof of correct behavior. |
| Sample access | **Partial** | Reads WAV, AIFF, and NCW samples from loose files and NKX/NKR containers. The `library-access` feature uses access data supplied by compatible libraries. | Missing, damaged, unsupported, or inaccessible samples can prevent or degrade playback. The feature is enabled in Cargo's default feature set, but this does not cover every protected library. |
| Audio and streaming | **Partial** | Sample playback, disk streaming, MIDI routing, and multiple outputs are implemented. Targeted playback tests and representative local patch checks exercise these paths. | No comprehensive Kontakt reference renders or real-DAW certification; timing and sound can differ. |
| KSP scripts | **Partial** | Selected initialization, note, release, controller, and UI callbacks and commands run in the built-in script engine. Unsupported calls are diagnosed. | Many KSP services and newer APIs are absent, including note-controller and MIDI-input callbacks, sample/zone editing, and complete asynchronous file and effect loading. |
| Instrument interface | **Partial** | The editor includes a library browser, mapping, groups, mixer, spectrum, keyboard, and supported imported controls. Parts can use Original, Vectorized, or KONTRA views. | Table, XY, waveform, file-selector and meter widgets, Komplete UI, and some original font and layer behavior are unsupported. |
| Effects and modulation | **Partial** | Selected group filters, EQ, effects, and modulation paths are imported and processed. | Some effect and modulation types are skipped or pass through. Parameter laws and sound have not been validated against Kontakt. |
| Articulations | **Partial** | Articulation mappings and channel-aware MIDI and script event routing are implemented. | Library-specific transitions, scripts, and every articulation have not been exhaustively checked. |
| Kontakt parity | **Untested** | No compatibility guarantee is made. | A successful load or short render is not a reference comparison. |
| Other sampler formats | **Unsupported** | None. | KONTRA does not load separate proprietary formats such as UVI, Toontrack, IK, or Ample Sound libraries. |

### Plug-in build targets

The repository's nightly workflow builds these targets. A build target does not certify operation in every host.

| Platform | CLAP | VST3 | Standalone |
|---|---|---|---|
| Linux x86_64 | Yes | Yes | Yes |
| Windows x86_64 | Yes | Yes | Yes |
| macOS arm64 | Yes | Yes | Yes |
| macOS x86_64 | Optional workflow build | Optional workflow build | Optional workflow build |

macOS bundles are signed ad hoc and are not notarized.

## Build

Install stable Rust and the pinned `cargo-moose` build tool:

```sh
cargo install --locked --git https://github.com/Matari-Audio/moose \
  --rev bffa4677d0b82119d38566ce7e932dc5c463d497 cargo-moose
```

On Linux, install the X11, OpenGL, Vulkan, ALSA, and JACK development packages. On macOS, install the Xcode command-line tools. On Windows, install Visual Studio Build Tools with the C++ workload.

```sh
cargo moose build --clap --vst3
cargo build --release --features standalone --bin kontakto-standalone
cargo run --release --bin kontakto -- inspect '/path/to/instrument.nki'
cargo run --release --bin kontakto
```

`cargo moose install --clap --vst3 --user` installs the plug-ins into user plug-in folders.

## License

Project-authored code is offered under Apache-2.0; see [LICENSE](LICENSE) and [NOTICE](NOTICE). Vendored and other third-party components keep their own terms. See [THIRD_PARTY.md](THIRD_PARTY.md), including the unresolved licensing note for `vendor/ni-file`, before redistributing this repository.

## Disclaimer

KONTRA is provided AS IS, without warranty of any kind. To the fullest extent permitted by law, the authors and contributors are not liable for damages arising from its use. See [LICENSE](LICENSE) for the full terms. Kontakt is a trademark of its owner; use of the name here describes file compatibility only. The owner does not sponsor or endorse KONTRA. Use only libraries you are licensed to use.
