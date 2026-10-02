# KONTRA

KONTRA is a Rust sampler for instruments that use the Kontakt file format. It runs as a CLAP or VST3 plug-in and as a standalone application, using the MOOSE plug-in framework.

KONTRA is being built as an independent alternative to Kontakt. It is not affiliated with or endorsed by the owner of Kontakt. Compatibility remains incomplete: loading a preset does not establish that its controls, scripts, routing, or sound match Kontakt.

KONTRA ships no instrument libraries or sample collections. Use libraries you are licensed to use, under their license terms.

## Download the latest nightly

[![Windows x64](https://img.shields.io/badge/Windows-x64-0078D4?style=for-the-badge)](https://github.com/DerpcatMusic/KONTRA/releases/latest/download/KONTRA-nightly-windows-x86_64.zip)
[![macOS Apple silicon](https://img.shields.io/badge/macOS-Apple_silicon-222222?style=for-the-badge&logo=apple)](https://github.com/DerpcatMusic/KONTRA/releases/latest/download/KONTRA-nightly-macos-arm64.zip)
[![macOS Intel](https://img.shields.io/badge/macOS-Intel-555555?style=for-the-badge&logo=apple)](https://github.com/DerpcatMusic/KONTRA/releases/latest/download/KONTRA-nightly-macos-x86_64.zip)
[![Linux x64](https://img.shields.io/badge/Linux-x64-168B76?style=for-the-badge&logo=linux&logoColor=white)](https://github.com/DerpcatMusic/KONTRA/releases/latest/download/KONTRA-nightly-linux-x86_64.zip)

Each ZIP contains the **CLAP plug-in, VST3 plug-in and standalone application**. These fixed links serve the newest complete [experimental nightly](https://github.com/DerpcatMusic/KONTRA/releases/latest), built on every push or merge to `main`. Release-profile Linux tests and native Windows/macOS compilation must pass before free public GitHub-hosted runners publish all four platforms together from one commit. These are experimental builds, not a stable-quality release: GitHub's release flag is set to Latest so its permanent download URLs work; the version remains a SemVer nightly such as `0.3.0-nightly.20261002.g0123456789ab`. See [build progress](https://github.com/DerpcatMusic/KONTRA/actions/workflows/nightly.yml).

The [Releases page](https://github.com/DerpcatMusic/KONTRA/releases) keeps the newest complete snapshot and one previous release for rollback. Each new release uses its immutable version tag and includes `release-manifest.json` with the public source commit, actual per-format build identities and archive SHA256 checksums. Older release downloads and managed nightly source tags are removed only after a complete replacement is verified and published; stable `vX.Y.Z` source tags are preserved. Older snapshots may lack versioned build metadata; their original source commit remains visible in their release notes.

GitHub permits up to 1,000 assets per release, each under 2 GiB, and currently documents no limit on total release size or download bandwidth. [Release limits](https://docs.github.com/en/repositories/releasing-projects-on-github/about-releases) are separate from [Actions storage and billing](https://docs.github.com/en/billing/concepts/product-billing/github-actions): successful runs remove their transient build artifacts; unreleased artifacts expire after one day. Hosting limits and pricing can change.

Nightlies are experimental snapshots. Linux builds use Ubuntu 24.04 and require compatible system libraries. The x86_64 plug-ins require **AVX2, FMA and BMI2**. macOS builds are signed ad hoc and are not notarized; after extracting the ZIP, remove quarantine from the downloaded files if macOS blocks them:

```sh
xattr -dr com.apple.quarantine KONTRA.clap KONTRA.vst3 kontakto-standalone
```

## Development and diagnostics

See [CONTRIBUTING.md](CONTRIBUTING.md) for development and validation guidance, and [CHANGELOG.md](CHANGELOG.md) for release changes. To report a problem, open the **Logs** tab, choose **Export support report…**, review the preview and new-folder destination, then choose **Export report**. The Logs tab works even when an instrument fails to import. Search or filter the retained events, then select an event for its stage, reason, source location and available load context.

The new report folder contains `report.json` with build/system/audio settings and load issues, `events.jsonl` with recent session events, and `journal.jsonl` with the current journal and retained inactive sessions from previous runs. The editor retains up to 2,048 events / 2 MiB; each active session rotates at 16 MiB total, and inactive journals are retained for up to 7 days / 64 MiB. Active sessions in other processes are excluded. Counts identify omitted, truncated or unwritten events; a partial-journal warning means the available report was saved with coverage gaps. Export failures leave an explicitly incomplete bundle. Paths are redacted by default; the report excludes script contents, samples and credentials. Review it before sharing.

For a native UI stall, launch the host or standalone with `KONTRA_NATIVE_UI_TIMING=1` in its environment, then drag a control. One ten-second capture appears as `native_frame_timing` in Logs and support exports. It measures application callbacks and presentation submission, including lock waits; it does not measure GPU completion or display FPS. Capture is disabled by default.

## Compatibility and known gaps

**Full** means verified end to end for the named behavior. **Partial** means only a subset is implemented or exercised. **Unsupported** means the feature is not implemented. **Untested** means there is not enough reference or host testing to make a claim. **Experimental** means limited validation and an evolving interface. No broad compatibility area below is marked full.

| Area | Status | Implemented | Known limits |
|---|---|---|---|
| Preset parsing | **Partial** | Reads NKI instruments and NKM multis, including common groups, zones, key and velocity maps, loops, tuning, release triggers, and voice groups. Native NKSN snapshots apply supported saved controls, instrument/group FX, known envelopes and modulation assignments to an explicit base NKI. | Unknown or unsupported structures may be skipped or rejected. Opaque source state and unknown saved scalars produce warnings and remain unapplied; parsing is not proof of correct behavior. |
| Instrument creation and preset writing | **Partial** | Creates new instruments from WAV/AIFF samples, with key/velocity mapping, loops, tuning and round robins. Writes KONTRA instruments, SFZ and limited unencrypted NKI presets; generated NKI mappings, scripts and playback have targeted readback checks. KONTRA also saves its own state and multis. Typed AHDSR/Flex envelope record writes preserve opaque metadata; 230,627 records across 782 files / 788 programs pass byte-exact and edited-value readback checks. | The format APIs preserve raw NIS containers and Kontakt chunks, including unknown metadata and opaque encrypted bodies; caller keys support extraction and subtree encoding. Typed LFO writes cover known fields; some waveform tables remain raw. Typed native v2 filename-table editing preserves segment identities, UTF-16 units, full timestamps and unknown records; three actual tables covering 102,026 references roundtrip exactly. Semantic editing of arbitrary imported presets and checksum regeneration remain unsupported. NKM creation is not implemented; generated files have not been validated in Kontakt. |
| Sample access | **Partial** | Reads WAV, AIFF, and NCW samples from loose files and NKX/NKR containers. The `library-access` feature uses access data supplied by compatible libraries. | Missing, damaged, unsupported, or inaccessible samples can prevent or degrade playback. The feature is enabled in Cargo's default feature set, but this does not cover every protected library. |
| Audio and streaming | **Partial** | Sample playback, disk streaming, MIDI routing, and multiple outputs are implemented. Targeted playback tests and representative local patch checks exercise these paths. | No comprehensive Kontakt reference renders or real-DAW certification; timing and sound can differ. |
| KSP scripts | **Partial** | Selected initialization, note, release, controller, and UI callbacks and commands run in the built-in script engine. Release callbacks honor their selected group. Waiting callbacks retain their events through key-up and pool reuse, so late following notes receive their release. Conditional regions are excluded before parsing, condition symbols carry across successful slots and compiled caches, and ranged array search is supported. Native sustain/release flags let scripts control those behaviors without duplicate system actions. Live `load_array_str` and explicit-path `save_array_str` use bounded worker jobs for typed NKA files, retain array/UI revisions and deliver `async_complete`. Header, numeric-value, resource, string-capacity and write failures are diagnosed. Internal slot-to-slot MIDI2 note-controller callbacks support registered, assignable and pitch-bend values, forwarding and waits. Host position, tempo, transport and time signature reach callbacks; song position advances at callback sample offsets. Unsupported calls are diagnosed. | Beat listeners retain elapsed-clock phase; unavailable host timeline data cannot be distinguished from a zero position. Mode-based array saves and external file dialogs remain unavailable and return status 0 with diagnostics. Many KSP services and newer APIs are absent, including external MIDI2 reception, multi-script MIDI-input callbacks, sample/zone editing, and complete asynchronous file and effect loading. |
| Instrument interface | **Partial** | The editor includes a library browser, mapping, groups, mixer, spectrum, keyboard, imported controls, and zone waveforms. Browser row offsets, counts and cursor lookup are cached; empty searches remain cached. Original uses library artwork and authored wallpaper pixel offsets. Its captions honor factory font colors, state inheritance and explicit text colors through the bundled font approximation. Named bitmap fonts declared during initialization support 256-glyph Windows-1252 RGBA strips. Vectorized retains the authored background/layout with KONTRA knob and fader faces; KONTRA reorganizes controls. | Exact factory glyphs remain approximate. Areia Advanced's named gray/orange font resources and switch states are verified; wider font-resource compatibility remains unverified. Table editing, XY, file-selector and connected meter widgets, Komplete UI, and some layer behavior remain incomplete or unsupported. |
| Timing alignment | **Partial** | Timing is measured off the audio thread once per patch and program, with separate first-note and legato values by velocity. Source validity, manual offsets, and per-part exclusion govern use of the measurements. | Edits at the same path do not invalidate saved measurements automatically; choose **Measure again** after changing samples or scripts. |
| Effects and modulation | **Partial** | Selected group filters, EQ, effects, and modulation paths are imported and processed. Scripted instrument-rack filter, EQ, Stereo Modeller and subtype controls update live DSP and support readback. Native LFO metadata is decoded and preserved. | LFO clocks, waveform generation and routing remain unsupported; typed rate/phase metadata does not establish DSP behavior. Some other effect and modulation types are skipped or pass through. Parameter laws and sound have not been validated against Kontakt. |
| Convolution IR selection | **Partial** | Cotton's Vintage/Room menus, Size stretching and Distance predelay work through worker-built kernels. Selected IRs and requested controls survive DAW and JSON state reload, including a sample-rate change. | Unequal early/late sizes use a diagnosed uniform stretch; separate early/late boundaries, reverse and unknown saved flags remain unmapped. Other responses and Kontakt sound equivalence remain untested. |
| Articulations | **Partial** | Articulation mappings and channel-aware MIDI and script event routing are implemented. Scripted Channel mode tracks physical input separately, supports independent same-pitch releases, and honors release-callback group selection. | Library-specific transitions, scripts, and every articulation have not been exhaustively checked. |
| MIDI and pedals | **Partial** | Scripted and unscripted Channel mode release each physical channel's own voices, including identical pitches and all-notes-off. Sostenuto captures individual voices. Pedal-deferred releases retain each event's groups and velocity. Aligned channel stops clear held-note bookkeeping and keep fresh notes behind queued stops. Channel-mode CC120 cancels only the selected physical input, including delayed callbacks, generated notes, queued fades and native release samples, even after script channel reassignment. Sound-off cancels delayed MIDI callbacks; Panic and host reset also clear pending alignment, held input and stale script pedal/bend/pressure state while preserving edited controls. Reset restores standard controller values and recorded library device defaults in native playback and scripts. Note-offs, pedal-up and stopping fades have reserved command storage; extreme stop overflow performs a counted, click-free channel cut. | Broader library and DAW/device behavior remains under review. UI, listener and asynchronous service callbacks remain active after Panic. |
| MPE | **Partial** | Both zones support independent member expression through reuse, delayed releases and queued attacks. Initial pressure reaches scripts and raw modulation. Initial host gain/pan applies from the first rendered frame. Member CC74/pressure freeze at key-up; manager CC74 adds (clamped), pressure uses the maximum. Master pedals, common controllers and combined master/member bend work through scripts; common controllers run one callback. Negotiated manager/member bend sensitivities include cents and update held notes; manager-channel notes also honor the negotiated range. Explicit MPE configuration restores default sensitivities. | Broader negotiation, pressure fallback/mapping policy and combined MPE/channel articulation remain incomplete. |
| Release-event identity and timing | **Partial** | Script-transposed input notes and following-parent children retain physical ownership and live expression. Samples created in release callbacks inherit the parent's frozen expression. Following children freeze before delayed callbacks and retain that snapshot for release samples; freezing has independent command storage. Surviving attack events keep separate release-counter clocks; pedal-deferred samples retain duration at key-up. | Release-only/exhausted-voice clocks and pre-engine script delays remain under review. Libraries with pitch-indexed script state can still mix identical pitches from different articulation channels in one script instance. |
| REAPER project migration | **Experimental** | Explicit SavedMulti mappings replace selected Kontakt instances in a copied RPP. An isolated REAPER check verified state through save/reopen and retained two tracks, MIDI and a send. | Opaque Kontakt state, parameter automation and nested containers are not translated. Other DAWs, host versions and sonic parity remain unverified. |
| Kontakt parity | **Untested** | No compatibility guarantee is made. | A successful load or short render is not a reference comparison. |
| Other sampler formats | **Unsupported** | None. | KONTRA does not load separate proprietary formats such as UVI, Toontrack, IK, or Ample Sound libraries. |

### Representative patch checks

These are local checks of specific patches, not guarantees for an entire library or exact Kontakt sound parity. Native Original-mode captures cover 27 playable cases across nine libraries at device scale 1.5. Separate Areia Basic/Advanced Original-view benchmark probes also pass. Selected callback updates are checked; this rendering coverage does not validate every action. One bounded paired Analog trial reduces warm Vectorized CPU planning from 2.567 to 0.566 ms (78%). A separate matched Analog trial reduces mean scalar-edit submission from 1.152 to 0.000240 ms by avoiding an interface copy; full observed frame means are 4.223 and 4.395 ms, so this does not establish a frame-rate gain. A separate ten-second native Analog Original drag capture retains 1,599 callbacks, with a 6.256 ms mean interval and 5.030 ms callback p99 at device scale 1.5. Its exported diagnostic event retains exact build identity and redacts paths. This measures application callbacks and presentation submission, not compositor/display FPS, GPU completion or input-to-pixel latency; broader library and host performance remains unverified.

A native Areia-to-CHORUS replacement now shows the new logo, controls, header and playable range. Replacement clears the previous instrument's script state and convolution settings; both queued live requests and script snapshots carry source epochs to reject stale updates. This selected transition does not validate every saved-state or host replacement workflow.

| Patch | Verified | Remaining limits |
|---|---|---|
| Vista Harp / Pacific Solo Harp Normale and Harmonic Pluck | All 2,000 / 4,160 / 1,280 zones load without missing or skipped sources. Script-selected dampers now start exactly four voices in groups 8/9/18/19, compared with zero before the fix, for both ordinary and pedal release. Six dry renders have zero command drops or streaming underruns; these presets contain no reversed groups or looped zones. | Damper fade-in and saved envelope semantics remain unverified against Kontakt; the reported swell is not established as fully resolved. Pacific retains an unsupported event-array parameter diagnostic. |
| Areia Full Ensemble — Core Techniques | Sustained and repeated same-pitch short notes in Channel mode retain their release ownership and stop after all inputs release. A measured overlapping-articulation burst now coalesces repeated same-sample parameter restores: 854 dropped writes become zero, with a preallocated lookup keeping callback time near the measured baseline. | Library-specific script warnings remain; broader articulation and host testing is ongoing. |
| Areia Basic / Advanced interface | Original-view probes pass. Advanced uses two verified 256-glyph, 14-pixel bitmap fonts with variable advances; a native capture confirms its S switch changes gray to orange. The checked Reverb Time caption is 1099.5 ms using the existing DSP conversion. | These selected resources and states do not establish every font, control action or Kontakt parameter law. |
| Una Corda Cotton / Felt / Pure | All three scripts initialize without diagnostics after menu-value normalization. Cotton's Space callback controls its convolution send; its Vectorized interface passes the UI audit. Each patch preserves 28 alternating loops. Selected real PCM from all three matches independently unrolled reflection playback exactly, including release, with group effects/modulation/scripts isolated. | Every control and preset variation has not been exercised. |
| Analog Strings | All 95,624 zones load; interface and finite playback checks pass without streaming underruns in the measured workload. Authored wallpaper pages and centered volume/FX captions are retained. Selected factory NKA loads—Blown Fuse, Ambivalence and Pluckhairs—change 131 / 181 / 221 retained UI values; a 32-cell rhythm table matches its loaded file. Two authored browser-star callbacks write a copied favorites file, with fresh byte readback and zero audio heap operations over 6,300 blocks; the second toggle restores the original bytes. | The installed header-favorite preset ID is absent from its registry, so that lookup cannot update favorites; no IDs were repaired. Individual factory/rhythm menu actions and preset sound remain only partly exercised. Exact caption glyphs, display/GPU timing and Kontakt sound parity remain unverified. Unsupported modulation, effect/parameter behavior and live meters remain. |
| Analog Strings snapshots: ANALOG STRINGS INIT / Accordia / Analog Wave | Three distinct saved persistence/FX states apply to the base instrument, with two decoded impulse responses each. Production checks verify saved group FX, known envelopes and modulation assignments. Each exercises notes/pedals/stops, channel articulations, lower MPE and upper MPE. | Opaque source state and unknown saved scalars remain warned and unapplied; full snapshot and Kontakt sound equivalence are unverified. |
| Areia 16 Violins Spiccato Fast / Dolce 3 Celli Spiccato / Solo Flute / Solo Trumpet Legato Combined | Focused reruns clear held attack voices and pending work after Panic across all four playback cases. | These checks do not cover every articulation, snapshot or host. |

The offline six-patch sweep covers the three Analog snapshots plus **Areia 6 Celli Legato Fingered**, **Dolce 7 1st Violins Legato**, and **CHORUS Women Traditional Articulations**: 24 cases and 670,704 blocks, with zero measured render-thread heap operations, nonfinite samples, dropped commands, streaming underruns or blocks exceeding their nominal duration. Fresh post-Panic voices have positive native gain and envelope levels in every case; final held keys, voices and pending work are zero, with last-block RMS at most 2.4 × 10⁻²⁰. Three isolated renders with effects disabled also confirm audible fresh notes after CC121 and baseline-matching recovery after CC120/Panic, resolving the 12 previously silent legato cases. The measured maximum Panic time is 0.331 ms in this offline run; live-host deadlines and broader library behavior remain unverified.

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

Install Rust 1.99.0 (pinned in `rust-toolchain.toml`) and the pinned `cargo-moose` build tool. See [CI and snapshot checks](docs/CI.md) for the faster development profile and shipping verification:

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
