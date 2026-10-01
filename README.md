# KONTRA

KONTRA is a Rust sampler for instruments that use the Kontakt file format. It runs as a CLAP or VST3 plug-in and as a standalone application, using the MOOSE plug-in framework.

KONTRA is being built as an independent alternative to Kontakt. It is not affiliated with or endorsed by the owner of Kontakt. Compatibility remains incomplete: loading a preset does not establish that its controls, scripts, routing, or sound match Kontakt.

KONTRA ships no instrument libraries or sample collections. Use libraries you are licensed to use, under their license terms.

## Compatibility and known gaps

**Full** means verified end to end for the named behavior. **Partial** means only a subset is implemented or exercised. **Unsupported** means the feature is not implemented. **Untested** means there is not enough reference or host testing to make a claim. **Experimental** means limited validation and an evolving interface. No broad compatibility area below is marked full.

| Area | Status | Implemented | Known limits |
|---|---|---|---|
| Preset parsing | **Partial** | Reads NKI instruments and NKM multis, including common groups, zones, key and velocity maps, loops, tuning, release triggers, and voice groups. | Unknown or unsupported structures may be skipped or rejected; parsing is not proof of correct behavior. |
| Sample access | **Partial** | Reads WAV, AIFF, and NCW samples from loose files and NKX/NKR containers. The `library-access` feature uses access data supplied by compatible libraries. | Missing, damaged, unsupported, or inaccessible samples can prevent or degrade playback. The feature is enabled in Cargo's default feature set, but this does not cover every protected library. |
| Audio and streaming | **Partial** | Sample playback, disk streaming, MIDI routing, and multiple outputs are implemented. Targeted playback tests and representative local patch checks exercise these paths. | No comprehensive Kontakt reference renders or real-DAW certification; timing and sound can differ. |
| KSP scripts | **Partial** | Selected initialization, note, release, controller, and UI callbacks and commands run in the built-in script engine. Release callbacks honor their selected group. Unsupported calls are diagnosed. | Many KSP services and newer APIs are absent, including note-controller and MIDI-input callbacks, sample/zone editing, and complete asynchronous file and effect loading. |
| Instrument interface | **Partial** | The editor includes a library browser, mapping, groups, mixer, spectrum, keyboard, imported controls, and zone waveforms. Original uses library artwork; Vectorized retains its background/layout with KONTRA knob and fader faces; KONTRA reorganizes controls. | Table, XY, file-selector and connected meter widgets, Komplete UI, and some original font and layer behavior remain incomplete or unsupported. |
| Timing alignment | **Partial** | Timing is measured off the audio thread once per patch and program, with separate first-note and legato values by velocity. Source validity, manual offsets, and per-part exclusion govern use of the measurements. | Edits at the same path do not invalidate saved measurements automatically; choose **Measure again** after changing samples or scripts. |
| Effects and modulation | **Partial** | Selected group filters, EQ, effects, and modulation paths are imported and processed. | Some effect and modulation types are skipped or pass through. Parameter laws and sound have not been validated against Kontakt. |
| Convolution IR selection | **Partial** | Cotton's Vintage/Room menus, Size stretching and Distance predelay work through worker-built kernels. Selected IRs and requested controls survive DAW and JSON state reload, including a sample-rate change. | Unequal early/late sizes use a diagnosed uniform stretch; separate boundaries, filters, reverse and unknown saved flags remain unmapped. Other responses and Kontakt sound equivalence remain untested. |
| Articulations | **Partial** | Articulation mappings and channel-aware MIDI and script event routing are implemented. Scripted Channel mode tracks physical input separately, supports independent same-pitch releases, and honors release-callback group selection. | Library-specific transitions, scripts, and every articulation have not been exhaustively checked. |
| MIDI and MPE | **Partial** | Scripted channel stops release their physical inputs. MPE tuning, gain and pan remain independent across member reuse, including delayed release samples and queued attacks; initial member pressure is remembered and reaches scripted mono-aftertouch modulation. Raw CC74 and pressure modulation freeze the member value at key-up; live manager CC74 adds to it (clamped), and manager pressure uses the maximum. Lower/upper master pedals, common controllers and combined master/member pitch bend work with scripts; common controllers run one script callback before reaching members. Sostenuto captures individual voices; pedal-deferred releases retain each event's groups, velocity and held duration. Surviving attack voices keep separate release-counter clocks across same-pitch retriggers. Live member bend ranges update the zone. Note-offs, pedal-up and stopping fades have reserved command storage; extreme stop overflow performs a counted, click-free channel cut. | Script-transposed input identity and script-generated release expression, pressure fallback/mapping policy, release-only/exhausted-voice counter timing and pre-engine script delays, complete MPE negotiation/master sensitivity, combined MPE/channel articulation, master pressure and broader real-host behavior remain under review. |
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

Project-authored code is offered under Apache-2.0; see [LICENSE](LICENSE) and [NOTICE](NOTICE). Vendored and other third-party components keep their own terms. See [THIRD_PARTY.md](THIRD_PARTY.md), including the unresolved licensing note for `vendor/ni-file`, before redistributing this repository.

## Disclaimer

KONTRA is provided AS IS, without warranty of any kind. To the fullest extent permitted by law, the authors and contributors are not liable for damages arising from its use. See [LICENSE](LICENSE) for the full terms. Kontakt is a trademark of its owner; use of the name here describes file compatibility only. The owner does not sponsor or endorse KONTRA. Use only libraries you are licensed to use.
