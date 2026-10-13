# Native reference validation — 2026-10-13

Fresh original-machine-code checks support specific Kontakt and UVI DSP laws and kernels. They do not establish complete engine, plugin, or Falcon parity. This summary contains evidence metadata only; vendor source, pseudocode, scripts, and binary data remain outside Git.

The evidence root is `/mnt/MAIN_STORAGE/kontra-runs/native-parity-20261013`. This review independently counted saved JSON records, checked declared result flags and receipt hashes, and cross-checked the fresh execution logs. It did not rerun native code or builds.

## Kontakt

`kontakt-dsp-law-reverification.json` reports `ORIGINAL_BYTE_LAW_CHECKS_PASSED` against the standalone EXE, SHA-256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`. The 15 structured lists in `kontakt-dsp-laws-fresh.json` independently sum to **1,222 records**. Its SHA-256 is `d69f5e51e1f5d0335eb42d7087cf970ffb172fc2305964c84999e9e67c02b1ce`, matching the revalidation receipt.

Coverage includes AHDSR controls/kernel/lifecycle, Galois parameters, saved EQ conversions, coefficient generation, scalar EQ PCM/history, EQ update masks/control cadence, Solid spline tables, stereo state/smoothing, bus timing, wrapper cadence, and external-modulator queues.

The EQ revalidation supersedes the earlier vacuous sample check caused by a zero-coefficient interceptor. All **32 PCM/history cases** now contain nonzero coefficient vectors; all **28 positive-frame cases** contain nonzero output. Cases cover mono/stereo, both tested gains, and frame counts 0, 1, 3, 4, 5, 32, 33, and 129. Reset checks are recorded for every case and EQ vector dispatch is disabled. The corrected scalar reference preserves the native f32 operation grouping.

Kontakt checks retain declared math, memory, reset, topology, and coefficient helper substitutions, with MXCSR `0x1f80`. These are isolated original-byte law/kernel checks, not a vendor-host run or SIMD-equivalence claim.

## UVI Workstation

The recorded native reference is `UVIWorkstationx64.exe`, SHA-256 `78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721`. A copied saved Ghidra project supplied the original loaded `.text` snapshot through a read-only export without reanalysis. The export log records **28,083,256 bytes** from `0x140001000`, SHA-256 `684a5557efed9e426a62cbba75c15cc727d764680e1660164b7c5ae88698fda1`, matching the retained snapshot identity. This recovered a previous capture; it was not a new live capture.

Fresh passing logs and receipts under `uvi-original-byte-checks/` establish **3,937 cases**, with no helper substitutions:

| Check | Verified receipt count | Evidence boundary |
| --- | ---: | --- |
| OnePole | 10 setters + 480 sample blocks = 490 | All 480 PCM/history flags are exact; coefficients are authored inputs and native coefficient conversion remains unverified. |
| LFO | 12 setters + 192 table/render blocks = 204 | Fresh log reports passed checks; tables are authored and factory waveforms remain unverified. The JSON has no separate `exact_*` result flags. |
| Shape/decimation | 480 rectifier + 495 fractional-decimation + 2,268 BitCrusher blocks = 3,243 | All output/history flags are exact for their stated scopes; internal parameters are authored and the complete effect pipeline remains unverified. |

The fresh UVI receipt files have the same hashes as their retained historical counterparts; freshness is demonstrated by the new rerun logs and recovered-snapshot/rebinding receipt, not by changed deterministic JSON contents. The earlier missing-snapshot limitation in `uvi-native-reference-review.json` is superseded by the verified recovery and reruns.

Workstation is evidence for demonstrated shared UVI DSP paths. No Falcon executable was located in the recorded inventory. Vendor host lifecycle, plugin behavior, SIMD equivalence, control mapping, runtime-initialized data, valid owner/TLS contexts, and full end-to-end parity remain unverified. **The UVI OnePole kernel has not been ported to production in this audit turn.**

## UNPACER and retention

The installed Workstation VST3 inspection was rejected with `Required section is absent or ambiguous`; `unpacer-workstation-inspection-error.json` records `target_execution: false`. The existing guarded profiles do not provide a generic UVI profile, so this is evidence of an unsupported target, not successful unpacking or behavioral verification.

The external companion JSON (`native-reference-validation.proposed.json`) records independent counts, boundaries, and SHA-256 identities for the supporting logs and receipts. Full audit artifacts stay in the external evidence root.
