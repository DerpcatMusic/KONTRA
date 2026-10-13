# Kontakt runtime topology and KONTRA target design

Research/design package `topology`, 2026-10-13. KONTRA baseline: `ce0a2e61`
(`origin/main` when this worktree was created). Source locations below refer to
that baseline; no engine behavior was changed in this package.

**The main requirement is to preserve processing scope, placement, and lifetime.**
A group is a shared definition, but its insert effects have independent voice
histories. A summed bus is a different kind of object. Moving a nonlinear effect
or a fader across either boundary changes sound. Low CPU comes from executing
only needed state transitions and active kernels, not flattening those boundaries.

## Evidence and limits

The executable hash, address ledger, direct constant reads and replay commands are
in [the evidence note](../re/KONTAKT_TOPOLOGY_EVIDENCE.md). B1–B17 below refer to its
rows. `Observed` means static code/bytes read here; `documented` means the public
contract; `inferred` means an implementation interpretation; `open` means there is
no sufficient evidence yet. Type/vtable identity does not prove an algorithm.

The requested full Ghidra pseudocode export is missing from the recovered cache;
its retained DSP excerpts, decompilation ledger, vtable table and catalog were
available. Focused radare2 `af`/`pdg` and instruction/constant reads supplemented
them. No full-engine trace or live native CPU measurement was made. This document
does **not** certify every internal Kontakt class, a universal 32-frame block,
or a numeric native “near zero CPU” result.

Vendor sections checked on 2026-10-13 (online manuals can be newer than the binary):

| Ref | Official source and exact relevant sections |
|---|---|
| D1 | [Kontakt 8 Classic view](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view): Building blocks; Instrument header; Instrument Options → Voices Handling / DFD; Group Editor → Voice Groups / Group Start Options; Source Module → Sampler / DFD / Wavetable / Tone and Time Machines; Amplifier Module; Script Editor; Outputs Section; Purge menu. |
| D2 | [Using filters and effects](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/using-filters-and-effects-in-classic-view): Group insert effects; Bus insert effects; Instrument insert effects; Instrument send effects; Main effects; Common parameters; Output channel inserts; Aux channels. |
| D3 | [Modulation](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/modulation): Assignment controls → Lag; LFO controls → Retrigger / Frequency / Fade in / Sync. |
| D4 | [Options dialog](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/options-dialog): Engine; Memory → Override Instrument's preload size. |
| D5 | [KSP callbacks](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks): General Information; on note; on release; on listener; on async_complete; on ui_control. |
| D6 | [KSP time commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands): wait; wait_async; stop_wait; set_listener; reset_ksp_timer. |
| D7 | [KSP group commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/group-commands): allow_group; disallow_group; get_purge_state; purge_group. |
| D8 | [KSP engine parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters): source/amp/routing constants; [Engine Parameter Commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands): set_engine_par / get_engine_par and group/slot/generic addressing. |
| D10 | [KSP Multi Script](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/multi-script): General Information; on midi_in; ignore_midi; set_midi. |
| D9 | [Effect reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference): individual processor availability, Send Levels, Convolution, Stereo Modeller. Module availability must be checked individually before adding a voice implementation. |

Existing measured laws remain authoritative within their stated coverage:
[KONTAKT_DSP_LAWS.md](KONTAKT_DSP_LAWS.md), its vectors,
[KONTAKT_FX_MOD_FORMAT.md](KONTAKT_FX_MOD_FORMAT.md),
[KSP_SEMANTICS.md](KSP_SEMANTICS.md), and [STREAMING.md](STREAMING.md).
Some older architecture prose is stale: current `parallel.rs:168` prepares script
and modulation state before workers; it no longer categorically excludes all
modulated voices. Read code when adopting this plan.

## Native model

### Definitions, instances, and ownership

The authored hierarchy is Multi → Instrument → Group → Zone → Sample reference.
A playing voice is an instance created from an eligible zone, not an authored
child permanently resident under every zone. A note may create multiple voices.
B1–B4 distinguish program/group/zone/sample/pool objects; B13 also identifies
voice storage and group reminders. Exact C++ ownership details remain open.

| Runtime scope | Shared definition / state | Independent running state |
|---|---|---|
| Multi / rack | Instruments, MIDI routing, output channels, auxiliary channels, Multi script slots | Cross-instrument output/aux FX histories, event routing, host clock/transport |
| Instrument (`BProgram`) | Groups/zones, instrument buses, insert/send/main racks, limits, KSP slots | Summed processor histories, send accumulators/returns, script globals and callback continuations, instrument controls |
| Group (`BGroup`) | Source mode/settings, zone membership, insert rack and amp split, amplifier, modulation definitions, start conditions, voice-group assignment | Shared control targets; group-level/free-running modulation where authored; active membership metadata. **Not one shared history for group insert FX.** |
| Zone (`BZone`) | Mapping, gain/pan/tuning, sample and loop ranges | No permanent sample cursor/DSP history merely because a zone is loaded |
| Voice | References to immutable group/zone/source program | Cursor/oscillator/grains, envelope and retriggered modulator state, route smoothing, voice insert histories, gate/release/steal fade, streaming reader window |
| Asset / sample pool | Immutable data, analyzed tables, resident heads/pages, codec metadata | Worker requests and residency ownership; many zones/voices may share one asset |

Instance rules follow the public polyphonic/summed distinction in D2. Shared LFO
ownership cannot be inferred just from UI placement: D3 exposes retrigger/freewheel,
and binary B16 distinguishes definitions and fanout. Verify per-group/global
retrigger behavior before changing a library's clock ownership.

### Audio path and tap positions

```text
Multi MIDI/KSP routing
  └─ Instrument script slots → selection / note admission
      └─ eligible Group + Zone → voice source
          → ordered pre-amp Group inserts
          → Group amplifier (envelope, volume, pan)
          → ordered post-amp Group inserts
          ├─ explicit Send Levels taps → sum per Instrument send slot
          └─ selected dry destination:
               Instrument insert input
               OR Instrument bus sum → bus inserts → bus amp/routing
               OR direct output path

Instrument inserts → dry branch ───────────────┐
Instrument send slots (parallel, summed input) ├→ Instrument Main FX
                                             └→ separately assigned return
Instrument final fader/pan / output assignment → Output channel sum → Output FX → host
Instrument aux sends / separately assigned send returns → Aux sum → Aux FX → host
```

This is a schematic, not a newly recovered total order for every header/aux tap.
D2 specifies eight-slot group/bus/instrument/main chains, explicit send taps,
16 instrument buses, bus insert-bypass routing, send return destinations and
cross-instrument output/aux scopes. B5 directly confirms channel-buffer bus amp
and output work; B6 confirms channel-count-specific routing; B14 confirms separate
send target/current state. Header fader location relative to every main/aux tap
needs a native impulse/nonlinear witness before implementing the final binding.

D8 gives a concrete routing contract: selector −1 means instrument output,
−2 bypasses instrument inserts, 0..63 selects a Kontakt output, and
`NI_BUS_OFFSET + 0..15` selects an instrument bus. Native instrument buses cannot
route into one another. Do not map all of these values to the generic master port.

Do not create an implicit instrument-send feed merely because a return exists.
Do not confuse an Instrument Main rack with a Multi-wide master effect rack.
Do not merge adjacent stages across an amp, tap, modulation target, or summing node.
An insert acts on its full input; a send accumulates only its explicit contributions.
Voice group inserts must keep separate histories even when all voices share settings.

### Sources and modulation

Source mode belongs to the group definition (B2/B16); each voice runs its own
source instance. Sampler and DFD produce the same source semantics with different
residency. Stretch/granular/vintage modes require distinct algorithms. Wavetable
requires oscillator state, including its internal audio-rate modulation path,
independent of ordinary sample playback. Source tables/analysis can be shared.

Kontakt has several clocks, not one global “control rate”:

| Stage | Established behavior | Still open |
|---|---|---|
| Resampler / source / FX sample kernel | Processes audio samples; B9 uses per-sample delay-ring recurrence | Full voice source dispatcher, SIMD variants and source-mode traversal |
| AHDSR | Existing original-byte checks B12 establish control-tick recurrence and rate-based lengths | Actual control rate passed by the engine; **sample rate / 32 is not proved** |
| Modern FX wrapper | B11 retains a 32-frame countdown across calls and updates enabled parameter pointers | Whether another variant has another quantum; inner core smoothing |
| One-band EQ wrapper | B12: 32-frame counter plus smoothing/coefficient updates after partial calls; `[33]` differs from `[32,1]` | Full caller partitioning and convergence/dirty-state paths |
| Modulation assignment | D3 exposes lag per assignment; B7 shows target/current state separation | Exact generic lag kernel/cadence and all transform orders |
| LFO | D3 exposes retrigger, freewheel, tempo sync, fade and frequencies extending into audio range | Exact native shared clock ownership, output interpolation, tempo-change phase continuity |

D3 documents assignment lag in milliseconds (default pitch-assignment smoothing
is 250 ms); saved library values take precedence over that UI default. The
actual generic lag recurrence is not recovered here.

A persistent internal quantum and host-block partition invariance are different
properties. Do not silently replace a documented/measured partition-sensitive
native wrapper with one global absolute grid and label it parity. The format
lowerer must select a checked clock/interpolation/smoothing contract per kernel.
The generic core can also offer partition-invariant laws for other formats.

### Voices, limits, and release

Voice groups are allocation domains spanning one or more authored groups, not
audio sum nodes. D1 describes instrument and voice-group limits, selectable
victim modes, fade overlap, exclusive groups, and source-specific Time Machine Pro
limits. B13 locates corresponding limit objects, but the actual victim-selection
algorithm was not recovered. In particular, `Kill Any` is not proven to mean
quietest, and the manual's `Pref.Rel` wording favors keeping released notes while
KONTRA currently chooses them as victims; that disagreement needs a native probe.

Release-trigger zones create separate source instances; releasing an attack voice
is a different operation. Preserve physical key-up, logical release, pedal/system
script handling, script-created events and note pairing. A release voice can
outlive its parent key; a bus tail can outlive all voices. Monophonic-release scope
must include instrument/plan identity, group identity and key, never a bare dense
group index reused across plans.

### Inactivity, bypass, purge, and streaming

Observed native mechanisms are narrower than a blanket “everything sleeps” claim:

- B9 has delay-ring activity and a tail query; B10 tests buffer activity/emptiness.
  B8 avoids part of mixing work for disabled/negligible paths. B11's empty spans
  do not update. These support an activity-driven design.
- The `2^-16` threshold at `0x144f385c0` participates in the inspected paths; it is
  not a safe universal threshold for every feedback effect, resonant filter,
  nonlinear module, or modulated source.
- D2 says bypass passes input without processing. Whether each native effect
  clears, freezes, drains or virtually advances hidden history when bypassed is
  open. B7 establishes explicit reset/copy state, not the bypass policy.
- A group with no voices need not execute its voice kernels. Shared clocks and
  callbacks can still be due. A zero-amplitude voice is not necessarily dead:
  a future controller, pre-amp tap, self-oscillating kernel or release can matter.
- D7's purge unload/reload contract is a residency operation with asynchronous
  completion, not a fader. D1's sample-usage purge also permits silent usage
  collection. Exact native in-flight-voice/purge behavior remains open.
- D1/D4 document resident beginnings plus disk remainder and configurable
  instrument/global preload. B4 establishes sample-pool identities and a named
  synchronized attachment job. The **DFD disk thread entry, queue, numeric default
  preload size, and wake policy were not found** in this bounded pass. Do not
  assert a universal 60 KB or a fixed disk-thread count.

Idle work can be nearly absent if no voices, tails, pending events or storage
requests exist. Playing necessarily costs source interpolation/DSP/script work.
DFD saves residency and startup I/O; it is not a mechanism for making an active
resampler or nonlinear voice chain free. Native and KONTRA CPU under Afflatus and
Conflux settings is **not measured by this package**.

### KSP relative to audio blocks

D1/D5/D6 establish event-driven note/release/controller callbacks, ordered instrument
script slots, listener timers, suspended callbacks (`wait`), and async completion.
D10 distinguishes a Multi raw-MIDI processor from an Instrument note-event processor.
Every MIDI message has a Multi event identity; note/voice commands are not
interchangeable with raw-MIDI commands. Waiting suspends the callback while other callbacks can run; it must not sleep the
render thread. `$KSP_TIMER` measures wall time and is unsuitable for musical timing.

The exact native host-block boundary, audio-thread affinity, wait-rounding rule,
same-time priority and UI→audio handoff are **open**. B15 did not identify the
KSP dispatcher. A function iterating 64 instrument slots is not proof of a
64-frame callback quantum. KONTRA's explicit sample-time queue is a useful
architecture, but its timing is not certified native parity by this research.

## KONTRA's current equivalent

| Native responsibility | Current implementation and useful behavior to retain |
|---|---|
| Authored definitions | `sampler-ir/src/lib.rs:67` Instrument, `:414` Group, `:539` Zone; physical Kontakt identities retained separately in `kontakt.rs` / `SourceIndices` |
| Kontakt lowering | `sampler-kontakt/src/library.rs:1327` builds voice chains around saved amp split; `effects.rs:1539` builds summed insert/send/main/bus DAG; `library.rs:794` resolves physical send-slot taps |
| Plan construction | `sampler-core/src/lower.rs:429` semantic lowering; `:903` zone/group source parameters; `:2287` bus lowering; `prepare.rs` immutable prepared resources |
| Note/voice ownership | `lib.rs`, `ownership.rs`, `gate.rs`, `note_event.rs`, `release.rs`; generational note/family/voice identities and retained plan revisions |
| Selection | `prepare/selection.rs`, `native_start.rs`, group masks, RR/articulation criteria; starts are admitted before source publication |
| Voice render | `render.rs:301` active-voice bitset; `:547` modulation prelude; `:664` voice render; `dsp.rs:696` pre/amp/post split; per-voice processor histories |
| Summed render | `bus.rs:137` validated topological ordering; `:323` dirty buffer clearing; `:352` bus render with independent histories and sends |
| Modulation | `voice_mod.rs:1420` generic control cells; `whole_voice.rs` admitted v1 envelope/external route owner; `v1_voice_controls.rs` measured control kernels |
| Scheduler | `schedule.rs:61` stable equal-time queue; `render.rs:197` callback/event application; `stages.rs` script slot forwarding; block fuel bounded independently of render fragments |
| Residency | `stream.rs:880` active demand/cache protocol; `sampler-kontakt/stream.rs:653` resident head preparation and workers; `:878` idle decoder parks |
| Parallel render | `parallel.rs:163` workload threshold, `:168` serial modulation/script prepare, deterministic output fold; avoid worker wake on one note |

Do not replace these proven boundaries just to introduce a new class hierarchy.
The present stereo DAG, strict ownership and preallocated voice state are useful
foundations. Native class names belong in the evidence/lowering layer, not in the
format-agnostic runtime's dispatch logic.

## Ranked structural mismatch table

Impact combines audible reach and CPU/residency cost. `Confirmed` means code
shows the structural mismatch against the public/native contract; it does not
mean a real-library A/B was recorded. `Open` rows are explicit parity risks and
proposed optimizations, not established Kontakt defects. All referenced binary
evidence is qualified in the ledger, especially identity-only rows.

| Rank / ID | Kontakt evidence + doc section | KONTRA location and mismatch | User consequence | Concrete fix / independent witness |
|---|---|---|---|---|
| P0 T01 Confirmed | B1/B2 (identity); D1 Instrument header, D2 Instrument inserts | `sampler-kontakt/src/library.rs:2489,2507` folds program volume/pan into zones; `sampler-core/src/script_params.rs:327` stacks live instrument layer into each voice; `render.rs:617` applies it before summed instrument FX | Header changes alter distortion/compressor drive and pan before FX; send/tail level may differ from native | Separate instrument output amp from zone/group amp and from note expression. Map saved/live header controls to the same output stage. Native constant source→distortion and impulse→send-return fader sweep first to locate main/aux taps. Test against independent `fader(FX(sum(voices)))` rather than `FX(sum(fader(voices)))`. |
| P0 T02 Confirmed | B4 (pool identity); D7 purge_group / get_purge_state | `sampler-core/src/ops.rs:1527` purge is `Attenuate`; no residency command or async job. `script_params.rs:756` globally enables script render path | Samples remain in RAM; muted voices and pre-amp FX may still cost CPU; returned purge state does not describe actual unload/reload | Distinct `ResidencyRequest(GroupId, purge/reload, AsyncId)` plus eligibility/residency state. Worker retires heads/pages off audio and publishes completion; shared assets retain references for unpurged groups. Test resident-byte decrease, shared asset preservation, no new source admission while purged, completion identity. |
| P0 T03 Confirmed | B5/B6/B14; D2 Bus output bypass, Send return routing, Output inserts / Aux | `sampler-ir/src/lib.rs:1718` Output only Master/Bus; `sampler-kontakt/src/effects.rs:1752,1782,1800` routes returns to one target and all dry groups through insert; `library.rs:1843` only recognizes 1000..1015 script buses; `sampler-core/src/script_params.rs:289` maps unknown/default selectors to runtime master | Direct output, bypass-instrument-insert bus and aux-separated wet returns take the wrong path; multi cannot faithfully share output FX | Typed port destinations: instrument insert, instrument main input, instrument output, rack output, rack aux. Decode each saved selector and compile explicit edges. Test an insert saturator with a bypass bus and isolated wet output; two instruments share one aux history. |
| P0 T04 Confirmed | B16 (source identity); D1 Source Module modes | `sampler-kontakt/src/library.rs:1287` plays non-Sampler/DFD/nonadmitted-WT modes as sampler with diagnostics; `sampler-ir/src/lib.rs:414` only optional limited WT beside sample playback | Stretch, formant, slices, vintage transposition and advanced WT patches have wrong source sound/response | `SourceProgram`/`SourceKernelId` and capabilities, independent pitch/speed, shared analysis assets, per-voice source state. Implement separate documented/measured kernels; retain diagnostics until supported. Pin Conflux source configuration before worker changes. |
| P1 T05 Confirmed for checked FX; global rate Open | B11/B12; D3 modulation, D9 individual FX | `sampler-core/src/voice_mod.rs:1430,1713` CELL = 64; `render.rs:255` splits on that grid. Native checked wrapper has its own 32-frame countdown and EQ partial-call updates | Modulation/coefficient timing can differ; a single global rate cannot reproduce checked wrapper scheduling | Separate render tile from source/route/kernel clock policy. Persistent countdown per checked kernel, correct initialization and call-boundary law. Preserve B12's `[33]` vs `[32,1]` witness; do not change every modulator to 32 by inference. |
| P1 T06 Confirmed approximation; exact native pitch path Open | B12 control tick, B16 fanout; D3 Lag/LFO | `sampler-core/src/render.rs:643` holds pitch at cell midpoint; `:588` legacy filter factor midpoint; `voice_mod.rs:1498` endpoint LFO evaluation | Pitch steps, onset/event timing and fast vibrato/filter motion differ from continuously rendered/ramped targets | Allow constant, sample ramp, native hold, or audio buffer lanes. Use authored phase integration/analytic chirp fixture and native pitch impulse sweep to select exact compatibility law. Do not merely shorten CELL. |
| P1 T07 Confirmed missing tail policy | B9/B10; D2 chain scopes / bypass | `sampler-core/src/bus.rs:332,371` any fed frames reset whole bus tail even when samples are zero; `render.rs:905` feeds by produced frames | Silent held sources keep expensive summed FX active; zero-input reverbs/filters cannot independently sleep | Input activity propagated per edge; processor tail query/bound and active-node list. Silence+impulse+long-held-zero witnesses must count kernel calls as well as PCM. Keep pre-amp send feeds independent of dry-output silence. |
| P1 T08 Confirmed truncation risk | B9; D9 delay/filter/convolution, D2 serial order | `sampler-core/src/lower.rs:171,2348` generic bus tails 100 ms and chain tail = max, not serial sum; `bus.rs:507` clears only ProcessorState at expiry | Long/serial delays or resonant histories can be truncated; hidden reverb/convolution state can survive sleep/re-wake | Processor-owned tail state, serial conservative bounds where needed; explicit reset all storage belonging to that processor. Test two serial delays whose sum exceeds both individual tails, high-Q decay, and sleep/re-wake without stale audio. |
| P1 T09 Confirmed allocation omission | B13 (limit identity); D1 Voice Groups → Excl.Grp / TMP limits | `sampler-kontakt/src/library.rs:854` reports exclusion group; `sampler-ir/src/lib.rs:517` VoiceLimit lacks exclusive/source-cost domains | Hi-hat/choke relationships across different voice groups fail; costly source domains cannot enforce native resource limits | AllocationDomainId membership plus explicit exclusive-set edges and source-specific budgets. Test two groups in separate voice domains assigned to one exclusive set; fading victims temporarily exceed logical count. |
| P1 T10 Confirmed isolation defect | B1/B2/B13 separate objects; D1 per-Instrument group/release semantics | `sampler-core/src/steal.rs:230` monophonic-release scan checks dense group/key without plan; `:228` caps victims at 8 | A release in one loaded plan can cut another plan's matching group/key; excess layers may remain | Check owner plan (and generation) before group/key; use existing allocation-domain membership instead of an arbitrary 8-entry victim array. Focused test with two concurrently retained plans, same group/key, and more than eight eligible layers. No code change in this package. |
| P1 T11 Confirmed ordering asymmetry | B2/B7; D1 Group amplifier, D2 pre/post amp | `sampler-core/src/lower.rs:918` gain > 1 becomes a pre-chain Gain; gain ≤ 1 stays in voice amp; `:911` has already multiplied zone and group gain | Crossing unity changes drive of pre-amp nonlinear FX instead of only amplifier level | Separate zone/source gain from group amp gain. Represent boost directly at amplifier, remove artificial attenuator restriction. Test a pre-amp saturator across group gains 0.5/1/2 and independent zone-gain changes. Verify zone-specific gain position natively before fixing that half. |
| P1 T12 Confirmed channel-layout gap | B5/B6; D2 multichannel group processors, D9 Convolution multichannel support | `sampler-core/src/lib.rs` Frame is stereo; `bus.rs:252` buffers and processor histories are two-channel; `sampler-ir/src/lib.rs:1643` impulse stores left/right only | Surround/multichannel zones, processors, impulses and output routing cannot retain authored channel relationships | Layout-aware source/stage/port buffers with compiled channel maps. Preserve a specialized stereo fast path. Test channel-tagged impulses and separate output lanes. Convolution is documented for instrument/send/output scope: rejecting it in a voice is **not** by itself a Kontakt topology bug. |
| P1 T26 Confirmed missing Multi execution scope | B17 (UI identity only), B1; D10 Multi Script General Information / on midi_in | `sampler-kontakt/src/container.rs:16` Multi exposes only programs/sample names; no Multi scripts/output graph; `sampler-ksp/src/parity_ledger.rs:1866,2008` lists ignore_midi/set_midi unsupported; IR `lib.rs:67` has Instrument but no rack MIDI program | A Multi with MIDI processors cannot route/generate/filter messages before instrument selection; event ownership cannot match native by running the script as an instrument callback | Add RackDefinition containing raw-MIDI callback programs and shared output sums, lowering before instrument ingress. Distinct RawMidiEventId and NoteEventId; preserve generated note-on/off pairing and channel/port changes. Focused harmonizer/filter/CC routing fixtures, then decode native Multi script persistence. |
| P2 T13 Confirmed eligibility omission | B2/B16; D1 Source Module MIDI / HQI | `sampler-kontakt/src/library.rs:1235` reports per-group MIDI filter; `sampler-ir/src/kontakt.rs:84` retains interp_quality only as source metadata | Wrong groups may sound for channel-specific patches; authored quality/perf policy not honored | Add MIDI eligibility to semantic group/source admission and explicit source interpolation policy. Test disjoint group MIDI channels; selected-quality resampler vectors and kernel-work counters. |
| P2 T14 Confirmed residency-policy omission | B4; D1 DFD tab, D4 preload override | `sampler-ir/src/kontakt.rs:29` preserves preload; production search finds only `sampler-kontakt/src/objects.rs:18` assigning it; `stream.rs:368` head size uses measured latency+slack+block | Authored preload/global override ignored; memory and cold-onset guarantees differ | Explicit residency intent per source and instrument preload lower bound, plus configurable global override and latency safety floor. Verify native byte units first. A modern adaptive policy is valid when disclosed/configured; it is not identical imported behavior. |
| P2 T15 Confirmed destructive load-time mute | B2/B4 (identity); D1 Group Editor; D7 group operations | `sampler-kontakt/src/library.rs:1213` muted group becomes empty default group and returns None | Live unmute cannot restore its zones/FX/limits; preserved numeric address is insufficient | Keep definitions and physical identities intact; compile dormant execution/resources separately. Test muted saved group unmute without reloading, including sample offset/loop/chain and physical `find_mod` identity. |
| P2 T16 Confirmed core semantic gap | B2/B5; D2 per-voice group vs summed Bus, format-independent requirement | `sampler-ir/src/lib.rs:423` Group.chain means summed group DSP; `sampler-core/src/lower.rs:908` rejects it | UVI/keygroup summed chains cannot lower through same core; naming invites treating Kontakt group inserts as sums | Separate `voice_program` from `sum_node`; implement true summed group/layer chains as explicit nodes. Kontakt group inserts reference voice program; UVI scopes choose voice or summed programs from their native contract. Test two nonlinear voices vs nonlinear sum. |
| P2 T17 Confirmed expressiveness gap, current Kontakt workaround correct | B14; D2 Send Levels placement | `sampler-core/src/lower.rs:2339` rejects PreChainSend; `sampler-ir/src/lib.rs:682` relocates group fader to tap bus; Kontakt insert taps already split into continuation buses (`effects.rs:1727`) | Other formats/pre-chain sends need special lowering; indiscriminate fader relocation changes group pre/post behavior | Explicit tap ports at chain boundaries. Preserve current continuation semantics until equivalent graph compiler exists. Never relocate Kontakt amp across a nonlinear post stage. Test gain-before/after-tap and parallel send accumulation. |
| P2 T18 Open native history law | B7/B8; D2 Common parameters / Bypass | `sampler-core/src/dsp.rs:1099` fully bypassed Mix freezes inner history | CPU skips work correctly; re-enable may resume stale delayed audio depending on native reset/freeze/drain law | Add per-kernel bypass lifecycle contract; native bypass→wait→unbypass impulse witness per temporal family. Keep dry passthrough and ramps; change hidden history only after evidence. |
| P2 T19 Open native victim law; confirmed architectural cost | B13; D1 Voice Groups → Kill Any / Pref.Rel | `sampler-core/src/steal.rs:19,194,201` Any=quietest and released-first victim; `:163` repeatedly scans allocated slots per admission | Dense layered attacks may waste CPU; native victim choice may differ | Maintain domain counts/member lists; scan only on actual over-limit demand. Native labeled victim probes for Any/Pref.Rel, attack vs release, ties and pending voices before changing selection. |
| P2 T20 Confirmed extra per-voice work; native sharing ownership Open | B16; D3 LFO retrigger/freewheel | `sampler-core/src/voice_mod.rs:1491` shared/freewheel math still evaluated per voice; free phase `:1505` = Hz × absolute time; `:1467` stores values per voice | Duplicate shared-source work; tempo changes can jump free phase | Shared SourceInstanceId with independent destination smoothing; integrate phase over elapsed tempo segments. Verify group/global retrigger scope. Test two groups/two sources and a tempo change without phase discontinuity. |
| P2 T23 Confirmed amp control omission | B5 reads invert/swap flags before balance; D8 PHASE_INVERT / LR_SWAP | `sampler-kontakt/src/effects.rs:1502` BusPlan retains volume/pan/groups but no invert/swap; `sampler-core/src/engine_parameters.rs:725` amp layer handles volume/pan/tune/output, not these flags | Group/bus polarity or channel swap writes lack audible consumers; separate Inverter FX support does not implement amp flags | Bind distinct amp matrix lanes with saved values, separate from rack Inverter. Verify group order and stored flag decoding; use B5 for bus sequence. Channel-tagged fixture checks swap/invert relative to bus balance. |
| P2 T24 Confirmed missing bus send tap | B5/B14; D2 Send Levels in Bus / explicit send taps | `sampler-kontakt/src/effects.rs:1614,1623` builds a bus Chain but only uses c.processors; `:1793` emits no bus sends, discarding c.send_taps | Send Levels in an instrument bus does not feed the return; bus-only wet signal is absent | Reuse ordered continuation/tap lowering for every eligible summed chain, not just instrument insert. Test bus-only Send Levels feeding a nonlinear summed return and a following insert. |
| P2 T25 Confirmed missing live send tap binding | B14 separate current send values; D2 Send Levels, D8 engine controls | `sampler-kontakt/src/library.rs:823` resolves voice taps with gain_control/bypass_control=None; `effects.rs:846` excludes SendLevels from dynamic Mix handling | Saved tap works but script/automation changes to its level/bypass cannot drive those compiled tap values | Attach physical group/rack/slot/send-index controls to tap edges, with checked transition law; update bus taps too. Test live level/bypass changes around an amp split without changing return identity. |
| P3 T21 Confirmed idle traversal; native complexity Open | B10/B13 support activity bookkeeping, no native O(1) proof; D4 Engine | `sampler-core/src/render.rs:238,258,270` scans all retained plans per cell; `bus.rs:323,364` walks all nodes; `whole_voice.rs:426` shared atomic counter per native voice block | Empty graphs still incur traversal; diagnostic atomics cause cache traffic on active voices | Active generation/node queues, dirty epochs, count diagnostics per worker then reduce once/block. Show idle cost independent of loaded dormant groups; preserve ascending voice sum. No claimed percent speedup without profiling. |
| P3 T22 Open native callback scheduling | B15 unresolved dispatcher; D5/D6 | `sampler-core/src/render.rs:197` resumes yielded callbacks before apply_due; `schedule.rs:61` equal-time submission order; `stages.rs:62` note-slot pipeline | Possible same-frame wait/event/async ordering differences; not established by docs | Preserve core sample queue and explicit host-block fuel. Add frontend TimingProfile only after native host-block/offset/zero-length/wait probes; separate worker completion from callback execution. |

The table covers structural mismatches found in the inspected engine/render/IR/
Kontakt translation paths. DSP formula coverage is a separate inventory; no claim
of exhaustive binary recovery or all-library conformance is made.

## Concrete format-agnostic target

### 1. Semantic graph, compiled execution plan, and address binding

Extend IR incrementally rather than adding Kontakt-shaped objects to the core:

```text
RackDefinition { raw_midi_programs, instrument_refs, output_sums, aux_sums }
ProgramDefinition
  sources: [SourceProgram { kernel, parameters, analysis_asset, residency_intent }]
  selectors: [SelectionDomain { group_id, zone_refs, eligibility, release_profile }]
  voices: [VoiceProgram { source_id, pre, amplifier, post, tap_ports }]
  sums: [SumProgram { stable_id, layout, ordered_stages, ports, destination }]
  modulation: [ModSource { instance_scope, clock, routes, inputs }]
  allocation: [AllocationDomain { limit, victim_policy, fade, exclusive_set }]
  controls: [ParameterBinding { semantic_owner, stage, law, transition_profile }]
  callbacks: [CallbackProgram { slot, event_kind, ownership, timing_profile }]

PreparedPlan
  immutable kernels/tables + dense parameter/stage/port indices
  bounded per-voice state layout + shared state layout
  active dependency schedule + stable mix fold order
  source-address → semantic-owner → compiled-parameter map
```

Use separate IDs for Group/SelectionDomain, VoiceProgram, SumNode, Instrument,
Asset and AllocationDomain. IDs are plan-generation-qualified at runtime.
`SourceAddress` remains frontend-owned metadata (`format, physical group, rack,
slot, parameter`), never a generic render switch on KSP strings or chunk IDs.
Unsupported source semantics remain explicit in the load report.

`sampler-kontakt` lowers group inserts into VoiceProgram and program/send/main/
bus/output chains into SumProgram. A UVI/Falcon frontend lowers its hierarchy
according to its own voice/layer/keygroup/part scope; container nesting alone
cannot select DSP scope. Other frontends use the same nodes/laws. Per-format
compatibility profiles select checked kernels/clocks; core processing does not
decode formats. No v1 file-format compatibility is needed.

Compilation resolves channel layouts, edges, state offsets and parameter consumers
off audio. Maintain DAG validation for feed-forward audio. Represent delay/feedback
inside a kernel initially; arbitrary graph feedback is not required for this
Kontakt fix. Split at every ordered tap; never commute a nonlinear stage.
Fold constant gains only within one scope with no tap/automation/modulation boundary.
Keep physical empty/bypassed slot addresses without allocating audio work for them.

### 2. Runtime state and schedule

Keep generational ownership and stable sample time. Add explicit running sets:

- **VoiceState:** source cursor/state, note/family/plan/group/domain IDs,
  private modulators/smoothers, private DSP, steal fade, streaming windows.
- **SharedSourceState:** source ID + declared shared scope; phase/seed/tempo epoch,
  output revision, next deadline; zero-consumer sources can be virtually advanced
  only where their native state law permits it.
- **SumState:** owned channel buffer, processor states, input-activity epoch,
  active/tail status, parameter wake flags, downstream edges.
- **DomainState:** live nonstolen count, members, fade members, exclusivity links;
  counts updated on admission/steal/end, not rediscovered on each note.
- **AssetState:** immutable revision, per-group residency references, resident
  spans/page ownership, outstanding request deadlines and purge/reload generation.

At an audio timestamp: drain due submitted events in stable order, run admitted
callbacks until completion/wait/fuel bound, apply their writes/selection, prepare
new source starts, then render until the next event or kernel boundary. Listener
deadlines and async-complete events keep clocks advancing even with zero voices.
The unresolved Kontakt same-time order is a frontend profile/probe concern, not
permission to change core ordering arbitrarily.

Compute shared modulation once per relevant epoch. Constant/event lanes stay
constant until revision changes; route lag and private envelopes remain private.
Audio-rate sources provide buffers; checked control kernels provide hold/ramp
lanes with their own persistent countdowns. A 64-frame SIMD tile can remain while
internally splitting at 32 or another verified quantum. Empty host renders may
settle due events but must not fabricate DSP progress.

Render only active voices (retain bitset), then dirty/active sum nodes in compiled
topological order. A sleeping upstream node provides an implicit zero buffer;
downstream tails can remain active. Preserve ascending voice slot fold and stable
edge order for current deterministic tests. Workers process only sufficiently
large batches; one note stays on the audio thread. No busy-spin pool or storage
wait is permitted on the realtime path.

### 3. Activity and lifecycle contract

Every kernel must declare enough behavior to skip safely:

```text
ProcessResult { output_activity, tail_state, next_internal_deadline, fault }
TailState = Empty | FiniteRemaining(frames) | StatefulActive
Capabilities { zero_input_can_generate, virtual_advance, reset, bypass_policy }
BypassPolicy = checked PassAndFreeze / PassAndReset / PassAndDrain / other explicit law
```

These are proposed core interfaces, not recovered Kontakt enum names.
Output activity must account for gain, phase cancellation, history and future
automation. A generic peak threshold alone cannot sleep self-oscillation. Use
exact empty state for FIR/delay where available; a verified energy/history bound
for stable IIR/reverb; special treatment for generators. Update activity while
processing buffers that must already be rendered, avoiding a second full scan.
Model serial tail propagation, rather than `max(tail)` for a whole chain.

Bypass/mute/purge differ: bypass follows a kernel's checked history contract;
mute keeps the appropriate musical/source clock; purge changes asset residency
and source eligibility. A live silent voice may virtually advance source/envelope
without sample reads when no pre-amp taps or stateful/nonzero-input-dependent
processors require audio. Wake on parameter/source/event revision, input activity,
or a due tail/clock transition. Never kill a held voice solely for amplitude zero.
Retire a completed source/voice and retire its DSP tail independently when required.

### 4. Streaming and CPU contract

Reuse the existing immutable AssetId, protected page epoch, bounded request/result
exchange, parked decoders and native live/offline contracts. Head sizing must cover
declared source start ranges, interpolation guards, maximum admitted rate and host
service latency. Do not copy/decode a full sample to play one voice. Deduplicate
page demand across voices; only active traversal windows request new pages.

Honor authored Sampler/DFD intent and preload override policy after units are
verified. Adaptive preload can increase safety, but report the effective bytes
and budget decisions. Purge/reload owns an async revision and group residency
reference; stale completion must not resurrect a purged group. Reclamation, file
I/O, decoding and shared-handle destruction occur off audio. A transient cold
start and a deliberately purged group need separate states.

Minimum CPU acceptance is structural before it is numerical: no storage wake with
no demand; no voice FX without active/tailing voices; no summed FX without input,
tail or generator work; constant parameters avoid conversion/recomputation; shared
sources avoid per-voice duplication; callbacks execute only when due. Preserve
diagnostics using per-thread counters reduced once per block.

Reuse evidence-backed v1 code where it is better. This pass read
`0cb7a8a0:src/fx/processor.rs:95` (`Sleep`, peak-dependent tail feeding and wakeup)
and `src/engine/voice.rs` (silent-source pause policy). Port those helpers where
their contract fits, adapting ownership/activity to the graph; do not equate
v1's −120 dB policy with B9's `2^-16` marker or claim native equivalence. The
existing v1 control-owner port should remain until its replacement passes the
same checked laws. No frozen v1 binary was available for measurement here.

Measure using existing `sampler-perf`/`tools/cpu-audit-*`, not a new benchmark
framework. Report release profile, rate/block, actual source voices, FX/tails,
loaded/dormant groups, callback rate, p50/p99, top perf frames and
`100 × render_time / (frames / rate)` percent realtime per core. Test silence,
one-note Afflatus, layered chord, Conflux synth movement, release tails and
zero-voice-but-listener cases. Compare identical native quality/source/FX settings;
frozen v1 directory was absent here. Numeric goals/claimed gains require the
coordinator's quiet measured window; this design supplies none.

## Migration sequence and tests that stay green

Each step is an independently reviewable commit on the worker's branch. Preserve
the old path until new semantic data can lower to an equivalent plan, then remove
that path once equivalence and the intended correction are demonstrated. Do not
overwrite existing expected PCM to hide a changed topology.

| Step | Owner/files and executable work | Acceptance before next step |
|---|---|---|
| M0 Evidence / fixtures | RE worker: restore full Ghidra export if possible; record native stage-order, callback, victim/bypass/DFD probes listed below | Hash-pinned address/doc law and independent expected vectors for each behavioral change; current evidence gaps remain explicit |
| M1 Ownership / allocation | Core worker: `steal.rs`, ownership tests. Fix T10 plan-qualified mono release and victim capacity; add domain IDs/counts, then T09 exclusivity. Do not change Any/Pref.Rel without probe | Two-plan isolation and complete multi-layer cuts; new fail-first fixtures; existing `steal`, `choke`, `release_selection`, generational ownership fixtures |
| M2 Semantic IR ports | IR/translation worker: `sampler-ir/lib.rs`, validation, Kontakt routing readers/`effects.rs`. Add optional typed destinations/source/voice/sum IDs and bindings; old import paths map explicitly | Existing pre/amp/post send witnesses retain PCM; each physical slot/address remains findable; unknown selectors remain reported; graph validation rejects cycles/mismatched layouts |
| M3 Fader/order correction | Translation/core worker: T01/T11 saved/live controls, route output amp stage; source gain and group amp separate. Coordinate `library.rs` ownership | Native order witness plus independent nonlinear order test. Existing `buses::voice_modulation_gain_is_at_the_amplifier_between_send_taps` and send-tap fixtures unchanged; new header sweep checks dry, wet and tail |
| M4 Activity/tail lifecycle | Core DSP worker: `bus.rs`, `dsp.rs`, per-kernel tail contracts. T07/T08/T21 active nodes first on summed scopes, then voice scopes | Impulse→silence→wake PCM, serial delay support, no stale history, exact kernel-call reductions. Existing bus-tail/generation/convolution and heap guards; no claimed CPU result from debug timing |
| M5 Purge and dormant definitions | Core/stream/KSP worker: T02/T15. Retain muted group content; separate residency request/eligibility/attenuation and publish AsyncId completion | Shared asset and in-flight job tests, resident bytes, purge state, unmute restoration, no premature async completion; existing `paged_render`, `stream_cache`, cold onset semantics remain green |
| M6 Modulation clocks / shared sources | Modulation worker: T05/T06/T20. Add per-kernel clocks/lanes and shared phase ownership with frontend profiles; retain measured v1 owner where applicable | B11/B12 original-byte vectors; offset events, tempo changes and fast LFO native witnesses; core partition-invariant tests only for laws declaring invariance; single/multicore PCM equivalence |
| M7 Sources / channel layouts / full routing | Source+format workers: T03/T04/T12/T13/T14/T16/T17/T23/T24/T25. Distinct source kernels, verified module scopes, shared rack sums and layout-aware buffers | Each source/FX admission has vendor+binary law, focused independent fixture; direct/bypass/aux output test, two-instrument shared aux, channel filtering and authored quality/preload policy |
| M8 Native callback profile + measured gate | KSP/host/perf workers: T26 Multi ingress, then T18/T19/T22 only after probes; adopt proved history/victim/timing laws and measure complete patches | Host block/offset tests and current callback/fuel/ownership tests; one final logged full gate by coordinator, with native audio and quiet CPU comparisons |

Workers run only their changed crate's `cargo check` and focused tests via
`~/.cache/kontakto-heavy`, then final workspace `cargo test --no-run`. The
coordinator runs the combined full suite/survey once. Existing tests named above
are preservation gates, not newly claimed native parity tests. This package
changes documentation only and therefore adds no implementation-mirroring tests.

## Required native probes before closing open rows

Use existing inspector/reference tools; no decrypted scripts/samples on disk.
Before an instrument-specific change, inspect that instrument's group/source/
saved values/script intent first. This package's synthetic design does not assume
Afflatus or Conflux authoring details without that inspection.

1. **Fader / routing:** constant source + nonlinearity on each rack, impulse send,
   header and group gain/pan at saved/live values; observe direct bus, bypass-insert,
   send-to-aux, main input and output separately. Trace numerical stage changes.
2. **Modulation:** LFO pitch/amplitude/cutoff at slow/fast rates; events at offsets
   0,1,31,32,33,63,64 under host blocks 32/64/128/257; retrigger in two groups,
   tempo change, empty block. Find the voice caller that sets AHDSR object rate.
3. **Allocator / release:** labeled pitches and ages at small limits; Any and
   Pref.Rel on/off, pending and released voices, shared voice domains, exclusive
   groups, multi-layer repeated release and pedal/system-script interaction.
4. **FX lifecycle:** impulse, zero input, bypass, wait, re-enable for each temporal
   family; correlate tail query with actual output. Confirm whether B9's threshold
   is a sleep input, a history marker, or both in its caller.
5. **KSP schedule:** callbacks that log only IDs/times/control changes; immediate
   note, wait(1/1000), listener, note-off and async-complete on one frame across
   host blocks. Distinguish musical sample clock from `$KSP_TIMER`. Recover dispatcher
   and continuation queue entry before claiming audio-thread affinity.
6. **DFD:** locate sample-pool preload setter and disk worker entry/queue; confirm
   stored bytes vs channel/sample units, override/default, requested offsets,
   shared residency, purge during playback and idle thread CPU. Native cold disk
   timing must be distinguished from warmed OS file cache.

No row becomes “parity complete” because a diagnostic was suppressed or because
the translator retained opaque metadata. Imported meaning must reach an audible
or observable runtime consumer and be validated at that boundary.


## Validation of this research package

- Documentation-only diff; no DSP, script or library data changed and no new test
  was added. Proposed future tests in the table were not executed in this package.
- Binary identity/selected functions/constants checked as recorded in the evidence
  note; vendor sections fetched; local links and source path/line bounds checked.
- Via `~/.cache/kontakto-heavy`: root `cargo test --no-run` passed (52.57 s build
  phase), then explicit `cargo test --workspace --no-run` passed (49.31 s build
  phase), with existing compiler warnings. These are compile checks, not test
  executions or performance measurements; queue time is excluded.
- New-file Markdown tables/fences and `git diff --check` validated. Temporary
  scratch and this package's target directory are removed after completion.
