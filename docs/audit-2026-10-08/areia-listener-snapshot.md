# Areia: listener state versus scanner snapshot

The frozen gallery mismatch is a scanner lifecycle difference. No KSP host
connection return change is justified by this witness. The initial model contains
the warning panel; the existing production runtime hides it and reveals the
articulation controls when its timer listener runs.

## Attribution

Item SHA256: `86504c5269f3a683007175ae0b92df56400bc6eae460bfd37f4b5d8adbe9f1a9`.
One physical script slot, slot 0, contains 19,264,696 source bytes. Authored source,
names, text values, resources and samples stayed in RAM.

The listener begins at source line 113876. Its public signal discriminator is
`$NI_SIGNAL_TYPE`; its signal branches include `$NI_SIGNAL_TIMER_MS`,
`$NI_SIGNAL_TRANSP_START` and `$NI_SIGNAL_TRANSP_STOP`. The timer subscription is
10,000 microseconds. Its reachable code reads `get_control_par` and `get_ui_id`,
and writes UI visibility and layout. It contains no host connection, headless,
standalone, tempo, output-count or engine-uptime query. There is no literal host
warning text in the script or compiled model text properties; the warning label
uses a picture resource. Private predicates were not exported.

At frozen v1 `0cb7a8a0`, `src/ksp/vm.rs:1272` returns the callback's signal for
`SignalType`. In v2, `crates/sampler-ksp/src/lower.rs:1032` supplies the listener's
signal. Both provide the timer signal to this callback. The unrelated
`$ENGINE_UPTIME` query uses elapsed sample time in milliseconds in both versions.

The frozen v1 instrumentation patch at lines 362–369 processes ScriptSetup ten
times with 480 samples **before** taking the interface/face. In v2
`src/ui/scan.rs:481–503`, the scanner installs the part and clones its initial
interfaces before audition. Later rendering does not apply runtime UI effects to
that snapshot. The plugin's `Shared::apply_effects` at `src/plugin.rs:1157`
does apply effects to script views and publish updated interfaces.

## Matched numeric witness

Probe source base: `56a315dcdd219730a0fc54fd71082e0334f0e863`.
The probe uses `load_read_streamed`, the authored script and production runtime,
48 kHz, a maximum 128-sample block, and host-style aggregate behavior fuel. It
drains effects into the bound ScriptViews after each block. It selects key 127
and plays no notes to keep the UI attribution independent of sample rendering.

| Snapshot | Warning label 6 hide | Articulation control 90 hide | Runtime faults |
|---|---:|---:|---:|
| Initial, 0 samples | 0 | 16 | 0 |
| Matched v1 settle, 4,800 samples | 16 | 0 | 0 |

The warning first becomes hidden at the observed block boundary, sample 1,664.
At 51,136 samples, 203 UI effect applications changed the model, with zero
runtime faults. The matched 4,800-sample visibility and zero-fault assertions
pass. Initial visibility supplies the failing witness for an init-only snapshot.
Warning label 6 is at numeric position (260, 115); controls 90–98 become visible.

Owned evidence directory: `~/.cache/kontakto-w5/ksp-coverage/`.

- Probe code `ksp_host_branch.rs` SHA256:
  `52a6e65473e1197415bf9129086474d369461ccbcefb72d412427a65aeda5656`.
- Probe executable SHA256:
  `15e5b713d163f35d69d4d6b58687e0fabab668aff4c5b47f8e30378250a44b60`.
- Numeric `areia-matched.log` SHA256:
  `9dfe37ef080bba6f8c394da946604637a94204559c35bc2fbba0d90e2dde4fb2`.
- Frozen gallery v1 executable SHA256:
  `ac5aed734bb7fca40d6d000ff1f3e89128436b8f38d9d674fd70466f90dcdf08`.
- Frozen gallery v2 executable SHA256:
  `7ce04ef47e7250d196a64a56e65017a7926d15cb8d610469f135a8361f7bc6c0`.

The attribution probe is not a presented-plugin or audio parity test. The
independent Conflux GPU transparency failure is not explained by this result.

## Scanner correction

W5 now owns the scanner fix. `settle_snapshot` runs the frozen adapter's ten
480-sample ticks in `MAX_BLOCK` chunks, applies drained effects to `ScriptUi`,
and regenerates changed interfaces before face capture and control-value reads.
It uses offline blocks, retains runtime faults in the existing scanner report,
and completes the block lifecycle. The scanner reports `snapshot_settle_ms`
separately from its loader metric.

The ignored, authored-library test
`areia_scanner_snapshot_applies_listener_before_face_capture` uses the production
`V2Loader` with all keys, the owned translation manifest, and numeric visibility
assertions. Before the fix it fails because the warning remains visible; after
the fix it passes with the articulation list visible, zero runtime faults and
zero streaming/offline failures. No authored source or identifiers are logged.
Run it explicitly with `cargo test --profile ci --features shots --lib
areia_scanner_snapshot_applies_listener_before_face_capture -- --ignored
--nocapture` through the normal wrapper.

Frozen before test executable SHA256:
`7f9e3aa18ed26433995c388ecd08cb69fe3488b0df7d313bd962a1fb9e34449d`.
Frozen after test executable SHA256:
`dc9a3086822dc571139c7650d3cdb9d597cf8e16f123ac3f790ca70539ab01b4`.
The before executable is the intentionally failing fixture on source base
`bc318a1f` with an empty settle seam; the after executable contains the correction.

## Quiet timing and validation

Code commit: `c643531d` (`port from v1 0cb7a8a0:src/engine/script.rs`).
Targeted authored Areia test: **1 failing before → 1 passing after**.
Root `ci` + `shots` library `--no-run`: PASS. The five timed before runs reproduce
the same expected failure; all five after runs pass the visibility, runtime and
streaming assertions. The original full KSP suite was not repeated for this
scanner-only change.

After W8's direct handoff, the timing runner alternated five runs per frozen test
executable. The measured scan phase is **production preparation, installation
and face snapshot**, excluding painting and audition. Each executable runs the
same authored fixture; the before executable intentionally fails its post-snapshot
visibility assertion. These are not end-to-end CLI or presented-plugin timings.

| Milliseconds | Before median | After median | Delta |
|---|---:|---:|---:|
| Prepare and snapshot scan | 3,095.710 | 3,154.575 | +58.865 (+1.90%) |
| Snapshot settle phase | 0.000 | 3.796 | +3.796 |
| Whole test process | 3,129.830 | 3,186.231 | +56.402 |

The timings are noisy: before scan range 2,971.672–8,830.450 ms, after range
2,971.194–10,565.942 ms. Medians use all five runs per binary, without dropping
outliers. No speedup or causal attribution for the total-time delta is claimed.
The accepted repeat's activity observer records **QUIET**, 45 samples at one-second
intervals, with no foreign heavy work or observer errors.

The first timing batch is retained separately as **CONTENDED / UNKNOWN**:
foreign cargo PID 2308769 and rustc PID 2308828 started during the request at
08:58:03 UTC. It is excluded from the table. One clean repeat completed after
those processes exited. No foreign processes were killed. The bounded window
ended with removal of W5's request/grant and direct handoff to W6 and coordinator.

Owned receipts: `areia-snapshot-timing.json`,
`areia-snapshot-activity/activity.{json,jsonl}`, and
`areia-snapshot-timing-contended.json` under the evidence directory above.
No further builds or library probes followed the handoff.

NEXT: W0 integration of the scanner fix and receipt; subscription stays parked.
