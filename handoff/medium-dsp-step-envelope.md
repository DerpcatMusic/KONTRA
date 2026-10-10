# Medium DSP slice: preserve Step during envelope duration edits

Private checkout: `/home/derpcat/.t3/worktrees/KONTAKTO/parity-medium-dsp-20261011`
Branch: `parity/medium-dsp-20261011`
Base: `7971dcc468bf44ef3b5d800128732176ef88ace5`
Source/test commit: `4955b8f85bd395067242d3b1e873decdd8a341a3`
Source/test tree: `8e06b04671c5bbb70560e54621e95bcf2637f679`

## Proven source contract and correction

`EnvelopeCurve::step` (`crates/sampler-core/src/envelope.rs:23`) promises holding a stage's starting level until its end. `Envelope::with_stage` (`envelope.rs:187`) promises keeping curvature when replacing a stage's duration. The previous recurve closure rebuilt from `Curve.curvature` alone, but Step is represented by `Curve.step=true` and curvature=0. Consequently, any Attack/Decay/Release duration edit converted Step to linear. This is a source-proven consumer regression; no execution or native-host result is claimed.

The closure now reconstructs Step from its flag. `Curve::new` (`envelope.rs:54`) retains the Step flag even for zero duration, so a 2→0→4 frame edit retains its authored shape. Zero-length phases already skip in `EnvelopeState::enter`; the coefficient, recurrence, interior curve evaluation, frame clock and state layout are unchanged. No importer admission or engine-parameter law changed.

## Consumers traced before edits

- `crates/sampler-core/src/lower.rs:1017` and `:1488`: authored `ir::Curve::Step` lowers into `EnvelopeCurve::step`; no lowerer edits.
- `crates/sampler-core/src/script_params.rs:307`: `GroupParams::envelope` applies script stage overrides via shared `with_stage`.
- `crates/sampler-core/src/control.rs:323`: fallback Attack/Release durations also use shared `with_stage`.
- `crates/sampler-core/src/control.rs:337`: `controlled_envelope` applies script overrides and changed native control lanes through `with_control`→`with_stage`.
- `crates/sampler-core/src/prepare/selection.rs:962`: captures the controlled envelope before admitting the voice.
- `crates/sampler-core/src/lib.rs:1756`: constructs the voice's `EnvelopeState`.
- `crates/sampler-core/src/source.rs:638`, `:893` and other source paths, plus `crates/sampler-core/src/dsp.rs:862`: render the state through `next`.

Only `crates/sampler-core/src/envelope.rs` and `crates/sampler-core/tests/envelope.rs` changed in the source/test commit. There was no graft directory in this checkout; normal source search was used. Prior root handoff DSP recovery and relevant Step mentions were checked; none described this correction. Parent confirmed exclusive ownership of these spans.

## Authored synthetic checks

`envelope_duration_controls_preserve_authored_step_curves_in_render` (`crates/sampler-core/tests/envelope.rs:364`) creates a native Prepared region with constant stereo PCM and authored Step Attack/Decay/Release. It installs the existing group amplitude schema, edits each duration from2 to4 through native ControlWrite values, starts a voice, schedules KeyUp at8, and renders across block sizes1/3/14. Expected output is exactly `[0,0,0,0,1,1,1,1,.25,.25,.25,.25,0,0]` in both channels with complete voice/family retirement. Existing `without_heap` wraps edits, trigger, event and render. On the old source, the staircase assertion would reject the linearized attack before frame4; this failure is inferred from source, not executed.

`step_curves_survive_zero_duration_stage_edits` (`crates/sampler-core/src/envelope.rs:520`) checks all three Step flags survive2→0→4 duration edits.

Future serialized commands for root, NOT_RUN here:

```sh
cargo test -p sampler-core --test envelope envelope_duration_controls_preserve_authored_step_curves_in_render -- --exact
cargo test -p sampler-core --lib envelope::tests::step_curves_survive_zero_duration_stage_edits -- --exact
cargo test -p sampler-core --test envelope
cargo test -p sampler-core --lib envelope::tests
```

The first two are focused new assertions. The latter two cover existing adjacent envelope clocks, scalar/skip state equivalence, release capture, one-shot retirement and zero stages, and include the focused assertions again; that duplication is explicit.

## Validation and limits

PASS: `rustfmt --edition 2024 --check crates/sampler-core/src/envelope.rs crates/sampler-core/tests/envelope.rs`; `git diff --check`.

Rust: **NOT_COMPILED / NOT_RUN**. No cargo, rustc, clippy, test binaries, audio probes, native tools, dev servers, installers or protected user fixtures were used. No shared ledger/manifests/evidence or parent checkout was changed. Commits used `core.hooksPath=/dev/null`.

This restores an existing core envelope contract and reaches actual Prepared→control→voice→render in authored regression source. Kontakt/v1 measured native parity, real library coverage and CPU/RAM/performance are **UNKNOWN**. Importer/external curve laws, retrigger/bypass features, and exponential retiming through zero are outside this Step-only slice.
