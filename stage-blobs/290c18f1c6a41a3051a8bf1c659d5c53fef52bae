# Fade round4 — minimal correction READY SOURCE, Rust UNRUN

Exclusive checkout `/home/derpcat/.t3/worktrees/KONTAKTO/native-fade-fix-round4`,
branch `pi/native-fade-fix-round4`, created clean from immutable frozen
`9075da681fbca5019640619822a4540bfd2a57a1`, tree
`ee617e185aecada9d74e22b5e1c2ad499af23f28`. Version remains 0.3.403.

Fix commit **`63a05d3745ef0ab4b5f8a1ee51feeebbe4590d8f`** changes five files.
Only production change is `render::ramp_mix`; all other changes are tests/fixtures.
Required supplementary witness commit **`ef9a21440b0dbbd501cc7678d62a7ac973c05a15`**
adds an explicit finite/nonzero/changed first64-frame assertion to the parallel
regression; it changes no production code. Source snapshot for the final static
receipt is ef9a2144, tree `b96549ccf5c46d1928fcbd0c1ee07a566d86bf51`.
No changes to lowerer, enum, event clocks, admission/error policy, modulation
state arrays, persistent Fade, Lua, Mapping, performance source, NKA or transport.
Do not cherry-pick historical native/DSP branches or edit the frozen source.

## Actual prior RED receipts

Read the frozen manifest and both logs before editing. Receipts live at
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/pi-integration-round3/`:

- `FROZEN-SOURCE.json` pins source9075/treeee617e and prior assembly.
- `combined-no-run.log` SHA256
  `e707e83483f504210043a39152d96c7a7beb6d383821814da8a3adbf5f85136c`:
  E0308 at external `fade_curves.rs:19`, Vec supplied to `Pcm::new` requiring Box.
- `ksp-params.log` SHA256
  `4ee5aae419d84f659653fcc7159f0c6a085ab6d97aa5e5a602c8de8cdfe84a68`:
  15 PASS / 3 FAIL. Equal-power fade-in quarter-time and inside-cell exponential
  fade-in both actually printed `[0.5, 0.5]`. Invalid-selector fault assertion
  passed; the subsequent expected-unfaded-0.5 assertion failed. That log does NOT
  print the actual invalid-selector PCM, so no measured value is inferred.

These receipts are the RED feedback signal. Per explicit owner restriction, this
worker did not rerun Rust or claim a Rust RED→GREEN loop. The aggregate remains
RED until the next centralized combined build/test receipts replace it.

## Root causes and complete call trace

All implementing paths below are pinned to fix commit63a05d37 unless identified
as the frozen historical version. Exact native runtime parity is UNKNOWN.

1. **Missing plain gain consumer explains both nonlinear KSP failures.**
   `sampler-ksp/src/builtins.rs:129-130,639-643` accepts the documented optional
   selector and maps names to our internal enum. `lower.rs:2276-2310` evaluates
   event at dst, duration at dst+1, curve at dst+2, and dynamic stop at dst+3.
   Argument evaluation uses scratch registers above its destination, so this
   order retains the earlier operands. `behavior.rs:824-831` includes curve in
   local capacity; `2468-2487` checks the enum and dispatches. `trigger_in` in
   `prepare/selection.rs:60-142` enters `stages.rs:56-96` synchronously;
   `behavior.rs:998-1006` resumes the callback. `script_params.rs:897-925` resolves
   the exported source event and stores the fade beginning at current `now`.
   `render.rs:566-608` deliberately removes an ACTIVE nonlinear fade from endpoint
   gains and puts it in ephemeral `Ramp.script_fade`. The chain amplifier
   (`dsp.rs:613-614`) and modulated mixer (`voice_mod.rs:1375-1383`) already read
   that factor. Frozen `render.rs:882-900::ramp_mix` did not. These fixtures have
   neither a chain nor voice modulation, so serial rendering reached this missing
   consumer at `render.rs:845`; parallel rendering reaches it at `parallel.rs:329`.
   Endpoint gains are unity, explaining observed unfaded0.5 without requiring an
   operand, selector, source-event or clock change.

   Fix `render.rs:892-906` adds the same per-sample factor at `now+i+1` to the
   plain consumer. The old loop is preserved EXACTLY in `else`; no-fade and
   Linear multiplication/order are not changed. Chain post-mix clears gains and
   script_fade through `Ramp::without_gains` (`voice_mod.rs:1429-1434`), avoiding
   a second multiplication. No new public API, persistent state or allocation.

2. **Invalid-selector test violated existing callback-fault policy.**
   `behavior.rs:2479-2487` rejects the selector BEFORE calling `fade_event`.
   `fail_behavior` (`2901-2913`) records InvalidInput and closes a NOTE owner
   with BehaviorFault; a PLAN owner does not close unrelated notes.
   `lib.rs:1804-1811` releases at current `now`. This is existing policy, not a
   new native equivalence claim. The old test expected its initiating note to
   remain audible after that closure; its expectation was wrong. We do not
   weaken selector validation or change error/invalid-event policies.

   `params.rs:284-321` now targets a previously audible note from a controller
   (plan-owned callback), asserting actual callback Fault(InvalidInput), the
   one-shot problem report, unchanged note gate/voice count, exact unfaded PCM
   across256 frames, and unchanged callback clock followed by render+256. It
   checks -1,5,99 rather than allowing a silent Linear substitution. Separate
   `params.rs:325-352` retains the NOTE-owner case, explicitly asserting closure
   of the initiating note at the callback clock while note61 remains held and
   audible0.25. No rollback of prior musical effects is assumed.

3. **Both new core fixtures require boxed PCM.**
   `prepare.rs:74` takes `Box<[Frame]>`. The external fixture at
   `tests/fade_curves.rs:19` and lib-only `script_params.rs:1021::curved_audio`
   both supplied Vec. Both now call `into_boxed_slice` at setup, outside the
   existing `support::without_heap` guard. The second mismatch was discovered
   statically; it was not a second compiler receipt. No unsafe allocator or
   dependency was added.

4. **Existing parallel test used the wrong event identity.**
   The fixture previously supplied host `external_id` as if it were a script
   event ID. `note_event.rs:109-135` explicitly distinguishes them and requires
   source export; unknown aliases are no-ops. The test helper now exports
   `source_event_id(note)`, asserts `resolve_source_event` resolves that SAME
   note, and uses the alias (`parallel.rs:697-710`). This repairs the fixture,
   not product admission. Its Linear scalar/parallel test remains. New
   `fade_curve_parallel_plain_mix_matches_serial_and_differs_from_linear`
   (`721-747` at supplementary ef9a2144) requires finite output and a NONZERO
   changed sample frame within the active first64-frame window, then each
   nonlinear serial PCM result to differ from Linear, then exact
   serial/2-thread/4-thread PCM and final voice-count equality, with
   observed parallel-block-count assertion. It cannot pass by comparing two
   unfaded buffers. These are PREPARED, not executed.

## Regression coverage and no-build evidence

- Existing KSP all-five quarter-time array selectors, dynamic stop and both
  directions remain at the same exact documented points; tolerance unchanged.
- Inside-cell KSP test now witnesses a live voice and a separate unfaded0.5
  control, equal render clocks, and unequal frame15 PCM before checking the
  original tight exponential value. Old optional/default-Linear clock control
  still covers origins0/128 and blocks1/17/128.
- External heap regression strengthens its existing active-fade witness with
  ten independent frame16/384 constants, all shapes/directions, chain/plain,
  blocks1/17/128, endpoint PCM and stop/retained voice counts. Setup is outside
  the allocator guard; dispatch/render/completion remain inside it.
- Existing direct core lib tests retain short/zero durations, time-mirror,
  interrupted fades, all-five/plain/chain/block PCM and legacy bits/Fade40 size.
- Repository has no `graft/` or tracked AGENTS.md. Applicable main-repository
  `/mnt/Windows11/DEV_PROJECTS/Repos/KONTAKTO/AGENTS.md` was read; exhaustive
  symbol/caller searches preceded multi-file changes. Both ramp_mix callers,
  all parallel test-helper callers, both PCM fixtures and existing fade exports
  were inspected. No new export/config/caller change is required.

Reproducible STATIC + independent mathematical check:

```sh
python3 docs/audit-2026-10-10/native-fade-fix-round4-check.py
```

`native-fade-fix-round4-checks.json` records PASS_STATIC_AND_INDEPENDENT_MODEL_ONLY:
exact legacy loop preservation, both consumer callers, checked dispatch order,
unchanged surrounding production seams, boxed fixtures, exported parallel event
witness, fault controls, ten independent constants, and20 mathematical cases
(all shapes/directions at origins0/128 across blocks1/17/128). The documented
exponential frame16/384 result0.0008680555555555555 differs from old unfaded0.5
and 64-frame endpoint interpolation0.003472222222222222. The model does not run
our Rust VM/DSP, does not validate Rust types, and is not native PCM.
`git diff --check` passed. No Cargo/rustc/clippy/rustfmt/build was run.

## Corresponding native specification check (equivalent, not runtime PASS)

Reused immutable authoritative NI specification rather than restarting the
previously timed-out REA search or executing a forbidden host:

<https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands#fade_in-->
and `#fade_out--`, archived coordinator
`handoff/official-docs/ksp-events.html`, SHA256 reverified:
`57f8f69b4deec41cc0eb98fd1cc5196f3c02b72e3d222faff7b91fe8551626ce`.
Text sections64-95 specify Kontakt8.12 signatures, five named equations,
time-mirrored fade-out, omitted Linear and stop-voice semantics. This exact
contract suffices for an evidence-based specification check of this consumer
correction, NOT native coefficients/clock or invalid-selector reaction.

Prior `native-script-round2.md` retains read-only REA receipt identity:
Kontakt8 executable SHA256
`fe12b6a7b652cfd026b5bec62ae6c096b2dfddcc0e17249ba518cb3ef0f0c8e0`,
static NI_FADE name offsets0x4e84898..0x4e848f0, timed-out search and closed binary.
Names prove names only. Selector0..4 remains INTERNAL, not native ABI ordinals.
No fresh native measurement is available: native clock, coefficient accuracy,
unknown-selector fallback/diagnostics, runtime parity and CPU/RAM are UNKNOWN.
No decompiled pseudocode, protected assets, sample/key dumps or reader processes.

## Integration NEXT — one future combined megabatch only

Compose fix63a05d37 plus required test-only ef9a2144 with independently routed
NKA/transport source as coordinator
chooses. Do not request another immediate Rust cycle or reopen old child threads.
Integration is the sole validation owner. Within its next serialized combined
release/offline/locked/-j1 cycle, require nonzero SHA-bound executable test lists
and actual runs of:

- core lib `kontakt_812_`;
- core lib `legacy_linear_fade_keeps_bit_order_and_state_size`;
- core lib `fade_curve_` (includes new parallel nonlinear witness);
- core lib `script_layered_voices_render_the_single_threaded_output_exactly`;
- core external `--test fade_curves` (existing allocator harness);
- KSP `--test compile --test params fade_curve_`, then complete `params`.

No CARGO_TARGET_DIR/RUSTC_WRAPPER overrides, simultaneous Rust jobs, host/reader,
Wine, generic gate.py probes/all14/UVI, installation/publication, servers,
nested subagents, schedules, or edits to another lane/frozen/v1 reference occurred.

**Status: READY SOURCE; compiler/tests UNRUN; native parity UNKNOWN.** This change
adds O(active nonlinear fade frames) shape evaluation and multiplication to the
previously broken plain path. No persistent RAM growth; no measured performance
claim. CPU AND RAM significantly lower than BOTH frozenv1 and Kontakt remains
**UNACHIEVED** until comparable measurements prove it. No invented threshold.
