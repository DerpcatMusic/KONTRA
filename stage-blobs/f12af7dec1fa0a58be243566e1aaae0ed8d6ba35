# W0 batch 383 integration and release audit

Tested product source: `4725f39a0cb11e6fc1e96b283445cd93a4cecf35`. This batch follows published 0.3.393 (`e53256e17f27d43d27ae17ee411d11fc5b5ccc7b`) and preserves its tree through an ancestry merge. The batch label is not the release version.

The 26 original commits below include implementations, tests, documentation and diagnostic-only changes. The proposed ledger adds ten distinct logical defects, extending the already counted wavetable playback defect without another increment. Browser improvements and diagnostic harnesses add no fix count.

## Validation

The corrected per-worktree target gate passed 2,135 workspace tests with zero failures and 266 ignored, 48 scheduler tests, 45 reader tests (one ignored), and three articulation normalization tests. Workspace/standalone/feature-off compilation, offline publication checks, GPU first-present and 24 damage frames passed. CPU probe self-check passed for correctness; its timings are not acceptance.

Native regression passed: installed Conflux1, Vista2, browser capture1; native scanner15/15 loads+audible with12 Original views retained; quick72/72 loads,68 audible, zero audio-thread allocations and finite audio. No matched stage/audibility changes occurred. Shared-target results from the invalidated window are excluded. The ten-fix ledger is accepted as0.3.403. Release freeze follows this metadata-only commit and binds the final clean source.

Receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w0-stable-383`. `combined-verdict.json` binds every completed gate log to the tested source with SHA-256.

## Delivered source

| Owner branch | Original SHA | Integrated SHA | Change |
|---|---|---|---|
| `v2/w6-conflux-modfx-381` | `61158054b809bb89f650e6659b32798517a33c21` | `c0999b4862374aea2027a3c314184d69398700d0` | Port admitted Kontakt wavetable playback from v1 |
| `v2/w6-conflux-modfx-381` | `484baae4c20f83e7eed9ab7b42bda54aa5b3e816` | `0bc1759ae3aa4785e01116e392fa26c976d6ab47` | fix(kontakt): retain measured zero-delay Digital Multi sine sources |
| `v2/w11-gestures-382` | `839ddd94d2c2d4f231c6016d522a0614556e2f99` | `dd69404549c30ef457826f50b9a9c43633fe8bbe` | test: reject listener-only widget gesture witnesses |
| `v2/w11-gestures-382` | `f7cbdd38800a3ffcee0b13f6897ba7333243750e` | `298acf93e94aa3f54090fbf8bd0bea65528c4d0c` | test: distinguish touched array cells in gesture receipts |
| `v2/w11-gestures-382` | `905a568beab4412d0274ce29b409d51a87ab73fa` | `312280eea86286227c26e093db48b63f88f1948c` | Attribute widget gestures and recall to accepted input owners |
| `v2/w8-load-attribution-resume` | `6885c2907f8f065b01666299950672ba200db8bd` | `f5d75b34c57d79184e3446f00146b13e26e9d7a5` | port from v1 0cb7a8a0:src/ksp/runtime.rs changed-value capture guards |
| `v2/w8-load-attribution-resume` | `7436689bfd0bbd01b494fe7af9f36ccd1d72ffe3` | `23dfcc9e216032531fdeac756dbc1ae4e0b8384d` | fix: capture script persistence directly into compact snapshots |
| `v2/w8-load-attribution-resume` | `323c5ccdf44fbeb7796ea70afdd9408dd3239fde` | `8091ac23bd1c0dba2b91f342745a7f372e82fe78` | fix: refresh coherent script snapshots only for changed cells |
| `v2/w8-load-attribution-resume` | `6d07afacc77598ee04089bad9a9848712c4e96e5` | `4b4f090a2cca1fabd5c6a784800dd2869b265a34` | docs: record completed W8 scanner and onset receipts |
| `v2/w9-v1-whole-voice-381` | `ccdfe714c711c1c1863e820c971645a0c1d9d76d` | `464c2a9d3bc2e33a14e16b16ee41f89b1198578e` | record checked wavetable source exclusion for parked whole voice admission |
| `v2/w13-host-scheduling` | `ff7c79c1f98d03aed986969f306e38c4d86bbae7` | `a9a6b9a0fcf09d55366278605625cb2e51f3f6ca` | Reject scheduler-instrumented callbacks from CPU acceptance before adding diagnostics |
| `v2/w13-host-scheduling` | `d0c89ffba3034565ee1c530464d3065b6e81e3fc` | `6a4bb905fde99bcfee23e5f6ef6eac0d187ecb15` | Record opt-in callback scheduler switches outside CPU acceptance |
| `v2/fix-editor-cycles-ready-344` | `7afed48253ed11f72c7504dc6b0f741d30e19a18` | `14816f3047240a9d5dcae650cbc0050acd87acdd` | Reproduce render-tree retention across close and cross-thread model destruction |
| `v2/fix-editor-cycles-ready-344` | `f96ae541940f012e2bc8e1b148d2aa490822f95b` | `5a07288f54da34bc032ba9b06e8c445f7914172e` | Keep all four render-owner cycle checkpoints in the regression |
| `v2/fix-editor-cycles-ready-344` | `901968c60438adfdeab5eb97078344ce2164d349` | `e7c5de119a75f263ec3f55fb0d7ee44bca734b64` | Release GUI render trees and scene caches at the window close boundary |
| `v2/fix-editor-cycles-ready-344` | `3197d008a945407a404a5ec77f60bb2a6618b85b` | `42f0e5fec8d22209679cc8ad87a90e1c639abfbd` | Check close releases render owners with the model retained and rebuilds on reopen |
| `v2/fix-editor-cycles-ready-344` | `1be3cfa680d0acf10071a4766ca89dd0196d8f04` | `b4990fc119ace96fccbdf268a03156accc6fdd55` | Document verified close and reopen render ownership regression |
| `v2/fix-uvi-scan-344` | `0833711d0b17b13631aa46d2d934f2a0d3a2caa2` | `d86b19618075b35b1e69f2a4feafb36e8c44dca8` | fix: route UVI MIDI attacks and host releases through Lua identity |
| `v2/fix-uvi-scan-344` | `4978ef66c85276f9438ba1d73fdc1ff5710826de` | `a7d77c7ad0f196667e98f5fb4d89ab7747a1b607` | port from v1 4bffbb18:src/library.rs UUID-based UVI favourites |
| `v2/w14-browser-382` | `d34141a2806a6b599e9f6440f97448a4a985d167` | `cf4fdf422c5831c85cd5bfe812ebb47f85561388` | Consolidate browser search and library banners |
| `v2/w15-vista-harp` | `415cb38384444d0a5ea7a251064afbf0270371ab` | `aaa90ffb8d0620c262313c665e1b869bee6e4bf4` | port from v1 0cb7a8a0:src/engine/filter.rs legacy lowpass slots and cutoff span |
| `v2/w15-vista-harp` | `d7bdd7702dd40578fb2353370f0fb8bd44a7eb4e` | `1adf79d0bab8dc1468d7f5f39826094dd928f2c1` | port from v1 0cb7a8a0:src/engine/filter.rs legacy highpass slots and cutoff routes |
| `v2/w14-browser-perf-383` | `cf8e723c170193afd6baac75c8d9ed50f2a1d91d` | `a2c5b4ab09c00914f1fe9817d7aa2e9523022373` | Cache browser catalog order across query updates |
| `v2/w4-keyswitch-382` | `a123ef2271baae25ecd8088edbd342fffbc84dff` | `c7c6cd69b5c2b71badef3d37b4a4d8f3e0f2f5ff` | Use authored key names for existing note-labelled articulations |
| `v2/w4-keyswitch-382` | `26ea80797948c9340dc89bdf5235248b1affb528` | `83629b69fc2e9d1e852aa1f9960ae743d5be3acf` | Reject preset browsers as articulation choice lists |
| `v2/fix-uvi-scan-344` | `4912e581f4bd3197f230de6dd16682b390cc6e3a` | `4725f39a0cb11e6fc1e96b283445cd93a4cecf35` | Opt-in diagnostic-only callback scheduler counters |

## Scope and remaining limits

Native scanner and quick regression compare the same installed Kontakt and clear UVI fixture identities against the preceding frozen batch. Safe-note audibility, load success, script fault regressions and finite audio are checked. They do not certify full authored widget behavior, native sound equivalence, protected installed UVI playback, or real Windows/macOS DAW operation.

The Conflux source admission covers saved wavetable state and 124 of 182 Digital Multi sine sources. The remaining 58 fade cases and live source controls stay open. Vista legacy filters use the reviewed v1 proxy; native topology and control timing remain unverified.

The gesture harness changes establish stricter attribution, not a complete widget axis. W9 whole-voice changes beyond the shipped EQ fix, new W14 performance work, W11 incomplete widget fixes, unapproved allocator trimming and speculative sharing remain outside this cut.

Quiet CPU, load, RSS and underrun acceptance remain unknown for this batch. No plugin installation is performed; shipping 0.3.393 artifacts stay immutable.
