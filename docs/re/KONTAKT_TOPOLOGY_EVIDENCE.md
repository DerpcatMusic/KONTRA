# Kontakt 8 topology evidence, 2026-10-13

This is a read-only static investigation of the real Windows executable, not a
Kontakt performance measurement or an executed full-engine reconstruction.
The implementation/design handoff is [KONTAKT_TOPOLOGY.md](../architecture-v2/KONTAKT_TOPOLOGY.md).

## Identity and recovered cache

- Target: `/mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe`.
- SHA-256 checked with Python `hashlib.file_digest`:
  `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
- Radare2 identifies PE32+, x86-64, image base `0x140000000`, file size
  178,386,944 bytes; PE build timestamp September 9, 2026.
- The requested `t3code-80fe786b` worktree no longer exists. Its retained cache is
  `/home/derpcat/.cache/kontra-recovery/20261012/ignored-evidence/t3code-80fe786b-66815b1c/artifacts/engine-analysis-2026-10-07`.
- Consulted `catalog.json`, `kontakt-engine/vtable-methods.csv`,
  `kontakt-engine/complete-export/decompilation.tsv`, and bounded `rg -n -A/-B`
  searches of `kontakt-engine/ghidra/dsp-pseudocode.c`. The full
  `complete-export/pseudocode.c` is **absent** in that recovery directory.
  Its ledger's `completed` status alone is not evidence that its algorithm was read.
  No whole pseudocode file was loaded. No decompiled implementation was copied.
- `catalog.json` indexes documented modules; it does not recover a render graph.
  RTTI and vtable rows identify types/entry points, not method semantics by themselves.

## Observations and confidence

Addresses below are preferred-image virtual addresses, not runtime ASLR addresses.
`G` means retained Ghidra excerpt read; `R` means focused radare2 read this session;
`T` means cache type/vtable identity only. Keep those distinctions when implementing.

| ID | Address/function and identity | What the evidence actually establishes |
|---|---|---|
| B1 | `BProgram` vtables `0x144e98338`, `0x144e98350`; entries `0x14081e000`, `0x140d0a2c0` (T) | Separate program object and serialization interface. The latter entry's complete-export row is at C line 4,731,753; body was unavailable. Does not establish audio fader placement. |
| B2 | `BGroup` vtables `0x144e97a98`, `0x144e97ab0`; entries `0x14081dc90`, `0x140d07580` (T) | Group object distinct from program; source/group serialization has separate identity. Use manual/source readers for field meanings. |
| B3 | `BZone` vtables `0x144e97548`, `0x144e97560`; `0x14081e230`, `0x140d06a30` (T) | Zone object distinct from group and sample; mapping object identity, not a DSP instance. |
| B4 | `BSample` vtable `0x144e94b88`, `0x14081e080`; `BSamplePoolItem` vtable `0x144e97bd8`, `0x14081e0c0` (T) | Sample and pool item have separate identities. RTTI also names `BSamplePool::AttachSample(BZone&, BSamplePoolItem*, SyncedThreadingJobManager*)`. Ownership/sharing exists; exact reference/purge policy unverified. |
| B5 | `BInsertBus` vtables `0x144e980a8`, `0x144e980c0`; audio candidate `0x140824910` (R), helper `0x140823d70` (R) | Candidate reads channel buffers at `+0xb0`, channel count `+0x20`, swap flag `+0x2d`, invert flag `+0x2c`, output selector `+0x34`. It swaps/inverts before later balance/output work, calls the helper separately with channel flags 0/1, then forwards to the selected output accumulator. Cache ledger confirms candidate (C line 3,409,112, warnings). Helper arguments from decompilation require ABI checking before arithmetic implementation. |
| B6 | `BGroupOutputRouting<1..16>`; `<2>` entry `0x14094a8e0`, shared entries `0x140958a40`, `0x140878fd0` (T) | Distinct channel-count routing implementations exist. Not proof of an exact channel enum or stereo-only topology. |
| B7 | `BEffectBase` slots 25/26/27: `0x140abd780`, `0x140a93ae0`, `0x140a94970` (G+R) | Setter writes target at `+0x1a4`; reset copies target fields `+0x1a4/+0x1a8` to current fields `+0x1ac/+0x1b0`; state-copy method copies current fields. Targets and histories are separate. Ghidra C lines 68,392 / 64,480 / 64,491. No universal smoothing time follows from these setters. |
| B8 | Common FX mixing helper `0x140ac28f0` (R) | Checks flag `+0x1b5`, target/current level against the threshold below, and input-buffer dimensions; adds/scales channel buffers, with constant and changing-level paths. Demonstrates guarded work and separate current/target values. Which high-level bypass state maps to every flag remains unverified. |
| B9 | Legacy delay `BEffectDelay` slot 23 `0x140aa1130`, slot 21 `0x14087f770` (G+R) | Ring writes/reads, absolute-magnitude tests, tracked positions `+0x2cc/+0x2d0`, counter `+0x30c`; the query returns `counter > 0`. This is a concrete tail/activity query, not a fixed elapsed-duration budget. Ghidra C lines 66,596 and 29,490. Counter initialization/full sleep caller not recovered. |
| B10 | FX activity predicate `0x140a98850`; thunk `0x14087f7d0` (G+R) | If flag `+0x1b7` is clear, rejects empty buffer dimensions and searches records for a positive activity field; otherwise returns true. This supports buffer-activity gating. Record semantics and owning scheduler are inferred, not executed. Ghidra thunk excerpt starts C line 29,500. |
| B11 | Replika outer wrapper `0x1408f98d0` (G+R) | Persistent countdown `+0x1bc`, reset to `0x20`; enabled parameter pointers are clamped and passed to core update, process spans decrement countdown. This is a **32-frame FX update quantum**, not host callback size. Ghidra excerpt C line 38,080; complete-export line 3,583,586. Existing original-byte checks: [DSP laws, Modern wrapper control cadence](../architecture-v2/KONTAKT_DSP_LAWS.md#modern-wrapper-control-cadence). |
| B12 | One-band EQ `0x140904770` → `0x140918420`; AHDSR `0x140ae46d0`, `0x140ae40f0`, `0x140ae5820`, `0x140ae5e40` (prior executed evidence) | Existing checked laws distinguish a persistent EQ countdown and updates after partial calls from envelope control ticks. AHDSR rate/sample-rate relationship remains explicitly open. See [DSP laws](../architecture-v2/KONTAKT_DSP_LAWS.md). These were not re-executed here. |
| B13 | `BVoiceLimit` vtable `0x144725438`; reader/writer `0x140d049b0`, `0x140d13370` (T) | Distinct voice-limit object/serialization; algorithm/victim ordering not recovered here. `BDoubleChainedArray<BVoice>::ReAlloc(...SyncedThreadingJobManager*)` and `BDsp::BGroupReminder` occur in RTTI. They do not prove a specific stealing policy or O(1) scheduler. |
| B14 | `BEffectSends` slots 26/27 `0x1408223e0`, `0x140834fc0` (R), vtable `0x144e97ed8` (T) | Reset/copy extends base current-value reset/copy with four paired send values and further fields. Separate send targets/history exist. Tap placement and send return routing come from the manual and reader, not these methods. |
| B15 | `BEngine` slot 2 `0x140883a00` (R), slot 3 `0x14083acf0` (R) | Traverses a linked list using `+0x8d4/+0x8dc` match fields and `+0x18` next link; related entry changes a status and counter. Likely voice/event lookup/lifecycle, **not identified as render**. Slots 5/15 (`0x1408ee6b0`, `0x1408ea1d0`) were inspected too; their 64/128 bounds must not be mistaken for audio-block constants. |
| B16 | `BParSrcMode` vtable `0x144e97640`, `0x14081dfb0` (T); `BEnvelopeAHDSR` `0x144728f40`; `BContMod` `0x144e93168` (T) | Source configuration and modulation objects are separate from voice/program objects. `BContModTargets<T,1..16>` specializations support variable target fanout; they do not by themselves establish evaluation rate/instance ownership. |
| B17 | `MultiScriptUIModule` entry `0x1405df9e0` (T) | A separate Multi-script UI type exists alongside instrument ScriptModule. This is UI identity only; raw-MIDI callback behavior comes from D10, and dispatcher/thread scheduling remains open. |

Correction rule: treat B1–B4/B6/B13/B16 as type evidence, never as a recovered
algorithm. Documentation-backed mismatches can be implemented with independent
tests, but unresolved rate/steal/bypass laws require the additional probes in the design.

## Constants checked directly

`pxj 4` with radare2 on this exact executable gives:

| VA | Bytes (little endian) | Interpretation / use |
|---|---|---|
| `0x144f385c0` | `00 00 80 37` | binary32 `2^-16 = 0.0000152587890625`; about −96.33 dB for a unit-amplitude sample. Used by the inspected delay activity and common level branch. **Not** a universal Kontakt silence threshold. |
| `0x144714b90` | `ff ff ff 7f` | `0x7fffffff`, clears a float's sign bit for magnitude comparison in the delay path. |
| `0x14470bad0` | `00 00 00 3f` | binary32 0.5; observed in sample-rate setup code. Does **not** establish sample-rate/32 envelope cadence. |

## Reproduction without full executable analysis

Use the native CLI already available (`r2`/`r2pipe`), not `aaa` on the whole image:

```bash
r2 -q -e scr.color=false -e bin.relocs.apply=true \
  -c 'af @ 0x1408f98d0; pdg @ 0x1408f98d0; q' \
  /mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe
r2 -q -e scr.color=false -e bin.relocs.apply=true \
  -c 'pd 8 @ 0x14087f770; pxj 4 @ 0x144f385c0; q' \
  /mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe
```

For another entry, substitute its address and compare instructions to the cache
excerpt and matching TSV row. Ghidra warnings, unresolved indirect calls, and
radare2's imperfect inferred prototypes are material limitations. Keep the PE
hash with every new observation. Scratch outputs were temporary and are not part
of this repository; all findings needed for handoff are recorded above.

## Unresolved native mechanisms

Full audio dispatcher → group/voice kernels, global modulation quantum,
KSP audio-thread affinity and wait quantization, DFD worker entry/queue/preload
default bytes, allocator victim ordering (especially `Pref.Rel`), and bypass
history reset/resume are not recovered. Vendor manuals establish public behavior,
not those implementation details. Do not turn their absence into invented facts.
