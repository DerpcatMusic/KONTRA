# W7 selection checkpoint

Branch `v2/fix-selection` includes integration `be4c5c21` through merge
`5aac621f`. This checkpoint is not a complete Kontakt parity verdict.

Engine contracts pass: source identity maps, current-event group masks,
creator slots and `$CURRENT_SCRIPT_SLOT`, callback IDs and `stop_wait`,
release velocity/counter reset, composed native predicates, saved default key,
explicit cycle reset, eight physical loop slots/count/tuning/serial traversal,
streamed/RAM partition equality, and sustain-delayed release age.
W4 remapped `Switch::Articulation` and source keys drive the same composed
predicates, including saved-default-to-runtime-zero numbering without zone tags.

Validation: core lib 59, lower 14, native criteria 2, paged render 15,
release 4, release selection 18, source 9, voice modulation 11 (1 ignored);
Kontakt lib 37 (3 ignored), KSP selection 8, IR source indices 1.
Root `cargo test --no-run` passed. Native vector gates remain **pending native
vectors**: mixed joins/precedence, counted wrap/ping-pong, tuning onset,
ping-pong crossfade, slots 1..7 traversal. Overlapping loop ranges reject
preparation. Alternating crossfade preserves metadata and an UnknownLaw
finding; exact native transition is not claimed. The new ignored real Una
Corda source proof passed on6ae82fa7; see the receipt below.

## Areia controls before W5 init-service merge

Existing reference harness, Sustained NKI, keys 60/72/84, velocities
20/60/100/127, four repeats. MIDI SHA256
`7c72ec986cdc840ad148d6aba6f09125d9eb1f6e8425146aa0a89b48e4aa3f42`.
Reference calibration passed; GUI state was skipped for this instrument.

| Comparison | Articulation/dynamic family agreement | Identified |
|---|---:|---:|
| Native vs same native WAV | 48/48 | 48/48 each |
| clean v1 `0cb7a8a0` vs native | 0/48 | 48/48 each |
| v2 baseline reader `6ba9fa72` plus existing key-filter helper `06d24f02` vs native | 0/48 | 48/48 each |

v1 uses the original compressed `v1.args` and detector `*0.5`, as required
by the harness because v1 caps output at 60 seconds. An earlier full-spacing
v1 run identified only 30/48 and is superseded. The table compares the first
NCC candidate after removing RR index and key suffix; it is not proof that every secondary greedy match sounded.
TSVs include all matches; offset/direction is supplied only for the fitted
primary source, and blank secondary values remain unresolved. Gain is the
detector's fitted gain, not a proposed correction.

Native strongest families vary by register: key60 Sustained VFMp/VFM Dyn2,
key72 Sustained VSpt Dyn2, key84 Tremolo Spt Dyn3. Baseline is Sustained
VFM/VFMp Dyn3. This may involve scripted group gain/selection, not only a
dynamics knob. W5 `76fa0487` init-service and six-sigil persistence merge and
unmodified rerender are the next gate; forced-state tests remain diagnostic.

Script intent inventory: dynamics and store saved at 127; dynamics range
0..127; init writes CC110 from the store and derives MOD_VALUE_ID1 from it.
Controller callback has no literal CC1/2/11 read. Group velocity ranges are
1..127 and native criteria empty. Script group masks combine authored
articulation/dynamic/repetition matrices. Five inline Program-private v0x71
automation candidate objects occur at offsets 77,129,181,233,285; tags
`pts_script_slider_0_40`, `_0_42`, `_0_43`, `_0_41`, `_0_44`, address-mode1,
IDs0/-1, ranges0..1. Field meanings and tag-to-widget mapping await RE.
The harness does not assign MIDI learn or edit dynamics. RE's read-only
Settings.cfg/user.reg inventory found no CC-to-widget binding keys. None of
this justifies inventing an initial value or a gain offset.

Own WAVs were written only in `~/.cache/kontakto-w7-render/` and removed as
soon as their metric TSVs existed. No source samples/decrypted bytes are
retained or committed. The retained baseline executable is in scratch;
finished `kontakto-audit-kontakt` target was deleted on coordinator request.

## Saved automation root cause and rerender

Merged W5 `76fa0487` and `ecda68c9`. Before the automation fix, W5 persistence/init changes alone still scored **0/48** families. The saved Program-private table is the missing route: CC1 → slot0 slider ordinal40 (dynamics); CC16 → ordinal41 (dynamic range); CC11 → ordinal42 (expression); CC21 → ordinal43 (vibrato); CC17 → ordinal44 (sample start). These are slider-only declaration ordinals, resolving to UI IDs33371/33373/33375/33377/33379, respectively. All five records are mode1, soft takeover0, range0..1. Their private record headers are at77/129/181/233/285, but the production parser walks preceding arrays rather than hard-coding those offsets.

The reference sends CC1=64 and CC11=127 before note0. Generic saved automation now invokes the original typed widget callback before the note. Dynamics is driven to64 and writes CC110 through its authored callback. Unsent CC16/21/17 are not invented or dispatched. No initial value or gain was patched. The rerender scores **40/48** families: key72 VSpt/Dyn2 and key84 Trml/Spt/Dyn3 match all16 notes each; key60 VFM/Dyn2 matches8/16, while native's other8 strongest fits are VFMp/Dyn2. Remaining prefix differences are failures, not RR penalties. No `Fault(InvalidInput)` was logged. Fitted gains remain diagnostic.

Native-vs-native control remains48/48; clean v1 and baseline v2 remain0/48 under family scoring. Separate `*-rr.tsv` files report observed strongest-fit RR sets/counts. This grid fits RR1 throughout native and v2; it does not demonstrate native RR cycling or establish a distribution parity gate. An identical native WAV control validates detector repeatability, not independent source truth. Eight key60 mismatches may involve mix weighting or sample variation; they are not assigned a cause without more evidence.

The new reader supports accepted Program versions0x91/0x92/0xa0..0xa8 with legacy min(inner,outer-capacity) semantics and0xa9..0xb5 with modern inner-count semantics. It walks raw0/version0x50 ArrayA/ArrayB elements, counts UTF-16 code units as2 bytes each, rejects ArrayB K>64, and reads BAO0x70/0x71. Older0x80/0x82/0x90 and unknown versions produce unsupported diagnostics. Raw private bytes and unrelated fields remain owned by the original reader.

Failing-first runtime and decoder contracts preceded implementation. Passing regressions cover CC/host scaling, same-timestamp callback order, soft takeover/rearm, nonempty arrays (first BAO header at exact offset140), legacy/modern version gates, malformed headers, truncation, and slider-only resolution despite labels/knobs. A fixture offset was corrected for the preceding label: the observed script variable occupies cell1. V1's controller path only enqueued controller callbacks; its separate UI path clamped values and ran UI callbacks. V1 also explicitly warned that native group criteria were retained but not evaluated. The v2 fix reuses typed widget admission and does not import a second VM or invent missing automation defaults.

## Current Una Corda source-loop proof

The runnable ignored real-library test passed for Pure zone3725, Felt/Cotton3726:3,036,139 frames per preset; actual source loop33761..462673; exact equality to independently unrolled PCM and nonzero audio. `una-loop-proof.tsv` retains only aggregate metadata. No WAV or source samples were written. This replaces the missing historical probe as a reproducible source/engine consistency check and remains separate from the five pending native vector gates.

## Landing gates

Final post-W5 checks: core lib62; controller stages5/controllers11/controls10; lower17; native starts2; paged render15; release4/release selection18; source9; voice modulation11 (1 ignored). KSP automation4, selection8, typed widgets7; decoder/loader4; actual Una source proof1 covering3 presets. Root `cargo test --no-run` passed. Automation CC execution is checked without heap activity; both callback capacity and pedal-domain capacity reject before widget/input writes. The pedal regression failed first (target changed90→127 despite Capacity), then passed using the shared pedal-domain preflight. Failed unbound pedal inputs preserve the old raw CC bank, while bound widget callbacks see the newly admitted CC.

## Native modulator/target lookup follow-up

IR API: `Instrument.source_indices.engine_lookups: Vec<sampler_ir::SourceEngineLookup>`, with `{group:i32, owner:i32, target:bool, name:String, index:i32}`. Mod records use owner-1 and physical modulator slot as index. Target records use the owning physical modulator slot and authored target ordinal; unnamed or unsupported targets still occupy their original ordinal. Internal/external identity remains in `source_indices.modulators`. Lookup inventory runs before script init and before muted/unsupported DSP omissions. Init group names also retain muted groups' physical positions. Runtime DSP source maps now assign internal modulator runtime refs before amplitude-envelope optimization.

`sampler_core::lower::source_engine_lookups(&SourceIndices)` converts the source records to W5's typed `EngineLookup` table. W7 supplies the same table to init evaluation; W5 owns installing it alongside authored parameter bindings in Prepared. The seam regression exercises init and note lookup with the authored runtime table explicitly installed. No behavior/current-slot/symbol service logic was duplicated. A second failing-first regression proves restored modulation intensity uses physical slot31 rather than a name hash; source-target ordinal tests retain unnamed target0 before named target1.

Standalone host bounds were subsequently verified by RE: BAO reader preserves fullu16, creation accepts0..=2048, and host limit initial value is2049. Program/Group runtime remap applies a delta then clamps to0..2048. Wire mapping for the delta fields is still unavailable, so lowering does not invent or apply a remap. W1 received the exact bound and this limitation. VST3 published capacity/address2048 meaning remain unverified independently of the UI ID ceiling.

RE provenance: [re/w7-vectors-20261008 at d773d214](https://github.com/DerpcatMusic/KONTRA/tree/d773d214/docs/research/w7-native-20261008). The checked translator uses exactly serialized0=AND,1=AND_NOT,2=OR. Reader vectors and host boundary vectors establish serialization/address behavior; they are not playback measurements. The edited native fixture was rejected before MIDI. All five criteria-precedence/count/tuning/ping-pong-crossfade/multiple-loop gates remain **pending native vectors**; no authored case was treated as a measured expected result.

Local rerun of the unmodified d773d214 verifier passed32 pinned original PE ranges,12 original-reader vectors with stubbed I/O, and16 native address-bound vectors. Verification read the pinned executable and wrote no prefix state or audio. The bounded BAO reader now rejects mode enums outside0..2 while retaining host wire address65535; invalid enum errors report the exact public-field offset.

## Event query and parameter follow-up

The nine newly enabled named audit contracts first failed (existing eight lifecycle
contracts passed): status0 instead of1, enumeration search-1 instead of0, mark0
instead of1, group allow0 instead of1, custom15=0 instead of42, thirteenth
modulator0 instead of42, affected-group size4096 instead of1, mode2 volume-9000
instead of-3000 and pan0 instead of-1000. The fixes use generation-scoped source
aliases and callback group projections, not a second event registry.

Nonignored `selection_contract` now covers all requested names plus creator/current
slot, distinct note/release callback IDs, stop/resume/disable-wait, release velocity
and release-counter reset. Boundary checks exercise unknown/retired IDs, terminal
acceptance and note-slot reuse, mark deletion, deferred generated-note group masks,
release-stage group masks/tags, all1001 modulator IDs alongside all16 custom
parameters, signed minimum values, invalid index separation, dynamic mode1 versus2,
physical group holes, dynamic affected-group search and allocation-free callbacks.

Custom indices0..3 alias `$EVENT_PAR_0..3`; storage holds the complete bounded native
modulator/custom domain, replacing the twelve-entry silent-drop ceiling. Voice
modulation borrows that state rather than copying it per chunk. Enumeration includes
lazy IDs and appends one zero sentinel; `%GROUPS_AFFECTED` describes native mapping
before script allow/disallow edits. Sources: [NI event commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands)
and [NI built-in variables](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/built-in-variables-and-constants).

Waveform asset seam: `Prepared::source_zone_region(zone_id:u32)->Option<usize>`
resolves explicit positive source IDs used by `EventInfo::ZoneId`; holes,0 and
absent source maps returnNone, with no runtime ordinal fallback. `Prepared::region_asset(region:usize)->Option<&Pcm>` returns the
immutable prepared asset, including its `AssetId`, for the control-side peak worker.
The importer retains physical zone order in `source_indices.zones`; lowering uses
physical index+1 as the source ID. This is independent of UI display ordering.
W2/W3 own provider/painter/worker wiring. A sparse3/9 source-ID regression passes.

## Analog saved automation inventory

The framed Program0xab private reader finds287 BAO0x71 records, all mode2 host
parameters at addresses0..286, all soft takeoverfalse/range0..1; there are **zero
mode1 MIDI-CC assignments**. The metadata-only `analog-automation.tsv` records every
address/tag and222 resolved slider-only ordinals. The65 switch tags remain diagnosed
as unmodeled; native switch ordinal decoding is not inferred from slider evidence.

Compressor gain bindings: host122 → slot2 slider71/ui32956, layer1 saved396933;
host128 → slider153/ui33136, layer2 saved396933; host173 → slider185/ui33198,
global saved560434. Compressor switch tags occur at host117/123/168/174. These
bindings run only when host automation arrives; saved MIDI learn does not explain
an init/CC compressor change here. The normal script init/persistence engine writes
are a separate W5 path. The subsequent reported static compressor probe supports
compressor on/off as the approximately 9 dB difference; native full-grid switch
state remains unknown. No gain correction is proposed. Existing Areia40/48 and
five **pending native vectors** gates remain unchanged; no new native audio parity
measurement is claimed.

Follow-up validation: KSP lib5, selection/event26, automation4 and typed widgets7;
core lib62, lower17, controllers11, note stages8, release4, release selection18,
script arrays3, source10 and voice modulation11 (one ignored). Root
`cargo test --no-run` passed. Source-ID lookup and real-library metadata probes
write no audio/sample data. Logs reside in `~/.cache/kontakto-w7-render/event-*`.

Analog init metadata: all compiled slots2/3/4 report no `set_controller` writes.
The normal slot2 init does emit gain/bypass engine requests, including
`$ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN=560434` at group-1/slot1/generic1 and
layer outputs396933 at group-1/slot1/generic1002 and1003. All108 selected
GAIN/BYPASS requests are retained in `analog-init-engine-writes.tsv`; they are
compile/init intentions, not measured runtime/native DSP outcomes. W5/W6 can compare
these exact addresses against typed service admission and saved rack state.

`Pcm::resident_frames()->Option<&[Frame]>` exposes read-only resident frames,
`Frame=[f32;2]`, for bounded peaks in memory. `frame_count`, `sample_rate` and
`asset_id` supply length/rate/identity. Streamed assets can returnNone; the asset
worker must service them separately, never invent an ordinal or silent waveform.

## Analog reported static compressor diagnostic

The reported metadata probe used the shared production Mix lowering with
the saved static gfx compressor and existing output trim. The exact compressor
address is group=-1, slot=1, generic=1, OUTPUT_GAIN=560434, bypass=0. At C4
(key60), velocity64, CC1=100 and CC11=127, reported RMS was0.0932991;
forced bypass produced0.0331272: about8.994 dB from the stated RMS values
(the supplied receipt reported approximately8.9945 dB).
`analog-static-compressor.tsv` retains these reported metrics; W7 did not perform
this render. No synthetic init CC or importer state change was made.

Verdict: this supports compressor on/off as the approximately9 dB fresh/cached
level difference. It does not establish the native full-grid compressor switch
state, which remains **unknown**, or complete native DSP parity. The saved
controller-automation inventory contains no MIDI-CC bindings, so this evidence
does not support a CC-binding explanation. No gain/default/bypass patch is justified
from this single static diagnostic.

## Lifecycle audit follow-up on W5 f040122b

Merged W5 `f040122b`, including the production physical-envelope service fix,
into `v2/fix-selection`. No service implementation was forked. Failing-first
log `~/.cache/kontakto-w7/lifecycle-red.log` reproduced marked-note release,
all-event release, and virtual pitch-bend admission failures (26 green, 3 red).

Ported the selector rules from `0cb7a8a0:src/ksp/runtime.rs::targets`: a plain
source ID, a union of marks, or `$ALL_EVENTS`. `KeyUpEvent` scans only notes in
its owning plan and retains the existing delay, fixed-duration, release-stage,
and pedal semantics. The bounded scan does not allocate; nested release
callbacks remain queued until the instruction ends. Other builtins' multi-event
selectors are outside this follow-up and retain their existing diagnostics.

The existing controller event path now admits script virtual controllers128
(pitch bend) and129 (mono pressure), including raw input, projected slot banks,
consumption, generated next-slot writes, and immutable note snapshots. Copied
v1's signed pitch units (-8192..8191) for `%CC[128]`, `$PITCH_BEND`, and
`set_controller(128, value)`; pressure remains0..127. Full-resolution native
values retain min/center/max. These script values are separate from native
`PREVIOUS_KEY`/axis selectors128/129; a PCM regression proves neither aliases.
Controller130 is rejected. Existing invalid-input fixtures now use130.

Validation: core lib62; controller stages5, controllers12, lower21, native
start2, note service2, release4, release selection18, source10. KSP lib5,
automation4, controllers10, selection32, typed widgets9. Allocation-free
contracts cover mark unions/zero masks, unmatched notes, original source IDs,
virtual signed extrema, consumed input, next-slot generated writes, and
snapshot retention. The original audit improves from37/43 to39/43: both W7
cases pass; the remaining async IR, global UI callback, PGS init ordering,
and MIDI-file buffer cases remain with W5. Logs are in
`~/.cache/kontakto-w7/lifecycle-{core,core-integration,ksp,audit}.log`.
Root `cargo test --no-run` passed; the compile receipt is
`lifecycle-root-no-run.log`.

The MIDI ingress/MPE pitch and pressure adapters currently update expressions
directly; routing those inputs through script callbacks and enforcing consumed
expression updates remains a W1 transport follow-up. W1 has the128/129 seam;
this receipt establishes the Runtime dispatch path, not adapter parity.

No Areia rerender was performed for this lifecycle change: the last measured
family agreement remains40/48 in `areia-automation-family.tsv`, with the
remaining key60 VFMp/VFM split deferred. All five native criteria/loop playback
gates remain **pending native vectors**, as stated by RE branch
[`re/w7-vectors-20261008@d773d214`](https://github.com/DerpcatMusic/KONTRA/tree/d773d214/docs/research/w7-native-20261008).
The Analog static compressor witness and unknown native full-grid switch
state caveat above are unchanged.
