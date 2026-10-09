# W4 Mapping and Effects integration handoff

Mapping waveform **495a998ffd34ab534bdde837614be66d48b7b622** is pushed on
`v2/fix-settings-parity`. The later **4bd96ad9** adds group search and shared
articulation selection. Take these in that order. The waveform's recorded REDs
and 24-check GREEN are in `waveform-ready.md` under
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w4-settings-parity/`; they include actual
Analog and 2 Horns probes and native 1180×780 / 900×640 captures.

The focused continuation ran `kontakto-heavy cargo test --profile ci --lib
mapping_ -- --test-threads=1`: **17 passed**, including both real-library probes
and the worker-loaded Falcon fixture. Receipts for this continuation are in
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w4-effects-integration-20261009/`.

## Effects dependencies and ownership

- W15's prepared parameter registry is required before W14's panel commits.
- W14 **383346b7**, then **4d548aee**, provide the descriptor-driven filter panel
  and the shared LP4 response adapter. Their receipts explicitly cover a
  standalone component, not the live Sound tab.
- W11's `v2/w11-effect-route` provides immutable `EffectSnapshot` metadata,
  explicit processor roles, generation-checked lane admission/readback and host
  recall. Its source candidate was **f1989643** when this handoff started.
  Use its final validated READY SHA, not its older `SOURCE_ONLY.json` as evidence.
- W4 owns the Sound tab seam and Mapping state. W14 owns Effects contents;
  W11 owns engine publication and persistence. No second tab strip, articulation
  model, DSP implementation or script-widget route is needed.

## Adapter contract

`inside::SoundTab::Effects` already reaches `sound_effects(ui, cx, slot)`. Keep
`sound-tabs-{slot}` / `sound-tab-{slot}-{Controls,Mapping,Effects}` and reuse
W14's `effects::filter`.

Select the snapshot by `(part generation, EffectKey { scope, chain, processor })`.
The processor ordinal is across pre-amplitude followed by post-amplitude stages;
it is not a library's native effect-slot number. Keep selection and editor drafts
per part. A changed generation or selected processor clears the draft and drag
state, even when the new part reuses the same parameter addresses.

Resolve cutoff, resonance and gain through `snapshot.roles`; never infer them
from labels, descriptor order, widgets or native slot numbers. The panel's read
closure maps its admitted `ControlId` back to that snapshot's descriptor address
and calls `Shared::effect_value_at(slot, generation, address)`. Its write closure
calls `Shared::set_effect_value_at` with the same tuple. Queue admission is not an
engine acknowledgement; paint the engine's published value on the next frame.
Missing metadata, stale generations and rejected writes must not invent values.

LP4 uses `FilterKernel::LadderLP4 { gain }`, `LadderSettings::cutoff_hz` and
`LadderSettings::magnitude`. Retain normalized resonance and normalized typed
entry. Biquad fixtures alone do not establish native Daft Hz/Q conversion. Keep
unverified processors visibly read-only until their shared adapter is verified.

The standalone panel currently has unqualified `effect-filter-*` IDs. Render
one active part's editor at a time or qualify every interactive ID before showing
multiple editors. Fit the complete panel within the actual rack body above the
keyboard header; standalone 900×600 geometry is not a full-editor layout check.

## Required live integration witnesses

Run through `kontakto-heavy`, with targeted RED before GREEN:

1. Selecting the exact processor opens its admitted lanes; idle frames write
   nothing. A real UI gesture reaches DSP output and authoritative readback.
2. Two parts with identical addresses cannot share focus, drag, typed drafts or
   writes. Replacement during typing cannot commit to the new generation.
3. Controls → Mapping → Effects → Mapping preserves the selected source zone
   and waveform viewport. Effects selection does not audition a note or arm the
   Sound voice-probe owner.
4. At 1180×780 and 900×640, with authored part-header rows and either keyboard
   state, controls and status remain inside the part. Capture and review our UI.
5. Host save/restore retains admitted DSP lanes without a script/widget ID;
   the previous v2 script schema and zero-allocation render guards stay green.

W11's runtime/persistence witnesses and W14's standalone fixtures are necessary
dependencies; neither replaces the full-editor UI witnesses above. Finish with
root CI no-run before pushing. No new full-suite, native-host or timed-performance
claim is made by this handoff.

NEXT: live Effects adapter after W11 READY; MIDI-learn cancellation and exclusive
ownership regressions remain the next independent W4 slice.
