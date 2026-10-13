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
`perf record -g --call-graph fp` records code addresses only. The ready/finished markers
bound profiling to held-note rendering, excluding load, bootstrap and release. Software
`cpu-clock:u` supports machines that deny hardware events; profile overhead is excluded from
acceptance timing. Each cell releases the heavy slot.

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

Pending baseline/profile execution; no CPU acceptance claim yet.
