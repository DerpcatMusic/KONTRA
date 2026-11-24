# Changelog

Newest first. Every release, with its full notes, is also on the
[releases page](https://github.com/DerpcatMusic/KONTRA/releases). The editor's
About box shows this file, embedded when the plugin is built.

- **Versions** are `0.MINOR.PATCH` while KONTRA is pre-1.0 (`tools/version.py`).
  Every release raises PATCH; MINOR rises only for a major feature. "v3" names
  the engine architecture, not the version.
- **A release is cut once a day** when something that ships has changed, and
  on demand. A release's notes are the `Release note:` lines of the pull
  requests merged since the previous release; add its section here in the
  pull request that bumps the version.

How a change gets from a pull request into the notes:
[CONTRIBUTING.md](CONTRIBUTING.md#pull-requests-commits-and-releases).
The hand-written notes of the 0.3 series (the previous engine) are kept in
[docs/history/CHANGELOG-0.3.md](docs/history/CHANGELOG-0.3.md).

## 0.6.24 — 2026-11-24

- About shows the version, the build's date and time, its target and formats, and this changelog
- Settings in clear sections; dropdown arrows drawn as icons instead of missing glyphs
- Appearance: theme presets, a tint control and per-colour editing, saved with your preferences; the default is neutral grey again
- Toggles, knobs, sliders, and latches switch instantly instead of easing; restored classic panner, mod/pitch wheels, and VLC-style master volume
- Preset browser redesign with format tabs, sort in the filter menu, and card grid
- Persistent event-driven library index with automatic library discovery, visible scan diagnostics, and auto-relocation of moved libraries
- A standalone KONTRA app (`KONTRA` / `KONTRA.exe` in Linux and Windows release archives)
- Kontakt parity improvements: Conflux fast snapshots & double-open fix, Analog Strings audio fix, start-condition precedence, wavetable routes, and unified Module/ModelId IR
- The plugin's version follows the release line (0.6.x) instead of 3.0.0-dev

## 0.6.23 — 2026-11-24

- matari: unified compiler/workflow guards, matari build/doctor, rt-paranoid gate & agent verification (#141)
- kontra: Kontakt StepMod/GroupDynamics translation, ni-file hardening, and #132/L4 parity (Wave 1B) (#144)
- kontra: unified library UI IR (U1-U3), protection lock badge, and Conflux UX fixes (#145)
- ci,docs: move mirrors & moose-conformance to self-hosted runner, sync stack plan (#136)

## 0.6.22 — 2026-11-23

- mui,kontra: flat slider look for any plugin; quiet horizontal sliders stay horizontal (#131)
- fix(moose): harden AU and AAX host integration (#127)
- Fix KONTRA plugin load checks and idle editor updates (#126)
- moose,mui: Wave 1A + 1B — retire legacy GUI/truce/winit/wgpu 29, archive 4 plugins & sync lockfiles (#121)

## 0.6.21 — 2026-11-22

- matari: index both plans, define done; keep Gemini's blueprint and fundamentals audits verbatim (#118)
- matari: per-project branches, PR titles and plans; kontra plan is IR vNext (#117)
- fix(kontra-decentsampler): carry every documented effect type with its documented defaults (#116)
- kontra: audit and fix Kontakt translation stack (ni-file, effects, translator) (#115)
- feat(kontra-host): multi-stem AudioGraph output routing in RackAudio and lock-free Rack::set_param(ParamAddr) (#114)
- feat(kontra): real-time granular voice engine, AudioGraph sidechain detector routing, and Engine::set_param(ParamAddr) (#113)
- feat(kontra): IR vNext Phase 3 — AudioGraph DAG, unified Generator/EffectFamily, and polyBLEP Oscillator (#112)
- feat(kontra): IR vNext Phase 2 — compact SoA ZoneMatch/ZonePlay, L2-hot ZoneIndex pre-filter, and native .kontra format (#111)
- feat(kontra): IR vNext Phase 1B — PerNoteCc, NoteExpr (Vibrato/Expression), RrScope::Key, ParamAddr/ModRoute (#110)
- feat(kontra): IR vNext Phase 1A — separate decay/release curves, equal-power & CC crossfades, lorand/hirand random start rules (#109)
- fix(ci): drop Python 3.11 tomllib and unset CARGO_TARGET_DIR when installing cargo-moose (#108)
- fix(ci): drop Python 3.11 tomllib and unset CARGO_TARGET_DIR when installing cargo-moose (#108)
- feat(kontra,moose): complete new MUI + MOOSE cross-device migration (AU multi-out, Wayland resize, X11 dialog parenting, cancel/log hooks, mui::audit) (#107)
- feat(kontra): browse .sfz, .dspreset, and multi-preset .sf2 banks (Bank.sf2/BBB:PPP Name.sf2p) (#106)
- fix(ci): run mirror-locks.sh from exported dest dir so monorepo .cargo/config.toml does not mask public Cargo.lock (#105)
- moose: AU ABI v14, modernized AAX, wait-free audio entry, and 4-format plugin wiring (#104)
- fix(kontra): fix public mirror/nightlies, unpin stale moose, wake Tick when DAW suspends audio, surface Linux portal errors (#103)
- Deliver one signed notarized universal macOS installer ([#20](https://github.com/DerpcatMusic/KONTRA/pull/20) by @DerpcatMusic)
- engine(k-timemachine): Time Machine gap table; TM2/Pro laws waiting on Kontakt 8 READY ([#102](https://github.com/DerpcatMusic/KONTRA/pull/102) by @DerpcatMusic)
- Document the one-engine architecture ([#96](https://github.com/DerpcatMusic/KONTRA/pull/96) by @DerpcatMusic)
- Fix pending notes, channel stop timing and library load completion ([#3](https://github.com/DerpcatMusic/KONTRA/pull/3) by @DerpcatMusic)
- ci: streamline checks and verify daily snapshots ([#5](https://github.com/DerpcatMusic/KONTRA/pull/5) by @DerpcatMusic)
- Shared dsp::biquad coefficient designs (bit-exact, step 1) ([#91](https://github.com/DerpcatMusic/KONTRA/pull/91) by @DerpcatMusic)
- Expand channel KSP effects format and browser compatibility ([#4](https://github.com/DerpcatMusic/KONTRA/pull/4) by @DerpcatMusic)
- Apply scripted source routing and retain preset UI resources ([#65](https://github.com/DerpcatMusic/KONTRA/pull/65) by @DerpcatMusic)
- Fix step-envelope edits, script arrays and stale artwork completion ([#57](https://github.com/DerpcatMusic/KONTRA/pull/57) by @DerpcatMusic)
- Script state, UVI MIDI and library/editor fixes (0.3.403) ([#53](https://github.com/DerpcatMusic/KONTRA/pull/53) by @DerpcatMusic)
- Fix Kontakt readers, callback persistence and piano release (0.3.381) ([#51](https://github.com/DerpcatMusic/KONTRA/pull/51) by @DerpcatMusic)
- Accept shipped UI font licenses in Nightly bundles ([#49](https://github.com/DerpcatMusic/KONTRA/pull/49) by @DerpcatMusic)
- Fix cold explicit-target packaging for 0.3.306 ([#42](https://github.com/DerpcatMusic/KONTRA/pull/42) by @DerpcatMusic)
- Ship only the universal macOS installer ([#36](https://github.com/DerpcatMusic/KONTRA/pull/36) by @DerpcatMusic)
- Adopt current MUI native host, browser layout and portal dialogs ([#32](https://github.com/DerpcatMusic/KONTRA/pull/32) by @DerpcatMusic)
- docs: make README ultra concise ([#30](https://github.com/DerpcatMusic/KONTRA/pull/30) by @DerpcatMusic)
- Expose safe macOS signing phase diagnostics ([#28](https://github.com/DerpcatMusic/KONTRA/pull/28) by @DerpcatMusic)
- Ship reviewed 0.3.123 core fixes and native VST3 bridge completion ([#26](https://github.com/DerpcatMusic/KONTRA/pull/26) by @DerpcatMusic)
- Release 0.3.115 with reviewed host, processing and crash-recovery fixes ([#24](https://github.com/DerpcatMusic/KONTRA/pull/24) by @DerpcatMusic)
- Handle duplicate Developer ID chain certificates safely ([#19](https://github.com/DerpcatMusic/KONTRA/pull/19) by @DerpcatMusic)
- Release 0.3.83: processing, source metadata and notarized Mac delivery ([#17](https://github.com/DerpcatMusic/KONTRA/pull/17) by @DerpcatMusic)
- Harden large interfaces, transport, articulation restores and sample loops ([#11](https://github.com/DerpcatMusic/KONTRA/pull/11) by @DerpcatMusic)
- Fix nightly CLAP and VST3 manifest selection ([#7](https://github.com/DerpcatMusic/KONTRA/pull/7) by @DerpcatMusic)
- Versioned diagnostics, support reports and verified nightly releases ([#6](https://github.com/DerpcatMusic/KONTRA/pull/6) by @DerpcatMusic)

## 0.6.20 — 2026-11-17

- Scripts now hear the release of notes they end themselves, as in Kontakt, so script-driven release samples and legato bookkeeping follow the instrument. ([#1597](https://github.com/DerpcatMusic/KONTRA/pull/1597) by @DerpcatMusic)
- Native-page hover now works (Conflux shows its macro assign toggles), and file drops find a library's drop areas, with over-long paths left out. ([#1598](https://github.com/DerpcatMusic/KONTRA/pull/1598) by @DerpcatMusic)
- Augmented Orchestra's LFO shape, speed, sync and trigger controls, and its step modulator speed, now change the sound; matching modulators no longer move together. ([#1599](https://github.com/DerpcatMusic/KONTRA/pull/1599) by @DerpcatMusic)
- Augmented Orchestra's amp, filter and pitch envelope knobs now shape the notes you play. ([#1594](https://github.com/DerpcatMusic/KONTRA/pull/1594) by @DerpcatMusic)
- UVI arpeggiators and step modulators keep running while you move controls. ([#1591](https://github.com/DerpcatMusic/KONTRA/pull/1591) by @DerpcatMusic)
- Scripts now read the highest playing zone's ID, and change_vol/change_pan mode 2 sets an absolute level as the KSP manual describes. ([#1600](https://github.com/DerpcatMusic/KONTRA/pull/1600) by @DerpcatMusic)
- feat(host): onsets example; host block sizes ruled out for Areia ([#1596](https://github.com/DerpcatMusic/KONTRA/pull/1596) by @DerpcatMusic)

## 0.6.19 — 2026-11-15

- Vibrato/Chorus plays Kontakt's circuit, with COLOR 1 and 2 at last; script-made convolutions start with Auto Gain and survive a recall. ([#1595](https://github.com/DerpcatMusic/KONTRA/pull/1595) by @DerpcatMusic)
- Kontakt instruments keep their own voice limit and stealing mode, and SFZ polyphony, note_polyphony, off_mode and choke groups now play as sfizz plays them. ([#1571](https://github.com/DerpcatMusic/KONTRA/pull/1571) by @DerpcatMusic)
- Supercharger GT compresses when a script or knob raises COMPRESS; Freak's Radio and Sidechain modes and feedback play; scripts can load an impulse into a Convolution they created. ([#1588](https://github.com/DerpcatMusic/KONTRA/pull/1588) by @DerpcatMusic)
- Kontakt Glide follows its intensity and 12-semitone range, release-trigger Monophonic cuts repeated keys, DFD script offsets beyond S.Mod play from the start, and wavetable glides stay smooth. ([#1593](https://github.com/DerpcatMusic/KONTRA/pull/1593) by @DerpcatMusic)
- Audio, MIDI and array files dropped on a Kontakt library's drop areas reach its script; Conflux's empty and chosen FX slots draw and pick correctly. ([#1592](https://github.com/DerpcatMusic/KONTRA/pull/1592) by @DerpcatMusic)
- Kontakt snapshots now restore key tracking, reverse, release triggers and interpolation quality per group, and KONTRA can save a loaded snapshot back as a Kontakt snapshot file. ([#1586](https://github.com/DerpcatMusic/KONTRA/pull/1586) by @DerpcatMusic)
- feat(inspect): per-voice envelope trace for onset checks ([#1590](https://github.com/DerpcatMusic/KONTRA/pull/1590) by @DerpcatMusic)

## 0.6.18 — 2026-11-15

- Supercharger GT's compressor now plays, and scripts can load Psyche Delay, Vibrato/Chorus, Freak, Bite, Distortion, Stereo Modeller, Solid G-EQ and Convolution. ([#1579](https://github.com/DerpcatMusic/KONTRA/pull/1579) by @DerpcatMusic)
- Kontakt amplitude envelopes respect their volume intensity, and UVI envelope delays now hold before the attack. ([#1573](https://github.com/DerpcatMusic/KONTRA/pull/1573) by @DerpcatMusic)
- Conflux knobs and faders turn again, its assign toggles show on hover, and VWinds' Virtual Space highlights the seat you pick. ([#1576](https://github.com/DerpcatMusic/KONTRA/pull/1576) by @DerpcatMusic)
- KSP scripts read an effect module's starting settings as soon as they load it, and tempo-unit controls read the unit the instrument saved. ([#1584](https://github.com/DerpcatMusic/KONTRA/pull/1584) by @DerpcatMusic)
- KSP scripts read an effect module's saved settings even when KONTRA doesn't play that module yet. ([#1582](https://github.com/DerpcatMusic/KONTRA/pull/1582) by @DerpcatMusic)
- Augmented Orchestra's Maximizer now looks ahead, blends its slow release and limits its recovery rate like UVI's, instead of clipping onsets. ([#1581](https://github.com/DerpcatMusic/KONTRA/pull/1581) by @DerpcatMusic)
- Augmented Orchestra's Play Mode menu and Glide slider now change how notes play; VWinds' pitch-bend crossfades step as in UVI. ([#1578](https://github.com/DerpcatMusic/KONTRA/pull/1578) by @DerpcatMusic)
- a UVI preset loaded from a note while a control callback waits now loads paced, instead of landing all at once. ([#1575](https://github.com/DerpcatMusic/KONTRA/pull/1575) by @DerpcatMusic)
- VWinds' seat and room changes keep the new seat when the room size is applied with them. ([#1572](https://github.com/DerpcatMusic/KONTRA/pull/1572) by @DerpcatMusic)
- KSP scripts read back the controls they set on effect modules KONTRA doesn't play yet. ([#1574](https://github.com/DerpcatMusic/KONTRA/pull/1574) by @DerpcatMusic)
- UVI scripts' sample offsets, fades and note durations now act like UVI's, and a preset picked in the editor loads at once. ([#1569](https://github.com/DerpcatMusic/KONTRA/pull/1569) by @DerpcatMusic)
- VWinds' seat, microphone pair, room and room size, and Augmented Orchestra's convolution IR, Time and PreDelay now change the sound. ([#1566](https://github.com/DerpcatMusic/KONTRA/pull/1566) by @DerpcatMusic)
- KSP scripts that only read an effect module's settings now get the instrument's saved values. ([#1585](https://github.com/DerpcatMusic/KONTRA/pull/1585) by @DerpcatMusic)
- Presets keep their exact knob values; snapshots restore each group's source and modulators, not only its effects. ([#1580](https://github.com/DerpcatMusic/KONTRA/pull/1580) by @DerpcatMusic)
- switching a layer to mono while a chord is held now leaves only the new note sounding. ([#1583](https://github.com/DerpcatMusic/KONTRA/pull/1583) by @DerpcatMusic)
- Omni parts keep each port's and channel's pedal, bend and CCs apart, as Kontakt does; snapshots group by the library's own Snapshots folder. ([#1570](https://github.com/DerpcatMusic/KONTRA/pull/1570) by @DerpcatMusic)
- fix: clear the clippy errors main carries ([#1577](https://github.com/DerpcatMusic/KONTRA/pull/1577) by @DerpcatMusic)

## 0.6.17 — 2026-11-14

- Portamento now plays: UVI mono, legato and slide layers glide and keep their voice like UVI, and Kontakt Glide modulators glide at their Time or Speed setting. ([#1568](https://github.com/DerpcatMusic/KONTRA/pull/1568) by @DerpcatMusic)
- Supercharger GT plays its saved modes, Replika's Analogue, Vintage and Tape modes saturate and hiss like Kontakt's, and scripts can load four more effects. ([#1567](https://github.com/DerpcatMusic/KONTRA/pull/1567) by @DerpcatMusic)
- Kontakt envelopes start their attack 33 samples in, as Kontakt does, and saved sustain levels below full now play at Kontakt's level (they were far too quiet). ([#1562](https://github.com/DerpcatMusic/KONTRA/pull/1562) by @DerpcatMusic)
- Replika's Analogue, Tape and Vintage modes now play, Distortion's Tube sounds like Kontakt 8's, and DStortion and Supercharger GT are new. ([#1561](https://github.com/DerpcatMusic/KONTRA/pull/1561) by @DerpcatMusic)
- Replika's Modern mode now modulates its delay with DEPTH and RATE as Kontakt does. ([#1556](https://github.com/DerpcatMusic/KONTRA/pull/1556) by @DerpcatMusic)
- Conflux, VWinds and Augmented Orchestra panels draw as in their manuals: meters, key ranges, menus, arpeggiator steps and hidden knobs. ([#1564](https://github.com/DerpcatMusic/KONTRA/pull/1564) by @DerpcatMusic)
- Library file browsers list, scroll and pick files as in Kontakt; XY pads and text fields run their callbacks when Kontakt does. ([#1557](https://github.com/DerpcatMusic/KONTRA/pull/1557) by @DerpcatMusic)
- KSP controls report their type, size and grid position like Kontakt; ctrl-click resets and drags continue from the default. ([#1559](https://github.com/DerpcatMusic/KONTRA/pull/1559) by @DerpcatMusic)
- Conflux lists all its snapshots, Augmented Orchestra presets keep their LFO shape, and parts listen on Kontakt's ports A–D (A1–D16). ([#1565](https://github.com/DerpcatMusic/KONTRA/pull/1565) by @DerpcatMusic)
- UVI layers that limit their notes (VWinds sustain, Augmented Orchestra mono leads) now steal voices like UVI, and Augmented Orchestra presets load about 20% faster. ([#1560](https://github.com/DerpcatMusic/KONTRA/pull/1560) by @DerpcatMusic)
- Snapshots sit under their instrument in the header and restore as Kontakt does; scripts' automatable controls show in your DAW under Kontakt's names. ([#1555](https://github.com/DerpcatMusic/KONTRA/pull/1555) by @DerpcatMusic)
- UVI scripts no longer allocate on the audio thread when a knob or controller moves. ([#1523](https://github.com/DerpcatMusic/KONTRA/pull/1523) by @DerpcatMusic)
- ci(release): release twice a day, not every third PR ([#1554](https://github.com/DerpcatMusic/KONTRA/pull/1554) by @DerpcatMusic)

## 0.6.16 — 2026-11-14

- Library meters, waveforms, wavetables and text fields draw as Kontakt does, and small library text stays readable at 50 %. ([#1538](https://github.com/DerpcatMusic/KONTRA/pull/1538) by @DerpcatMusic)
- UVI widgets follow their mapper and unit; hierarchical menus open as sub-menus; the flute's path matching is linear. ([#1553](https://github.com/DerpcatMusic/KONTRA/pull/1553) by @DerpcatMusic)
- Heavy controls on UVI flutes stall the audio less when moved, about 5 ms at worst instead of about 6 ms. ([#1552](https://github.com/DerpcatMusic/KONTRA/pull/1552) by @DerpcatMusic)

## 0.6.15 — 2026-11-14

- The KSP coverage report ranks open rows by library use; get_ui_wf_property works. ([#1549](https://github.com/DerpcatMusic/KONTRA/pull/1549) by @DerpcatMusic)
- Moving a heavy control, such as the jazz harmonics knob on UVI flutes, no longer stalls the audio for about 100 ms. ([#1550](https://github.com/DerpcatMusic/KONTRA/pull/1550) by @DerpcatMusic)
- The KSP coverage table no longer shows a command as done when real library calls to it fail. ([#1551](https://github.com/DerpcatMusic/KONTRA/pull/1551) by @DerpcatMusic)

## 0.6.14 — 2026-11-14

- Wavetable quality settings now sound like Kontakt's, and scripts that read the date or time now work. ([#1547](https://github.com/DerpcatMusic/KONTRA/pull/1547) by @DerpcatMusic)
- Internal tool fix; no change for players. ([#1548](https://github.com/DerpcatMusic/KONTRA/pull/1548) by @DerpcatMusic)
- Morphology Evolved's snapshots play again: 100 of its 101 snapshots were silent, and now all 101 sound. ([#1545](https://github.com/DerpcatMusic/KONTRA/pull/1545) by @DerpcatMusic)

## 0.6.13 — 2026-11-14

- Wavetable instruments such as Conflux now follow their script's modulator wave shape, tuning and phase, so they sound closer to Kontakt. ([#1544](https://github.com/DerpcatMusic/KONTRA/pull/1544) by @DerpcatMusic)
- Multis now keep each instrument's MIDI channel, output, volume and pan, and run their own multi script, so split multis play each part on its own channel. ([#1539](https://github.com/DerpcatMusic/KONTRA/pull/1539) by @DerpcatMusic)
- Heavy controls in UVI library scripts, such as jazz harmonics on the VWinds flutes, now work instead of stopping with a script error. ([#1546](https://github.com/DerpcatMusic/KONTRA/pull/1546) by @DerpcatMusic)

## 0.6.12 — 2026-11-14

- Knobs and sliders no longer jump back and forth while you drag them, in every library and in the part header. ([#1543](https://github.com/DerpcatMusic/KONTRA/pull/1543) by @DerpcatMusic)
- UVI script knobs follow LFO, mapped, inverted and multiplicative routes; Xpander Algorithm II plays zero-delay; UVIScript Param* widgets and ParameterValue work. ([#1531](https://github.com/DerpcatMusic/KONTRA/pull/1531) by @DerpcatMusic)
- Libraries whose snapshots pick unloaded sounds, such as Morphology Evolved, play again instead of staying silent. ([#1541](https://github.com/DerpcatMusic/KONTRA/pull/1541) by @DerpcatMusic)

## 0.6.11 — 2026-11-14

- Preset names in the browser no longer repeat their folder's name, so snapshot and patch lists are easier to read. ([#1540](https://github.com/DerpcatMusic/KONTRA/pull/1540) by @DerpcatMusic)
- Scripts can now create user zones, load samples and loops into them, and set more zone parameters. ([#1535](https://github.com/DerpcatMusic/KONTRA/pull/1535) by @DerpcatMusic)
- Library knobs and sliders no longer jump back and forth while you drag them in a DAW. ([#1542](https://github.com/DerpcatMusic/KONTRA/pull/1542) by @DerpcatMusic)

## 0.6.10 — 2026-11-14

- Kontakt scripts can analyse samples (key, pitch, tempo, loudness), react to incoming MIDI and per-note controllers, and set voice limits. ([#1534](https://github.com/DerpcatMusic/KONTRA/pull/1534) by @DerpcatMusic)
- UVI scripts can delay controllers they post, and Kontakt multis now load a part per slot and reopen with a project. ([#1536](https://github.com/DerpcatMusic/KONTRA/pull/1536) by @DerpcatMusic)
- fix(engine): midi_in event params without indexing ([#1537](https://github.com/DerpcatMusic/KONTRA/pull/1537) by @DerpcatMusic)

## 0.6.9 — 2026-11-14

- Library dropdowns open as popups, and the window scales down to 50 % for small monitors. ([#1533](https://github.com/DerpcatMusic/KONTRA/pull/1533) by @DerpcatMusic)
- UVI scripts now change or hold back controllers, bend and aftertouch the way UVI does, and Kontakt scripts can set pitch bend and pressure. ([#1532](https://github.com/DerpcatMusic/KONTRA/pull/1532) by @DerpcatMusic)
- VWinds' breath, pitch-bend and transport handling now runs, and Kontakt scripts hear pitch bend and channel pressure. ([#1517](https://github.com/DerpcatMusic/KONTRA/pull/1517) by @DerpcatMusic)

## 0.6.8 — 2026-11-14

- Script and control changes to a group's filter or EQ after load are now heard, such as Una Corda's velocity-dependent tone. ([#1519](https://github.com/DerpcatMusic/KONTRA/pull/1519) by @DerpcatMusic)
- UVI libraries show their own artwork, the browser shows about ten libraries at once in a grid, and presets can be starred. ([#1530](https://github.com/DerpcatMusic/KONTRA/pull/1530) by @DerpcatMusic)
- Scripts can now use the MIDI object, read zone samples and loops, and detect a sample's pitch, key, loudness and tempo. ([#1529](https://github.com/DerpcatMusic/KONTRA/pull/1529) by @DerpcatMusic)

## 0.6.7 — 2026-11-14

- Conflux's Edit page lays out its modules and FX slots correctly, and its tabs and captions no longer show grey boxes. ([#1528](https://github.com/DerpcatMusic/KONTRA/pull/1528) by @DerpcatMusic)
- Kontakt scripts that read key-range names and notes, set poly aftertouch, colour map-editor events or load a Komplete UI now work. ([#1527](https://github.com/DerpcatMusic/KONTRA/pull/1527) by @DerpcatMusic)
- docs: KSP data and file side coverage table ([#1526](https://github.com/DerpcatMusic/KONTRA/pull/1526) by @DerpcatMusic)

## 0.6.6 — 2026-11-14

- The preset browser shows library pictures edge to edge, drops repeated names, finds presets by abbreviation such as tpt leg, highlights hits and works from the keyboard. ([#1525](https://github.com/DerpcatMusic/KONTRA/pull/1525) by @DerpcatMusic)
- Una Corda's mechanical noise (and any purged group a script turns on) now loads and plays. ([#1524](https://github.com/DerpcatMusic/KONTRA/pull/1524) by @DerpcatMusic)
- Libraries with smoothed LFO routes (Augmented Orchestra) use about a third less audio-thread CPU per voice. ([#1522](https://github.com/DerpcatMusic/KONTRA/pull/1522) by @DerpcatMusic)

## 0.6.5 — 2026-11-13

- UVI controllers that move script knobs (filter drive, MIDI curve) now play. ([#1518](https://github.com/DerpcatMusic/KONTRA/pull/1518) by @DerpcatMusic)
- Missing Kontakt and SFZ sample folders can be relinked too, and the Kontakt readers survive damaged chunk payloads. ([#1521](https://github.com/DerpcatMusic/KONTRA/pull/1521) by @DerpcatMusic)
- Loading a library now reports every stored setting the translators do not carry. ([#1516](https://github.com/DerpcatMusic/KONTRA/pull/1516) by @DerpcatMusic)

## 0.6.4 — 2026-11-13

- Kontakt script labels in the pixel font look like Kontakt's at 150 %, and grey fonts 20, 21 and 23 get their real weight. ([#1514](https://github.com/DerpcatMusic/KONTRA/pull/1514) by @DerpcatMusic)
- docs(perf): the 300-voice gap, measured ([#1520](https://github.com/DerpcatMusic/KONTRA/pull/1520) by @DerpcatMusic)
- feat(inspect): census runtime script faults across Kontakt libraries ([#1512](https://github.com/DerpcatMusic/KONTRA/pull/1512) by @DerpcatMusic)

## 0.6.3 — 2026-11-13

- The one-pole high-pass filter now cuts at the same frequency as in Kontakt. ([#1511](https://github.com/DerpcatMusic/KONTRA/pull/1511) by @DerpcatMusic)
- fix(engine): name the memo slot's aged value so clippy passes ([#1513](https://github.com/DerpcatMusic/KONTRA/pull/1513) by @DerpcatMusic)
- ci(release): say why notarization failed; skip 'none (…)' notes ([#1510](https://github.com/DerpcatMusic/KONTRA/pull/1510) by @DerpcatMusic)

## 0.6.2 — 2026-11-13

- Host tempo and transport changes reach scripts at the exact sample, and per-note pressure runs a script's poly-aftertouch callback. ([#1509](https://github.com/DerpcatMusic/KONTRA/pull/1509) by @DerpcatMusic)
- UVI instruments' Maximizer releases at UVI's rate, so busy passages pump less. ([#1503](https://github.com/DerpcatMusic/KONTRA/pull/1503) by @DerpcatMusic)
- step-envelope Smooth now follows the host tempo. ([#1500](https://github.com/DerpcatMusic/KONTRA/pull/1500) by @DerpcatMusic)
- Per-note pressure and brightness now reach each voice, and host macro automation lands on the exact sample. ([#1498](https://github.com/DerpcatMusic/KONTRA/pull/1498) by @DerpcatMusic)
- Una Corda plays in its stretched tuning again, instead of 5–6 cents off on most keys. ([#1497](https://github.com/DerpcatMusic/KONTRA/pull/1497) by @DerpcatMusic)
- tempo-synced LFO rate modulation and comb Q modulation now play in Augmented Orchestra. ([#1493](https://github.com/DerpcatMusic/KONTRA/pull/1493) by @DerpcatMusic)
- Convolution reverbs now honour their early and late Size settings, including Una Corda's Size slider. ([#1483](https://github.com/DerpcatMusic/KONTRA/pull/1483) by @DerpcatMusic)
- Augmented Orchestra Xpander, comb and flanger-feedback modulations now play. ([#1481](https://github.com/DerpcatMusic/KONTRA/pull/1481) by @DerpcatMusic)
- Wavetable instruments saved with a modulation oscillator (FM, PM, ring, mix) now play it, matching Kontakt. ([#1477](https://github.com/DerpcatMusic/KONTRA/pull/1477) by @DerpcatMusic)
- Augmented Orchestra's keygroup comb filters now sound, with their frequency following the controls. ([#1465](https://github.com/DerpcatMusic/KONTRA/pull/1465) by @DerpcatMusic)
- Augmented Orchestra's smoothed step sequences glide between steps instead of jumping. ([#1459](https://github.com/DerpcatMusic/KONTRA/pull/1459) by @DerpcatMusic)
- Augmented Orchestra's pulsing FX sends are steady across host block sizes and tempo changes. ([#1456](https://github.com/DerpcatMusic/KONTRA/pull/1456) by @DerpcatMusic)
- Augmented Orchestra's flanger, phaser and shaper mixes now follow their LFOs and step envelopes. ([#1445](https://github.com/DerpcatMusic/KONTRA/pull/1445) by @DerpcatMusic)
- Conflux's Raum reverb and Wow/Flutter now process sound, and its wavetable modulation oscillator (FM, PM, ring, mix) plays. ([#1447](https://github.com/DerpcatMusic/KONTRA/pull/1447) by @DerpcatMusic)
- UVI scripts now switch effect racks on and off and move effect settings such as EQ frequencies and reverb decay. ([#1446](https://github.com/DerpcatMusic/KONTRA/pull/1446) by @DerpcatMusic)
- Augmented Orchestra's LFO- and step-driven FX sends now pulse in time instead of sitting still. ([#1441](https://github.com/DerpcatMusic/KONTRA/pull/1441) by @DerpcatMusic)
- UVI instruments' effect on/off switches now change the sound. ([#1438](https://github.com/DerpcatMusic/KONTRA/pull/1438) by @DerpcatMusic)
- Conflux's Inharmonic control now works from its presets and modulation, not only when saved in the instrument. ([#1436](https://github.com/DerpcatMusic/KONTRA/pull/1436) by @DerpcatMusic)
- Wavetable instruments (Conflux) now play their Inharmonic mode, stretching or squeezing partials away from the harmonic series as Kontakt does. ([#1431](https://github.com/DerpcatMusic/KONTRA/pull/1431) by @DerpcatMusic)
- Shreddage's Slide Speed knob now changes how fast slides play, at their own pitch. ([#1426](https://github.com/DerpcatMusic/KONTRA/pull/1426) by @DerpcatMusic)
- Wavetable instruments (Conflux) now play at Kontakt's level (they were 7 dB too loud) and start each note at a random point in the cycle when Phase Random is up. ([#1424](https://github.com/DerpcatMusic/KONTRA/pull/1424) by @DerpcatMusic)
- Script labels in Kontakt's stock fonts match Kontakt's size, weight and placement. ([#1507](https://github.com/DerpcatMusic/KONTRA/pull/1507) by @DerpcatMusic)
- Learnt host macros carry the control's name, and Lua panel controls follow automation at the exact sample. ([#1506](https://github.com/DerpcatMusic/KONTRA/pull/1506) by @DerpcatMusic)
- Script switch and button pictures line up with Kontakt's to the pixel. ([#1502](https://github.com/DerpcatMusic/KONTRA/pull/1502) by @DerpcatMusic)
- Move any library panel control and it becomes host-automatable as Macro 1–8. Bounces no longer drop sample pages. ([#1495](https://github.com/DerpcatMusic/KONTRA/pull/1495) by @DerpcatMusic)
- Escape closes menus and Settings; part headers no longer repeat the preset's name. ([#1490](https://github.com/DerpcatMusic/KONTRA/pull/1490) by @DerpcatMusic)
- Menus and the loading indicator no longer cut their text short. ([#1487](https://github.com/DerpcatMusic/KONTRA/pull/1487) by @DerpcatMusic)
- Conflux's Edit and Settings tabs now switch the page. Any native page tab whose press re-renders it now works too. ([#1474](https://github.com/DerpcatMusic/KONTRA/pull/1474) by @DerpcatMusic)
- A library's view now fits the rack without scrolling the whole editor. ([#1471](https://github.com/DerpcatMusic/KONTRA/pull/1471) by @DerpcatMusic)
- Smoother library panels while turning knobs. ([#1467](https://github.com/DerpcatMusic/KONTRA/pull/1467) by @DerpcatMusic)
- Escape closes menus, and the list of a library's saved files fills its sheet. ([#1463](https://github.com/DerpcatMusic/KONTRA/pull/1463) by @DerpcatMusic)
- A library's own Save and Load buttons now open KONTRA's file sheet, and what they save is kept on disk for that instrument. Long preset names in menus read in full. ([#1453](https://github.com/DerpcatMusic/KONTRA/pull/1453) by @DerpcatMusic)
- UVI text too wide for its box narrows to fit and keeps its height, as in UVI. ([#1448](https://github.com/DerpcatMusic/KONTRA/pull/1448) by @DerpcatMusic)
- A UVI instrument's own Save/Load preset menu and a Kontakt script's file save/load now save and load back. Presets in the part header that share a name show the folder that tells them apart. ([#1440](https://github.com/DerpcatMusic/KONTRA/pull/1440) by @DerpcatMusic)
- Saving a preset: type the name and press Enter; the More button's tip no longer covers its menu. ([#1433](https://github.com/DerpcatMusic/KONTRA/pull/1433) by @DerpcatMusic)
- UVI menus keep their text clear of the dropdown arrow, and UVI single-line text draws at its full size. ([#1428](https://github.com/DerpcatMusic/KONTRA/pull/1428) by @DerpcatMusic)
- Morphology's LFO rate readouts show the rate in Hz. ([#1422](https://github.com/DerpcatMusic/KONTRA/pull/1422) by @DerpcatMusic)
- Clicking in KONTRA's window lets keys like Escape and typing reach it in your DAW. ([#1421](https://github.com/DerpcatMusic/KONTRA/pull/1421) by @DerpcatMusic)
- UVI labels with several lines space them as UVI does. ([#1419](https://github.com/DerpcatMusic/KONTRA/pull/1419) by @DerpcatMusic)
- Kontakt panels show the strip above their top controls as Kontakt does: Pacific's "Mic Mixer" and "Additional Controls" headings are no longer cut off. ([#1418](https://github.com/DerpcatMusic/KONTRA/pull/1418) by @DerpcatMusic)
- Kontakt 8 instruments draw their own curves and shapes: Conflux's velocity and aftertouch curves are no longer empty boxes. ([#1415](https://github.com/DerpcatMusic/KONTRA/pull/1415) by @DerpcatMusic)
- Escape closes the Settings and preset sheets. ([#1417](https://github.com/DerpcatMusic/KONTRA/pull/1417) by @DerpcatMusic)
- UVI instruments that move or resize a control after building their page now draw it in its new place: V Winds' ADV PITCH BEND and USE MPE menus no longer draw over other rows. ([#1416](https://github.com/DerpcatMusic/KONTRA/pull/1416) by @DerpcatMusic)
- UVI instruments draw their text at the size UVI does: V Winds' preferences no longer run words into their boxes. ([#1414](https://github.com/DerpcatMusic/KONTRA/pull/1414) by @DerpcatMusic)
- In UVI instruments, text labels line up where the instrument places them instead of drifting to the centre (Augmented Orchestra's sound browser, V Winds settings). ([#1413](https://github.com/DerpcatMusic/KONTRA/pull/1413) by @DerpcatMusic)
- Missing sample folders are reported once, and a moved library can be relinked from the Report tab. ([#1505](https://github.com/DerpcatMusic/KONTRA/pull/1505) by @DerpcatMusic)
- Kontakt scripts reading the Convolution, Chorus depth and Jump knob labels now get their text. ([#1499](https://github.com/DerpcatMusic/KONTRA/pull/1499) by @DerpcatMusic)
- Augmented Orchestra's FX macros no longer report as unplayed; their saved settings sound as saved. ([#1434](https://github.com/DerpcatMusic/KONTRA/pull/1434) by @DerpcatMusic)
- V Winds' EQ peaks now play at the right order, with no gap reported. ([#1430](https://github.com/DerpcatMusic/KONTRA/pull/1430) by @DerpcatMusic)
- Augmented Orchestra's FX layer (feedback comb, drive, filter) now sounds. ([#1425](https://github.com/DerpcatMusic/KONTRA/pull/1425) by @DerpcatMusic)
- a second instance of the same instrument uses much less memory. ([#1491](https://github.com/DerpcatMusic/KONTRA/pull/1491) by @DerpcatMusic)
- instruments with many voices use less memory. ([#1486](https://github.com/DerpcatMusic/KONTRA/pull/1486) by @DerpcatMusic)
- switching instruments gives memory back to the system instead of keeping every instrument ever loaded. ([#1479](https://github.com/DerpcatMusic/KONTRA/pull/1479) by @DerpcatMusic)
- loading an instrument no longer randomly pins several times more sample memory when part of the library is on a slower drive; instrument switches are measured. ([#1469](https://github.com/DerpcatMusic/KONTRA/pull/1469) by @DerpcatMusic)
- Script group-count and event-note reads cost less per call. ([#1466](https://github.com/DerpcatMusic/KONTRA/pull/1466) by @DerpcatMusic)
- Script host calls skip a redundant pure-op lookup. ([#1464](https://github.com/DerpcatMusic/KONTRA/pull/1464) by @DerpcatMusic)
- Note starts pick their zones from a per-key index, trimming chord-block CPU spikes. ([#1462](https://github.com/DerpcatMusic/KONTRA/pull/1462) by @DerpcatMusic)
- notes played while an instrument's samples are still loading start from their first frame instead of joining mid-sample. ([#1460](https://github.com/DerpcatMusic/KONTRA/pull/1460) by @DerpcatMusic)
- wavetable instruments keep their tables when the plugin loads their samples in the background. ([#1458](https://github.com/DerpcatMusic/KONTRA/pull/1458) by @DerpcatMusic)
- patches with many zones use much less CPU per block (Areia's median block 25 → 7 us). ([#1457](https://github.com/DerpcatMusic/KONTRA/pull/1457) by @DerpcatMusic)
- instruments become playable before their sample heads finish loading. ([#1451](https://github.com/DerpcatMusic/KONTRA/pull/1451) by @DerpcatMusic)
- UVI programs with scripts load faster again. ([#1454](https://github.com/DerpcatMusic/KONTRA/pull/1454) by @DerpcatMusic)
- ANALOG STRINGS and other big-script patches no longer take a 10-15 ms audio block while loading, and voices cost less CPU. ([#1449](https://github.com/DerpcatMusic/KONTRA/pull/1449) by @DerpcatMusic)
- Kontakt libraries with many archives open their sample archives in parallel. ([#1450](https://github.com/DerpcatMusic/KONTRA/pull/1450) by @DerpcatMusic)
- Instruments that aren't in memory yet load several times faster (ANALOG STRINGS from about a minute to under half a minute). ([#1444](https://github.com/DerpcatMusic/KONTRA/pull/1444) by @DerpcatMusic)
- Kontakt instruments with large sample archives read their member headers with fewer disk calls. ([#1432](https://github.com/DerpcatMusic/KONTRA/pull/1432) by @DerpcatMusic)
- ANALOG STRINGS and other patches with a send delay use less CPU while the delay is idle. ([#1427](https://github.com/DerpcatMusic/KONTRA/pull/1427) by @DerpcatMusic)
- KONTRA's editor uses the graphics card again after an earlier graphics crash, instead of staying on the CPU. ([#1412](https://github.com/DerpcatMusic/KONTRA/pull/1412) by @DerpcatMusic)
- a damaged or foreign project state now says so and starts with an empty rack; a fault in the audio render is silence plus a message instead of an unwind into the host. ([#1501](https://github.com/DerpcatMusic/KONTRA/pull/1501) by @DerpcatMusic)
- Pressing an instrument's own preset arrow twice quickly now leaves KONTRA's header on the second preset. ([#1492](https://github.com/DerpcatMusic/KONTRA/pull/1492) by @DerpcatMusic)
- Sustain-pedal releases now run the instrument's release scripts again when the pedal lifts, as Kontakt does. ([#1489](https://github.com/DerpcatMusic/KONTRA/pull/1489) by @DerpcatMusic)
- ANALOG STRINGS' own next and previous preset arrows change the preset in KONTRA's header again. ([#1482](https://github.com/DerpcatMusic/KONTRA/pull/1482) by @DerpcatMusic)
- Presets can now be saved from UVI instruments. ([#1437](https://github.com/DerpcatMusic/KONTRA/pull/1437) by @DerpcatMusic)
- docs(census): effect and filter law coverage, probed against Kontakt ([#1508](https://github.com/DerpcatMusic/KONTRA/pull/1508) by @DerpcatMusic)
- test(oracle): script_trace adds a mod-wheel sweep and retries zones ([#1504](https://github.com/DerpcatMusic/KONTRA/pull/1504) by @DerpcatMusic)
- docs(census): Kontakt census of 682 instruments; inspect action counts ([#1496](https://github.com/DerpcatMusic/KONTRA/pull/1496) by @DerpcatMusic)
- perf(memory): heap example splits a second instance's memory by site ([#1494](https://github.com/DerpcatMusic/KONTRA/pull/1494) by @DerpcatMusic)
- perf(reverb,engine): flat convolver tail, one voice pass per CC ([#1488](https://github.com/DerpcatMusic/KONTRA/pull/1488) by @DerpcatMusic)
- perf(reverb): spread the convolver's partition sum over its block ([#1484](https://github.com/DerpcatMusic/KONTRA/pull/1484) by @DerpcatMusic)
- fix(engine): zone index prefix sum without indexing ([#1485](https://github.com/DerpcatMusic/KONTRA/pull/1485) by @DerpcatMusic)
- perf(engine): refresh a callback's written notes once at its end ([#1480](https://github.com/DerpcatMusic/KONTRA/pull/1480) by @DerpcatMusic)
- perf(engine): memo LFO/envelope reads within a voice refresh ([#1478](https://github.com/DerpcatMusic/KONTRA/pull/1478) by @DerpcatMusic)
- perf(racks): skip voice-unit retunes to unchanged settings ([#1476](https://github.com/DerpcatMusic/KONTRA/pull/1476) by @DerpcatMusic)
- perf(dsp): cache halfband design; outlier blocks 134 to 43 ([#1475](https://github.com/DerpcatMusic/KONTRA/pull/1475) by @DerpcatMusic)
- perf(plan,filter): lazy LFO sine, skip unchanged tan ([#1472](https://github.com/DerpcatMusic/KONTRA/pull/1472) by @DerpcatMusic)
- perf(voice): zero only uncovered window frames; count voices ([#1470](https://github.com/DerpcatMusic/KONTRA/pull/1470) by @DerpcatMusic)
- feat(inspect): load vs play block timing, mean block ([#1468](https://github.com/DerpcatMusic/KONTRA/pull/1468) by @DerpcatMusic)
- perf(vm): fused compare/mask ops, direct num_groups and key_down ([#1461](https://github.com/DerpcatMusic/KONTRA/pull/1461) by @DerpcatMusic)
- feat(ui): KONTRA_FRAME_TIMES=1 prints editor frame stage times ([#1452](https://github.com/DerpcatMusic/KONTRA/pull/1452) by @DerpcatMusic)
- fix(load): the header warm-up builds on Windows again ([#1455](https://github.com/DerpcatMusic/KONTRA/pull/1455) by @DerpcatMusic)
- feat(inspect): slow blocks report the script work they ran ([#1442](https://github.com/DerpcatMusic/KONTRA/pull/1442) by @DerpcatMusic)
- build: raise the size caps to 1,500 lines per file and 15,000 per crate ([#1443](https://github.com/DerpcatMusic/KONTRA/pull/1443) by @DerpcatMusic)
- feat(inspect): per-block max and p99 audio-thread time ([#1439](https://github.com/DerpcatMusic/KONTRA/pull/1439) by @DerpcatMusic)
- refactor(kontakt): move eight effect families into their own crate ([#1435](https://github.com/DerpcatMusic/KONTRA/pull/1435) by @DerpcatMusic)
- fix(engine): plain voices play the same at every host block size again ([#1429](https://github.com/DerpcatMusic/KONTRA/pull/1429) by @DerpcatMusic)
- refactor(ui): move the library-face view into its own crate ([#1423](https://github.com/DerpcatMusic/KONTRA/pull/1423) by @DerpcatMusic)
- build: every release raises only the patch number ([#1420](https://github.com/DerpcatMusic/KONTRA/pull/1420) by @DerpcatMusic)

## 0.6.1 — 2026-11-13

- Settings › Size now makes the editor window 100 %, 125 %, 150 % or 200 % of its normal size. ([#1410](https://github.com/DerpcatMusic/KONTRA/pull/1410) by @DerpcatMusic)
- Scrolling a library's list past its end no longer scrolls the rack. ([#1409](https://github.com/DerpcatMusic/KONTRA/pull/1409) by @DerpcatMusic)
- Library panels stay smooth while you hold or drag a control; the editor no longer burns a CPU core doing it. ([#1411](https://github.com/DerpcatMusic/KONTRA/pull/1411) by @DerpcatMusic)

## 0.6.0 — 2026-11-13

- Scrolling lists in library panels stay inside their box and scroll with the wheel or their scrollbar. ([#1408](https://github.com/DerpcatMusic/KONTRA/pull/1408) by @DerpcatMusic)
- Vector icons in library panels stay sharp at every size. ([#1407](https://github.com/DerpcatMusic/KONTRA/pull/1407) by @DerpcatMusic)
- Number boxes on Kontakt 8 panels (Conflux's Settings: key range, pitch bend, velocity, aftertouch) show their real values. ([#1406](https://github.com/DerpcatMusic/KONTRA/pull/1406) by @DerpcatMusic)

## 0.5.0 — 2026-11-13

- Library panels look sharp at every size; UVI art uses the library's high-resolution pictures. ([#1404](https://github.com/DerpcatMusic/KONTRA/pull/1404) by @DerpcatMusic)
- See what an instrument is loading, how far it is and how much sample memory it takes, as it loads. ([#1405](https://github.com/DerpcatMusic/KONTRA/pull/1405) by @DerpcatMusic)
- test(oracle): drive every panel control, rank breaks by mechanism ([#1374](https://github.com/DerpcatMusic/KONTRA/pull/1374) by @DerpcatMusic)

## 0.4.1 — 2026-11-13

- Conflux's shaper menus now change the wavetable's shape. ([#1402](https://github.com/DerpcatMusic/KONTRA/pull/1402) by @DerpcatMusic)
- Short notes on Kontakt instruments release the way Kontakt releases them. ([#1403](https://github.com/DerpcatMusic/KONTRA/pull/1403) by @DerpcatMusic)
- Attack curves start slowly or quickly the same way as in Kontakt again. ([#1401](https://github.com/DerpcatMusic/KONTRA/pull/1401) by @DerpcatMusic)

## 0.4.0 — 2026-11-13

- Kontakt instruments that vary pan, pitch or level at random per note, or let one modulator drive another (Conflux and others), now play those movements. ([#1395](https://github.com/DerpcatMusic/KONTRA/pull/1395) by @DerpcatMusic)
- Short, fast notes on instruments with a slow attack play at full level, as in Kontakt. ([#1397](https://github.com/DerpcatMusic/KONTRA/pull/1397) by @DerpcatMusic)
- Recalling a Kontakt snapshot into a playing instrument now sounds the same as opening it (Conflux's presets no longer keep the previous patch's filters). ([#1393](https://github.com/DerpcatMusic/KONTRA/pull/1393) by @DerpcatMusic)
- Library LFOs whose panel switches between a tempo-synced and a free rate now follow that switch. ([#1392](https://github.com/DerpcatMusic/KONTRA/pull/1392) by @DerpcatMusic)
- Solid G-EQ boosts and cuts now match Kontakt's level, about 1 dB less than before at full boost. ([#1386](https://github.com/DerpcatMusic/KONTRA/pull/1386) by @DerpcatMusic)
- Compressor and transient knobs on library panels now change the sound, not only the level. ([#1382](https://github.com/DerpcatMusic/KONTRA/pull/1382) by @DerpcatMusic)
- Equalizer knobs on library panels (for example Una Corda's Bass, Mid and Air) and stereo width knobs now change the sound. ([#1381](https://github.com/DerpcatMusic/KONTRA/pull/1381) by @DerpcatMusic)
- Short notes in Kontakt libraries no longer play a soft extra attack before the real one (Areia Spiccato Fast and others that use a curved attack). ([#1373](https://github.com/DerpcatMusic/KONTRA/pull/1373) by @DerpcatMusic)
- Scripts' fade_in and fade_out now honour Kontakt 8's fade curves (equal power, S-curve, exponential, logarithmic). ([#1369](https://github.com/DerpcatMusic/KONTRA/pull/1369) by @DerpcatMusic)
- The algorithmic reverb uses about a third less CPU on dense string patches, with identical sound. ([#1365](https://github.com/DerpcatMusic/KONTRA/pull/1365) by @DerpcatMusic)
- Starting notes and script UI updates no longer allocate memory on the audio thread, removing a source of dropouts under load. ([#1362](https://github.com/DerpcatMusic/KONTRA/pull/1362) by @DerpcatMusic)
- Deleting the preset a part plays now shows the part's instrument name, marked changed, instead of the deleted preset's name. ([#1359](https://github.com/DerpcatMusic/KONTRA/pull/1359) by @DerpcatMusic)
- Flair (Kontakt's comb flanger) now sounds when an instrument's script loads it: Morphology Evolved, Conflux and Shreddage can switch its chords and modes and sweep it as Kontakt does. ([#1352](https://github.com/DerpcatMusic/KONTRA/pull/1352) by @DerpcatMusic)
- Dolce's reverb and ANALOG's echo cost less CPU while playing; the sound is unchanged. ([#1344](https://github.com/DerpcatMusic/KONTRA/pull/1344) by @DerpcatMusic)
- Conflux and other Kontakt 8 native_ui pages show their interface again. ([#1342](https://github.com/DerpcatMusic/KONTRA/pull/1342) by @DerpcatMusic)
- none (internal). ([#1343](https://github.com/DerpcatMusic/KONTRA/pull/1343) by @DerpcatMusic)
- Scripts can move zones to other groups and key ranges (`set_zone_par`), and a knob's declared range reads back: Conflux's wavetable selector now changes the sound. ([#1335](https://github.com/DerpcatMusic/KONTRA/pull/1335) by @DerpcatMusic)
- Kontakt instruments whose scripts divide by zero now set the same values as in Kontakt (Dolce's release samples start where Kontakt starts them; the tail no longer runs 2–5 dB quiet). ([#1333](https://github.com/DerpcatMusic/KONTRA/pull/1333) by @DerpcatMusic)
- UVI instruments: chords played within a millisecond keep every note on scripted legato instruments (VWinds). ([#1331](https://github.com/DerpcatMusic/KONTRA/pull/1331) by @DerpcatMusic)
- Skreamer, Saturation, Distortion and Lo-Fi now play in instrument and bus effect racks too (Una Corda's Vintage page). ([#1329](https://github.com/DerpcatMusic/KONTRA/pull/1329) by @DerpcatMusic)
- Legato scripts that release chords play their release samples like Kontakt (louder, fuller chord tails), and very short script waits no longer slow script timing down. ([#1324](https://github.com/DerpcatMusic/KONTRA/pull/1324) by @DerpcatMusic)
- Release tails of Pacific and Dolce strings now fade like Kontakt's instead of ringing on, and fast repeated notes keep their level. ([#1323](https://github.com/DerpcatMusic/KONTRA/pull/1323) by @DerpcatMusic)
- Lua scripts in UVI programs no longer stall the audio thread while another script or instance loads, and Augmented Orchestra's scripts no longer fall back to the system allocator during playback. ([#1318](https://github.com/DerpcatMusic/KONTRA/pull/1318) by @DerpcatMusic)
- Notes no longer go missing when a script plays more notes in one go than the instrument's note table holds. ([#1305](https://github.com/DerpcatMusic/KONTRA/pull/1305) by @DerpcatMusic)
- Una Corda's sustain pedal holds notes again; script slots' conditions follow the KSP manual. ([#1303](https://github.com/DerpcatMusic/KONTRA/pull/1303) by @DerpcatMusic)
- none (internal refactor). ([#1269](https://github.com/DerpcatMusic/KONTRA/pull/1269) by @DerpcatMusic)
- none (internal). ([#1264](https://github.com/DerpcatMusic/KONTRA/pull/1264) by @DerpcatMusic)
- Scripted stutter and legato crossfades that fade notes out and back in now play; stolen and choked notes fade out in a straight 10 ms line. ([#1229](https://github.com/DerpcatMusic/KONTRA/pull/1229) by @DerpcatMusic)
- Libraries whose scripts play their own release samples, such as Afflatus Brass, now fade release samples with the script's release time instead of letting them ring at full level. ([#1253](https://github.com/DerpcatMusic/KONTRA/pull/1253) by @DerpcatMusic)
- none (internal). ([#1259](https://github.com/DerpcatMusic/KONTRA/pull/1259) by @DerpcatMusic)
- Trumpets and other libraries that play release samples from their scripts no longer start each release sample several times; note onsets are on time again. ([#1243](https://github.com/DerpcatMusic/KONTRA/pull/1243) by @DerpcatMusic)
- Curved modulation crossfades follow the saved shape more closely. ([#1228](https://github.com/DerpcatMusic/KONTRA/pull/1228) by @DerpcatMusic)
- Pacific's short and repeated notes play their release samples at the right level, and Shreddage no longer loses notes. ([#1210](https://github.com/DerpcatMusic/KONTRA/pull/1210) by @DerpcatMusic)
- Sampled reverbs retain their intended level when their response uses a different sample rate. ([#1199](https://github.com/DerpcatMusic/KONTRA/pull/1199) by @DerpcatMusic)
- Stereo sampled reverbs now keep the intended level when their channels have different strengths. ([#1197](https://github.com/DerpcatMusic/KONTRA/pull/1197) by @DerpcatMusic)
- Legacy Chorus now produces its delayed, animated sound. ([#1191](https://github.com/DerpcatMusic/KONTRA/pull/1191) by @DerpcatMusic)
- Conflux's panel lays out its Settings and Edit pages and its macro names properly, and switching pages no longer marks the preset changed. ([#1394](https://github.com/DerpcatMusic/KONTRA/pull/1394) by @DerpcatMusic)
- Settings → Kontakt snapshots lets you choose the folder your Kontakt user snapshots are in, and says when none is set. ([#1372](https://github.com/DerpcatMusic/KONTRA/pull/1372) by @DerpcatMusic)
- Double-clicking a snapshot or your own preset in the browser now plays it in the focused part, as Kontakt does; instruments still open in a new part. ([#1371](https://github.com/DerpcatMusic/KONTRA/pull/1371) by @DerpcatMusic)
- The wheel over an open menu or dialog scrolls it and no longer turns the instrument's controls underneath; long preset lists scroll and follow the arrow keys. ([#1363](https://github.com/DerpcatMusic/KONTRA/pull/1363) by @DerpcatMusic)
- Kontakt's own user snapshots appear under "User Content" in the browser. ([#1357](https://github.com/DerpcatMusic/KONTRA/pull/1357) by @DerpcatMusic)
- Conflux's interface responds: its tabs switch pages, its knobs and the Mod strip move their controls, and its knob pointers turn. ([#1349](https://github.com/DerpcatMusic/KONTRA/pull/1349) by @DerpcatMusic)
- Conflux and other Kontakt 8 libraries with a native_ui page now show their own interface. ([#1336](https://github.com/DerpcatMusic/KONTRA/pull/1336) by @DerpcatMusic)
- UVI instrument menus that change their items (preset and layer lists) update in the editor without reloading the instrument. ([#1330](https://github.com/DerpcatMusic/KONTRA/pull/1330) by @DerpcatMusic)
- Augmented Orchestra shows its own face (layer art, crossfaders, single tab captions); UVI level meters animate; Kontakt waveform displays and status messages show. ([#1321](https://github.com/DerpcatMusic/KONTRA/pull/1321) by @DerpcatMusic)
- Kontakt library headings and controls more closely match their original appearance. ([#1271](https://github.com/DerpcatMusic/KONTRA/pull/1271) by @DerpcatMusic)
- Kontakt library controls have clearer text and more faithful stock artwork. ([#1267](https://github.com/DerpcatMusic/KONTRA/pull/1267) by @DerpcatMusic)
- Level meters fall smoothly and hold their peak like Kontakt's; holding a value field's arrow keeps stepping. ([#1261](https://github.com/DerpcatMusic/KONTRA/pull/1261) by @DerpcatMusic)
- Value edits show Kontakt's step arrows on hover when a script enables them; level meters use Kontakt's scale and show their own channel and instrument bus; text fields show the text a script writes. ([#1256](https://github.com/DerpcatMusic/KONTRA/pull/1256) by @DerpcatMusic)
- Centered Kontakt labels better match the installed library interface. ([#1257](https://github.com/DerpcatMusic/KONTRA/pull/1257) by @DerpcatMusic)
- Kontakt value fields use the installed stock artwork when available. ([#1254](https://github.com/DerpcatMusic/KONTRA/pull/1254) by @DerpcatMusic)
- Library pictures stay clearer when enlarged. ([#1250](https://github.com/DerpcatMusic/KONTRA/pull/1250) by @DerpcatMusic)
- none (internal crate split). ([#1247](https://github.com/DerpcatMusic/KONTRA/pull/1247) by @DerpcatMusic)
- Kontakt articulation labels display tabbed text cleanly. ([#1245](https://github.com/DerpcatMusic/KONTRA/pull/1245) by @DerpcatMusic)
- Libraries with built-in arpeggiators or harmonizers now show the notes they play on the keyboard. ([#1240](https://github.com/DerpcatMusic/KONTRA/pull/1240) by @DerpcatMusic)
- Hovering a keyswitch on the keyboard shows the name the library gives it. ([#1236](https://github.com/DerpcatMusic/KONTRA/pull/1236) by @DerpcatMusic)
- Lit stock buttons and their text use the installed artwork when available. ([#1234](https://github.com/DerpcatMusic/KONTRA/pull/1234) by @DerpcatMusic)
- Library knobs, sliders and number boxes now drag at Kontakt's speed, tables can be drawn on, a double-click on a number box lets you type a value, and text fields take typing. ([#1226](https://github.com/DerpcatMusic/KONTRA/pull/1226) by @DerpcatMusic)
- Stock knobs look sharper and their layers line up at high zoom. ([#1231](https://github.com/DerpcatMusic/KONTRA/pull/1231) by @DerpcatMusic)
- Kontakt stock value fields use their installed bitmap font when available. ([#1227](https://github.com/DerpcatMusic/KONTRA/pull/1227) by @DerpcatMusic)
- KONTRA automatically finds installed Kontakt knob artwork and positions its highlight correctly. ([#1220](https://github.com/DerpcatMusic/KONTRA/pull/1220) by @DerpcatMusic)
- Stock knobs can use artwork from your installed Kontakt when configured. ([#1215](https://github.com/DerpcatMusic/KONTRA/pull/1215) by @DerpcatMusic)
- The keyboard shows the key colours a Kontakt instrument sets for its keyswitches and playable range. ([#1212](https://github.com/DerpcatMusic/KONTRA/pull/1212) by @DerpcatMusic)
- Kontakt instrument controls that react to Shift-, Alt- or Ctrl-clicks now see those keys. ([#1207](https://github.com/DerpcatMusic/KONTRA/pull/1207) by @DerpcatMusic)
- Library pictures in loose resource folders load when their names differ only in letter case. ([#1209](https://github.com/DerpcatMusic/KONTRA/pull/1209) by @DerpcatMusic)
- Library buttons show their held artwork and script faces use a consistent drawing plan. ([#1208](https://github.com/DerpcatMusic/KONTRA/pull/1208) by @DerpcatMusic)
- Keyswitch names and other multi-line labels now appear on separate lines. ([#1194](https://github.com/DerpcatMusic/KONTRA/pull/1194) by @DerpcatMusic)
- Hovering a control on a Kontakt instrument's panel shows the instrument's own help text. ([#1193](https://github.com/DerpcatMusic/KONTRA/pull/1193) by @DerpcatMusic)
- Script switches in Kontakt instruments now react when you press the mouse, as in Kontakt. ([#1190](https://github.com/DerpcatMusic/KONTRA/pull/1190) by @DerpcatMusic)
- Wavetable instruments (Conflux) now respond to their modulation of wavetable position and the two forms, per voice, and scripts can move those controls live. ([#1366](https://github.com/DerpcatMusic/KONTRA/pull/1366) by @DerpcatMusic)
- Kontakt instruments' CC 7 volume now follows Kontakt's exact curve for each CC 7 range setting. ([#1364](https://github.com/DerpcatMusic/KONTRA/pull/1364) by @DerpcatMusic)
- Phasis now runs Kontakt's real structure (feedback loop, rounded LFO, level law, Ultra), and its sweep matches Kontakt through a steady tone. ([#1360](https://github.com/DerpcatMusic/KONTRA/pull/1360) by @DerpcatMusic)
- Choral now runs Kontakt's real structure: seven voices, Scatter, the per-voice filters and the second LFO. Its chorus sweep matches Kontakt's sample by sample. ([#1358](https://github.com/DerpcatMusic/KONTRA/pull/1358) by @DerpcatMusic)
- Kontakt user snapshots (User Content) now show in the browser and preset picker with their instruments, read only. ([#1351](https://github.com/DerpcatMusic/KONTRA/pull/1351) by @DerpcatMusic)
- Kontakt snapshots now recall their effects (insert/send/bus/main racks and group effects), without a glitch while notes play. ([#1348](https://github.com/DerpcatMusic/KONTRA/pull/1348) by @DerpcatMusic)
- Kontakt's Tape Saturator now plays, built stage by stage from Kontakt's own processing, with gain, warmth and roll-off responding to instrument scripts. ([#1346](https://github.com/DerpcatMusic/KONTRA/pull/1346) by @DerpcatMusic)
- Choral (Kontakt's chorus) now sounds when an instrument's script loads it: Morphology Evolved, Conflux and Shreddage's effect pages switch its mode and move its voices, width, depth, rate, feedback and mix as Kontakt does. ([#1345](https://github.com/DerpcatMusic/KONTRA/pull/1345) by @DerpcatMusic)
- Phasis (Kontakt's notch phaser) now sounds when an instrument's script loads it: Morphology Evolved, Conflux and Shreddage's effect pages move its notches, centre, spread, rate and mix as Kontakt does. ([#1340](https://github.com/DerpcatMusic/KONTRA/pull/1340) by @DerpcatMusic)
- A key-position (or velocity, or controller) route into a modulation envelope's attack now shapes that envelope per note, as in Kontakt. ([#1339](https://github.com/DerpcatMusic/KONTRA/pull/1339) by @DerpcatMusic)
- Una Corda's Skreamer knobs now move the sound, its presets without a filter load, and UVI programs that rewrite thousands of modulation amounts in one knob move (W Strings Harmonic's pitch-bend range) do it in half the time, without allocating. ([#1338](https://github.com/DerpcatMusic/KONTRA/pull/1338) by @DerpcatMusic)
- none (internal). ([#1337](https://github.com/DerpcatMusic/KONTRA/pull/1337) by @DerpcatMusic)
- Save your own presets from the part header (Save, Save as, Rename, Delete); they appear in the library's User folder and reload with your project. ([#1328](https://github.com/DerpcatMusic/KONTRA/pull/1328) by @DerpcatMusic)
- Fixes an audio dropout when VWinds programs load their default preset; library data files with tabs or accented characters now load. ([#1332](https://github.com/DerpcatMusic/KONTRA/pull/1332) by @DerpcatMusic)
- Script-driven Solid Bus Comp and Solid G-EQ knobs combine correctly in any order. The G-EQ bell switches and gain readouts work. Instruments restore their saved state exactly as Kontakt does on load and on project reload. ([#1327](https://github.com/DerpcatMusic/KONTRA/pull/1327) by @DerpcatMusic)
- The part header follows an instrument's own preset browser (ANALOG STRINGS' arrows name the snapshot in the header), and marks a preset with `*` once you change it; a saved project keeps the mark. ([#1326](https://github.com/DerpcatMusic/KONTRA/pull/1326) by @DerpcatMusic)
- UVI libraries' own preset menus (VWinds, Augmented Orchestra's arpeggiator) now load their presets, and the part header always shows which preset is playing. ([#1320](https://github.com/DerpcatMusic/KONTRA/pull/1320) by @DerpcatMusic)
- Una Corda's compressor page now moves the compressor; scripts that address an effect slot the wrong way no longer count as errors. ([#1322](https://github.com/DerpcatMusic/KONTRA/pull/1322) by @DerpcatMusic)
- Library scripts now find zones by name, read zone settings and slot effect types, and set LFO phase and retrigger. ([#1310](https://github.com/DerpcatMusic/KONTRA/pull/1310) by @DerpcatMusic)
- Libraries with a nested product, picture fields that run together, and scripts that call `hide_inst_icon` or read `%NOTE_DURATION` now load and draw correctly. ([#1307](https://github.com/DerpcatMusic/KONTRA/pull/1307) by @DerpcatMusic)
- Big chords on a full voice limit no longer click, and more library scripts read and set their modulation depths. ([#1306](https://github.com/DerpcatMusic/KONTRA/pull/1306) by @DerpcatMusic)
- Kontakt scripts can load a Solid Bus Comp into an effect slot. ([#1304](https://github.com/DerpcatMusic/KONTRA/pull/1304) by @DerpcatMusic)
- Library scripts that react to polyphonic aftertouch or refresh their panel after a change now work, and scripts that share many values across slots no longer risk audio dropouts. ([#1302](https://github.com/DerpcatMusic/KONTRA/pull/1302) by @DerpcatMusic)
- tests and tools no longer leave saved preset files that change what instruments load; only the plugin saves. ([#1270](https://github.com/DerpcatMusic/KONTRA/pull/1270) by @DerpcatMusic)
- scripts that open a file dialog or save to their Data folder get an answer instead of waiting forever, and an array file larger than its array no longer loads. ([#1268](https://github.com/DerpcatMusic/KONTRA/pull/1268) by @DerpcatMusic)
- Library scripts that ask about effects an instrument doesn't have now get Kontakt's answer, so their panels and logic behave as in Kontakt. ([#1224](https://github.com/DerpcatMusic/KONTRA/pull/1224) by @DerpcatMusic)
- switching snapshots or instruments fades out instead of clicking, a preset save that fails is reported to the instrument's script, and array files a script names are read only up to 32 MiB. ([#1262](https://github.com/DerpcatMusic/KONTRA/pull/1262) by @DerpcatMusic)
- ANALOG STRINGS loads its rhythm presets and its preset browser, CHORUS saves its presets (to KONTRA's own data folder, never into the library), and scripts that re-read saved values after a snapshot switch get the snapshot's values. ([#1255](https://github.com/DerpcatMusic/KONTRA/pull/1255) by @DerpcatMusic)
- Libraries that look for optional user data files at load (such as Areia) now get Kontakt's "file not found" answer instead of no answer. ([#1242](https://github.com/DerpcatMusic/KONTRA/pull/1242) by @DerpcatMusic)
- Instruments can find factory data when a compatible installation is present. ([#1230](https://github.com/DerpcatMusic/KONTRA/pull/1230) by @DerpcatMusic)
- More Kontakt scripts can locate their library folder. ([#1223](https://github.com/DerpcatMusic/KONTRA/pull/1223) by @DerpcatMusic)
- Kontakt scripts can now find the folder containing their instrument. ([#1218](https://github.com/DerpcatMusic/KONTRA/pull/1218) by @DerpcatMusic)
- More Kontakt instruments can load their resource-backed text arrays. ([#1214](https://github.com/DerpcatMusic/KONTRA/pull/1214) by @DerpcatMusic)
- VWinds controls now load in programs with large instrument trees. ([#1213](https://github.com/DerpcatMusic/KONTRA/pull/1213) by @DerpcatMusic)
- Knobs, sliders and number boxes in library scripts start at their lowest setting, as in Kontakt. ([#1202](https://github.com/DerpcatMusic/KONTRA/pull/1202) by @DerpcatMusic)
- Streaming sampled instruments uses about a tenth less CPU, with identical sound and no extra memory. ([#1391](https://github.com/DerpcatMusic/KONTRA/pull/1391) by @DerpcatMusic)
- The Choral chorus uses about 40 % less CPU, and moving its Feedback and Scatter knobs no longer steps. ([#1387](https://github.com/DerpcatMusic/KONTRA/pull/1387) by @DerpcatMusic)
- The Phasis phaser uses about a third of the CPU it did, with no audible change. ([#1385](https://github.com/DerpcatMusic/KONTRA/pull/1385) by @DerpcatMusic)
- The algorithmic reverb uses a little less CPU, with identical sound. ([#1383](https://github.com/DerpcatMusic/KONTRA/pull/1383) by @DerpcatMusic)
- Moving a modulation wheel or a script-driven modulation on a sounding chord costs much less CPU. ([#1334](https://github.com/DerpcatMusic/KONTRA/pull/1334) by @DerpcatMusic)
- Script-heavy instruments (Areia, Dolce, ANALOG STRINGS) spend about half as much CPU running their scripts, so chords and control moves cause far smaller CPU spikes, and Kontakt instruments with saved script values load faster. ([#1325](https://github.com/DerpcatMusic/KONTRA/pull/1325) by @DerpcatMusic)
- Libraries with large script memories (Audio Imperia Areia, Chorus, Dolce) no longer spike the CPU on every tick, project saves are much faster, and the header shows the slowest recent block as Peak. ([#1319](https://github.com/DerpcatMusic/KONTRA/pull/1319) by @DerpcatMusic)
- Instruments with reverbs, convolution or delays use much less CPU when nothing is playing, and Dolce-style legato scripts no longer cause CPU spikes on chords. ([#1317](https://github.com/DerpcatMusic/KONTRA/pull/1317) by @DerpcatMusic)
- Moving a control on a large instrument (Areia, Chorus) no longer costs several CPU cores, and a closed editor costs nothing. ([#1315](https://github.com/DerpcatMusic/KONTRA/pull/1315) by @DerpcatMusic)
- Switching between snapshots of a loaded Kontakt instrument is now near-instant and no longer reloads the instrument. ([#1233](https://github.com/DerpcatMusic/KONTRA/pull/1233) by @DerpcatMusic)
- Releases now come every few changes with a full changelog, numbered 0.4.0 onward. ([#1400](https://github.com/DerpcatMusic/KONTRA/pull/1400) by @DerpcatMusic)
- Send-level knobs on library panels (for example a reverb amount) now change the sound. ([#1384](https://github.com/DerpcatMusic/KONTRA/pull/1384) by @DerpcatMusic)
- Conflux and other Kontakt libraries no longer offer multis as presets that fail to load, and a snapshot opened on its own keeps the instrument's own settings (Conflux's effects stay on). ([#1389](https://github.com/DerpcatMusic/KONTRA/pull/1389) by @DerpcatMusic)
- Install KONTRA on Intel or Apple Silicon Macs with a signed universal installer, and use the included install scripts on Windows and Linux. ([#1239](https://github.com/DerpcatMusic/KONTRA/pull/1239) by @DerpcatMusic)
- Downloads now always give you the newest build, updated with every change. ([#1221](https://github.com/DerpcatMusic/KONTRA/pull/1221) by @DerpcatMusic)
- feat(inspect): show instrument load stage timings ([#1396](https://github.com/DerpcatMusic/KONTRA/pull/1396) by @DerpcatMusic)
- build: restore the Windows graphics dependency versions in the lockfile ([#1398](https://github.com/DerpcatMusic/KONTRA/pull/1398) by @DerpcatMusic)
- ui: CPU snapshots render through mui-headless ([#1390](https://github.com/DerpcatMusic/KONTRA/pull/1390) by @DerpcatMusic)
- deps: lock latest MUI and moose main ([#1388](https://github.com/DerpcatMusic/KONTRA/pull/1388) by @DerpcatMusic)
- docs: a Progress page with measured numbers, plain issue forms, README ([#1375](https://github.com/DerpcatMusic/KONTRA/pull/1375) by @DerpcatMusic)
- chore(tools): count heap allocations inside the audio callback ([#1368](https://github.com/DerpcatMusic/KONTRA/pull/1368) by @DerpcatMusic)
- ci: readable versions, a daily release with history, credited notes ([#1370](https://github.com/DerpcatMusic/KONTRA/pull/1370) by @DerpcatMusic)
- oracle: scoreboard uses this worktree's build, onset against the first note's own peak, per-library score cache ([#1367](https://github.com/DerpcatMusic/KONTRA/pull/1367) by @DerpcatMusic)
- oracle: scoreboard total names its binary, build time and spread root (78 vs 80 was --spread) ([#1361](https://github.com/DerpcatMusic/KONTRA/pull/1361) by @DerpcatMusic)
- oracle: repeats spread moves note-ons within Kontakt's 0-3 ms MIDI spread (Pacific/Una Corda build-up is phase, not a voice rule) ([#1354](https://github.com/DerpcatMusic/KONTRA/pull/1354) by @DerpcatMusic)
- Kontakt loads are deterministic: resource archive members in the file's own order ([#1353](https://github.com/DerpcatMusic/KONTRA/pull/1353) by @DerpcatMusic)
- tools(perf): instructions and cycles per block over one playtest segment ([#1355](https://github.com/DerpcatMusic/KONTRA/pull/1355) by @DerpcatMusic)
- Flair: the 4 Hz LFO pole confirmed by a steady-sine Kontakt render; stimrender sine_steady ([#1356](https://github.com/DerpcatMusic/KONTRA/pull/1356) by @DerpcatMusic)
- ci: no size-count conflicts between PRs; site build only for site changes ([#1350](https://github.com/DerpcatMusic/KONTRA/pull/1350) by @DerpcatMusic)
- Scoreboard: key-up fits at the bound are unfitted; fast repeats scored against v3's own key-up jitter ([#1347](https://github.com/DerpcatMusic/KONTRA/pull/1347) by @DerpcatMusic)
- ci: retire the framework source gate (red since #1316) ([#1341](https://github.com/DerpcatMusic/KONTRA/pull/1341) by @DerpcatMusic)
- Track MUI and moose main instead of pinned revs ([#1316](https://github.com/DerpcatMusic/KONTRA/pull/1316) by @DerpcatMusic)
- Migrate KONTRA to the shared MUI and Moose stack ([#1298](https://github.com/DerpcatMusic/KONTRA/pull/1298) by @DerpcatMusic)
- Laws: the 7 laws with no Kontakt8.exe address read as measured only ([#1265](https://github.com/DerpcatMusic/KONTRA/pull/1265) by @DerpcatMusic)
- Laws: exact inverses replace hand-written unit conversions ([#1263](https://github.com/DerpcatMusic/KONTRA/pull/1263) by @DerpcatMusic)
- Laws: finish the migration, one name per law, binary-verified addresses, kontra-inspect law ([#1258](https://github.com/DerpcatMusic/KONTRA/pull/1258) by @DerpcatMusic)
- Curves are data: every parameter law goes through the Law evaluator ([#1252](https://github.com/DerpcatMusic/KONTRA/pull/1252) by @DerpcatMusic)
- Name Kontakt pitch-index parameter laws ([#1248](https://github.com/DerpcatMusic/KONTRA/pull/1248) by @DerpcatMusic)
- Name Kontakt exponential parameter laws ([#1246](https://github.com/DerpcatMusic/KONTRA/pull/1246) by @DerpcatMusic)
- Name Kontakt integer-power parameter laws ([#1244](https://github.com/DerpcatMusic/KONTRA/pull/1244) by @DerpcatMusic)
- Name UVI key mapping laws ([#1241](https://github.com/DerpcatMusic/KONTRA/pull/1241) by @DerpcatMusic)
- Express UVI fades as a shared sine law ([#1238](https://github.com/DerpcatMusic/KONTRA/pull/1238) by @DerpcatMusic)
- Engine split: the script control store moves into kontra-script-ui (pure move) ([#1237](https://github.com/DerpcatMusic/KONTRA/pull/1237) by @DerpcatMusic)
- ci: CodeRabbit's vendor-name rule matches README rule 2 ([#1235](https://github.com/DerpcatMusic/KONTRA/pull/1235) by @DerpcatMusic)
- Voice lifecycle 2: every early end fades through one function, over Kontakt's steal time ([#1219](https://github.com/DerpcatMusic/KONTRA/pull/1219) by @DerpcatMusic)
- Name the normalized bipolar control law ([#1225](https://github.com/DerpcatMusic/KONTRA/pull/1225) by @DerpcatMusic)
- ci: a running release finishes; only the newest waiting main runs next ([#1222](https://github.com/DerpcatMusic/KONTRA/pull/1222) by @DerpcatMusic)
- ci: CodeRabbit reviews PRs into main and pushes for the stronger design ([#1217](https://github.com/DerpcatMusic/KONTRA/pull/1217) by @DerpcatMusic)
- fix: a library scan that finds nothing new draws no frame (idle-tick flake) ([#1216](https://github.com/DerpcatMusic/KONTRA/pull/1216) by @DerpcatMusic)
- Centralize Kontakt modulation law entries ([#1211](https://github.com/DerpcatMusic/KONTRA/pull/1211) by @DerpcatMusic)
- Link internal changes in nightly notes ([#1206](https://github.com/DerpcatMusic/KONTRA/pull/1206) by @DerpcatMusic)
- Remove the retired v2 workspace ([#1205](https://github.com/DerpcatMusic/KONTRA/pull/1205) by @DerpcatMusic)
- docs: group corpus gaps by feature ([#1204](https://github.com/DerpcatMusic/KONTRA/pull/1204) by @DerpcatMusic)
- docs: rank corpus load gaps by affected libraries ([#1203](https://github.com/DerpcatMusic/KONTRA/pull/1203) by @DerpcatMusic)
- Use v3 evidence for website compatibility ([#1200](https://github.com/DerpcatMusic/KONTRA/pull/1200) by @DerpcatMusic)
- ci: release archives run on Python 3.13 ([#1201](https://github.com/DerpcatMusic/KONTRA/pull/1201) by @DerpcatMusic)
- Remove the v2 DSP benchmark crate ([#1198](https://github.com/DerpcatMusic/KONTRA/pull/1198) by @DerpcatMusic)
- Retire the v2 playback probe ([#1196](https://github.com/DerpcatMusic/KONTRA/pull/1196) by @DerpcatMusic)
- Remove obsolete v2 analysis tools ([#1195](https://github.com/DerpcatMusic/KONTRA/pull/1195) by @DerpcatMusic)
- Retire legacy v2 CI workflows ([#1192](https://github.com/DerpcatMusic/KONTRA/pull/1192) by @DerpcatMusic)
