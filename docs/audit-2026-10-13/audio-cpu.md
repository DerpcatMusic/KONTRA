# Audio-thread CPU audit (gpt/cpu)

## Reproduction

The audit extends `crates/sampler-perf` and `tools/cpu-audit-run.py`. It streams real sample
assets through production Kontakt translation, script initialization, lowering, MIDI ingress,
stream servicing, render, effect draining and note/behavior retirement. No decoded sample,
script or key material is persisted. PCM evidence consists only of aggregate energy and a hash.

Build in the CPU worktree through the shared wrapper:

```sh
CARGO_PROFILE_RELEASE_DEBUG=1 CARGO_PROFILE_RELEASE_STRIP=none \
RUSTFLAGS='-C force-frame-pointers=yes' \
/home/derpcat/.cache/kontakto-heavy cargo build --release -p sampler-perf
python3 tools/cpu-audit-run.py v2 \
/mnt/Windows11/DEV_WORKSPACE/Toolchains/User/cargo-target/kontakto-gpt-cpu/release/sampler-perf \
/home/derpcat/.cache/kontakto-gpt-cpu/before --sustain
```

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

The first fix ports v1's sleeping-bus early exit, adapted to v2 exact declared tails.
A per-buffer dirty-state bit resets processor cells once at tail expiry instead of
re-clearing zeros on every dormant chunk. No timing law or PCM computation changes.
`bus::cpu_tests::sleeping_bus_clears_once_and_wakes_with_identical_pcm` failed first
(101 clears versus 1), then passed. It checks independent biquad recurrence and
bit-identical sleep/wake PCM. Crate check passed before the fix; final checks pending.

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
