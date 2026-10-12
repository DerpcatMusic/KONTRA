# Dolce silence at frozen 9993

The first divergence is the amplitude envelope, before group/FX/output routing. Six representative regressions become audible when their bound envelope state is restored through the shared native parameter service. This is diagnostic evidence, not a production fix or an all-60 repair claim.

## Frozen evidence and scope

Product: `9993db691a5f69d31980357694a678e358785e5e`. Installed shared scanners verified by SHA256: v1 `870cea2140b5c9db5361831664966848302a2f57e82fcb6c7ed1b3534545ce5e`; v2 `19e2f2c76771ceeb1f5db47956d404a290e16ef61dabb6e34909176b362b6751`. All 60 Dolce v1-audible/v2-silent rows were joined to JSON exclusively with the installed driver's `signature(path, binary_sha)`, with matching JSON revision. Safe notes: 56 at 60/64, one at 48/64, one at 72/64, two at 73/64. The four Pacific trills silent in both and 348 Afflatus not auditioned are outside this finding.

The first witness uses signatures v1 `218c3671e677751586d6cd3e362d38319974eeee3e4c52def08e6000f1953c8d`, v2 `27f561979efb17ac91194b0377ee465e6bd151a2762a1b55c5339a66c81100f0`. Both retain 1224 zones; actual notes match and runtime faults/underruns are zero. V1 has 30 retained nonterminal load-fault records, separately from zero runtime faults. Init and persistence_changed complete in both.

## Observation method

Branch `v2/diagnose-dolce-9993` adds the existing shared signal graph to the frozen product, then an ignored, explicitly requested production-host witness in `src/sound/v2/dolce_diagnostic.rs`. Trace overlay commits: `4656a88e`, `a2c94329`, `b5ec8654`, `961e4c77`, `81d39a8d`. No Kontakt importer or KSP changes. Cold-hold fixes and later CPU changes are excluded. The opt-in trace renders scalar voices and can change worker timing, so these are functional diagnostics, never CPU cells.

The witness uses V2Loader.prepare, V2Core.install, dynamics_start100, CC1=100, CC11=127, the matched key/velocity64, no keyswitch, 180 blocks of128 with3ms sleep (23040 audio frames,0.48s). It does not paint Original UI. All authored script source, saved payloads and PCM remain in memory. Reports contain public file identities, numeric state, public parameter names and typed modulation descriptions only. Actual decoded sample probes run **after** audition to avoid warming samples before the diagnostic.

Frozen diagnostic executable: `~/.cache/kontakto-w6/bin/dolce-9993-probe`, SHA256 `0c92fe873301451c899c3f558969d46b484ee90a4f801466fff5d5c51244ba4b`. Logs and reports: `~/.cache/kontakto-w6/dolce-9993/`. Existing shared scanner binaries were neither rebuilt nor invoked for new collection.

## Numeric evidence

| Patch | Key | Baseline output peak | Envelope counterfactual output peak |
|---|---:|---:|---:|
| 7 1st Violins Sustained Con Sordino |60|2.58858e-19|0.0247664|
| 7 1st Violins Harmonics |73|2.58858e-19|0.0287552|
| 5 2nd Violins Harmonics |73|2.58858e-19|0.0217566|
| 4 Violas Harmonics |72|2.58858e-19|0.00236731|
| 3 Basses Bend FX |48|0|0.00727335|
| Ensemble Trill WT |60|2.57144e-19|0.275568|
| Audible control: 7 1st Violins Legato |60|0.0271413|0.0399572|

Except Bass Bend, counterfactual changes only bound ENGINE_PAR_SUSTAIN to1000000 after loader init. The Legato control is not a level-parity target; restoration changes other groups too. Bass Bend requires restoring authored attack/hold/decay/sustain/release at their physical modulator slots using the existing native laws; sustain-only remains silent.

The first witness admits36 attack regions at frame512, with36 cold starts, no refusals, drops or underruns. Real sample-source peak is0.0401746. Every amplifier output is exactly0. Group volume reads630009, script/note gain unity; mapped routes reach groups, buses, sends and master. The trace-on and trace-off output peaks and selection outcomes are identical. All72 mapped candidate assets decode successfully and have nonzero PCM in the first16384 frames; reserved residency is not used as PCM proof.

Authored amplitude envelope: attack3840frames, sustain1, release5760frames. The bound live readbacks and voice envelope instead have attack/hold/decay/sustain/release0. Sustained voices therefore remain silent. Bass Bend selects6 regions but starts no live/cold voices: its authored one-shot AHD has80ms attack and25s decay, replaced by zero timing. Restoring timing gives6 live voices and audible output without changing source/modulation/routing/FX.

All reported shared traces are complete with0 dropped records. The tiny baseline residue is reverb state, not voice output. Other modulation legitimately mutes some groups (from-script1, CC111 and crossfade shapes); the envelope-only counterfactual proves remaining mapped paths can reach the host output. It does not establish exact v1/native level or RR identity parity.

## Cause and ownership

At9993, `sampler-kontakt/src/load.rs::script_environment` passes an empty engine_values table to init evaluation. `sampler-ksp/src/eval.rs::GetEnginePar` uses authored values when available, otherwise zero for envelope parameters. Init setter requests replay into real bound envelope controls. This is the identified feedback seam, consistent with the causal measurements; W5 is checking the witness's getter/setter intent in memory and owns the shared service fix. In v1 `src/ksp/calls.rs` getters consult the real engine before the environment, rather than unconditionally using an empty authored table.

Required next gate: W5 must seed lawful authored parameter values before init, preserve physical addresses and subsequent shared getter/write semantics, then run the same witnesses without restoration. Recheck all60 through the unchanged shared scanner on the resulting product checkpoint. Six representative diagnoses do not prove the remaining54 have identical cause.

Validation: targeted ignored witness runs and counterfactuals pass; root library area `cargo test -p kontakto --lib --no-default-features --features plugin,library-access ... --no-run` passes (`build4.log`). No broad corpus, release/install, performance acceptance or v1/native level-parity claim.
