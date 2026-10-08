# W8 load checkpoint

Acceptance is **not met**. The shared once-init path and bounded startup are fixed, but the current standalone W8 branch still misses warm v1 first-sound and editor RSS on several fixtures. Authored picture residency (W3), runtime storage (W9), combined scanner acceptance, and callback-lowering work remain open.

## Provenance and protocol

- Product before: `7e82b152` plus numeric probes only, frozen probe SHA-256 `a4572f6b08108616ac024554d5cfbf4ad2eb6d7ecc722bb413d1ee225cc5c493`. All 14 rows were rebuilt/repeated after the original audit binary was pruned; no failed probe row is used.
- Intermediate after: `205b506e`, frozen probe SHA-256 `2b4e11e92bb45c2bc8eb081f0509213b49fd9548854e0183bffc1dcd9d5fa6b9`. These rows predate the v2 numeric header cache and are not the final cold/warm result.
- Pinned Kontakt v1: `0cb7a8a0` with equivalent numeric stage instrumentation on `audit/w8-load-v1-stages-20261008@2f51deec`, frozen probe SHA-256 `761f9c8ebaa0847d281e4d665d7776b9fdb1119a31e542b53840701fac5e7d6f`. UVI requires the shared sidecar, not this older Kontakt-only build.
- Probes use 48 kHz, 64-frame blocks, C4 velocity100, CC1/11=127, first finite audio above1e-7. First sound is wall time from load request. A silent controller multi has no fabricated first-sound time. The shared scanner uses its own frozen identical per-ID note plan and threshold.
- `tools/audit-load.py` runs the probe with library/cache resources read-only in bwrap, volatile temporary storage, and persists only numeric stage/PCM/RSS metadata. For v2 cold/warm the only writable namespace is a private `kontra/v2-headers` numeric cache. OS page cache and shared machine contention are uncontrolled; "cold" means empty product cache, not physically cold disks.

## Root cause and allocation profile

Two early KSP passes ran before loading the real NCKP. Inferred knob bounds0..1,000,000 drove millions of control/menu iterations. Deleting only one pass would retain this cost. Initialization now runs once with authored controls and saved state; the retained result feeds native parameters and host-rate callback lowering. W5 engine-parameter service is untouched.

| Conflux slot0 init environment | Fuel | Allocation calls | Cumulative allocated bytes | Peak live bytes | Instrumented time ms |
|---|---:|---:|---:|---:|---:|
| Empty performance view |40,014,847|16,183,886|537,113,587|82,662,440|5250.8|
| Authored performance view |15,936|91,151|12,132,239|6,849,636|6.7|

Allocation timing includes allocator/builtin instrumentation; compare live allocation and work counts, not its elapsed time with uninstrumented stage totals. Empty-view builtin counts included get_ui_id12,001,095; get_control_par11,000,406; set_control_par_str_arr9,000,544; get_menu_item_value4,000,008. Authored initialization removes the erroneous loops. v1 prepares its performance view before runtime compilation.

## Intermediate 14-preset comparison

| Fixture | Before first sound ms | After first sound ms | Before editor RSS MiB | After editor RSS MiB |
|---|---:|---:|---:|---:|
|Conflux|5747.0|332.9|203.7|192.8|
|Areia-FullEns|12830.5|3652.1|783.5|443.3|
|Dolce-Vln1|11400.5|1874.3|444.2|320.3|
|Barbarian|2341.4|527.2|204.7|167.7|
|UnaCorda-Felt|753.4|426.5|182.4|165.1|
|AnalogStrings|8116.1|2715.9|1239.9|1062.4|
|Multi-ConfluxBigScreen|—|—|99.7|95.7|
|UVI-AugOrch|1833.2|34652.9|249.0|243.6|
|Morphology|566.3|487.9|173.9|171.6|
|Vista-Harp|1092.2|743.3|123.7|113.6|
|Solo-Violin|3479.4|1280.8|304.4|208.5|
|Pacific-Cellos|1274.5|369.8|162.3|133.7|
|VWinds-Clarinet|613.0|631.8|158.8|160.8|
|VWinds-Flute|1030.2|1679.3|166.9|168.8|

These fixed-C4 intermediate runs improve many original v2 rows but do not establish v1 parity. The UVI Augmented Orchestra directory namespace pass took33.3 s in this after run; that path is outside Kontakt source resolution. Clarinet A is a no-regression check: the separate UVI audit measured v2 lower than v1 (140.36 vs418.60 MiB), not the reversed claim.

## Conflux equivalent stages (ms)

| Stage | v1 empty product cache | v1 existing cache, read-only | v2 intermediate333 ms |
|---|---:|---:|---:|
| Import/cache/translation total |1544.7|26.5|57.2¹|
| Source resolution |4.4|12.3|63.7|
| Header/latency admission |1683.9|0.1|71.0|
| KSP frontend/view |32.6|8.2|5.3|
| KSP init |12.2|7.4|1.9|
| Callback lowering |10.6|6.6|42.9|
| v2 retained diagnostic positioning |—|—|0.39|
| First audible note |3286.6|111.0|332.9|
| Editor-open settled RSS MiB |114.8|114.2|192.8|

¹ v2 read2.71+decrypt2.81+chunks0.42+objects1.48+translation49.73; frontend/init/native-DSP/zones are nested inside translation, and must not be added again. v1 source/header stages are inside its initial bank; deferred full sample preload is excluded from first sound. v1 probe load_run_ms includes later preload, so use first_audio_ms for onset. v2 remaining stages include callback assembly46.16, UI metadata1.6, plan lowering10.0, runtime install13.2; uninstrumented/remainder contributes about68 ms.

## Shipped policies and outstanding acceptance

- Source resolution and header startup use bounded workers; indexed archive members reuse positional shared file descriptors. Metadata hits bypass repeated header reads/stat for known archive members.
- The versioned v2 numeric cache stores only file stamps/size, offsets, lengths, rate/frame counts and a keyed flag; keys are reconstructed from current access. It rejects stale/malformed/legacy data and caps stored bytes64 MiB. It never reads v1 cache files.
- Plugin sample preload is lazy with8 MiB of admitted head data; page pool24 MiB. Generic eager APIs preserve their prior policy. A first missing page holds onset for at most50 ms but resumes immediately when data arrives. This is not a mandatory50 ms delay; no claim of better onset than v1 is made until cold/warm measurements pass.
- Header artwork uses async worker preparation,1024×512 downscale and8 MiB bounded cache. Authored strip/native pictures remain W3-owned; their Original fidelity and transient/resident RSS must be measured on the combined build.
- Changes in other streams are limited to `sampler-core/src/source.rs` onset handling (W9), shared positional sample handles (W9 streaming), and a Kontakt destructure in `sampler-native/tests/pedals.rs`. There is no W5 service or W1 publication rewrite.
- Scanner extension source9dcf05e is retained exactly, with a small once-init observation checkpoint adaptation. Frozen baseline binaries and identical note plans are required; previous scanner rows are invalid for these claims. New matched before/after scanner shard is pending.
