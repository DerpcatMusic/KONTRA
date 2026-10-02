# Changelog

Human-reviewed changes belong in the unreleased section before publication. Nightly
notes compare those entries with the previous published source, retain known limits,
and include the complete shipped public commit messages and merged PR descriptions.
Each published release manifest also retains its versioned changelog. Frozen entries
below record reviewed source checkpoints; they are not claims about pending work.

## Unreleased

### Added

- Headless `audit-patch` accepts explicit snapshot and program selection while
  using the existing import, script, bank, effect and paced note/chord path.
  It retains structured load stages and diagnostics without opening an editor.

- Experimental Bitwig VST3 project inspection and explicit SavedMulti-to-KONTRA
  migration create a new project copy and report; source bytes are rechecked and
  retained. Shared plug-in-state entries and overlapping device mappings are
  rejected instead of changing multiple devices through one cached state.
- Independent authored performance pages expose each script slot's prepared UI
  buffer. Footer selection keeps callbacks on the selected owning slot; late
  publications and replacement epochs retain that ownership.

- Cached instrument snapshots appear in a compact header preset row with owned
  category labels, alongside the existing explicit snapshot picker/drop workflow.
- Independent user UI zoom preferences and persistent global editor window size.
- Native file-picker callbacks for supported KSP file selectors, with prepared paths
  and retained asynchronous callback routing.
- Imported Creator Tools performance-view controls for supported exported records,
  including null lists for empty menus and the initialization-only constraint.
- Declared JPEG resource names share the image decoder instead of being discarded.
- Prepared rack storage and viewport rows remove the previous fixed part cap; browser
  drops can append parts in the empty canvas. This is a feature, not a fix-count unit.
- Persistent library display aliases use the detached catalog without renaming
  library files. Native Reveal validates its target path before dispatch.
- Bounded source-identity diagnostics identify unsupported Kontakt 8 wavetable
  playback instead of representing it as supported sample playback.

- Every nightly records reviewed Added/Changed/Fixed/Known limits deltas, complete
  shipped public commit messages and merged PR descriptions in its release body and
  versioned manifest. The previous release source is the comparison baseline;
  source checkpoints remain traceable even when exports squash private history.
- Load supported snapshots from the instrument header picker or an explicit
  header drop. Parsing and base validation run on the loader before changing the
  active source; base and snapshot paths survive host state and KONTRA multis.
- Save successfully applied native script parameter edits alongside persistent
  script variables, and seed authored initialization getters during restoration.
  Prepared storage avoids audio-thread growth; refresh visits edited slots rather
  than every default parameter in a large library.
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
- Imported zero-crossfade alternating sample loops share reflected playback mapping
  between resident and streaming readers. Cache and NKI export retain their direction.
  Crossfaded alternating loops retain metadata and warn about forward-crossfade fallback;
  endpoint/interpolation equivalence with Kontakt remains unverified.
- Opt-in native UI timing capture records one bounded ten-second drag window and
  summarizes it on the diagnostics worker. It measures application callback and
  presentation-submission time, without forced GPU synchronization or display-FPS claims.

### Changed

- Rack headers give instrument titles more room beside
  compact MIDI and output routing controls. Combined 900/1180/1920 viewport
  bounds and routing/navigation/mute/remove callback checks pass.

- Logs search is simpler and copying includes complete retained diagnostic details;
  event text is owned before the query changes. Rack header artwork is more visible.
- Encrypted preset access failures explain the lookup boundary; XML fields tolerate
  surrounding whitespace. These diagnostics do not add keys or bypass encryption.
- Group processing follows decoded pre/post Amplifier insert order for supported
  filter and drive stages. Interleaved filter states remain out of shared lanes;
  effect indicators disclose partial processing instead of implying every FX runs.

### Fixed

- Ordinary import preserves readable modulation slots when one bounded sibling
  record is undecodable. Unknown slots keep their positions and precise warnings;
  valid envelopes and later target identities survive. Snapshot decoding remains
  strict, and malformed container boundaries still fail.

- Process all eight native filter/EQ inserts and up to 32 sections instead of
  truncating supported chains. Eight four-band GEQs match the existing rack
  reference with live slot-7 edits and no audio heap operations; fixed state
  increases by 1,248 bytes per voice.
- Format, clone and retire large persistent script/native values outside the
  editor mutex. Revalidate the script epoch before committing a snapshot;
  unchanged host JSON and prepared audio buffers are retained.
- Clicking either diagnostic row text line selects the event, as does its blank
  area. Native Copy all verification retained 148 events and eight source excerpts.
- Rejected zone mappings identify the offending field/value, original ranges,
  source/version, zone, group and sample. Validity checks remain strict.
- Preserve native GPU surface errors and flush startup stages before driver calls.
  This improves blank-editor diagnosis; a Windows driver crash is not reproduced.
- Read generated dependency license JSON explicitly as UTF-8, fixing Windows
  packaging on a CP1252 default locale. The non-ASCII generation regression passes.

- Native group feedback compressor, limiter, Solid Bus
  Compressor and Transient Master stages reuse bounded rack processing at the
  decoded Amplifier split. Rack-reference PCM, native edit/readback and zero-heap
  checks pass; this does not establish native Kontakt parameter or sound equivalence.
- Import and NKI writing retain native group start records.
  Record preservation does not implement every start condition or establish
  arbitrary imported-preset editing.
- Periodic audio snapshots wake a separate managed worker
  during instrument loads. Cumulative playback counters retain their baseline
  across generation changes, preventing repeated totals from appearing as new
  drops or underruns; independent wakeup, bounded handoff and exact delta
  regression checks pass.

- Parse/runtime diagnostics now show readable bounded source context with slot,
  line/column markers and the relevant command arguments. Serialized event data
  keeps the excerpts visible in journal/export and Logs Copy all; both exact
  diagnostic regressions passed on the corrected compiled binary.

- Decode complete extended PCM WAVE format descriptors, including the checked
  20-byte fmt records; two actual supplied samples decode fully. All 15 authored
  descriptor cases and the combined audio regression passed.
- Reveal actual library/log directories after validating the path, including
  extended Windows drive and UNC spellings. Explorer launch remains unverified.
- Resolve relative encrypted-preset paths and whitespace-delimited library access
  fields with actionable missing-data errors; right-key and wrong-key checks pass.
  The unavailable Emotional Piano payload has not been independently validated.
- Accept documented symbolic MAIN/GROUP/INSERT level-meter chain selectors while
  retaining rejection of invalid selectors. This is not a claim that every meter
  source works. The actual checked preset initializes three script slots with
  378, 22 and 1 controls without errors; Lua, unsupported taps and full playback
  remain separate limits.
- Accept Creator Tools null menu lists for exported empty menus. The actual checked
  Conflux resource loads 378 controls in 11 families; 100 callback operations show
  no measured heap operations. Lua UI and unsupported level taps remain explicit.
- Preserve independent script-slot pages and their callback ownership through
  footer changes, delayed publications and replacement epochs. Authored UI/backend
  checks pass; the Circle Bells payload was not available for validation.

- Forward initialization RPN messages after receiving script slots initialize.
- Persist global editor window size instead of losing the saved size between editors.
- Decode native Kontakt 8 flat filename tables and explicit effect-slot identities;
  retain supported records instead of rejecting their valid layout or misreading slots.
- Archive errors retain directory signatures and exact read boundaries. Rejected
  samples and loop bounds retain their actual failure cause and typed skip counters.
- Count KSP faults omitted by the retained source-location cap, and retain command,
  argument, signal and runtime-value context for invalid note/listener operations.
- Dispatch the documented legacy PGS callback spelling.
- Keep the rack welcome drop area and scrollbar gutter stable while accepting
  append drops on the empty canvas.
- Resolve declared JPEG resources and route decoded module envelope bypass and
  supported modern target depths through the existing processing paths.
- Schedule millisecond and beat listeners independently; registration, disabling
  or retuning one clock no longer overwrites the other clock's phase or generation.
- Preserve release tails during offline overload rendering.
- Preserve native group insert order around the Amplifier, including separate
  state for interleaved filters and honest partial-effect indicators.
- Resolve the authored compressor native ID through its documented KSP names.

- Group drive processing retains all eight native insert slots. A third drive
  stage was previously discarded, leaving Analog Strings' Saturation control
  editable without reaching its DSP.
- Original views retain unchanged control subtrees across live publications,
  while changed table rows, active gestures and replacement epochs rebuild.
- Windows editors default to Direct3D 12 instead of implicitly initializing
  Vulkan. Explicit `WGPU_BACKEND` selections remain authoritative; renderer
  startup, adapter details and recoverable failures enter persistent diagnostics.
  This avoids the reported Intel Vulkan path by default, but has not yet been
  verified against that FL Studio crash on the affected machine.
- Live UI refresh skips unchanged menu rows, and repeated identical indexed
  integer writes no longer dirty entire table snapshots. Listener behavior and
  audio work budgets remain unchanged.
- Import reuses one decoded filename table for samples, resources and impulses,
  avoiding three repeated full-table decodes in large instruments.
- Failed or canceled load reports retain their status and cause when diagnostics
  or artwork from the active instrument arrive later.
- Native SV Notch 4 filter type 58 uses the existing four-pole processing path;
  an actual Accordia resident-sample render now changes its PCM with zero render
  heap operations. This does not establish Kontakt sonic equivalence.
- Vectorized views retain broad pictured value graphs and their authored
  callbacks while continuing to replace ordinary knobs and faders.
- Compiled script programs share immutable UI revision-owner maps between
  runtimes. Revisions and mutable values remain local; matched Areia callback
  measurements show unchanged cost, without a runtime-speedup claim.
- Leaving or unfocusing an editor cancels delayed pointer restoration, including
  release events queued before the next frame. Popup menus capture hover and outside
  dismissal clicks so tooltips and underlying controls cannot cover or activate them.
- Solid G-EQ gain captions use the existing DSP's signed decibel conversion instead
  of raw normalized integers; this changes display text without changing its gain law.

- Shared controls recover movement before the drag threshold, so closed physical
  mouse paths return to their starting values at 100%, 150% and 200% display scale.

- Host position, tempo, play/stop and time signature reach script callbacks before
  MIDI input. Song position advances within the block at callback sample offsets;
  bar duration follows the host meter. Start/stop listener subscriptions are independent.
  Beat listeners still use elapsed-clock phase; missing host timeline validity is
  an upstream limitation, so unavailable beat position cannot be distinguished from zero.
- Repeated same-sample group parameter restores use a preallocated address lookup.
  The checked Areia Core F3 channel-overlap burst drops zero writes instead of 854,
  without increasing queue capacity; measured event-plus-render time stays near
  baseline at 3.56 versus 3.53 ms. Distinct-sample timing and latest-value reads remain.
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


### Known limits

- Root's shipping-profile focused checks through `e68d28c` passed 20 library checks
  and all 83 playback checks (4 ignored). NI's 13 focused checks passed. These results
  do not certify every actual library or a Windows/macOS DAW.
- Actual Conflux import/initialization at `0c0019a` reports no initialization
  errors and exposes 378 controls. This does not validate full playback, Lua UI,
  every control or actual meter signals; unsupported level taps remain explicit.
  Circle Bells' multi-page interface was not available; authored page tests do
  not certify that library.
- Source excerpts retain up to five source lines, each clipped at 512 UTF-8 bytes
  without splitting characters, plus labels/column markers. They intentionally
  retain readable source text; full raw scripts are not included in copied reports
  or support bundles. Missing source/locations cannot produce an excerpt.
- Bitwig migration is an experimental partial copy, not a verified replacement
  project: Kontakt automation/static host values are not translated, routing comes
  from an explicit SavedMulti, and original Kontakt bus assignments remain undecoded.
  VST2/CLAP instances are not classified; Bitwig reopening and sonic parity remain
  unverified. Unmapped devices stay unchanged, and shared-state migration is refused.
- Actual module-envelope callback checks retain 483 groups, 480 bypass sources and
  959 of 960 known filter/formant depth targets across three Analog Strings states;
  one opaque target remains unsupported. Finite PCM differences use an authored
  stimulus and replacement sample map with flattened buses and omitted program FX.
  This does not validate stock legacy depth controls or original-map/Kontakt audio.
- Native group insert-order checks cover rack split references, threshold changes,
  subtype retuning and 83 playback cases. Three factory-state probes perform 4,500
  finite resident renders without heap operations; their reference is the same
  engine with authored routing changes. Added inline fields measure 116 bytes per
  voice on the checked Rust 1.98.1 build, not a throughput or parity improvement.
- Wavetable source diagnostics do not implement wavetable DSP. Partial group FX,
  decoded envelopes and selected compressor IDs do not establish Kontakt sonic
  equivalence; unsupported processing remains visible.
- Larger racks remain bounded by memory, voice budgets and host/editor capabilities.
  User zoom, cached snapshots and native file-picker tests do not certify every
  gesture, resource, preset or file-dialog workflow.

### Reviewed source changes

> Forward init RPN messages after receiving script slots initialize

> Simplify Logs search and copy complete retained diagnostics

> Make rack header artwork slightly more visible

> Persist global editor size and add independent UI zoom preferences

> Expose cached instrument snapshots in a compact preset row

> Decode Kontakt 8 flat filename tables and explicit effect slots

> fix(nkx): report directory signatures and read boundaries

> fix(samples): report rejected zone and loop bounds

> fix(diagnostics): count KSP faults omitted by the location cap

> Dispatch the documented legacy PGS callback spelling

> Append browser drops anywhere in the rack empty canvas

> Resolve declared JPEG resource names with shared image decoding

> Preserve native group Amplifier insert split metadata

> fix(load): classify skipped zones by their actual failure cause

> Route decoded module envelope bypass and modern target depths

> feat(ksp): route file selectors through native picker callbacks

> Own event detail text before updating the Logs query

> fix(diagnostics): serialize typed zone skip counters

> Load exported performance-view controls before KSP compilation

> Enforce the documented performance-view initialization constraint

> Keep snapshot categories in owned menu labels

> Check performance-view slot results using the runtime result type

> Keep rack welcome drop area and scrollbar gutter stable

> Inspect bounded source identities and report unsupported Kontakt 8 wavetable playback

> test(ksp): verify selector callbacks and prepared paths

> Accept Creator Tools null lists for empty exported menus

> docs: track generic compatibility gaps and validation boundaries

> fix(ksp): retain command and arguments in note validation faults

> Schedule KSP millisecond and beat listeners independently

> Check independent listener delivery on the allocation-free audio path

> Retain listener command and argument details in bounded faults

> fix(render): retain release tails during offline overload

> Preserve native group insert order around the amplifier

> Report fixed group voice state size in the pipeline proof

> Keep group effect indicators honest about partial processing

> Keep interleaved group filter states out of shared lanes

> Remove the rack part cap with prepared storage and viewport rows

> Resolve the authored compressor native ID through KSP names

> Add persistent library display names and validate native Reveal paths

> Explain encrypted preset access lookup and accept XML field whitespace

> Use the detached catalog for library display aliases

> Record complete nightly notes and count reviewed logical fixes

> fix(audio): decode extended PCM WAVE format descriptors

> Show bounded script source context with parse and runtime diagnostics

> Expose authored performance pages with independent script-slot buffers

> Advance to 0.3.18 for eighteen reviewed fixes

> Use imported tab helper for performance page selectors

> Fix source excerpt regression lease and report serialization

> Accept documented symbolic level-meter chain selectors

> feat(migration): inspect and migrate Bitwig copies with shared-state guard

> Record validated native group pipeline and actual state budget

> docs: record actual module envelope callback validation boundaries

> Describe reviewed source batch and omit unshipped candidates from notes

> Advance to 0.3.22 after four further compatibility fixes pass

> Read source excerpts from serialized diagnostic event data

> Advance to 0.3.23 with verified readable script diagnostics

## 0.3.0-nightly.20261002.g4399f700e590 — 2026-10-02

Public release source: `4399f700e5904114ebfdf84ef50091716f5867c7`.
Reviewed export checkpoint: `e53150559f37407197be3d6182aed9a1c3619e89`.
Previous release source: `521e6954749840ab15c3d9365d7cd3e95a4867ea`.

### Added

- Load supported NKSN snapshots from the instrument header picker or a header
  drop, after loading their base NKI. The loader validates the base and applies
  supported snapshot state before changing the active source. Both source paths
  survive DAW state and KONTRA multis; rejection preserves the existing source
  generation, installed epoch and source services.
- Persist successfully applied native script parameter edits alongside persistent
  variables. Restore authored initialization getters, retain decoded values actually
  applied by the engine, and replay current effect edits after processor rebuilds.
- Process all eight native group drive insert slots. Previously a third drive was
  discarded; actual Analog Strings Tube SHAPE/BYPASS edits now reach both groups'
  DSP. Matched sample renders are finite and change with the edit, with zero render
  heap operations over 2,250 calls per comparison side.

### Changed

- Save active native edits with prepared address storage and a reusable numeric-key
  hasher; refresh edited parameter slots rather than every default parameter.
- Share immutable compiled UI revision-owner maps between runtimes while keeping
  revisions and mutable values local. This reduces duplicate retained metadata;
  native-state persistence separately added about 0.08 ms in the measured Areia case.
- Avoid unchanged menu-row copies and redundant bounded live-refresh passes. Equal
  indexed integer writes leave table revisions current; script callbacks are retained.
- Decode large filename tables once for samples, resources and impulses. All three
  actual-library fingerprint comparisons match the prior implementation.
- Retain unchanged Original control subtrees across live publications. Changed rows,
  active gestures and replacement epochs still rebuild their affected controls.
- Keep compact articulation-mode help readable and preserve broad pictured value
  graphs with authored callbacks in vectorized views.

### Fixed

- Default Windows editors to Direct3D 12 rather than implicit Vulkan. Explicit
  `WGPU_BACKEND` choices remain authoritative. Persist renderer initialization,
  adapter details and recoverable startup failures in diagnostic journals.
- Map native SV Notch 4 filter type 58 to the existing four-pole DSP path. An actual
  Accordia resident-sample render changes its PCM without render heap operations.
- Preserve failed and canceled load status when later active-script diagnostics or
  pending artwork report warnings. A rejected snapshot retains its failure status.
- Replace a cached loading placeholder with the completed failure or empty-source
  state; include loading/status changes in the performance-view cache dependency.
- Preserve NIS/NKS decoder family, cursor/offset, declared lengths, available bytes,
  version and chunk context, including decompression and structured-object errors.
  Recognized malformed NIS files retain the original decoder cause; invalid metadata
  returns an error instead of panicking. Valid synthetic container roundtrips remain
  byte-exact, with truncated-container coverage preserving the underlying EOF cause.
- Make the Logs export regression wait for a newly started request and a re-enabled
  UI scene, rather than accepting a previous completion. It still proves an existing
  destination fails without overwriting the earlier report.
- Extend native-edit, snapshot rejection and save/reload fixtures to use resident
  banks, settle persistence, retain source generations and avoid dumping values.

### Known limits

- The Windows backend policy has not been tested against the reported FL Studio
  crash on the affected machine. Hosted Windows/macOS builds do not certify DAW use.
- Snapshots require their base NKI and cannot open independently or target programs
  inside an NKM. Opaque source state and unknown saved scalars warn and remain unapplied.
- Native drive/filter support and finite sample renders do not establish Kontakt
  parameter-law or sonic equivalence. Compatibility remains partial.
- Control reuse, owner-map sharing and bounded refresh proofs do not establish native
  display FPS, GPU completion latency or a whole-runtime speedup.
- Exact-source hosted CI passed 410 library and 78 playback tests. The downloaded
  Linux package passed 38 CLAP checks (6 skips, no warnings/failures) and strict-level-5
  VST3 validation at 48 kHz/128 samples with GUI checks skipped. These scoped checks
  do not validate every library, host deadline or Windows/macOS runtime.

### Reviewed source changes

- Load factory snapshots through the plugin worker and persist their source
- Keep old snapshot state regression independent of later appended fields
- Map native SV Notch 4 filter id to existing DSP
- Share immutable UI revision ownership across script runtimes
- Journal snapshot validation and preserve active source services on rejection
- Keep snapshot rejection commits in source request view lock order
- Finish validation trace before locking the visible snapshot report
- Correct snapshot trace value and worker proof return types
- Measure notch pass bands by normalized signal power
- Preserve applied native script edits across host-state restoration
- Replay current native effect edits after processor rebuilds
- Validate saved native addresses without scanning the edit list
- Report exact snapshot rejection fixture state changes without dumping values
- Use existing standard storage and share the native-state test instrument
- Install a resident bank in the native edit callback fixture
- Preserve failed load status when publishing the active script diagnostics
- Settle active snapshot persistence before testing rejection and retain failed merge status
- Keep rejected snapshot status when pending artwork reports warnings
- Preserve broad pictured value displays in vectorized views
- Keep articulation mode help readable in compact editors
- Snapshot only active native edits and reuse the locked numeric-key hasher
- Document snapshot loading and native control state restoration
- Assert rejected snapshot retains source generation and installed epoch
- Capture the decoded native value already applied by the engine
- Skip unchanged KSP menu rows during bounded live refresh
- Decode large sample file tables once per import
- Leave KSP table revisions current when indexed integers are unchanged
- Default Windows renderer to DX12 and persist GPU startup diagnostics
- Document renderer policy and large-library refresh/import fixes
- Retain unchanged Original controls across live row publications
- Retain all eight native group drive insert slots
- Record retained UI controls and complete group drive slots
- Wait for fresh completed exports and an enabled UI scene
- Replace cached loading placeholders after instrument failure
- fix(import): preserve container decoder boundaries in load errors
