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

## Current installed checkpoint

Installed `bb60218e0226043ece56de9c3d66d20f655c748c` replaces the standalone,
CLAP and VST3. Build hash is `8e95f13c778a73d1`, import hash
`8322b33fd869e7d5`; clean source, baseline x86_64, optimized `ci` profile.
Previous binaries are backed up; settings and all 26 access records are
byte-identical. Installed file hashes, standalone link and build information
match the package. Both plugin factories and dynamic dependencies pass.

Combined all-target compiler checks pass at `0cd0415`, and the optional-UVI-disabled
check passes at `4c62461`; differences after those checks are documentation only.
Exact-source package build passed in 75 seconds. An independent current-source
review passed 17 focused compressor, Lua failure-context, worker privacy and
memory-accounting checks. Native comparison matrices were reviewed, not rerun
in that review; no broad-suite repeat is claimed.

Actual original/V2 Clarinet CLI inventories are Ready with zero resource/font
failures, one bank font each, 273/285 widgets, 38/39 pictures and 720×480 roots.
These frame-zero inventories do not exercise drawing or interactions. The
four-second original render retains 192,000 frames, zero worker errors and WAV
SHA-256 `a35808f0ffca715cd85d8682aaa79387c2c2a402c912df12d361dcf6a2815aa0`.
It recorded seven deadline misses under concurrent work; sustained realtime
playback and live Bitwig behavior remain unverified.

Fresh catalog validation finds 37 libraries: 11 Kontakt and 26 UVI banks,
660 UVI declarations and zero unavailable banks. Saved-cache hydration takes
11.437 ms and preserves identities; scan timing, playable coverage and instrument
initialization remain separate. Dependency notices and exact modified MPL
source archives are included and verified against the source/cache.

This checkpoint includes [Table styles and parent order](UVI_PANEL_ORDER_EVIDENCE.md),
[admission reuse and phase cancellation](UVI_LOADING_ADMISSION_EVIDENCE.md),
[bounded shared CompExp](UVI_COMPEXP_STATIC_EVIDENCE.md), the unadmitted
[Brickwall cascade](UVI_BRICKWALL_CASCADE_EVIDENCE.md), and per-fuel instruction-budget
source locations. Detailed proofs retain their native/source boundaries; no
complete Falcon, whole-program native fidelity or host deadline claim follows.

## Later source: verification pending

The installed checkpoint above remains unchanged. Following the user's CPU
restriction, work continues through static decoding, source review and small
metadata inspection. Builds, compiler checks, playback replays, native execution
loops and broad bank audits are suspended. New source is not a new tested binary.
The last combined all-target UVI check covered `3c8e5ae`; the default-feature check
at that source also passed before the restriction. Neither validates subsequent
integration.

Later source includes initial parsed Mapping inspection with bounded pages,
live resource dimensions, and existing activation-currentness checks; staged
local Lua failure context; distinct grouped failure children; integer control
keyboard steps; and [renderer scratch reuse](UVI_RENDERER_ALLOCATION_EVIDENCE.md).
The scratch and earlier UI/context variants have focused completed evidence.
The final Mapping currentness/index revision remains uncompiled and unexecuted.

The narrow [EffectRack setter correction](UVI_EFFECTRACK_GAIN_COMPATIBILITY_EVIDENCE.md)
has native registration evidence, but the final safe-lookup revision and actual
Pan callback/PCM verification remain pending. Part/Synth writes now fail at their
unowned host call before that parameter or DSP command changes; this source
correction is also uncompiled. It does not supply parent ownership or MPE.

The [initial owned PCM cache](UVI_STATIC_PCM_CACHE_EVIDENCE.md) is experimental and
disabled by default. Completed earlier-candidate sample and timing evidence does
not verify the final combined Worker/UI integration. Browser catalog caching
remains separate and already exists in the installed checkpoint.

The [dual-Lua C ABI playback proof](PLAYER_ABI_PLAYBACK_EVIDENCE.md) delivered
audio and root completions in a private authored fixture. The production neutral
backend integration and fixed audio marshaling are still pending. The user is
independently refactoring the shared core; this branch does not claim integration
with that separate work.

All 620 owned Augmented declarations decode, but none of their complete graphs
is admitted. The [measured Smooth inventory](UVI_LFO_SMOOTH_EVIDENCE.md#actual-augmented-scalar-inventory-and-remaining-production-state)
contains only tiny positive Smooth defaults, so the unverified normal-Smooth
draft would unlock no programs in that bank. The draft remains private and
unadmitted. Decode, static admission, initialization, actual control behavior,
finite playback, native fidelity and realtime deadlines remain separate claims.

Static rejection counts represent known distinct rejected nodes. Processor
checks inspect nodes individually, but control-graph construction stops at its
first failure; the report records that scope explicitly. The count is not a
complete census of every unsupported setting in a rejected program. Later
Kontakt monolith source also rejects descending offsets and malformed markers
instead of overflowing or asserting. These latest diagnostic/parser changes
remain uncompiled and unexecuted under the same CPU restriction.

The [missing PanLaw default correction](UVI_PANLAW_DEFAULT_EVIDENCE.md) aligns
the shared Lua/state baseline with retained native loaded-program getter values
for Program/Layer/Keygroup. Explicit XML, synthetic parents, SamplePlayer and
renderer gates are unchanged. It is source-reviewed and remains uncompiled;
newly saved omitted-property deltas cannot restore in older implementations.

The [browser presentation changes](UVI_BROWSER_PRESENTATION.md) retain authored
UVI preset folders and enable exact bank-stem image sidecars on both artwork
worker paths. Native product-cover discovery remains unresolved; the inspected
banks have no sidecars. Failure notices are shorter, Info presents structured
evidence once, and later worker errors cannot replace an independent captured
endpoint cause. These changes have static review and prepared unrun checks.
The [user's Piccolo request-capacity failure](UVI_PICCOLO_REQUEST_CAPACITY_EVIDENCE.md)
identifies queue exhaustion during playback, not a decoding failure. The
[render timer boundary](UVI_PACKET_COST_BOUNDARY_EVIDENCE.md) excludes additional
worker service work; a below-budget mean does not establish sustainable playback.
One bounded service retry before a full-ring abort preserves packet order and
capacity. Optional bridge-owned failure frontiers are published with the first
cause and shown separately from later worker observations. These additions are
source-reviewed only; prepared queue/atomic/UI checks are uncompiled and unrun.
They do not establish that the user's Piccolo playback failure is fixed.
Separate native browser metadata in `TagLibrary.ufs` is now a documented cover
discovery lead. It is absent from the checked locations and current catalog;
the bank/product-to-image join remains unimplemented.

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
processor/node/frame at the instrumented boundaries. Installed `817b03a` includes
a [local Lua context panel](UVI_LOCAL_LUA_FAILURE_CONTEXT.md),
using retained structured frames and already loaded approved source. Its bounded
code excerpt is excluded from copied records, journals and support exports.
Initialization failures retain a known entry without guessing their failing line. Missing source context is not
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

The [V2 performance matrix](UVI_VWINDS_PERFORMANCE_EVIDENCE.md) records nine
bounded instrumented Player tapes and an exact eight-control MPE-off restore
completed before the CPU restriction. Controller and transition paths produced
finite PCM; native modeled-transition fidelity, frontend gestures and realtime
host integration remain unverified. MPE setup failed before its tape began.

## Checkpoint validation

The following section retains the preceding `817b03a` installation and the
source follow-ups observed before `bb60218` was installed. Current installation
evidence is [above](#current-installed-checkpoint).

Installed checkpoint `817b03a` replaces `1213001` for the standalone, CLAP and
VST3, with previous files backed up, settings unchanged and all 26 private access
records byte-identical. Installed hashes, build identity and the standalone link
match the package. CLAP/VST3 factory and dynamic-dependency checks pass. Both
actual Clarinet UI diagnoses initialize with zero resource failures; each owns
one bank font. The original four-second Clarinet render is byte-identical to
the prior checkpoint: 192,000 frames, WAV SHA-256
`a35808f0ffca715cd85d8682aaa79387c2c2a402c912df12d361dcf6a2815aa0`,
zero worker errors and two measured render-deadline misses. These CLI observations do
not establish native visual fidelity or exercise the installed Bitwig editor.

The current catalog contains 11 Kontakt libraries and 26 UVI banks declaring
660 programs (620 Augmented Orchestra and 40 VWinds). Immediate publication of
this saved catalog measured 10.765 ms; a full scan measured 19.555 seconds with
37 total libraries and zero unavailable banks. Catalog declarations are distinct
from the historical 98-program decode frontier and its 41 static admissions.
The later [same-source census](UVI_AUGMENTED_PROGRAM_CENSUS.md) replaces the old
eight-program Augmented sample: all 620 decode/parse with zero errors, and none
passes static preflight at `384ed21`. Historical unique decode coverage reaches
710 across Augmented, VWinds and external Starter; this does not establish
same-revision preflight for all 710. Initialization, interaction, sustained
playback and native musical comparison remain separate evidence.

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

Installed `817b03a` includes the post-`1213001` changes that restore authored
help text, index preparation resource paths, avoid cloning completed full load
reports and skip unused Lua collection tables. A conservative Step gate rejects
transport snapshots inside a logical generation block; measured Diode circuit
and DC/pre-gain leaves remain separate from complete program admission. A
bounded audio conversion-buffer reuse preserves all 158,605,863 scalar bit patterns,
resource metadata and progress events in the actual V2 Clarinet initial load.
That load made zero sample-decryption calls; buffer allocation requests fell
from 38,903 to 1,231 without a demonstrated total startup speed improvement.

That checkpoint also includes [hierarchical preset menus](UVI_HIERARCHICAL_MENU_EVIDENCE.md)
with actual source UI/hosted-callback checks across 360/720/1080 pixels and
[bounded static BiquadFilter admission](UVI_BIQUAD_STATIC_EVIDENCE.md). The filter
has 36 actual Rust/native cases and 69 actual Rust/native 32,768-frame tone
cases; two of seven observed nodes pass its gate, while **zero whole presets**
become playable. Menu native style/placement, changed/connected filter controls,
broader settings and whole-host audio parity remain unverified. These focused
source observations are not installed-DAW interaction or full-corpus validation.

Source through `2b13e77` is later and **not installed**. It adds a bounded
malformed Kontakt UTF-16 diagnostic (count/reason instead of the input payload),
the [dual-Lua architecture proof](PLAYER_LUA_RUNTIME_BOUNDARY.md),
[authored Table colors and owned parent-subtree ordering](UVI_PANEL_ORDER_EVIDENCE.md),
and [immutable preflight reuse with phase cancellation](UVI_LOADING_ADMISSION_EVIDENCE.md).
The reuse preserves actual fresh/saved audio, state, UI and resource captures;
its three-baseline/three-candidate source measurements reduce median worker
initialization wall time by 3.11% for one instrument. This excludes UI asset
loading and audio adoption and does not establish a whole-startup or deadline
gain. Phase cancellation retains real errors and does not interrupt an already
executing Lua callback or renderer preparation unit. Full Falcon compatibility,
native end-to-end audio/UI fidelity and sustainable live deadlines remain unfinished.

Subsequent source through `4c62461` adds [static shared CompExp admission](UVI_COMPEXP_STATIC_EVIDENCE.md),
the unadmitted [Brickwall cascade](UVI_BRICKWALL_CASCADE_EVIDENCE.md), and typed
instruction-budget source context. These changes are not installed in `817b03a`.
Actual Session recovery preserves separate failure locations across a failed
save and a later callback; metadata excludes commercial source excerpts.

At historical installed checkpoint `75998c3`, the combined `uvi,standalone` optimized CI test run passed 977 library checks,
89 CLI/playback checks and four additional binary/integration checks. Thirty-five
external-fixture checks were ignored and one screenshot check was excluded.
The authored and retained native regressions include root Choke ownership, state construction and
resource precedence, no periodic onSave, real two-part rack save/reopen exact PCM,
UI scaling bounds, graph admission reporting and DSP/Lua error attribution.
These checks are scoped regressions, not a count of compatible Falcon features.
Later source changes use focused functional checks and feature checks; this
broad suite count is historical and does not describe a new checkpoint run.

The previously installed `f4e2a17` CLI was also checked against all 40 catalog programs in
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
