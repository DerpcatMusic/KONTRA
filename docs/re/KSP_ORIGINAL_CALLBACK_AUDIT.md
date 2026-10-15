# Original Areia and Solo callback audit

This audit reads the installed instruments in memory. It stores counts, source
locations and hashes, with no vendor script, artwork, sample or native pseudocode.
No native application was launched. Static native evidence is distinguished from
an independently replayed native result.

| Original case | Located owner and finding |
| --- | --- |
| A2, Areia slot 0 line 23623 | Directly in `persistence_changed`, lines 15255–23713. The missing lookup is an optional branch guard testing a positive result, not an unconditional group selection. All 1,122 source groups remain present, with no source-map holes; the requested name has zero exact, case-folded or normalized/trimmed matches. |
| A3, Areia line 47119 | Directly in `ui_control`, lines 47112–48282; no shared helper lost a note context. It reads a variable declared polyphonic at line 8190 while clearing a cache. The branch follows a changed-value test and loops bounded by 18 and 10. Native documentation permits polyphonic variables only in note/release callbacks; no note owner should be invented for this UI callback. |
| P3, Solo Pads line 8821 | Directly in `note`, lines 8245–8854, querying an explicit generated-event ID and `$EVENT_PAR_PLAY_POS` while the host note is held. The originating context is present. The unsupported constant selector falls through the generic rejection; runtime-selected selector 12 also returns zero. |
| P5, Solo Pads line 2599 | An initialization waveform attachment uses saved source ID 748. The initial source environment is empty because translation initializes scripts before decoding zones, then reuses that initialized state for UI projection. All 806 source zones are later present. This is a proven ordering defect; it is separate from the unresolved relationship between persisted native IDs and serialized zone order. |
| P6, Solo Pads line 12900 | Directly in `ui_control`, lines 12400–13409. This is a silent zone-cache scan: create a note, attenuate it, select groups, wait, query its zone, then explicitly terminate it. Native negative-duration handling does not require fabricating an originating note for this callback. |

Source identities: Areia slot 0 SHA256
`81075813d580bfa3847a1a45162ae3cb29edf81cd1b8a3123b4d3c052d39a3e0`;
Solo Pads slot 0 SHA256
`965a6023eb7e1d751b0a3d91145e9fc57a61c92c93e2071da7e17e5011c1a875`.
Solo Soprano is a separate control instrument, with 28 groups and 8,932 zones;
its original report does not contain these five locations. This audit does not
establish its articulation extractor or runtime switching parity.

## Native negative-duration law and implemented correction

The existing Kontakt 8 read-only decompilation identifies the statement executor
at `0x140798900`, command case `0x50`. Its note-owner capture is conditional on
callback context 1. For a negative duration with no captured owner, it sets the
duration to zero; the zero-duration branch uses the ordinary play path. Context 2
also sets the release-family mode but does not capture that negative-duration
owner. This supports note-only gate linkage and ordinary sample completion for
negative durations in release, controller, polyphonic-aftertouch and UI contexts.
Native executable SHA256:
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
This is static branch evidence, not a native audio replay.

KSP lowering now applies that law to both constant and runtime-selected negative
durations, including values below -1. Key and velocity validation still precede
note publication. Thirteen event-ID fixtures pass, including a 20-case context /
constant-versus-dynamic matrix under allocation/deallocation guards, explicit
termination and invalid-MIDI-argument cases. A release fixture now expects ordinary
sample completion instead of incorrectly assuming a closed originating gate.

P3 now reports an unsupported event parameter rather than falsely claiming the
note callback is absent. Play-position readback remains unimplemented: native
documentation defines an absolute source cursor in microseconds, but does not
specify selection among simultaneously layered voices. A correct implementation
must resolve the plan-scoped event, inspect its live voice's pinned source revision
and physical cursor, and establish layered-voice, loop/reverse/fractional-position,
starvation and retired-event laws. Prepared/current target metadata cannot replace
an old sounding voice's retained cursor/rate. No arbitrary first/max voice rule is
introduced.

## Lookup and zone identity boundaries

Legacy `find_group` documentation gives zero for a missing name, while the current
`get_group_idx` contract uses `$NI_NOT_FOUND`. The implementation currently conflates
them. Initialization compares exact text, whereas callback lowering/runtime compare
ASCII case-insensitively. Before changing the generic lookup law, native evidence
must cover first/middle/last successes, missing names, duplicate names and case
variants. Areia's positive-only optional guard remains false for either missing
sentinel; this audit does not establish an audible group-selection regression.

The current zone authority uses one-based serialized source identities and retains
holes after admission. Saved zone ID 748 is positive and later admitted, but the
raw serialized zone at that identity has different group/key metadata from the
cache's selected group/key. That observation is not permission to remap cached
IDs by ordinal, name or nearest key. Native identity persistence needs independent
validation.

Initialization now reads the bounded serialized zone metadata before executing
scripts. It uses the same one-based physical IDs as normal admission, without
resolving samples or reading payloads. Final admission reconciles captured and
restored initializer state against the surviving source map: missing or changed
identities lose their attachment and properties with a diagnostic; no replacement
identity is selected. The final script binder also checks its source authority
against the prepared plan.

The installed Solo Pads regression retains requested ID 748, mapping it to IR
zone 747, asset 344, source group 24 and keys 69–70. All 806 physical source entries
survive, and the earlier empty-authority attachment warning is absent. This is
metadata/UI evidence; native persisted-ID equivalence remains unverified. Tests
also cover missing/unbound sources, sparse and reordered admission, changed
ownership, cached initializer restoration and reload. No vendor source or payload
is retained in the fixture or report.

A frozen existing inspector (`f537f96754463ec5c0f7b0f60a6533e4da992e14fd760666d67b8b15c44da909`)
confirmed successful Pads initialization/UI projection with the original P5/P6
warnings retained. Its exact build commit was not independently recorded; it is not
presented as the new candidate binary. The private numeric receipt is
`original-pads-initialization-audit.json`. Its report and child caches stayed in
memory; no notes or UI gestures were dispatched.

Primary references: [variables](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/variables),
[play_note](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands),
[event parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/built-in-variables-and-constants),
[zone commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/zone-commands),
[group commands](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/group-commands),
[legacy KSP reference](https://www.native-instruments.com/fileadmin/ni_media/downloads/manuals/kontakt/KONTAKT_602_KSP_Reference_Manual.pdf).
