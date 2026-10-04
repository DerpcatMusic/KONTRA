# UVI Linux test checkpoint

This is a private experimental checkpoint, not complete Falcon compatibility.
The optional UVI player shares KONTRA's browser, rack mixer, routing and keyboard.
Unsupported native graphs remain explicit failures. Starter coverage is still
1/50; the paid-library audit and native DSP comparisons have separate limits in
[uvi-compatibility.md](uvi-compatibility.md).

## Start and load

Build the CLI, plugin library and standalone with the `uvi,standalone` features.
The test package identifies its exact source and enabled features in its manifest.
It contains no vendor executable, bank, sample, script or access state.

Configure the local official Workstation reader in library settings or with
`KONTRA_UVI_READER`. Bounded installed-reader discovery is a fallback. The
decoder reads the executable; it does not run or activate Falcon/Workstation.
For protected banks, the normal desktop/DAW access store is
`$XDG_CONFIG_HOME/kontra/uvi-access` (usually `~/.config/kontra/uvi-access`).
`KONTRA_UVI_AUTHORITY_DIR` overrides that directory. Private bank-bound JSON
files are named by the bank UUID (32 lowercase hexadecimal characters plus `.json`).
Selecting a supported encrypted bank automatically prepares this private record
after verifying a complete bank PNG. Records bind the bank UUID, name and size;
changed banks are verified again, and legacy records are verified and upgraded.
Unsupported protection, missing suitable resources and corrupt/insecure private
records remain explicit errors. No manual key command is needed in the desktop
or plugins. These local records are separate from saved instrument controls.
The reader and bank files must remain present. The CLI setup commands are in the
[README](../README.md#experimental-uvi-interoperability).

Add the folder containing the banks in KONTRA's library settings and rescan.
Later launches publish the saved catalog immediately and verify it in the
background. This caches library/preset metadata, snapshot bindings and source
identities; samples are still loaded for the selected instrument. Rescan forces
rediscovery. Cached publication cannot restart an already staged instrument.
The browser saves its **Unified / By player** choice. Unified keeps one catalog;
By player gives Kontakt and UVI / Falcon separate headings with both available.
Library and preset rows identify their player; favorites, recent items and search
retain native bank identities. Existing settings default to Unified.
Select a supported UVI
program and wait for its controls before playing. Actual sample rate, host block
size and preparation limits can affect admission.

## Check the behavior

1. Play from the keyboard, then from the DAW. Check release, overlap and sustain.
2. Resize the editor; artwork and hit targets should follow one uniform scale.
3. Change a native control and verify the sound follows it.
4. Change rack gain, pan, mute and solo alongside another part.
5. Save a KONTRA multi, reopen it, and compare controls and sound. Test DAW save
   and reopen separately; the host pre-save API cannot report a failed capture.
6. Remove the part or reset the audio configuration and check that a replacement
   loads without old controls acting on the new instrument.

Save requires already applied control edits and a processed audio boundary.
If rack Save says it is waiting, resume audio processing and save again. A failed
native onSave retains the previous successful payload. DAW capture failures are
reported in Logs and retain prior state; they must not be read as fresh saves.
Ordinary playback/UI polling never runs onSave. Voice state and transport are
not part of instrument persistence.

Report the exact build identity, bank/program, sample rate and host block size
with any failure. Private logs or screenshots can contain purchased-library
names and artwork. Render-deadline reliability, complete native fonts/units,
advanced displays and full Falcon sound parity remain unproven.

## Read a diagnosis

`kontakto uvi-diagnose bank.ufs member.uvip --reader UVIWorkstationx64.exe
--content-key-file private-state.json` emits JSON after attempting initialization.
Use `--sample-rate` for the actual rate. Failed initialization still emits the
available report on stdout and exits with an error. This command sends no notes:
Ready means initialized, not that the musical behavior has been exercised.

Add `--ui` for an explicit control-thread UI census. It requests initialized
processor snapshots and decodes their authorized artwork, reporting panel sizes,
widget/visible-widget/callback counts, picture counts, byte residency and failed
or limited references. It emits no captions, widget values or artwork paths.
Missing snapshots, artwork failures, limits and unavailable UI builds produce a
partial/unavailable report and a nonzero exit after JSON output. Snapshot waiting
and processor/reference/byte counts are bounded. This sends no notes or control
edits and explicitly reports drawing, interaction and native comparison as false.
The census is opt-in; ordinary diagnosis and playback do not perform this work.

The report separates decoded node identity and preflight rejection reasons from
worker evidence. Preflight admission is conditional on later resource/DSP/script
validation; even a bypassed unknown processor is rejected because a script could
enable it. Packet counters and retained voice instances describe this renderer,
not Falcon equivalence or audible voices. A zero error count with zero rendered
packets proves no playback. Explicit diagnoses request a stamped worker snapshot
at a processed boundary. Instrumented oscillators and inserts report processed block counts, current
bypass state and retained oscillator voice instances. Silent processing still
counts; containers, control-source nodes and scripts without probes remain unknown. A cached snapshot keeps its original frame when playback advances.
These counters do not prove audibility, script callback coverage or vendor fidelity.
Support exports keep a bounded UVI context and label omitted node rows with exact
included/omitted coverage. CLI diagnoses and each worker cache retain full rows.
No full-graph snapshot runs during ordinary packet or UI polling.

The editor's existing Logs and diagnostic export include UVI initialization
stages and failures. Rack worker snapshots retain current status/counters; worker
initialization durations remain available while a load is running. The rack
shows the current loading activity and elapsed seconds, and terminal failures
remain failures. Genuine initialized controls can be published after authored
Lua initialization while fresh renderer preparation is still running; gestures
remain disabled until matching audio adoption. Static sample decoding still
precedes Lua. Saved-state restoration keeps its prevalidated renderer/resource
and authored callback order. Cancellation and stale activation cannot retain an
initialized panel. A runtime failure after audio adoption retains the initialized
panel for inspection, disables its controls and closes its popups; a new source
or epoch hides it. Info shows live load stages, graph counts, initial-resource
counts and PCM residency. The first endpoint failure includes its actual error,
stage, frame, Rust source location and build revision. A bounded code excerpt
appears only when the local source matches the digest captured by the binary;
otherwise the reason it is unavailable is explicit. Repeated warning/error rows
share one parent with distinct item/cause/location entries. Full exports keep
the original records. Host latency admission survives engine resets while the native
source remains selected, preventing a zero/loaded-latency restart cycle; physical
compensation is retained only with matching already allocated delay storage.
Actual Bitwig behavior still requires testing with the newly installed plugin.
Worker load journals retain decoded graph details even when admission fails. Lua errors
retain processor/chunk/line and resumed frame when available; DSP errors identify
processor/node/frame at the instrumented boundaries. Missing source context is not
invented. Authored print counts and drops are measured separately from errors.
The JSON/journal contain names and failure messages, not full commercial scripts,
program XML, samples, reader constants or access keys. Review private names before
sharing. Journals and exports have their existing retention and truncation limits.

## What is actually established

| Area | Established evidence | Remaining work |
| --- | --- | --- |
| UFS/program/sample access | Owned-corpus directory and decode checks; native identities retained | Unsupported protection/layouts remain failures; no universal-bank claim |
| Lua and MIDI | Authored initialization, measured original-node Gain/Pan defaults, timed commands, control edits, transport and rooted note tests | Complete host API, MPE/tuning/expression and every library interaction |
| Native DSP | Scoped native comparisons and explicitly bounded admitted settings | Unsupported oscillators/routes/modes and broader live/rate fidelity |
| VWinds controls | Original-source panel with uniform intrinsic 720×480 scaling; 273 widgets/38 images and owned bank fonts retained | Native visual comparison, units, advanced displays and full interaction/audio comparison |
| Rack persistence | Real two-part save/reopen control and exact PCM regression | Host save failures cannot be returned through the framework hook |
| Playback scheduling | Allocation-free callback transport and bounded packet ownership tests | Sustainable deadlines on user hardware; prior paid-worker misses remain |
| Compatibility accounting | Parsed/admitted/rejected graph, worker lifecycle and explicitly requested instrumented node counters | Uninstrumented nodes, script callback coverage and native end-to-end musical parity |

VWinds is a hybrid sample/modelling product. Its publisher describes harmonic
alignment, airflow/vibrato and recorded or modelled transitions in the
[VWinds manual](https://www.acousticsamples.net/index.php?product_id=112&route=product/productmanual).
The installed program must be examined individually: implementing a generic
physical model does not establish its compatibility, and decoding its full graph
does not prove its transition or timbre behavior.

## Checkpoint validation

Installed checkpoint `1213001` replaces the standalone, CLAP and VST3 binaries,
with the previous files backed up and settings/access records unchanged. Both
actual Clarinet UI diagnoses initialize with zero resource failures; each owns
one bank font. The original four-second Clarinet render is byte-identical to
the prior checkpoint and reports no worker errors. These CLI observations do
not establish native visual fidelity or exercise the installed Bitwig editor.

The current catalog contains 11 Kontakt libraries and 26 UVI banks declaring
660 programs (620 Augmented Orchestra and 40 VWinds). Immediate publication of
this saved catalog measured 12.9 ms; a full scan measured 21.3 seconds. Catalog
declarations are distinct from the 98 decoded programs in the bounded audit
(including 50 external Starter presets), and from its 41 static preflight
admissions. Initialization, interaction, sustained playback and native musical
comparison remain separate evidence.

This checkpoint uses focused functional verification and feature checks,
including audio first-cause retention, actual generated-note velocity cases,
UI resource loading and feature-disabled compilation. Authored `displayText`,
eight documented numeric units, skins and bank fonts are included. Focused UI
checks exercise double-click editing: raw values commit on Enter/blur, Escape
cancels, and displayed units do not silently rescale engine values. Pan formatting
and native display precision remain uncalibrated.

Earlier indexed lookup and source memo changes preserved exact Alto Flute
PCM/event/state captures but still reached request capacity. The installed
reverb source-read cache preserves those captures and reduces the measured cold
renderer packets from roughly 38 ms to 21 ms. A fresh source-direct replay at
`72c671e` ran for 10 and 20 seconds with finite nonzero output, no endpoint
failures, no Bridge or Worker underruns and no backpressure. These private
Worker/Bridge observations use disclosed unchanged SDK glue; they do not verify
live Bitwig operation, sustained deadlines on every machine or native musical
parity. The original reported blackout remains unclassified.

Source changes after `1213001` are not yet installed. They restore authored
help text, index preparation resource paths, avoid cloning completed full load
reports and skip unused Lua collection tables. A conservative Step gate rejects
transport snapshots inside a logical generation block; measured Diode circuit
and DC/pre-gain leaves remain separate from complete program admission. A
bounded audio conversion-buffer reuse preserves all 158,605,863 scalar bit patterns,
resource metadata and progress events in the actual V2 Clarinet initial load.
That load made zero sample-decryption calls; buffer allocation requests fell
from 38,903 to 1,231 without a demonstrated total startup speed improvement.

At installed checkpoint `75998c3`, the combined `uvi,standalone` optimized CI test run passed 977 library checks,
89 CLI/playback checks and four additional binary/integration checks. Thirty-five
external-fixture checks were ignored and one screenshot check was excluded.
The authored and retained native regressions include root Choke ownership, state construction and
resource precedence, no periodic onSave, real two-part rack save/reopen exact PCM,
UI scaling bounds, graph admission reporting and DSP/Lua error attribution.
These checks are scoped regressions, not a count of compatible Falcon features.
Later source changes use focused functional checks and feature checks; this
broad suite count is historical and does not describe a new checkpoint run.

The installed `f4e2a17` CLI was also checked against all 40 catalog programs in
25 paid banks: parsing/preflight, script checks, initialization-only diagnosis
and short authored-note Worker renders passed. Each written WAV independently
verified 72,000 finite stereo frames at 48 kHz and nonzero output. This scoped
pass did not exercise the GUI, every articulation, typed-root host transport or
native program comparison; the [evidence ledger](uvi-compatibility.md#current-evidence-ledger)
retains those distinctions.

Separate source-direct UI checks captured the original Clarinet's 273 widgets
and 38 pictures, and V2's 285 widgets and 39 pictures, with zero failed/limited
references and 720×480 roots. These initialized snapshots have frame zero and
do not exercise controls. A real V2 loader/mailbox/renderer-source check published
the panel and decoded artwork 160 ms before audio Ready; the initialized-snapshot
boundary led Ready by 195 ms. Static resources still took about 2.84 seconds in
that run. Cache/CPU variation prevents attributing total loading improvement to
this change. These are controlled source-helper observations rather than a new
Bitwig or official-reader screenshot comparison.

The loading checkpoint also covers automatic private access preparation, catalog
cache integrity/invalidation, stable host latency across resets and current load
activity. The 512-byte cipher block loop measured 2.14–2.40 times faster in paired
optimized synthetic checks without new dependencies, nightly Rust or threads.
Indexed graph lookup and removal of repeated finite checks on immutable validated
PCM retain exact paid/audio/event/state hashes in focused comparisons. A real
saved-root scan found 11 Kontakt libraries, 835 preset files, 1003 snapshot
bindings and 40 programs across 25 UVI banks. The full scan took 31.1 seconds;
a fresh instance published its saved index in 11.6 ms and completed background
verification/artwork in 188 ms. Settings remained byte-identical. This measures
metadata publication, not complete UI startup or sample loading time.

The current source also preserves exact paid PCM/command/state captures while
compiling modulation targets to stable numeric slots. Paired Renderer thread CPU
medians improved by 7.51% for the original Clarinet and 12.70% for V2; the V2
capture still exceeded its audio duration, so this is not a realtime guarantee.
A single-pass PCM validation change improved measured preload thread CPU by
5.85% and 9.48% respectively. Neither measurement represents whole-load latency.
The selected sample datasets made zero sample-decryption calls during preload.

Audio-resource tasks now finish with their callback for both success and failure.
Product callbacks run through the existing cooperative scheduler, can yield, and
retain exact task errors. Failed reads or metadata validation retain the previous
resource and create a bounded desktop warning; successful swaps affect new
voices. Native background timing/interleaving remains unverified. Explicit
`uvi-diagnose --ui` reports snapshot/artwork census without claiming drawing,
interaction or native comparison.
