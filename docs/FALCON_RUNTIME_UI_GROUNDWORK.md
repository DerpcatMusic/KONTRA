# Falcon runtime and UI groundwork

This document preserves the **2026-10-02 implementation boundary audit**. At that checkpoint, no Falcon runtime, audio rendering, GUI session, bank extraction or licensed content was executed. Statements below that bank scripts, module identities or runtime support were unknown describe that historical scope.

See [current UVI capabilities and evidence](uvi-compatibility.md) for the subsequent native archive/program/sample implementation, offline Lua host, synthetic reference probes and privately decoded corpus validation. Those checks do not establish complete Falcon playback or GUI compatibility; the original requirements and unresolved contracts remain useful evidence below.

## Evidence and missing evidence

The format audit found 25 local UFS banks, 5,812,620,371 bytes total. All have `UFS2` plus a little-endian integer 3; the bounded header names identify VWinds double reeds and clarinets, with original/V2 pairs, and five flute families. The format agent's bounded prefix/tail scan found no readable preset, script or module references. Opaque/high-entropy payloads do not establish encryption, compression, or a decoded archive index. Private paths/header captures remain in ignored artifacts.

Representative future reference cases: VWinds Oboe V2, VWinds Bassoon V2, VWinds BbClarinet V2, VWinds C Flute, and their available original-version counterparts. These are bank names, not inspected presets. Exact script versions, imports, callbacks, widget classes, fonts, module types, routing, automation IDs and saved state remain unknown for every bank.

Public vendor manuals provide a more useful behavioral frontier. Double Reeds V2 uses recorded samples combined with proprietary H.A.T. modeling, continuous air-flow control, automatic/timed/manual vibrato, legato transitions, mic mixing, EQ and spatial controls. These require tests of continuous timbre and transitions, not merely note-to-sample selection. They do not identify the internal implementation. [Acousticsamples Double Reeds V2 manual](https://www.acousticsamples.net/index.php?product_id=107&route=product/productmanual)

Flutes adds attack ramps tied to velocity and current air flow, glide modes and chiff control. The manual establishes feature requirements; it does not prove whether a feature lives in Lua, a modulation graph, or native DSP. [Acousticsamples Flutes manual](https://www.acousticsamples.net/index.php?product_id=119&route=product/productmanual)

Two official downloadable XML programs from the [UVI examples gallery](https://lua.uvi.net/_examples_page.html) were inspected privately; their reuse license was not established, so their source is not committed:

| Official example | Actual observed content | Immediate requirements |
| --- | --- | --- |
| [MappingArticulations.uvip](https://lua.uvi.net/MappingArticulations.uvip), 8,240 bytes | `UVI4` tree; SampleMappingOscillator, DAHDSR/SignalConnection; embedded 5,598-byte Lua script | Two menus, boolean economy button, fractional progress slider; `onInit`/`onNote`; layer-targeted CC120, `dim1` dispatch, asynchronous mapping callback plus spawned 30ms polling; zone purge/unpurge. |
| [FXControls.uvip](https://lua.uvi.net/FXControls.uvip), 5,143 bytes | `UVI4` tree; MinBlepGenerator and Drive; embedded 2,275-byte Lua script | Panel-relative bounds; parameter-bound boolean bypass, fractional drive amount and enumerated mode; nonvisual ParameterValue and live callback readback. |

These bytes establish actual script/widget/module requirements, not engine behavior or local-bank coverage. MappingArticulations explicitly supplies no mapping/sample set; it waits for the owner's mappings. Its restore guard prevents persistent `changed` handlers from reloading before `onInit`. FXControls demonstrates a synthesized source rather than sample-only playback, so it needs an honest unavailable-generator diagnostic until that source is implemented.

## Languages and hosts

| Surface | Proven language/host | Reuse boundary |
| --- | --- | --- |
| Existing Kontakt scripts | KSP VM and Kontakt normalized parameter addresses | Keep as the Kontakt adapter. |
| Native Kontakt UI candidate | Lua 5.4, `mlua = 0.12.1`, `.nui` component lowering; editor/worker owned | Reuse bounds, resource identity and queue safety patterns; its host is not UVIScript. |
| Falcon ScriptProcessor | Sandboxed Lua 5.1 with UVI host objects and extensions | Separate UVI host bindings and language compatibility profile. |

UVI documents base/table/string/math, Lua-only `require`, `bit`, `class`, and helpers such as `table.copy`. It excludes `os`, `io` and external C modules. Musical callbacks use preallocated memory and bounded execution; the main chunk and save/load callbacks have relaxed allocation constraints. A stock UI Lua VM does not satisfy that contract. Lua 5.4 cannot be assumed source-equivalent to 5.1. Do not add another interpreter or replace the UI interpreter until real scripts and runtime checks establish the required language differences and embedding route. [UVI Lua reference](https://lua.uvi.net/_lua_reference.html)

## Prioritized compatibility map

| Priority | Requirement | Existing ground | Required boundary or proof |
| --- | --- | --- | --- |
| P0 | Clear format/resource identity | Bounded resolution/preparation; format agent's inspector | Versioned format diagnostics; unsupported banks stay unsupported. No blanket `.ufs` success. |
| P0 | Musical event processing | Event IDs, per-note ownership, timed commands, transport, release and expression | UVIScript dispatch semantics and scope must reach neutral engine operations without KSP translation. |
| P0 | Real-time Lua safety | KSP fuel limits and fixed event queues; UI Lua has limits | Separate callback scheduler, allocator/pool bounds and over-budget cleanup proofs. No GUI mutex/file I/O on audio. |
| P0 | UVI object identity | Groups/zones and Kontakt addresses | Preserve Program/Layer/Keygroup/Oscillator identities, ordered processors and routing; do not flatten them into Kontakt groups. |
| P1 | Sample-only playback/mapping | Sample decoding, streaming, key/velocity maps, loops and voice IDs | Verify UVI units, dimensions, round robin, purge/load completion and same-block start offsets. |
| P1 | UI semantics | MUI controls, retained vector/bitmap preparation, editor edit queues | UVI widgets with float/bool/table state, parameter maps, changed callbacks, layout and reload behavior. |
| P1 | Persistence/automation | KSP snapshots and source epochs | Preserve UVI widget state, custom Lua data, IDs and restore order separately from KSP state. |
| P2 | Wider Falcon DSP | Sample and Kontakt wavetable paths; envelopes and FX kernels | Module-specific behavior and audio reference checks; same effect name does not establish equal sound. |
| P2 | VWinds behavior | General expressive playback primitives | Actual readable owner presets/scripts and controlled reference recordings; H.A.T. internals remain unknown. |

## Event and engine boundaries

UVI defines note/release/CC/pitch-bend/channel and poly-aftertouch/program-change/transport callbacks, plus init/save/load. Missing musical handlers forward their event; an authored handler must forward explicitly. `onEvent` takes precedence over specialized handlers. Widget construction ends after the main chunk. Restore calls `onLoad` after widget restoration and before processing. Events include optional channel/input/layer/oscillator routing; the external channel range is 1–16, so it must not inherit KONTRA's zero-based channel assumptions. [UVI callbacks](https://lua.uvi.net/group___event_callbacks.html)

Callbacks are cooperative coroutines. `spawn` is deferred to the end of the current instant; `run` begins immediately. `wait`, `waitBeat` and note-release suspension need sample/beat scheduling. Spawned threads have no triggering note, so note-held queries cannot borrow a parent's key lifetime. Preserve these distinctions rather than treating all work as a UI callback or generic delayed function. [UVI timing](https://lua.uvi.net/_time_intro.html)

The engine hierarchy is Synth → Part → Program → Layer → Keygroup → Oscillator, with processors, modulators and effect routing attached to applicable levels. `this` identifies the current processor. That scope matters for both event order and parameter lookup. [UVI engine](https://lua.uvi.net/group___engine.html)

Parameter definitions carry `int`, `float`, `bool` or `string`, defaults and optional bounds. `getParameter`/`setParameter` return and accept those values. `children` is name-keyed; `synthChildren` is an ordered synthesis-only view; oscillators have their own keygroup collection. Keep unknown typed parameters and module identities in import data, and report unavailable operations instead of manufacturing values. [UVI Element](https://lua.uvi.net/class_element.html)

Concrete repo coupling: [Instrument](../src/import.rs) at 180 uses `Vec<String>` scripts and `crate::ksp::Persisted`; [Engine](../src/engine/mod.rs) at 204 and 341 owns `ksp::Runtime`; [script setup](../src/engine/script.rs) at 37 resolves KSP addresses; [GroupSettings](../src/engine/bank.rs) at 66 derives Kontakt modulation/source assumptions. The useful shared layer is below these: sample storage, voice ownership, timed execution, transport, expression, mixing and kernels. Add a small explicit host dispatch when a second runtime exists; do not prebuild a generic runtime framework or copy these primitives into a Falcon-only engine.

## UI and assets

UVI uses standard controls, panels/viewports, Table/XY, waveform/meters and file/drop controls, with automatic grid or explicit bounds. Bitmap resources have `@2x` variants. This requires separate logical geometry and device scale. Preserve authored bitmap controls when their frame changes encode meaning. [UVI UI catalog](https://lua.uvi.net/group___u_i.html)

Value changes have per-widget callbacks; widget callbacks cannot directly yield. Bound parameter widgets derive ranges/units/mapper from their target, and exported widgets expose host automation. Their state cannot be reduced to the native candidate's scalar `i32` KSP binding. [UVI UI guide](https://lua.uvi.net/_u_i_page.html)

Persistent widgets are saved by default and invoke `changed` when reloaded. Knobs expose custom strip images and optional callback suppression on `setValue`. These are behavior contracts for bidirectional bindings, not just painting. [Widget](https://lua.uvi.net/class_widget.html), [Knob](https://lua.uvi.net/class_knob.html)

SVG has its own UVI widget and placement behavior. Labels can load TrueType fonts and specify alignment, size and horizontal squeezing. [SVG](https://lua.uvi.net/class_s_v_g.html), [Label](https://lua.uvi.net/class_label.html)

The audited native UI candidate at `627bb6c` supports only ZStack/Text/Rectangle, frame and tap gestures. Its `.nui` lowering and Kontakt host bindings must remain distinct. The uncommitted native asset candidate prepares SVG/WebP/PNG/JPEG off audio with path and byte limits. It rejects SVG text/images/masks/filters/patterns and other unsupported nodes. MUI's retained-vector candidate has device transforms, winding, gradients, clips and CPU/GPU paths. Reuse that painter and preparation machinery, with a UVI resource resolver and class-to-control mapping. It does not yet cover UVI font or SVG parity; its `Resources/native_ui` namespace is Kontakt-specific.

## First implementation and verification gates

The following are KONTRA implementation requirements inferred from the documented contracts and actual example usage, rather than verified reference-host timing:

- Preserve integer/float/boolean/string parameter types and widget state. `Table` changes need their edited index; menu selection needs its selected entry; buttons need boolean values. Parameter-bound controls write the engine and observe engine/automation/preset updates back into the UI. Keep independent logical bindings such as ParameterValue. Do not cast fractional/bool state through Native UI's KSP `i32` bridge.
- Snapshot automatic widget persistence separately from `onSave`'s custom scalar/nested-table data. Reject cyclic/userdata values explicitly. Reload invokes persistent widget callbacks, so retain restore suppression/guards and preserve authored initialization behavior. The relative ordering of `onInit`, every widget callback and `onLoad` needs a host trace; the MappingArticulations guard is concrete evidence that this order affects disk I/O.
- Apply the documented `onEvent` precedence/default forwarding once per processor. Preserve processor scope and ordered chains; never dispatch one input through both master and specialized handlers. Establish ordering between resumed coroutines, new input events and async/UI completions with authored trace checks, rather than borrowing KSP priority accidentally.
- Every scheduled note, wait-for-release and generated child must retain exact source/processor epoch and originating note identity. Overlapping same-key input, rerouting, sustain, callback failure, preset replacement and late note-off require explicit checks. Reuse existing voice/host ownership primitives where they cover those facts; do not use note number alone as ownership.
- Async work must carry source/processor identity and a request generation. Publish completion through bounded host work; an old preset's completion must not mutate a replacement. Distinguish request acceptance, progress and success. Concurrent mapping swaps/purge/unpurge and cancellation policy are unresolved reference semantics: test them before choosing whether a stale request completes, is ignored, or reports cancellation. Do not present a proposed KONTRA cancellation policy as UVI behavior.

1. Accept a documented clear mapping plus authored sample assets first. UVI `.dmap`/`.xml` maps carry key/velocity plus `dim1`/`dim2`, with explicit round-robin dispatch and asynchronous load/purge. KONTRA currently has no such dimension fields. Keep them in the UVI model until dispatch can be represented honestly. [UVI sample mapping](https://lua.uvi.net/_sample_mapping_intro.html)
2. Establish authored Lua checks for absent-handler forwarding, `onEvent` precedence, coroutine ordering, exact note ownership/release, CC forwarding and script replacement. A failed or over-budget callback must not leave hanging generated notes.
3. Establish an authored UI scene with a fractional knob, boolean button, menu, Table, panel, strip image, SVG and local font. Check callbacks, suppression, automation, persistence/reload, 1×/2× scaling, clipping and keyboard access. Run actual UVI reference behavior before describing it as compatible.
4. Check asynchronous failures and stale completions against source identity. The documented API offers sample/IR/MIDI/data/state operations; those are distinct from a full-program export. [UVI async API](https://lua.uvi.net/group___async.html)
5. `saveState` serializes ScriptProcessor state; `saveData` writes plain Lua values as JSON. They are usable metadata/state inspection primitives inside a running owner host, not UFS extractors. The inspected official API/search did not establish a headless full-program export command. Official saving instructions use the Falcon menu; no host was present in the bounded local installation scan. [UVI async guide](https://lua.uvi.net/_async_intro.html), [UVI program saving](https://support.uvi.net/hc/fr-fr/articles/360001193577-Apprendre-Falcon-102-Sauvegarder-un-programme)
6. Only after clear import, runtime and UI gates pass, compare representative VWinds air-flow ramps, vibrato modes, rapid/slow legato, mic/space changes and state reload against the owner's installed reference host. Keep scripts/assets/audio and extracted owner data in ignored artifacts; commit only authored fixtures and sanitized aggregate findings.

No runtime code or synthetic "Falcon preset" was added in this audit: the official examples establish clear XML and API usage, but an executable reference host is still absent. The format inspector supplies the runnable bounded-IO frontier; the gates above identify the next concrete behavior evidence needed.
