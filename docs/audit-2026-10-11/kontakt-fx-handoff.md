# Kontakt FX continuation receipt — 2026-10-11

## Branch + worktree path + commit SHAs (oldest first)

Branch: `fix/kontakt-fx-parity`.
Worktree: `/mnt/Windows11/DEV_PROJECTS/Repos/KONTAKTO/.claude/worktrees/agent-a937570fb7242048f`.

Inherited: `4bc26089` EQ, `82afc8ab` ladder taps, `17b97ef7` AR admission.
Continuation: `56330925` Classic Saturation, `7d046b21` native Distortion,
`71ef759e` AR checkpoint evidence correction, `618e8ee9` stateless Saturation bus tail,
`0007a5fa` initialization writes and exhaustive processor labels. The receipt commit
follows these; no branch was pushed or merged.

## Per item: before → after, root cause, evidence

The numerical nulls below compare implementations with retained vectors; they are
**not new calibrated Kontakt host recording nulls**. No item receives a new host
parity claim from this continuation.

| Item / reach | Before → after; root cause | Evidence / error |
| --- | --- | --- |
| Morphology Filter FX v0x95 / 904 | Inherited LP1–LP3 taps now share the LP4 loop; this does not cover all saved types. Existing numeric payload census includes 30, 32, 33, 35 and 41. Types 35 and 41 remain unsupported. | `effects.rs:1010`; inherited ladder evidence needs accepted paired recording; new host error **not verified**. |
| Filter v0x92 / 475 | AR 100–108 admission covers those records, not every v0x92 filter. The inherited combined dump has types 13 and 39 outside current supported kernels. Its lack of file identity prevents assigning every v0x92 record to Morphology. | `effects.rs:1049`; native checkpoint error below; new host error **not verified**. |
| ANALOG Distortion / 483 | Dropped slot → Tube/Transistor processor, saved Drive/Damping, DC blocker and damping filter. Ported our v1 `0cb7a8a0:src/fx/blocks.rs`, preserving f32 operation order. | `effects.rs:865`, `dsp/distortion.rs:204`: `distortion_retains_v1_native_transfer_and_filter_vectors`, **−150.03 dB** decimal transfer-vector null; exact retained damping/DC vectors and split-block state. Saved-slot admission test observed red then green. Full host error **not verified**. |
| “Surround Panner” / 483 | Legacy serialized 0x1d name concealed the Classic Saturation/Shape module. Dropped slot → signed Shape processor, scalar and batched, stateless bus tail. Saved ANALOG mode is Classic; unsupported selectors remain diagnosed. | `effects.rs:881`, `dsp/shaping.rs:97`: `classic_saturation_matches_v1_measured_vectors`, **−170.33 dB** decimal vector null. `classic_saturation_saved_slot_is_admitted_in_both_scopes` observed red then green. Full host error **not verified**. |
| Formant I / 483 | Existing v1 vowel proxy remains diagnosed; no speculative table was installed. Exploratory native fits do not support treating the existing serial peaks as proven native topology. | `effects.rs:911`; native spectral error **not verified**. |
| Inverter / 353 | Existing matrix and output gain retained. Serialized flag order still has only control-order evidence. | `effects.rs:539`; stereo basis capture needed, native null **not verified**. |
| AR LP2/4 / 106 and LP2 / 100 | Native FilterDJ kernel retained; corrected comments that incorrectly described native instruction checkpoints as plugin recordings. | `dsp/ar_kernel.rs:175`: `all_subtypes_match_original_instructions_at_native_checkpoints`, **−∞ dB** checkpoint null for all nine modes, 81 parameter cases × three phases. Original instruction fixture provenance remains in `docs/audit-2026-10-10/w15-ar-native.md`; CRT/table/mix stubs limit host conclusions. Recording comparison **not verified**. |
| Phaser / 13 | Generic v1 six-allpass proxy was not promoted to native parity. | Native null/spectral error **not verified**; modulation/topology lacks accepted vectors. |
| Reverb algorithm / 4 | Existing reverb and diagnostic retained. Setter vectors establish stored parameter laws, not algorithm topology. | `effects.rs:1411`; native error **not verified**. |
| Convolution / 6 | Existing convolution retained. Native IR preparation, early/late filters, size and decimation remain unresolved. | Native response error **not verified**; no accepted reference IR-preparation vectors. |
| Compressor mode / 1 | Existing mode-0 evidence does not establish the alternate detector mode. Diagnostic retained. | `effects.rs:1132`; native envelope/error **not verified**. |
| Dolce instrument bus pan | Existing unimplemented pan diagnostic retained; Stereo Modeller law was not borrowed. | `effects.rs:1597`; native gain law/error **not verified**. |

Initialization also needed repair: `apply_writes` previously ignored Shape, Drive and
Damping. It now uses the v1 signed/normalized laws on validated typed records at
the addressed physical slot/rack. The focused
`init_shape_drive_and_damping_writes_update_only_addressed_valid_slots` test at
`effects.rs:2576` failed with Shape 0 instead of 0.5 before the fix and passed after.
It checks wrong-rack writes, another module and truncated payloads. This is a
control decoding test, not a dB measurement.

One metadata-only ANALOG read found 483 groups, three scripts, and 483 translated
instances of each new drive processor. Authored controls reference Shape, Drive,
Damping, Formant Talk/Size and effect bypass; they do not write the Shape or
Distortion algorithm selector. Script bodies and library PCM were neither logged
nor written. The temporary intent probe was removed after preserving work with
`git wip`. Translated cache fingerprints include the modified sources, so existing
translated metadata invalidates without a separate cache-version change.

## Files touched outside your scope (and why)

- `crates/sampler-ir/src/lib.rs`, `validate.rs`: two processor variants and their
  finite/range validation; necessary to carry the new kernels through translation.
- `crates/sampler-core/src/lower.rs`: two processor match arms and one zero-tail
  entry for stateless Saturation. No modulation code changed.
- `crates/sampler-core/src/trace.rs`: two trace labels/coefficients.
- `src/sound/v2.rs`, `src/ui/chain.rs`: exhaustive insert names and detail labels;
  the first workspace compile identified these missing match arms.
- This receipt. Core DSP routing/lanes and kernels are in the assigned FX scope.

## Still open: item + concrete reason

The reference tool attached to the inherited Kontakt 8 PID **975760**, already
loaded with `D:\kontra_ref_src\calibration.nki`. It passed the pinned GUI state
checks (master 0 dB/440 Hz, instrument tune 0, pan Center, volume after explicit
CC7=127). Inside `kontakto-quiet`, which held all three heavy slots, pre-session
calibration was refused by `record.sh`:

```
record: load average 11.41 >= 8, Kontakt drops voices under load; wait or run inside a kontakto-heavy slot
```

No pre-calibration audio completed; therefore there was no valid session, paired
post-calibration, or repeat comparison within 0.1 dB. Load stayed above the gate
during follow-up checks (including 18.58). No guards were bypassed, Wine prefix or
yabridge configuration changed, foreign job stopped, or replacement Kontakt
process started. The inherited instance remains available with its synthetic
calibration script bypassing the group filter, editor view closed, unsaved.

Prior `kontakto-fxparity` exploratory recordings do not supply the requested valid
paired sessions. Most logged load above 8; t4/t5 were below 8 but lack the full
paired calibration/repeat receipt. t11 is additionally short and marked `.short`.
Those inherited files were preserved. The inherited EQ fit is reported as 0.06 dB
RMS, but this continuation does not certify its complete session acceptance.

Consequently, the unsupported filter types, Formant topology, Inverter flag order,
Phaser, reverb algorithm, convolution preparation, alternate compressor mode and
Dolce bus pan remain open with native error **not verified**. Saved and init drive
FX now execute, but live Shape/Drive/Damping parameters need registry/control-lane
work owned by the modulation agent; no live FX parity claim is made here.

## Anything the coordinator must act on (conflicts, follow-ups)

Arrange a quiet reference window, then record each unresolved law with calibration
before/after and agreeing repeats. AR additionally needs a whole-host recording
despite exact native instruction checkpoints. Coordinate live drive controls with
the modulation agent and reconcile the minimal `lower.rs` match/tail changes;
`voice_mod.rs` and `parameter_registry.rs` were untouched.

Validation: `cargo check -p sampler-kontakt` passed. Only the owned focused tests
listed above were run; the metadata intent probe also passed once. The first final
workspace `cargo test --no-run` failed on the two exhaustive UI matches introduced
by these variants. After the minimal fixes, the final rerun passed, exit 0, in
18.02 seconds (existing warnings remain). No full test suite was executed.

Cleanup: the continuation's scratch directory
`/home/derpcat/.cache/kontakto-gpt-kontakt-fx` and rebuilt 6.6 GB target
`/mnt/Windows11/DEV_WORKSPACE/Toolchains/User/cargo-target/kontakto-agent-a937570fb7242048f`
were removed. No owned build or reference job remains running. The inherited
Kontakt instance and other agents' targets/caches were retained.

Skipped: unsupported selector guesses and proxy DSP promotions. Remaining risk:
the saved drive kernels and exact AR checkpoints still need a valid whole-host
recording; live drive controls remain unconnected.
