# Changelog

Human-reviewed changes belong here before release. Dates and stable release headings
are added when a release is actually published; commit history is not a changelog.

## 0.3.0 — unreleased

### Added

- One package version and build identity across CLI, standalone, plugin metadata,
  About, diagnostics and package manifests. Identity includes the actual full Git
  revision, optional export source revision, UTC timestamp, target, profile and features.
- Deliberate SemVer release preparation and reproducible nightly prereleases, with
  focused local checks and documented contributor workflow.
- Searchable structured Logs with failed-load context, source locations, bounded
  recent history, and private support reports that include retained crash journals.
  Reports identify omitted events, write errors and partial journal coverage.
- Script condition inheritance between successfully initialized slots, native
  pedal/release conditions, and additional supported script syntax and zone queries.
- Native NKSN snapshots applied to an explicit base instrument, including supported
  saved controls, instrument/group FX, known envelopes and modulation assignments.
  Three Analog Strings snapshots have production state checks and two decoded IRs each.
- Bounded asynchronous `load_array_str` reads and explicit-path `save_array_str`
  writes for typed NKA files, with retained array/UI revisions, completion callbacks
  and header, value, resource, capacity and write diagnostics. Three real Analog
  factory loads and the retained rhythm table are verified; read callback/install/
  refresh paths record zero heap operations over 7,800 blocks of 128 samples. Two
  unchanged-script browser-star callbacks write a copied favorites file with fresh
  byte readback, restore its original bytes and record zero audio heap operations
  over 6,300 blocks. All 11 original metadata files remain unchanged. Mode-based
  saves and external file dialogs remain unavailable, returning status 0.
- AHDSR/Flex envelope record writers that preserve opaque metadata. The selected
  782-file / 788-program corpus verifies 230,627 byte-exact record roundtrips and
  edited-value readbacks with zero errors.
- Typed native LFO parsing and writing for known fields, with 9,600 actual-library
  chunks round-tripping byte-for-byte. Import preserves this metadata; LFO clocks,
  waveform generation and routing remain unsupported, some tables remain raw,
  and typed rate/phase metadata does not establish DSP behavior.
- Named bitmap font loading for 256-glyph Windows-1252 RGBA strips declared during
  initialization. Actual Areia Advanced resources verify two 256-glyph, 14-pixel
  fonts with variable advances and the native gray/orange switch-state change;
  broader font compatibility remains unverified.
- Internal slot-to-slot MIDI2 note-controller callbacks, registered/assignable/bend
  values, forwarding, waits and startup persistence delivery. External MIDI2 input
  and multi-script MIDI-input callbacks remain absent; the selected 13-script
  census contains no uses, so this does not establish an actual-library benefit.
- Lossless native v2 filename-table records preserve segment kinds, UTF-16 units,
  full timestamps, uninterpreted sample records and trailing metadata. Three actual
  tables covering 102,026 sample references pass byte-exact and edited readback checks.
  Existing raw chunk writing was already lossless; this adds typed editing access.
- Opt-in native UI timing capture records one bounded ten-second drag window and
  summarizes it on the diagnostics worker. It measures application callback and
  presentation-submission time, without forced GPU synchronization or display-FPS claims.

### Fixed

- Host position, tempo, play/stop and time signature reach script callbacks before
  MIDI input. Song position advances within the block at callback sample offsets;
  bar duration follows the host meter. Start/stop listener subscriptions are independent.
  Beat listeners still use elapsed-clock phase; missing host timeline validity is
  an upstream limitation, so unavailable beat position cannot be distinguished from zero.
- Instrument replacement clears previous script state and convolution settings;
  source epochs on both live-request and snapshot queues reject stale updates.
  A native Areia-to-CHORUS transition verifies the new logo, controls, header and
  playable range, resolving the observed cross-instrument state contamination.
- Physical note ownership, delayed callback cancellation, MIDI stop ordering and
  selective sound-off across articulation channels sharing one engine channel.
- Sustain release bookkeeping, generated-note lifecycle and MPE expression/tuning
  routing in the covered playback paths.
- Script-selected native release groups now start for all note durations when
  automatic release triggering is bypassed. Vista Harp and two Pacific Solo Harp
  presets restore exactly four damper voices on ordinary and pedal releases;
  their envelope/swell behavior still lacks Kontakt reference validation.
- Bounded physical-input CC120 cleanup and Panic termination for previously lingering
  Areia, Dolce and CHORUS script lifetimes. Reset restores recorded library device
  defaults alongside standard controllers in native playback and scripts, fixing
  all 12 previously silent legato fresh-note cases in the focused rerun.
  The six-patch, 24-case run covers 670,704 blocks with zero measured render-thread
  heap operations, nonfinite samples, drops, underruns or offline duration overruns;
  every fresh post-Panic voice has positive gain/envelope levels and final held,
  voice and pending state is clear. Three renders with effects disabled confirm
  audible CC121 recovery and baseline-matching CC120/Panic recovery.
- Zone ID mapping after import filtering and source-parser allocations in validated
  preset/container paths.
- Browser scaling/layout and selected DSP effect processing paths.
- RV2 Reverb Time captions use the existing DSP time conversion; the checked
  Areia Advanced state displays 1099.5 ms. This does not establish Kontakt's law.
- Original interface wallpaper page offsets and viewport rendering, authored fader
  travel, factory font color/state inheritance and explicit caption text colors.
  Factory glyphs still use the bundled font approximation.
- Editor publication of live script views through the existing bounded buffers,
  without formatting diagnostic reports or writing journals in editor frames.
- Delayed script callbacks now publish font, caption alignment and text-offset
  changes, including Analog Strings' centered Original-mode volume/FX captions.
  Native Original captures cover 27 playable cases across nine libraries at
  device scale 1.5; selected callbacks are checked, not every control action.
- Warm Vectorized CPU planning drops 78% in one bounded paired Analog trial
  (2.567 to 0.566 ms). This does not establish overall GPU/frame/input latency.
- Scalar edits avoid copying the imported interface under the view lock. A matched
  Analog trial measures mean edit submission at 1.152 to 0.000240 ms; whole observed
  frame means are 4.223 and 4.395 ms, providing no frame-rate improvement evidence.
- Changed publications reuse unchanged control storage; a separate matched Analog
  trial measures publication mean at 0.122 to 0.047 ms. Its final macro update copies
  3 of 934 controls; startup updates copy more. Drawing uses published revisions
  instead of scanning all control properties every frame. Native FPS remains unverified.
- Internal pitch AHDSR routing to voice modulation, selected filter coefficients
  and worker-built convolution cutoff processing. These changes do not establish
  Kontakt parameter-law or sonic equivalence.

### Compatibility

Kontakt preset import, scripts and playback remain partial. Successful import or a
passing synthetic test does not establish sonic parity for every library. Existing
compatibility notes and unsupported-operation diagnostics remain applicable; these
changes do not announce complete format, script or sound equivalence.

The focused offline results do not certify live-host deadlines or every library.
Opaque snapshot source state and unknown saved scalars are warned and remain unapplied.

Analog Strings' live factory-preset and rhythm menus use bounded NKA reads; selected
menu checks do not establish that every action or preset works. The installed
header-favorite preset ID is absent from its registry, preventing that lookup from
updating favorites; no supplied IDs were repaired or compared with Kontakt.
