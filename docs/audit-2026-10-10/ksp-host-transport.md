# Production KSP host transport — source handback

## Status and immutable integration boundary

SOURCE READY; Rust compilation/runtime/heap regressions **UNRUN**.
Native execution/clock/listener parity **UNKNOWN**.
Lower CPU AND RAM than both frozen v1 and Kontakt **UNACHIEVED / UNMEASURED**.
This slice belongs to a **following** integration cycle, not the frozen 9075 cycle.

Exclusive checkout: `/home/derpcat/.t3/worktrees/kontra-ksp-host-transport`.
Branch: `pi/ksp-host-transport-429e`.
Base: `429e7dff3ecb0f53aef32c43201ab796ccc8c28c`.
Base tree: `94ad5caaa5e5ec70e1595d34166c25110d294c67`.
Ordered cherries:

1. `95662cae908fb3a042b42d0f8200598eac4a4e6d` — production block/script regressions (expected RED; not executed).
2. `dffbafabba75d26451515ec2ee86576462a6e858` — boundary wiring, validated host state and two additional edge regressions.

Product/test tree at dff: `42a21ea48f01892f9e7c502ec46a593344f10a11`.
Only product code changes are `src/sound/v2.rs` and new `src/sound/v2/host_transport.rs`.
Tests are in new `src/sound/v2/host_transport_tests.rs`.
No core/KSP arrays, NKA service, file completion, fade, plugin effect or dependency edits.
NKA edits other spans of v2.rs; cherry-compose, do not overwrite that file.

## Authoritative native-spec equivalent check

The REA catalog was discovered directly. No binary session or vendor process was
opened. The immutable archived **vendor specifications** below are the equivalent
evidence for the exact documented units and listener consumer contract in this
slice. They are more authoritative for these public units than unrelated binary
strings. Each actual HTML byte digest was recomputed and matched the existing
archive manifest, not copied into an invented receipt. This checks the public
native specification; it does **not** establish Kontakt runtime equivalence.
Kontakt 8.13.1 has not been executed for this item. No native PASS is claimed.

Archive root:
`/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff/official-docs/`.

| Precise item | Authoritative URL | Archived HTML SHA-256 |
|---|---|---|
| `$DURATION_*`, `$NI_SONG_POSITION`, `$SIGNATURE_NUM`, `$SIGNATURE_DENOM`, `$NI_TRANSPORT_RUNNING` | https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/built-in-variables-and-constants#time-and-transport | `9a5ab14188022767684a1dea13a0ca0e060e0838a788073b7a26b50b0030247e` (`ksp-variables.html`) |
| `set_listener($NI_SIGNAL_TIMER_BEAT, parameter)` and transport start/stop registration | https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands#set_listener-- | `aafd0e71a266571cc5a6adb9dbfb15bb31f4c4819e9748427634d171b34fa2f9` (`ksp-time.html`) |
| `wait_ticks(960)` is a quarter note, `wait` microseconds | https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/time-related-commands#wait_ticks-- | same `aafd0e71...` document |
| `on listener`, tempo changes can omit ticks | https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks#on-listener | `f91a2a1411c18e84e15f4e89e74499c3ae74dec75f230bb8c99df65e316267ba` (`ksp-callbacks.html`) |

Variables archive retrieved 2026-10-10T02:46:27.918489Z; time commands and
callbacks retrieved 2026-10-10T02:13:00Z. These are official unversioned documents,
not proof that a specific installed Kontakt build implements every detail.

Exact documented contracts checked:

- Quarter/eighth/sixteenth and their triplets are **microseconds at current tempo**.
- BAR is microseconds and is **zero when the host clock is stopped**.
- SONG_POSITION is **960 pulses per quarter note**, not frames or milliseconds.
- Signature values are the raw numerator/denominator, not an enum or bitmask.
- TRANSPORT_RUNNING is 1/0.
- The beat listener example explicitly says it triggers even when stopped.
- Transport START/STOP are distinct listener signals; acknowledging them as
  named constants does not mean production dispatch exists.
- `$NI_TEMPO`/`$NI_BPM` are not specified in that time-and-transport section.
  Slot19 is deliberately **untouched**. No alias or BPM-times-1000 ABI was invented.
- ENGINE_UPTIME is sample-rate-derived milliseconds and KSP_TIMER is CPU-clock
  microseconds. Neither clock implementation was changed.

## Exact implementing paths, pinned to dffbafabba75d26451515ec2ee86576462a6e858

| Path and lines | Symbol / evidence | Source result (not runtime proof) |
|---|---|---|
| `src/plugin.rs:3029-3038` | Plugin block entry | Existing caller forwards playing, tempo, position_beats and signature. Unmodified. |
| `src/sound/mod.rs:111-127` | `Transport`, `BlockInfo` | Block-start host contract. Unmodified. |
| `src/sound/v2.rs:484,1575,2094-2112` | `V2Core::transport`, `with_parts`, `Core::begin_block` | Normalize shared host snapshot, set offline/tempo and slots8-18 for every part **before** queued alignment flush and subsequent MIDI. |
| `src/sound/v2/host_transport.rs:9-38` | `State::default`, `State::update` | Startup120 BPM/4/4/position0/stopped; finite positive tempo; both signature entries positive; finite seek overrides. Missing nonfinite position uses already-rendered `Align::clock` elapsed at the **previous** tempo/playing state, before applying new tempo/start/stop. |
| `src/sound/v2/host_transport.rs:45-68` | `State::values` | Current tempo durations, stopped BAR0, floor(beats*960), raw signature, 1/0 running. Bounded i32 outputs; i64 duration intermediates. |
| `src/sound/v2.rs:2145-2184` | `Core::render` | Existing global alignment clock counts actual rendered frames on both direct and alignment paths. No new time syscalls. |
| `crates/sampler-ksp/src/lower.rs:77-110,1049-1054` | `host_slot`, `Gen::sys` host read | Existing ABI8-18 used; no new VM instructions. |
| `crates/sampler-core/src/ops.rs:853-855,1639-1642` | `Runtime::set_host_value`, `Op::ReadHost` | Existing fixed32-slot setter and consumer. |
| `crates/sampler-core/src/voice_mod.rs:1422-1431` | `Runtime::set_tempo` | Existing validated quarter-notes/minute input for beat-synced LFOs now has a production block caller. LFO audio regression not added here. |
| `crates/sampler-ksp/src/lower.rs:2002-2012`; `crates/sampler-core/src/ops.rs:1246-1262`; `behavior.rs:2522-2540` | `wait_ticks` -> `TimeConversion` -> `WaitMicros` | Existing wait conversion reads host quarter slot8 when the wait begins; scheduled waits remain fixed frame deadlines. |
| `crates/sampler-ksp/src/lib.rs:1204-1258`; `lower.rs:375-442` | Existing timer listener driver | TIMER_BEAT reads quarter slot8 each newly scheduled period. Source seam reused, not replaced with a new scheduler. |

Legacy policy evidence (ours, not vendor runtime):
`0cb7a8a0:src/ksp/runtime.rs:702-713,1013-1019,2182-2187` retains valid tempo,
positive signature pair and elapsed prior position; `src/ksp/calls.rs` duration
arms use truncating quarter microseconds and integer subdivisions. This slice
uses that existing missing-value/integer policy instead of inventing vendor rules.
Extreme-input i32 saturation and minimum quarter1us are **our safety policy**;
Kontakt's extreme-input reaction is UNKNOWN. Signature validation deliberately
does not require power-of-two denominator; positive raw pair is the v1 policy.

## Own synthetic regression manifest (all UNRUN)

`src/sound/v2/host_transport_tests.rs`, at the product SHA above:

| Lines / test | Intended runtime witness |
|---|---|
| 93 `host_block_publishes_microseconds_960_ppq_and_raw_signature_to_every_part` | Host block -> production V2Core -> authored note callback -> 11 script cells, realtime/offline and two parts. 240 BPM, 7/8, position12.5 gives quarter250000us/bar875000us/PPQ12000. |
| 109 `stopped_block_zeroes_only_bar_and_running_not_tempo_or_host_position` | Stopped -> running -> stopped, changed60 BPM/5/8, negative preroll, zero BAR only when stopped. |
| 130 `missing_tempo_and_signature_use_defaults_then_retain_last_valid_pair` | NaN/infinity/zero/negative tempo and invalid signature halves; defaults then last valid values. |
| 158 `positive_signature_pair_is_raw_not_restricted_to_power_of_two_denominators` | Raw255/3 and atomic retention of prior pair on zero denominator. |
| 171 `stopped_block_state_is_visible_to_midi_flushed_from_alignment` | Queued MIDI reaches callback with the new stopped/240 BPM snapshot. |
| 194 `missing_position_advances_rendered_time_then_stop_holds_and_seek_overrides` | 24000 already-rendered samples at120 BPM advance4 ->5 quarters (PPQ4800), despite new240 BPM/stop; stopped time holds; finite seek overrides; negative fraction floors. |
| 215 `block_growth_and_part_replacement_preserve_missing_host_state` | Capacity adoption and newly installed part consume retained host state at next block. |
| 233 `host_tempo_changes_affect_new_waits_not_an_already_scheduled_wait` | Existing fast wait stays12000 frames; next wait uses48000 frames after60 BPM update, both offline modes. This is the owned current wait contract, not measured native retiming parity. |
| 258 `existing_beat_listener_consumes_each_block_tempo_even_when_stopped_offline` | Existing driver consumes changing quarter250000 ->1000000; fast/slow counts and callback slots while stopped. Native execution remains UNKNOWN. |
| 280 `extreme_finite_inputs_saturate_ksp_integer_values_without_faults` | Subnormal/huge tempo and huge positive/negative position stay inside script integer bounds. |
| 295 `host_block_updates_and_script_consumption_make_no_audio_heap_calls` | Existing plugin allocation/free counter surrounds repeated begin_block/MIDI/render. Requires plugin feature; no new allocator. |

### Following-cycle request to the sole integration build owner

Do not run a second build or alter target/wrapper environment variables. Merge
with NKA by spans, not whole-file replacement. Preserve frozen current cycle.

- Optional RED: cherry test95662 alone; run root host_transport_tests filter.
  Expected consumer failures: tempo remains default, BAR remains2e6 stopped,
  PPQ/signature/running never propagate. This expected RED is not an observed RED.
- GREEN candidate: cherry both95662 anddff.
- `cargo test -p kontakto --lib host_transport_tests` — default/plugin features
  needed for heap witness. Run all11 tests, not just Runtime setter tests.
- `cargo test -p sampler-ksp --test tempo_waits` — existing wait/listener regression.
- `cargo test -p kontakto --lib v1_auto_align` — existing alignment ownership checks.
- Run the integration's normal applicable root/core/KSP area gate, serialized.
- Pin combined revision/tree, artifact hashes, exact commands/logs/exit codes.
  Failure requires diagnosis; source-ready is not GREEN or native parity.

## Checks actually performed and limits

`ksp-host-transport-source-checks.json` contains exact revision, docs and test names.
Receipt SHA-256: `d9db548cc525f5525e7b84e783803ad96db04a834442589d31c853e14b3f113b`.
Actual checks: archive SHA recomputation; documented units/alias exclusion;
slot8-18 mapping; publication-before-flush source invariant; immutable-base gap;
independent rational duration,960PPQ,negative-floor,previous-tempo fallback and
frame fixture calculations; integer intermediate bounds; no heap/clock-IO
constructs in the helper; rustfmt syntax/targeted formatting; git diff --check.
Python oracles **do not execute Rust**. No runtime/heap PASS is inferred.

Per-block cost is O(11 * installed parts), with one fixed host snapshot and an
11-value stack array. Existing runtime host arrays are reused; no per-part heap
allocation or resizing was added. CPU/RAM benefit is **not measured**; no savings
percentage is asserted. This correctness slice does not prove the performance goal.

### Explicit remaining gates

1. Host position is a **block-start snapshot**. It is not advanced per sample
   inside callbacks; sub-block note-offset PPQ/native clock parity is UNKNOWN.
2. DISTANCE_BAR_START is a note-on microsecond variable; it is not silently
   synthesized here. Actual host bar-start offsets are not in BlockInfo.
3. TRANSP_START/STOP dispatch is a separate missing feature: at this SHA the
   compiler's listener branch only creates timer drivers (`lib.rs:1206-1213`);
   `bind_modules` signal registration only admits PGS/async (`lib.rs:741-756`).
   Runtime exposes no host transport dispatch seam. Slots/constant names alone
   do not implement transport listener callbacks. Native parity is not claimed.
4. A listener period already scheduled before the first block (Runtime starts
   plan programs at construction) can use its default tempo. Each subsequent
   period reads the current quarter. Native initial-phase/tempo-change tick
   omissions and in-flight retiming have not been measured or changed.
5. Compile-time `on init` evaluator timing defaults are unchanged; host state
   publication applies to actual production runtime blocks, not off-audio init.
6. Shell adapters may supply default finite0 beats/120 BPM/4/4 when host fields
   are absent. BlockInfo has no availability flags; finite0 is an authoritative
   host position, not distinguishable from an adapter's absent-position default.
7. Sample-rate changes still require part reload under the existing Core contract.
8. Full native timing/listener/audio fidelity and comparable CPU/RAM benchmarks
   against both references remain future acceptance gates.

No cargo/rustc/clippy/build/native host/official reader/Wine/generic gate/server,
install/publication, nested agents, schedules or frozen-reference mutation ran.
Other lanes' dirty files and branches were left unchanged.

NEXT: following-cycle integration validates the exact cherries and reports actual
failures; coordinator routes any separate transport-listener/sample-clock feature.
