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
Corda source proof is not yet green at this checkpoint.

## Areia controls before W5 init-service merge

Existing reference harness, Sustained NKI, keys 60/72/84, velocities
20/60/100/127, four repeats. MIDI SHA256
`7c72ec986cdc840ad148d6aba6f09125d9eb1f6e8425146aa0a89b48e4aa3f42`.
Reference calibration passed; GUI state was skipped for this instrument.

| Comparison | Top source agreement | Identified |
|---|---:|---:|
| Native vs same native WAV | 48/48 | 48/48 each |
| clean v1 `0cb7a8a0` vs native | 0/48 | 48/48 each |
| v2 baseline reader `6ba9fa72` plus existing key-filter helper `06d24f02` vs native | 0/48 | 48/48 each |

v1 uses the original compressed `v1.args` and detector `*0.5`, as required
by the harness because v1 caps output at 60 seconds. An earlier full-spacing
v1 run identified only 30/48 and is superseded. The table compares the first
NCC candidate; it is not proof that every secondary greedy match sounded.
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
