# Audio-thread CPU audit (gpt/cpu)

## Reproduction

The audit extends `crates/sampler-perf` and `tools/cpu-audit-run.py`. It streams real sample
assets through production Kontakt translation, script initialization, lowering, MIDI ingress,
stream servicing, render, effect draining and note/behavior retirement. No decoded sample,
script or key material is persisted. PCM evidence consists only of aggregate energy and a hash.

The latest user rule prohibits worker builds and measurement runs. The coordinator supplies
one integrated release `sampler-perf` executable with debug symbols, no stripping and
`-C force-frame-pointers=yes`. The original pre-fix executable was already built before the
rule and is preserved in this worker's ignored `artifacts/audio-cpu/before-bin` (117,288,632 bytes).
Its SHA256 is `b505f24033c13537d4fc97541da5cf2eda7e4d3f9a8e6c00a238ffcf9d467594`;
its engine base is `ce0a2e61`, harness `e0e40a71`. It survives cleanup of the worker target/cache.

Exact coordinator-only commands, from the integrated checkout, once the after binary exists:

```sh
CPU_AUDIT_BEFORE_BIN=/home/derpcat/.t3/worktrees/KONTAKTO/gpt-cpu/artifacts/audio-cpu/before-bin
CPU_AUDIT_AFTER_BIN=/mnt/Windows11/DEV_WORKSPACE/Toolchains/User/cargo-target/kontakto-t3-41d67f01/release/sampler-perf
CPU_AUDIT_OUT=/mnt/Windows11/DEV_WORKSPACE/kontra-runs/gpt-cpu-comparison
PERF_METADATA=1 python3 tools/cpu-audit-run.py v2 "$CPU_AUDIT_BEFORE_BIN" "$CPU_AUDIT_OUT/before" --sustain
PERF_METADATA=1 python3 tools/cpu-audit-run.py v2 "$CPU_AUDIT_AFTER_BIN" "$CPU_AUDIT_OUT/after" --sustain
python3 tools/cpu-audit-run.py v2 "$CPU_AUDIT_BEFORE_BIN" "$CPU_AUDIT_OUT/before-profile" --sustain --profile --only afflatus --block 256 --notes 1 --seconds 20 --frequency 9999
python3 tools/cpu-audit-run.py v2 "$CPU_AUDIT_AFTER_BIN" "$CPU_AUDIT_OUT/after-profile" --sustain --profile --only afflatus --block 256 --notes 1 --seconds 20 --frequency 9999
```

Use a fresh output directory and a quiet machine for acceptance. The driver itself wraps each
cell in `kontakto-heavy`; do not hold an outer wrapper slot. The first two commands each
produce all 12 cells (three patches, two blocks, two chord sizes), with idle/held/release/
settled release p50/p99 in JSON. Profile commands produce separate diagnostics and flat/
callchain reports, including `.audio.perf.txt`. If the coordinator uses a different worktree,
change only the after executable's target basename. Keep the CPU worktree until baseline
measurement is complete; the ignored frozen executable is not carried by a cherry-pick.

The frozen before harness predates external-service and bootstrap error reporting. Its
phase timing, note schedule and PCM fingerprints are identical in design, but a new
file/MIDI-service failure invalidates comparison for that cell until the host service works.
No production DSP skip may be justified by a failed or silent cell. `sampler-perf compare`
currently accepts the original `run` schema, not `sustain`; inspect the nested phase metrics.
Require identical per-cell PCM fingerprints, voice counts and zero error/underrun counters
before accepting a speedup. Block 256 / Afflatus / one key must be below 1% core (53.33 us),
and settled idle must have zero voices/PCM with near-zero cost. No native parity is inferred
from matching KONTRA hashes. There is no after profile yet.

Add `--profile --only afflatus --block 256 --notes 1 --seconds 20` for the held-note profile.
`perf record -g --call-graph fp --user-callchains` records user code addresses only. The ready/finished markers
bound profiling to held-note rendering, excluding load, bootstrap and release. The sampled event is `cycles:u`; excluding kernel callchains permits recording with
`perf_event_paranoid=2`. Profile overhead is excluded from acceptance timing. Each cell releases the heavy slot.

Conditions: 48 kHz, one render thread, block 64/256, MIDI C4 and C4/D4/E4/F4/G4/A4/B4/C5,
velocity 100, CC1 110 and CC11 127. One second of bootstrap and one second of attack settling,
three seconds per measured phase, eight additional seconds of release settling. Release and
post-release are distinguished because legitimate sample/effect tails are audible work.
Wall and Linux thread-CPU p50/p99/mean are expressed as percentage of a single core's
real-time deadline: ns / (block / 48000 * 1e9) * 100. The thread-CPU clock pair adds a small
measurement overhead; it is retained identically before/after.

Patches: Afflatus `Instruments/1. Ensembles/Multi Instruments/4 Horns KS.nki`, Conflux
`Instruments/Conflux.nki`, Conflux `Multis/Hybrid Walk.nkm`. Multi measurement loads every
physical program, retaining independent scripts and streamers, and auditions each on MIDI
channel 1. It does not emulate rack channel filtering, rack output faders, output summation,
or global multi scripts. This is a per-program rack-workload audit, not whole-multi sound parity.

The original sampler-perf never drained the KSP outbox. A full outbox suspends `Emit`, so it
cannot establish steady Conflux behavior. The shared render helper now drains emitted effects
between blocks. UI effects need no presentation in a headless render; external async services
still require explicit reporting rather than a claim of full host parity.

## Reference and topology evidence

NI Kontakt manual, “Using filters and effects in Classic view”, “Group insert effects”,
“Bus insert effects”, “Instrument insert effects”:
https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/using-filters-and-effects-in-classic-view
Accessed 2026-10-13. Group inserts are polyphonic per voice, optionally before/after the
amplifier; bus/instrument inserts process summed signals. Moving a group insert to a shared
bus changes sound and is not an admissible CPU optimization without separate proof.

Repo references: `docs/re/DSP_SYSTEM_INVENTORY.md` (original-machine-code checks, Kontakt
signal flow), `docs/architecture-v2/KONTAKT_DSP_LAWS.md` (control cadence and scoped evidence).
The handoff's old worktree/cache `t3code-80fe786b` is absent. Its copied spec documents are
present in `docs/re`; no unseen pseudocode cache is claimed as inspected.

Read-only radare2 on `/mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe` confirms
function `0x1408fa270`: instruction `0x1408fa2e0` tests the persistent countdown at +0x1bc;
`0x1408fa2ed` reloads 0x20 (32), then enabled streams dispatch bounded values to parameters.
This confirms that particular Daft control path, not all Kontakt modulation or FX sleep laws.
Current KONTRA uses a 64-frame voice/control chunk with effect-specific cadence inside it.

v1 source inspected: `0cb7a8a0:src/fx/processor.rs` (`Sleep`, `process`, `mix_buses`),
`src/engine/mod.rs` (`render`), `src/engine/voice.rs` (SIMD source mixing), `src/engine/rack.rs`
(rack routing). v1 sleeping bus early exit is relevant to avoiding inactive work; its -120 dB
peak/tail heuristic must not be substituted for current exact declared tails without audio
proof. The frozen binaries at `~/.cache/kontra-v1` are absent; a numerical v1 comparison is
unavailable and v1 is not rebuilt.

## Measurements and fixes

Baseline Afflatus (10 s profile diagnostic, block 256, one held MIDI key) has
16 active source voices, wall p50 270.225 us / 5.0667% of a core, p99 584.190 us /
10.9536%; thread-CPU p50 265.2 us / 4.9725%, p99 534.6 us / 10.0238%. Idle p50
4.710 us / 0.0883%; settled post-release p50 6.210 us / 0.1164%, zero PCM/voices.
No audio heap calls, event/callback errors or underruns. These are CONTENDED,
not quiet-window acceptance measurements. CPU: Ryzen 7 7800X3D (8 cores / 16 threads).

User-only cycles perf capture succeeded: 7,728 process samples, zero lost samples;
audio-thread filtering leaves about 6k samples. Largest flat frames: NativeEq::process
8.37%, dispatched source kernel 6.58%, PreparedVoiceChain::render 6.13%, Stereo::process
6.00%, source window assembly 4.89%, render_filtered 4.81%, VoiceModState::evaluate
4.63%, Ramp::gains_at 3.86%, Cursor::address 3.18%, Shape::evaluate 3.13%.
`native_eq.rs` and `stereo.rs` are in-use files and are not edited by this package.
The first perf attempt failed because default callchains requested kernel data under
perf_event_paranoid=2; explicit --user-callchains succeeded without changing settings.
The runner now accepts the intentional SIGINT termination after flushing perf data.
The old runner reported failure for that intentional signal even though the capture succeeded;
the stored profile remains valid. The separate 256-frame no-profiler diagnostic above was
not a successful profile. The real profiled timing was 5.6287% p50 / 11.3790% p99 and is
excluded from acceptance.

The first fix ports v1's sleeping-bus early exit, adapted to v2 exact declared tails.
A per-buffer dirty-state bit resets processor cells once at tail expiry instead of
re-clearing zeros on every dormant chunk. No timing law or PCM computation changes.
`bus::cpu_tests::sleeping_bus_clears_once_and_wakes_with_identical_pcm` failed first
(101 clears versus 1), then passed. It checks independent biquad recurrence and
bit-identical sleep/wake PCM. Both final crate checks (`sampler-core`, `sampler-perf`) passed.

Coordinator topology handoff read from commit `2a231791`: KONTAKT_TOPOLOGY.md and
KONTAKT_TOPOLOGY_EVIDENCE.md. T07/T08/T21/T20/T05 are assigned to this package when
confirmed by profiling. The recovered Ghidra excerpts are now located in
~/.cache/kontra-recovery/20261012/ignored-evidence/t3code-80fe786b-66815b1c/artifacts/engine-analysis-2026-10-07;
the complete pseudocode export is absent. Clock ownership and a universal 32-frame
modulation rate remain open; shrinking CELL by assumption is not an optimization.


### Source and native-control fixes

- Port `0cb7a8a0:src/engine/voice.rs::taps` into the existing span-verified
  realtime source run. At step 1 / fractional position 0, samples away from a
  crossfade use the center tap. Neighbour-dependent signed-zero results still
  use the original polynomial. High quality, fractional pitch, crossfade seams,
  multi-loop fallback, starvation and source mapping are unchanged.
  `source::tests::integer_crossfade_runs_skip_cubic_and_preserve_pcm_cursor_and_envelope`
  failed first because the direct-run counter stayed zero, then passed exact PCM,
  cursor and envelope checks for block lengths 1/3/4/63/64/65, curved envelopes,
  signed zero and tiny samples.
- Restore the v1 `voice.rs::plan` settled-input policy in the pinned control owner.
  Live IR depth/bypass bindings formerly invalidated native controls on every cell;
  residual sources could also invalidate unrelated native controls. The owner now
  observes each voiced assignment's actual raw input/depth and invalidates only
  changes. Shaping/lag arithmetic and independent envelope/LFO clocks are unchanged.
  `v1_voice_controls::adapter_tests::live_native_intensity_writes_invalidate_only_changed_cached_controls`
  failed first at a same-stamp depth write (0.25 versus expected 0.625), then passed
  depth, bypass, negative intensity and forced-recomputation equivalence checks.

## Coordinator-only measurement gate

The user rule added 2026-10-13 prohibits this worker from builds, workspace checks,
`--no-run`, corpus/perf runs and further CPU measurements. The coordinator stopped
`kontakto-gpt-cpu-before-1`; only the Afflatus 64/one-key cell and two Afflatus
256/one-key diagnostics completed. No after numbers or sub-1% acceptance is claimed.
Crate-scoped focused tests use `kontakto-heavy cargo test -j 4`; the old workspace
compile requirement is superseded. Commands in Reproduction are for the coordinator
alone after integration, not permission for another worker to build or measure.


### Idle traversal and amplifier preparation

- T21 dormant traversal: 32 control tiles through a 100-bus fixture → zero tiles.
  `render.rs::render_segment` checks physical voice activity and bus render state once
  per event segment; event/callback dispatch and clock advancement stay in `render_inner`.
  The pending one-time history clear is render activity, not plan ownership. The focused
  `render::cpu_tests::dormant_sum_graph_skips_cells_but_keeps_events_clock_and_wake_pcm`
  failed first and passed event deadline, idle PCM, absolute clock and exact wake checks.
  `--test buses bus_tail_retains_only_its_generation_after_host_note_retirement` also passed.
  This is the dormant fast path; active-node lists and active-plan traversal remain open.
- Amplifier slopes: two differences/divisions per sample → one preparation per existing
  voice-chain cell, as in v1 `voice.rs` delta preparation. The F32 formula, absolute offset,
  F64 multiplication association and per-sample nonlinear Fade clock remain unchanged.
  `script_params::tests::amplifier_prepares_gain_slopes_once_with_exact_pcm_and_fade_clock`
  failed first (12 sample gain preparation calls → zero), then passed exact gain/PCM
  comparisons, all five fade curves, boundary clocks and the production chain path.
- Native cache coverage now also compares 400 cells per lag mode against the forced pinned
  recomputation path, with nonlinear shaping, changing raw CC/depth and cell lengths 1/7/64.
  The existing focused native-control test passes exactly for gain, pitch, settling and lag
  state. No envelope/LFO progression is suspended.

### Before matrix and remaining work

All recorded numbers are wall p50 / p99, percentage of a single core at 48 kHz. Thread-CPU
and phase voice ranges are retained in the JSON receipts. After values are **not measured**.

- Afflatus, one key, block 64, 3 s phases: idle 0.1643 / 0.2468%; held 5.0979 / 9.1802%
  (16 voices); release 0.1680 / 4.1026% (0–16 voices); settled 0.1680 / 0.3540% (0 voices,
  zero PCM). Load average at completion: 14.40 / 18.89 / 22.47.
- Afflatus, one key, block 256, 10 s diagnostic without active profiler: idle 0.0883 /
  0.3092%; held 5.0667 / 10.9536% (16 voices); release 0.0855 / 2.6104% (0–16 voices);
  settled 0.1164 / 0.3137% (0 voices, zero PCM). Load average: 19.89 / 21.41 / 23.83.
- Afflatus eight keys, Conflux one/eight keys, Hybrid Walk one/eight keys, each at both
  block sizes: before and after **not measured**, because the coordinator stopped the
  full run and the user now prohibits worker measurement. No sub-1% claim is made.

The pre-ban structural read showed Afflatus's 220 groups / 17,104 zones / 1,268 modulators,
one impulse and 172 unsupported IR entries. The first sustain groups combine four dynamic
layers with Hall/MidHall/Decca/Close microphones; the loaded script has on-note, on-release,
on-controller, listener and UI callbacks. The audit sends CC1 dynamics and CC11 expression
and preserves those script selectors. Sixteen physical voices for one held MIDI key are a
workload fact, not proof all microphone layers should be audible. No groups were disabled
by this patch. Unsupported-reader details and saved mixer/articulation intent still need
library-owner verification; neither this worker nor its CPU hashes establish native parity.

Open topology rows and concrete reasons:

- T07: input frame count still replenishes a bus tail even for exact-zero samples.
  The Afflatus profile does not identify summed-bus FX as a dominant cost. Processor-owned
  activity needs tests for all stateful kernels and silence generators (noise, DC injection,
  modulation phase and changing parameters); blanket zero-input skipping would alter output.
- T08: `lower.rs` still uses generic 100 ms fallbacks and max of serial processor tails.
  Correct tail activity affects audible output and must accompany the processor contracts,
  particularly feedback delay, serial convolution/reverb and impulse swaps. A serial-sum
  guess or lower silence threshold is not an equivalent optimization. Conflux attribution
  and native-tail calibration are blocked by the measurement rule, not marked solved.
- T20: shared/freewheel LFO waveform evaluation remains per voice. Native shared ownership
  is open in topology B16. Pure waveform memoization with complete inputs might be equivalent,
  but the Afflatus profile does not isolate it as a major frame. Clocks/seed/fade ownership
  must be established before sharing modulators.
- T05/T06: the existing 64-frame voice grid and midpoint filter/pitch sampling remain.
  Kontakt's verified 32-frame Daft wrapper is local to that processor. It does not authorize
  halving the universal grid; doing so changes output and usually increases CPU.
- Remaining held-note profile: NativeEq processing/tuning, Stereo processing/targets,
  source addressing/window work, PreparedVoiceChain processing and modulation/shaping.
  The full original flat list is in `audio-cpu-receipts/before-top-frames.txt`; there is no
  remaining-frames **after** list. EQ/stereo/lane/kernel files are assigned to other workers.
  The scalar eligibility gate excludes script/modulated voices; extending SIMD there needs
  owner coordination and exact per-voice state tests. Do not collapse nonlinear group FX
  into a summed bus or adopt v1's LTI sharing without its admission proof.

Frozen v1 is absent. Quiet before/after, calibrated Kontakt audio, full Conflux/multi
semantics and the below-1% target remain open. This package's safe skips/preparations are
verified by exact focused tests, not a claimed factor-of-five CPU reduction. Numeric receipts
and test outcomes are committed; the worker target and scratch are deleted at handoff and
no worker unit remains running. No in-use/protected files, steal or purge were edited.
