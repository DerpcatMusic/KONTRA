# W13 alpha 0.3.306 live CLAP receipt

Candidate source: **72bb976744762435ab726f0a00908f47d9f470c5**, clean detached worktree. Built through `kontakto-heavy` with `cargo build --release --locked --no-default-features --features clap,library-access --lib --bin kontakto`; optimized release build passed. BuildInfo reports version 0.3.306, release profile, clean source and features clap/library-access/plugin.

Frozen receipt directory: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w13-live-host-alpha-readback-20261008`. `bin/BUILD.json` records source, build environment, reader and all artifact hashes; `bin/SHA256SUMS` checks the frozen files.

| Artifact | SHA-256 |
|---|---|
| Candidate CLAP | `8cdd77ac8518b4f0b0890e0f5455027fcf1f32c439d747022423ae3b9e5dbe97` |
| Candidate CLI | `6cec38668dec9300f4175768664b086e275d19b9f54ef68b50fdedbff026ffaa` |
| Native live host | `989f6f7e5496f23aaf140d31e541b1120886ef6f019de98d3c7414c2d1136033` |

The frozen v1 plugin and CLI passed their existing `SHA256SUMS`; neither was rebuilt. Host, Python driver, observer and diagnostic capture are byte-identical to source 72bb9767 (`PROBE_SOURCE.json`). Native host compile/selfcheck and failing-first Python native-readback/settings checks passed before the quiet request.

Gate source **52438eb49b3d9daa45e88eeead48f610c9dfaa95** supplies the same 22 items as the original 3f49 gate, with all 23 program audition plans. Both versions receive the same gate key, velocity, keyswitch and sample-exact event schedule. Planned cells: CLAP64 all items, plus CLAP32/256 Vista 3 Violins FFF Overlay (index 8) and Areia Full Ensemble (index 15), six seconds per audition at 48 kHz. `KONTRA_THREADS=1`; private version-2 settings with imported=true/empty roots. The staged official UVI reader is explicitly selected by environment so discovery does not probe Wine.

Admission requires QUIET activity, successful native processing, finite nonzero audio, complete event dispatch and native CLAP state.save readback matching source/program/MIDI/output/gain/aux and part order. Script/widget recall is outside this readback's scope. CPU wall/thread p50/p99, process and wake deadlines, sampler underruns, whole-process streaming I/O and v2's numeric perf model are separate measurements. Frozen v1 perf-model readback remains UNKNOWN. The editor is closed; these cells do not certify a rendered DAW frame, VST3 or UI interaction. OS page-cache state is uncontrolled.

Quiet status: **DEFERRED**, zero timed cells and no grant. The observer found foreign-project Cargo work and existing project jobs. At the coordinator's priority change, W13 stopped its own waiting orchestrator and removed its request immediately, allowing W0's follow-up validation/rebuild. W9 was directly notified. `WINDOW.json` records removal; the frozen 72bb artifacts are retained. Installation hash comparison is pending W0's installed artifact. W13 retakes the first window before W9 after W0 reports the follow-up installed.

Settings/features parity: [84-row source inventory](w13-settings-parity.md), explicitly retaining three missing named global DAW parameters, Kontakt full script/engine recall and v1 settings migration as gaps in this frozen alpha.

NEXT: source-only host-control parity scope; retake quiet after the follow-up installed handoff, then CLAP64/32/256 and direct W9 handoff.
