# KONTRA UVI/Falcon implementation status

Source checkpoint, reviewed 2026-10-05. The user stopped further verification
and requested an immediate commit/push. The installed binary remains `96dd6c8`;
this preview source has not been packaged or installed. This report separates implemented features from complete
native instrument equivalence. The package build-info, manifest and verification
receipt identify the exact binary. Earlier measurements retain their original
revision and fixture scope; they are not new tests of this checkpoint.

## Implemented in source within the documented scope

- Cached library/preset catalog, saved Unified / By player browser view, native
  bank/preset identities, favorites and recent items. Catalog publication avoids
  a fresh foreground discovery pass; selected instrument preparation still runs.
- Optional UVI integration with bounded UFS/program/resource readers and supported
  owned protected-bank access. The supported local reader path does not run or
  activate Workstation. Unsupported layouts/access cases still fail explicitly;
  there is no universal bank/protection support guarantee.
- Worker-owned Lua and DSP integration, shared rack mixing/routing/keyboard,
  processed-boundary persistence, bounded audio transport and off-thread retirement.
- Wrapped grouped Logs, unique cause/items, preserved raw support history, guarded
  genuine local source context and endpoint-first failure identity. Live Info
  validates source/restore/activation ownership, distinguishes absent counts from
  zero, and reports owned PCM plus separately scoped wall/CPU observations.
- Supported authored controls and Vectorized controls, hierarchical menus, exact
  GUI FIFO dispatch receipts and uniformly scaled authored panels.
- Initial key/velocity/source Mapping inspection and global sample-zone selection,
  including duplicate resource paths, group selection and paging.
- Common dry selected-sample preview: actual Kontakt/UVI decoder paths prepare
  mono/stereo PCM off the audio callback; one shared cursor feeds the main route
  through master gain. Source replacement, restore, reset, stop and window lifecycle
  invalidate exact owners. Instrument scripts, RR, tuning, loops and effects are
  bypassed intentionally for this raw one-shot source inspection.
- Scoped corrections for quiet exact host attacks, rejected-zero note ownership,
  documented AfterTouch omni forwarding, terminal worker cleanup and repeated
  modulation setting/coefficient lookup work.

## Partially implemented or partially verified

| Area | What exists | What remains |
| --- | --- | --- |
| Whole-instrument playback | Historical finite/nonzero short renders for 40 owned VWinds programs and selected expressive tapes | Native-equivalent musical behavior, every parameter/route, sustained polyphony and current installed DAW validation |
| Runtime overruns | Selected Flute2/Oboe Air tapes pass after targeted fixes; exact first-fault reporting and terminal cleanup | Universal RequestCapacity resolution; sustainable all-program/hardware deadlines; proof of complete control responsiveness |
| Lua/property ownership | Typed native objects, cooperative callbacks, resource completion and note ownership | Complete host/property inventory, asynchronous callback equivalence, synthetic Part/Synth setters, Pan/MPE/tuning coverage |
| DSP/modeling | Selected native leaf comparisons, shared numeric kernels and scoped admissions | Complete connected modulation/DSP graphs, native owner/write invalidation, modeled airflow/legato/vibrato trajectories and nonunit-depth kernels |
| Instrument UI | Many real authored widgets/assets plus vector controls and source interaction checks | Native visual/behavioral equivalence, yielded callback completion, reduced polling/buffer latency and advanced displays |
| Mapping | Initial serialized zones, ranges, exact source selection and bounded dry preview | Live script-selected zones, RR/microphone classification, native resampler equivalence and all codecs/channel layouts |
| Resource lifetime | Pinned bank identity, optional PCM caching, observed owned PCM and bounded preview payloads | Protected-UFS streaming and aggregate codec scratch/RSS bounds; same-inode undetected file modifications |
| Shared platform | Common browser/rack/transport and some consumed DSP kernels | Public neutral backend API, stable external ABI, independent Kontakt feature isolation and integrations beyond current players |

## Not implemented or not established

- Complete Falcon/Workstation compatibility. Decoding, static admission,
  initialization, finite audio and native equivalence are different stages.
- Full Augmented Orchestra playback: the retained census decoded 620/620 programs
  but admitted 0/620 complete graphs. Starter coverage remains 1/50 scoped render
  coverage. No new admission is granted by the dry-preview change.
- Complete semi-physical-modeling equivalence for VWinds.
- Native XY pointer/orientation/order bindings; genuine WaveView/AudioMeter endpoint
  displays. Registry metadata alone is insufficient.
- Exact native bank UUID-to-cover association. Sidecar artwork publication works,
  but missing native cover identity is unresolved.
- SINE/Koda/SFZ/other-player integration and a stable third-party player ABI as a
  consequence of the UVI work. They require separate real implementations.

## Remaining work, in priority order

1. Reproduce and resolve remaining real instrument/controller failures using exact
   source owners and first-cause diagnostics; verify current binaries in the DAW.
2. Implement measured native modulation preparation/lifecycle and remaining connected
   DSP routes without lowering cadence, bypassing nodes or silently narrowing voices.
3. Establish native musical comparisons for VWinds modeling, legato/vibrato, Pan/MPE
   and asynchronous Lua timing; expand whole-program admission only with evidence.
4. Complete advanced controls/displays, truthful artwork identity, live Mapping
   classification, protected-bank streaming and preview source/layout coverage.
5. Integrate the neutral player interface and independent feature ownership with
   the user's separate core refactor; preserve floating velocity at that boundary.

## Verification for this checkpoint

The pre-layout-correction source passed 48 focused checks. After adding intrinsic
nonshrinking inspector rows and stronger visual bounds assertions, the compile
passed but the compact Mapping test failed: wrapped caption baselines still
exceed their flow box. The final run stopped at that failure (29 passes of 48
selected checks); remaining checks were not rerun. A definite-width caption
correction is privately prepared but unintegrated/unverified. This is a known
source UI defect, not a passing final release checkpoint.

The UVI-disabled pre-layout source compiled and passed 29 common preview checks.
No final-source non-UVI check, production build, new installation or actual-bank
dry-preview test was performed. Further work is paused at the user's request.

See [dry-preview evidence](RAW_SAMPLE_PREVIEW_EVIDENCE.md) for the focused checks,
source API change, authored visual captures and exact payload/resampling limits.
This checkpoint adds no whole-bank census, native-engine oracle, DAW exercise or
throughput measurement. Historical checks are not summed into a new suite count.
