# KONTRA

KONTRA is a Rust sampler for instruments that use the Kontakt file format. It runs as a CLAP or VST3 plug-in and as a standalone application, using the MOOSE plug-in framework.

KONTRA is being built as an independent alternative to Kontakt. It is not affiliated with or endorsed by the owner of Kontakt. Compatibility remains incomplete: loading a preset does not establish that its controls, scripts, routing, or sound match Kontakt.

KONTRA ships no instrument libraries or sample collections. Use libraries you are licensed to use, under their license terms.

## Download the latest nightly

[![Windows x64](https://img.shields.io/badge/Windows-x64-0078D4?style=for-the-badge)](https://github.com/DerpcatMusic/KONTRA/releases/download/nightly/KONTRA-nightly-windows-x86_64.zip)
[![macOS Apple silicon](https://img.shields.io/badge/macOS-Apple_silicon-222222?style=for-the-badge&logo=apple)](https://github.com/DerpcatMusic/KONTRA/releases/download/nightly/KONTRA-nightly-macos-arm64.zip)
[![macOS Intel](https://img.shields.io/badge/macOS-Intel-555555?style=for-the-badge&logo=apple)](https://github.com/DerpcatMusic/KONTRA/releases/download/nightly/KONTRA-nightly-macos-x86_64.zip)
[![Linux x64](https://img.shields.io/badge/Linux-x64-168B76?style=for-the-badge&logo=linux&logoColor=white)](https://github.com/DerpcatMusic/KONTRA/releases/download/nightly/KONTRA-nightly-linux-x86_64.zip)

Each ZIP contains the **CLAP plug-in, VST3 plug-in and standalone application**. These fixed links always serve the latest complete [nightly pre-release](https://github.com/DerpcatMusic/KONTRA/releases/tag/nightly). At 03:23 UTC daily, new build-relevant changes on public `main` start a snapshot on GitHub-hosted runners; maintainers can also run Nightly manually. The exact source commit must pass release-profile tests and native compile checks before all four packages build. Downloads update together after all builds, archive checks and uploads succeed. Documentation-only changes do not trigger scheduled rebuilds. The links become available after the first successful publication. See [build progress](https://github.com/DerpcatMusic/KONTRA/actions/workflows/nightly.yml) and the release notes for the source commit.

Nightlies are experimental snapshots. Linux builds use Ubuntu 24.04 and require compatible system libraries. The x86_64 plug-ins require **AVX2, FMA and BMI2**. macOS builds are signed ad hoc and are not notarized; after extracting the ZIP, remove quarantine from the downloaded files if macOS blocks them:

```sh
xattr -dr com.apple.quarantine KONTRA.clap KONTRA.vst3 kontakto-standalone
```

## Compatibility and known gaps

**Full** means verified end to end for the named behavior. **Partial** means only a subset is implemented or exercised. **Unsupported** means the feature is not implemented. **Untested** means there is not enough reference or host testing to make a claim. **Experimental** means limited validation and an evolving interface. No broad compatibility area below is marked full.

| Area | Status | Implemented | Known limits |
|---|---|---|---|
| Preset parsing | **Partial** | Reads NKI instruments and NKM multis, including common groups, zones, key and velocity maps, loops, tuning, release triggers, and voice groups. | Unknown or unsupported structures may be skipped or rejected; parsing is not proof of correct behavior. |
| Instrument creation and preset writing | **Partial** | Creates new instruments from WAV/AIFF samples, with key/velocity mapping, loops, tuning and round robins. Writes KONTRA instruments, SFZ and limited unencrypted NKI presets; generated NKI mappings, scripts and playback have targeted readback checks. KONTRA also saves its own state and multis. | The format APIs preserve raw NIS containers and Kontakt chunks, including unknown metadata and opaque encrypted bodies; caller keys support extraction and subtree encoding. Semantic editing of arbitrary imported presets and checksum regeneration remain unsupported. NKM creation is not implemented; generated files have not been validated in Kontakt. |
| Sample access | **Partial** | Reads WAV, AIFF, and NCW samples from loose files and NKX/NKR containers. The `library-access` feature uses access data supplied by compatible libraries. | Missing, damaged, unsupported, or inaccessible samples can prevent or degrade playback. The feature is enabled in Cargo's default feature set, but this does not cover every protected library. |
| Audio and streaming | **Partial** | Sample playback, disk streaming, MIDI routing, and multiple outputs are implemented. Targeted playback tests and representative local patch checks exercise these paths. | No comprehensive Kontakt reference renders or real-DAW certification; timing and sound can differ. |
| KSP scripts | **Partial** | Selected initialization, note, release, controller, and UI callbacks and commands run in the built-in script engine. Release callbacks honor their selected group. Waiting callbacks retain their events through key-up and pool reuse, so late following notes receive their release. Conditional regions are excluded before parsing, condition symbols carry across successful slots and compiled caches, and ranged array search is supported. Native sustain/release flags let scripts control those behaviors without duplicate system actions. Unsupported calls are diagnosed. | Many KSP services and newer APIs are absent, including note-controller and MIDI-input callbacks, sample/zone editing, and complete asynchronous file and effect loading. |
| Instrument interface | **Partial** | The editor includes a library browser, mapping, groups, mixer, spectrum, keyboard, imported controls, and zone waveforms. Browser row offsets, counts and cursor lookup are cached; empty searches remain cached. Original uses library artwork; Vectorized retains its background/layout with KONTRA knob and fader faces; KONTRA reorganizes controls. | Table, XY, file-selector and connected meter widgets, Komplete UI, and some original font and layer behavior remain incomplete or unsupported. |
| Timing alignment | **Partial** | Timing is measured off the audio thread once per patch and program, with separate first-note and legato values by velocity. Source validity, manual offsets, and per-part exclusion govern use of the measurements. | Edits at the same path do not invalidate saved measurements automatically; choose **Measure again** after changing samples or scripts. |
| Effects and modulation | **Partial** | Selected group filters, EQ, effects, and modulation paths are imported and processed. Scripted instrument-rack filter, EQ, Stereo Modeller and subtype controls update live DSP and support readback. | Some effect and modulation types are skipped or pass through. Parameter laws and sound have not been validated against Kontakt. |
| Convolution IR selection | **Partial** | Cotton's Vintage/Room menus, Size stretching and Distance predelay work through worker-built kernels. Selected IRs and requested controls survive DAW and JSON state reload, including a sample-rate change. | Unequal early/late sizes use a diagnosed uniform stretch; separate boundaries, filters, reverse and unknown saved flags remain unmapped. Other responses and Kontakt sound equivalence remain untested. |
| Articulations | **Partial** | Articulation mappings and channel-aware MIDI and script event routing are implemented. Scripted Channel mode tracks physical input separately, supports independent same-pitch releases, and honors release-callback group selection. | Library-specific transitions, scripts, and every articulation have not been exhaustively checked. |
| MIDI and pedals | **Partial** | Scripted and unscripted Channel mode release each physical channel's own voices, including identical pitches and all-notes-off. Sostenuto captures individual voices. Pedal-deferred releases retain each event's groups and velocity. Aligned channel stops clear held-note bookkeeping and keep fresh notes behind queued stops. Channel-mode CC120 cancels only the selected physical input, including delayed callbacks, generated notes, queued fades and native release samples, even after script channel reassignment. Sound-off cancels delayed MIDI callbacks; Panic and host reset also clear pending alignment, held input and stale script pedal/bend/pressure state while preserving edited controls. Note-offs, pedal-up and stopping fades have reserved command storage; extreme stop overflow performs a counted, click-free channel cut. | Broader DAW/device behavior remains under review. UI, listener and asynchronous service callbacks remain active after Panic. |
| MPE | **Partial** | Both zones support independent member expression through reuse, delayed releases and queued attacks. Initial pressure reaches scripts and raw modulation. Initial host gain/pan applies from the first rendered frame. Member CC74/pressure freeze at key-up; manager CC74 adds (clamped), pressure uses the maximum. Master pedals, common controllers and combined master/member bend work through scripts; common controllers run one callback. Negotiated manager/member bend sensitivities include cents and update held notes; manager-channel notes also honor the negotiated range. Explicit MPE configuration restores default sensitivities. | Broader negotiation, pressure fallback/mapping policy and combined MPE/channel articulation remain incomplete. |
| Release-event identity and timing | **Partial** | Script-transposed input notes and following-parent children retain physical ownership and live expression. Samples created in release callbacks inherit the parent's frozen expression. Following children freeze before delayed callbacks and retain that snapshot for release samples; freezing has independent command storage. Surviving attack events keep separate release-counter clocks; pedal-deferred samples retain duration at key-up. | Release-only/exhausted-voice clocks and pre-engine script delays remain under review. Libraries with pitch-indexed script state can still mix identical pitches from different articulation channels in one script instance. |
| REAPER project migration | **Experimental** | Explicit SavedMulti mappings replace selected Kontakt instances in a copied RPP. An isolated REAPER check verified state through save/reopen and retained two tracks, MIDI and a send. | Opaque Kontakt state, parameter automation and nested containers are not translated. Other DAWs, host versions and sonic parity remain unverified. |
| Kontakt parity | **Untested** | No compatibility guarantee is made. | A successful load or short render is not a reference comparison. |
| Other sampler formats | **Unsupported** | None. | KONTRA does not load separate proprietary formats such as UVI, Toontrack, IK, or Ample Sound libraries. |

### Representative patch checks

These are local checks of specific patches, not guarantees for an entire library or exact Kontakt sound parity.

| Patch | Verified | Remaining limits |
|---|---|---|
| Vista Harp | All 2,000 zones load; two-part loading and finite playback pass; three interface modes render. A targeted pedal-release render confirms four damper voices rather than eight duplicated voices, removing about 6 dB of extra release level. | Import/modulation warnings remain; recorded dynamics and full Kontakt sound parity have not been compared. |
| Areia Full Ensemble — Core Techniques | Sustained and repeated same-pitch short notes in Channel mode retain their release ownership and stop after all inputs release. | Library-specific script warnings remain; broader articulation and host testing is ongoing. |
| Una Corda Cotton / Felt / Pure | All three scripts initialize without diagnostics after menu-value normalization. Cotton's Space callback controls its convolution send; its Vectorized interface passes the UI audit. | Every control and preset variation has not been exercised. |
| Analog Strings | All 95,624 zones load; interface and finite playback checks pass without streaming underruns in the measured workload. | Unsupported modulation, effect/parameter behavior, live meters, and other warnings remain. |

### Plug-in build targets

The repository's nightly workflow builds these targets. A build target does not certify operation in every host.

| Platform | CLAP | VST3 | Standalone |
|---|---|---|---|
| Linux x86_64 | Yes | Yes | Yes |
| Windows x86_64 | Yes | Yes | Yes |
| macOS arm64 | Yes | Yes | Yes |
| macOS x86_64 | Yes | Yes | Yes |

macOS bundles are signed ad hoc and are not notarized.

## Build

See [CI and snapshot checks](docs/CI.md) for each check, local commands and manual runs.

Install Rust via rustup (the repository pins its tested toolchain) and the pinned `cargo-moose` build tool:

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

## Experimental REAPER migration

Inventory a project without changing it:

```sh
python3 tools/migrate_project.py '/path/to/song.RPP'
```

Save the desired instruments, routing and controls as a KONTRA `.kontra-multi`, then map each instance explicitly. Track and FX indices are zero-based. Build the CLI with its default plug-in feature, and prepare a ReaScript:

```sh
cargo build --release --bin kontakto
python3 tools/migrate_project.py '/path/to/song.RPP' \
  --map '0:0=/path/to/Strings.kontra-multi' \
  --output-copy '/path/to/song-kontra.RPP' \
  --reaper-script '/path/to/migrate.lua' \
  --state-exporter target/release/kontakto
```

Run the generated script in REAPER with the source project open and KONTRA VST3 installed. It creates a copy beside the source to preserve relative media paths, refuses existing output paths, verifies installed state after save/reopen, and writes a migration report. Unsupported mappings are reported; Kontakt's opaque state is not recovered. Review the report and listen to the copy before using it.

## License

Project-authored code is offered under Apache-2.0; see [LICENSE](LICENSE) and [NOTICE](NOTICE). Vendored and other third-party components keep their own terms. [THIRD_PARTY.md](THIRD_PARTY.md) records dependency licenses and upstream parser metadata.

## Disclaimer

KONTRA is provided AS IS, without warranty of any kind. To the fullest extent permitted by law, the authors and contributors are not liable for damages arising from its use. See [LICENSE](LICENSE) for the full terms. Kontakt is a trademark of its owner; use of the name here describes file compatibility only. The owner does not sponsor or endorse KONTRA. Use only libraries you are licensed to use.
