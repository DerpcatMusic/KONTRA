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

The production probe is not a presented-plugin or audio parity test. This change
records attribution only; no product behavior or scanner correction is included.
W3 owns the scanner seam and received the matched witness: settle and apply UI
effects before capturing the face. The independent Conflux GPU transparency
failure is not explained by this result.

NEXT: remaining semantic-command declaration and contract attribution.
