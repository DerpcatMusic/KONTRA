# Fade/Lua next-batch assembly — source ready, executable validation pending

Private checkout: `/home/derpcat/.t3/worktrees/KONTAKTO/native-pair-round3`.
Branch: `pi/native-pair-round3`. Frozen base remains
`429e7dff3ecb0f53aef32c43201ab796ccc8c28c`, tree
`94ad5caaa5e5ec70e1595d34166c25110d294c67`.
No delivered branch, integration freeze, dirty checkout, or reference was edited.

## Exact dependency order

Cherry-pick ONLY this ordered slice onto frozen429e or reconcile each patch with
its combined successor. Do not merge a historical worker branch wholesale.

1. `1544be8afb0262245f3bb9e24f8a9d6909f39378` — native typed fade seam;
   cherry-pick of `9fe764e4dcb3e876f82a1d19e2656cf8fe02ffe6`.
2. `7327daa51a381778f63f8149d87915913249d565` — DSP-owned runtime;
   cherry-pick of `12475a0d731b89af515ee1f43c5b23175ed67813`.
   Steps1+2 MUST be paired before a build. Step1 alone is intentionally incomplete.
3. `b897df90963127071eeecb3ce52cd522111db5fa` — within-cell KSP witness;
   cherry-pick of `9da42c031f86e610734db7386e32ad79c4552ed8`.
4. `1264126e1c58ab8ed6e733f147804e706cdb0ed6` — independent Lua inline
   probes/comment/evidence; cherry-pick of `27dfb4272544a72aee9442daf057a38c961e14d3`.
5. `70005b680de9fffa2e218a9d6edc4e448b1925e8` — corrected test phase,
   nonzero-origin/default-Linear control, actual DSP voice capacity, reusable
   external allocator regression. Required before executing the prepared tests.

The first two patches applied without conflicts; their source-level exports,
operand registration, signatures, lowering, runtime caller and render consumer
are present together. This is compile-ready SOURCE, not a successful compiler
receipt. The Lua slice is independent; it changes no VM, dependencies or runtime.
Alias/menu changes already in429e were not duplicated.

## Phase proof and test-only fixes

All spans below are pinned to `70005b680de9fffa2e218a9d6edc4e448b1925e8`:

- `crates/sampler-core/src/prepare/selection.rs:60-140`: trigger applies due
  commands at the current clock, then enters the note stages.
- `stages.rs:56-96`: note-stage entry calls `start_note_context`.
- `behavior.rs:998-1006`: context admission synchronously resumes the callback.
- `script_params.rs:897-927`: fade start uses `self.now`.
- `voice_mod.rs:1377-1383,1410-1429` and `dsp.rs:613,632`: physical amplifier
  samples at `at + sample_index + 1`. `without_gains` removes the ephemeral
  nonlinear factor after the chain amplifier. No engine clock change.
- `voice_mod.rs:1395` / `dsp.rs:870`: gain cell is64 frames, not32.
- `sampler-ksp/tests/params.rs:158-270`: quarter-time index1199 means
  elapsed1200/4800; within-cell index15 means elapsed16/384. The new
  `fade_curve_script_clock_and_omitted_linear_match_sample_end_control` checks
  origins0/128, both directions, blocks1/17/128, first/inside/end/post-end
  samples and exact default-versus-explicit-Linear PCM. Tight quarter-time
  tolerance2e-7 replaces the previous broad1e-3 helper for this witness.
- `script_params.rs:1020-1069`: direct runtime helper now reserves ONE voice,
  not zero, and asserts the voice exists. All-five/all-block/chained/plain
  regressions remain. Without this correction the helper cannot test audio.

Independent arithmetic distinguishes frame16 exponential0.0008680555555555555
from frame17 value0.000979953342013889 and straight64-frame-cell interpolation
0.003472222222222222. These calculations validate the fixture's discriminant,
not the Rust runtime or native PCM.

## Reference evidence: precise contract, not native runtime PASS

Reused immutable authoritative documentation as the corresponding evidence-based
native specification check; no large REA session or native host was restarted.

- NI KSP event commands:
  <https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/event-commands#fade_in-->
  and `#fade_out--`. Archived `handoff/official-docs/ksp-events.html` SHA256
  `57f8f69b4deec41cc0eb98fd1cc5196f3c02b72e3d222faff7b91fe8551626ce`;
  section text lines64-95. Specifies8.12 optional signatures, five named shapes,
  equations and time-mirror. Hash rechecked this round.
- UVI LuaVM: <https://lua.uvi.net/_lua_reference.html#LuaVM>.
  Archived `uvi-lua.html` SHA256
  `339c49ba91c9acb7eef7c49014801fc426fc78f6bc2a060dd45ae5de79b54bec`;
  hash rechecked. Documents Lua5.1.4 rather than Luau. Three authored inline
  construct probes are characterization, not complete VM parity.
- Prior native read-only REA receipt, exact Kontakt executable identity and
  timed-out name search are preserved in `native-script-round2.md`. Static
  names establish names only. No native clock/shape coefficient measurement.

`behavior.rs:39-63` explicitly marks selectors0..4 INTERNAL, not vendor ABI
ordinals. `builtins.rs:639-643` maps names to those internal values. Unknown
selector rejection is OUR admission policy; native fallback remains UNKNOWN.
ALL_EVENTS/by_marks admission remains unchanged. No AR/importer/address/routing
change was made. Generated equation fixtures are not native reference proof.

## Heap regression and performance limit

`crates/sampler-core/tests/fade_curves.rs:6-117` reuses existing external
`tests/support/mod.rs::without_heap`; no allocator implementation was added and
no unsafe code was included in core library or the new test. It covers dispatch,
render, completion, all five shapes, both directions, chained/plain and
blocks1/17/128. PCM and voice assertions prevent a silent/no-voice pass.

This check is PREPARED, UNRUN. It measures allocation/free calls if integration
executes it; it does not establish retained RAM or callback CPU superiority.
Active nonlinear fades add per-sample sine/cosine/quadratic work; inactive/legacy
paths and persistent Fade40-byte test are retained. CPU AND RAM lower than BOTH
frozenv1 and Kontakt remains UNACHIEVED until comparable measurements prove it.

## Integration-owned next combined cycle

No cargo/rustc/clippy/build, native process, install, publication, server or
numerical/native runtime probe ran here. No environment overrides were set.
Integration alone may build/run in its globally serialized next combined cycle.
Do not extend the current429e tested freeze with these untested changes.

Required targeted filters (after the complete pair/test fix is present):

```sh
cargo test -p sampler-core --lib kontakt_812_
cargo test -p sampler-core --lib legacy_linear_fade_keeps_bit_order_and_state_size
cargo test -p sampler-core --lib fade_curve_
cargo test -p sampler-core --test fade_curves
cargo test -p sampler-ksp --test compile --test params fade_curve_
cargo test -p sampler-ksp --test params
cargo test -p sampler-uvi --test lua_compatibility
```

Use combined compiler artifact/test-list/execution receipts, pin the combined SHA
and manifest, report exact failures without changing the amplifier clock. Local
no-build checks passed: cherry-pick reconciliation, rustfmt syntax, whitespace,
source safety/phase/capacity/coverage invariants, doc hashes and arithmetic
fixture discriminant. Rust and native runtime statuses remain UNRUN/UNKNOWN.

NEXT verified script-service gap is explicit-path NKA array load/save; separate
source inspection is recorded in `nka-explicit-path-next.md`. This pair makes no
NKA implementation claim and includes no speculative host/VM compatibility work.
