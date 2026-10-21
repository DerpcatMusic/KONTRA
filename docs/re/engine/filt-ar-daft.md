# Engine unit: AR LP/HP/BP (2, 4, 2/4) and Daft / Daft HP (Kontakt 8)

Status: **gap table and design written; implementation waiting on Kontakt 8 decompilation.**
`/data/dev/workspace/Research/KONTRA/source/kontakt8/READY` does not exist yet, and the
`kontakt8/` export directory is empty (Ghidra project `Kontakt8.lock` is present in `source/ghidra/`).
Nothing below was taken from new decompilation. Every address cited is from existing repo evidence.

Scope: Kontakt 8 only. Format mapping lives in `crates/sampler-kontakt/src/effects/`;
the shared engine lives in `crates/sampler-core/src/dsp/filter/`; neutral config in
`crates/sampler-ir/src/fx/filter.rs` and `crates/sampler-ir/src/lib.rs` (`Ar`, `Daft`).

## 1. AR family (saved types 100..=108)

### Identity
- Saved 100..=108 map to internal 76..=84 and to one native class, `BFilterDJ`, whose core is
  `NI::KONTRAFX::FilterDJ` (`docs/audit-2026-10-10/w15-ar-native.md`, "Identity").
- Mode table (saved → mode, topology, output): 100→0 2-pole LP, 101→1 2-pole BP, 102→2 2-pole HP,
  103→3 4-pole LP, 104→4 4-pole BP (separate branch), 105→5 4-pole HP, 106→6 combined 2/4 LP,
  107→7 combined 2/4 BP, 108→8 combined 2/4 HP.
- Documented intent (Kontakt Manual, filter reference, `#ar-*`): resonance decreases with high
  input amplitude and increases at low levels; 2-pole 12 dB/oct, 4-pole 24 dB/oct; combined modes mix
  the two- and four-pole responses.

### Parameters
| Control | Units | Range | Default | Notes |
|---|---|---|---|---|
| Cutoff (`ENGINE_PAR_CUTOFF`) | stored normalized | 0..=1 | saved value | Law: table `T[i] = 2^(i/60 − 20)`, adjacent-f32 interpolation, position `min(145·x, 140)·5 + 1381.881591796875`; pole coefficient is a 5th-order polynomial in frequency/rate (native cutoff `0x140b04a30`). |
| Resonance (`ENGINE_PAR_RESONANCE`) | stored normalized | 0..=1 | saved value | Normalized ramp, updated before scale (`0x140af1d54..d78`). |
| Subtype | saved id | 100..=108 | — | Selects mode. |

Clock and ramps: 32-frame control quantum; ramp duration `max(1, trunc(rate·0.001/32 + 0.5))` quanta;
a pending request restarts the full duration even if unchanged; steps with square < 1e-15 snap to target
(`0x140b03300`, `0x140b03360..3a5`, `0x140b03424`). Wrapper clock `0x1408f9d90` keeps its own 32-frame
countdown. Per-channel state: 9 f32 cells at core offset `0x70`, stride `0x24`; sections clamp integrator
states before soft saturation; a shared stereo detector follows band output, releases with rate scaling
and reduces feedback as level rises (`0x140afedc0` dispatcher, `0x140af0632` static, `0x140af1f30` ramp).

### Evidence in repo
- `crates/sampler-core/src/dsp/filter/ar_kernel.rs`: independent equations; test
  `all_subtypes_match_original_instructions_at_native_checkpoints` (PASS, run 2026-10-21).
- `crates/sampler-core/src/dsp/filter/ar.rs`: 22 stereo cells, test-only runtime. Tests
  `ar_test_only_runtime_matches_natural_native_wrapper_checkpoints`,
  `ar_test_only_runtime_preserves_quantum_state_across_partitions`,
  `ar_low_pass_2_4_matches_the_kontakt_recording`, pending-request and tiny-step tests (all PASS, 6/6 focused).
- Vectors: `ar_native_vectors.json` (81 synthetic scenarios, 243 static/ramp/retune cases, max error 0.0),
  `ar_runtime_vectors.json` (180 cases, strict 2e-6).
- Native binary identity and hashes: see `w15-ar-native.md`.

### Gap table (AR)
| Behaviour | Engine status | Gap |
|---|---|---|
| All 9 modes, cutoff/resonance laws, 32-frame clock | Covered (`ar_kernel`, `ar.rs`) | None proven in Rust beyond repo vectors. |
| Ramp state, pending-request priority, partition invariance | Covered | Tests are test-only (`cfg(test)`); the runtime reserves 22 cells in the shipped `Core::Ar` path. |
| Config → IR → kernel mapping (`effects/mod.rs` arm at ~1175) | Covered for 100..=108 | None. |
| Host-recorded acceptance (real Kontakt PCM) | Not covered | Needs calibrated paired captures; `Fidelity::Exact` label in `filter.rs` rests on vectors, not host audio. |
| Wrapper rounding: `f32(rate_inverse·2092.3…)` grouping | Covered by the kernel | None. |
| Native CRT `pow` last-bit equivalence | Not proven | Table initialized from the verified law. Expected ≪ 2e-6. |
| Clock: 64-frame source vs 32-frame control | Open (`W15_NATIVE_CUTOFF.md`) | Whole-block timing parity not established. |
| CPU acceptance | Open | Needs a quiet run. |

Decision: no AR change in this unit. Its Rust checks pass against the recorded vectors, so the
`Exact` label stays. Re-check it after the Kontakt 8 decompile finishes.

## 2. Daft (saved 70 = LP, 71 = HP)

### Identity
- Native class `BFilterDaft` owning `NI::KONTRAFX::FilterDaft` at wrapper offset `0x210`
  (`docs/re/DSP_SYSTEM_INVENTORY.md`, "Daft parameter laws and scheduling").
- Parameter setter `0x140b05900`; cutoff tail-call `0x140b04c80`; table initializer `0x1404d14d0`.
- Internal selector: wrapper `0x140af0170` maps internal 68→mode 0 (LP), 69→mode 1 (HP), other values→0.
  The internal-to-saved mapping (68/69 vs 70/71) is **not proven**; the code cites v1's stored-ID table
  (`effects/mod.rs` ~1198, marked `ponytail: unverified`).

### Parameters
| Control | Units | Range | Default | Notes |
|---|---|---|---|---|
| Gain (leading value of extra params) | stored normalized | see below | saved value | Kernel: amplitude `lookup(x·119.58941 + 1200)`, unity at 0, 3.981133 (+12 dB) at 1, 0.707950 at −0.25. Mapping clamps to 0..=1 (`effects/mod.rs` ~1200). **Conflict with the signed test, see open questions.** |
| Cutoff | stored normalized | 0..=1 | saved value | Kernel: table `2^(i/60−20)`; cutoff Hz runs 25.956726 at 0 to 30 000 at 1 (`parameter_laws` test, values from `DSP_SYSTEM_INVENTORY`). |
| Resonance | stored normalized | 0..=1 | saved value | Kernel: 0.5 → 0.75 (`targets` element 2). |
| Response | derived from saved id | 0 (LP) / 1 (HP) | — | Kernel: response control 0.49 → 0 (LP), 0.5 → 1 (HP). The mapping sets it from the saved id. |

Clock: 32-frame quantum (absolute sample clock, not per-instance countdown); ramp length about
2 ms in whole quanta (`ceil(0.002·rate/32).max(1)`), per `ramp_length_follows_the_sample_rate`.

### Evidence in repo
- `crates/sampler-core/src/dsp/filter/daft.rs`: 2x oversampled trapezoidal SVF; tests
  `parameter_laws_match_the_specified_table_points` and `ramp_length_follows_the_sample_rate` (PASS).
- `crates/sampler-kontakt/src/effects/mod.rs` tests `daft_filter_slot_keeps_its_normalized_controls` and
  `ladder_and_daft_cutoff_routes_retain_the_authored_physical_slot` (PASS).
- `docs/architecture-v2/W15_NATIVE_CUTOFF.md`: Daft route amount test (−0.143 dB residual, +8.0 dB HF change on
  Analog Strings); this only checks routing, not the kernel.

### Gap table (Daft)
| Behaviour | Engine status | Gap |
|---|---|---|
| Cutoff and resonance laws, table points | Covered (`parameter_laws`) | Table values come from the inventory, not from a native run here. |
| Ramp length, 32-frame cadence | Partially covered | Cadence follows the absolute clock; the native per-instance countdown is unverified. |
| Gain law (sign, range, curve) | **Not proven** | Mapping clamps to 0..=1; kernel test assumes signed gain. |
| Saturation transfer | **Stand-in** (`soft()`, rational tanh; CORE-FILTERS-029) | Native transfer not recovered. |
| Oversampling / resampler (linear up, two-tap down) | **Stand-in** | Native resampler not recovered (CORE-FILTERS-030/031). |
| Feedback nonlinearity | **Missing** | Not recovered. |
| Stored id 70/71 vs internal 68/69 | **Unproven** | Depends on v1 table; verify with the decompile and with a saved-file oracle. |
| Routing / physical slot / Mix-block retention | Covered | None. |
| Audio kernel vs Kontakt recordings | Not covered | Needs calibrated captures. Daft is `Fidelity::Partial` (correct). |

## Open questions (to close after READY)
1. Daft Gain: does the native law accept negative leading values? If yes, drop the mapping clamp in
   `effects/mod.rs` (Daft arm) and the `drive` clamp in the kernel path.
2. Daft saturation and resampler: recover from the `FilterDaft` process routine; replace `soft()` and
   `inputs`/`out` interpolation only with the recovered form.
3. Daft id 70/71 vs internal 68/69: confirm through the constructor table and the wrapper selector.
4. AR clock: confirm the 64-frame source vs 32-frame control relationship in the dispatcher.

## Plan once READY exists
1. Grep `source/kontakt8/` and `source/INDEX.md` for `FilterDaft`, `FilterDJ`, `0x140b05900`,
   `0x140af0170`, `0x140b04c80`.
2. Recover the Daft gain law, saturation and resampler; add a bit-exact golden test or recorded-vector test.
3. Map the signed gain (if confirmed) in the Kontakt arm only; keep the kernel shared.
4. Draft PR until the decompile closes open questions 1–3.
