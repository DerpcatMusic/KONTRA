# W8 load checkpoint

Acceptance is **not met**. The shared once-init path and bounded startup are fixed, but the current standalone W8 branch still misses warm v1 first-sound and editor RSS on several fixtures. Authored picture residency (W3), runtime storage (W9), combined W3/W5/W9 scanner acceptance and shared-function lowering remain open. Big Screen embedded-program audio regresses on W8 and blocks acceptance.

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
- Scanner extension source9dcf05e is retained exactly, with a small once-init observation checkpoint adaptation. Frozen baseline binaries and identical note plans are required; previous scanner rows are invalid for these claims. The matched14-row shared scanner shard is complete; detailed results follow below.

## Equal page-cache interleaved check

The 205/3f/4c probe binaries were frozen before subsequent edits. Six rounds rotate205→3f→4c,3f→4c→205,4c→205→3f. `vmtouch` was unavailable. Every invocation first reads all515 Conflux library files,1,831,135,278 bytes, through the same8 MiB buffer; every probe has its own empty v2 numeric cache. No cache hit occurs in the3f/4c cold observations. One bounded heavy invocation per round releases the slot between rounds.

| Frozen product revision | First sound ms, rounds1–6 | All6 median ms | Last3 median ms |
|---|---|---:|---:|
|205b506e|834.3, 272.2, 183.0, 186.8, 183.7, 361.6|229.5|186.8|
|3f01b470|1655.6, 411.4, 179.0, 182.1, 176.1, 292.9|237.5|182.1|
|4c7ea505|1380.9, 179.3, 178.5, 174.4, 175.8, 498.8|178.9|175.8|

| Stage, all6 median ms |205|3f|4c|
|---|---:|---:|---:|
|translate_resolve_ir|56.52|59.27|42.85|
|sample_source_resolve|4.82|3.27|1.68|
|sample_headers_latency_probe|8.53|7.25|4.17|
|ksp_callback_lower|41.15|42.00|32.05|
|lower_plan_bind|11.24|11.28|7.81|
|runtime_alloc_init|15.91|16.43|12.74|

The full-library pre-read itself ranged125.7–6007.0 ms. Slow initial rounds inflate unchanged translation and callback compilation as well as I/O. In all6 medians,3f is8.0 ms slower than205; its changed source/header stages improve4.82→3.27 and8.53→7.25 ms. The differences are primarily translation56.52→59.27, callbacklower41.15→42.00 and runtime15.91→16.43 ms plus uninstrumented remainder. Last3 medians are186.8/182.1/175.8; no cache/header-stage regression is established. This does not prove the shared machine is contention-free. The final4c median178.9 is50.6 ms lower than205; the earlier isolated333→1396 numbers cannot serve as a causal before/after comparison.

## Final standalone W8 cold/warm stage probes

These14 after rows use frozen `4c7ea505` probe SHA-256 `ab806ef3e6124b1dd5b0cf7cae81c9bf4189271279eed8e045ed6d1a6dbd7ad1`. Kontakt cold loads have an empty v2 numeric header cache; warm is the second load from that cache, with every Kontakt header-cache hit observed. UVI does not use this Kontakt cache; those second-load rows are OS/process repeats, not an implemented UVI metadata-cache claim. V1 cold disables existing preset/header caches; warm reads its preexisting matching caches read-only. Numeric-only policy prevents writing v1 decrypted preset caches.

| Fixture | v2 cold first ms | v1 cold first ms | v2 warm first ms | v1 warm first ms | v2 cold/warm editor MiB | v1 cold/warm editor MiB |
|---|---:|---:|---:|---:|---|---|
|Conflux|283.5|3286.6|180.1|111.0|193.8/191.8|114.8/114.2|
|Areia-FullEns|2529.0|5504.6|1332.7|2880.5|444.3/442.5|910.0/913.7|
|Dolce-Vln1|1164.7|1441.2|668.8|532.3|320.7/349.2|523.0/532.4|
|Barbarian|539.1|1646.0|162.9|72.3|168.1/167.3|312.0/312.8|
|UnaCorda-Felt|3127.5|1233.0|147.4|41.7|163.4/162.4|202.3/201.7|
|AnalogStrings|8913.8|6666.6|1444.8|1917.4|1063.5/1060.8|1059.9/1046.6|
|Multi-ConfluxBigScreen|—|—|—|—|95.9/96.1|43.8/44.2|
|UVI-AugOrch|2774.8|—|8017.2|—|245.8/246.1|—/—|
|Morphology|239.2|297.1|202.9|450.2|171.1/170.7|140.5/141.2|
|Vista-Harp|436.6|220.6|110.3|36.2|114.4/113.6|98.0/95.7|
|Solo-Violin|877.5|1116.2|406.2|426.3|209.4/208.3|805.2/804.7|
|Pacific-Cellos|591.0|298.3|119.4|70.4|135.4/134.8|177.3/175.0|
|VWinds-Clarinet|1517.6|—|1553.2|—|159.9/159.8|—/—|
|VWinds-Flute|14301.8|—|4957.6|—|171.0/170.7|—/—|

Conflux cold283.5 vs3286.6 ms improves; warm180.1 vs111.0 ms still fails. Editor193.8/191.8 vs114.8/114.2 MiB still fails. Cold first sound regresses on Una Corda, Analog Strings, Vista and Pacific in this pass; therefore the every-preset target is not met. V1 UVI onset is unknown in these stage probes because0cb7a8a0 has no UVI; its shared sidecar comparison below proves admission/audio/RSS but the requested onset columns remain pending. Big Screen here is controller program0 only; the audible embedded-program failure is recorded separately.

## Shared scanner: identical notes, Original, all14 fixtures

Baseline v2 SHA-25618fe63fe…, pinned Kontakt v1a4b3f8c7…, UVI v1d565661a…, standalone W8 optimized adapteref9d3af7… from4c7ea505. All use the frozen installed shared Python driver SHA-2567364721f… including the sidecar note-origin correction. W8 initially copied the9dc drivercd472de4…; its only difference was the sidecar origin export. The common installed driver was then copied unchanged and regenerated/revalidated W8 cached rows (14reused); worker measurements and note signatures are unchanged. No private corpus collector was introduced.

| Fixture | Baseline v2 load ms | W8 load ms | v1 load ms | v2 before→W8 peak MiB | v1 peak MiB | Audio before/W8/v1 |
|---|---:|---:|---:|---|---:|---|
|Conflux|5177.0|214.9|125.8|230.2→179.8|70.7|yes/yes/yes|
|Areia-FullEns|11199.3|2641.3|4692.6|778.0→485.2|306.7|yes/yes/yes|
|Dolce-Vln1|11090.0|1141.0|1724.9|422.5→348.2|222.2|yes/yes/yes|
|Barbarian|2250.6|389.9|124.4|195.0→117.2|67.6|silent/silent/silent|
|UnaCorda-Felt|1197.3|286.2|109.2|175.3→159.8|80.4|yes/yes/yes|
|AnalogStrings|2792.2|2193.8|2733.0|1276.9→1081.3|561.0|yes/yes/yes|
|Multi-ConfluxBigScreen|5637.3|337.9|222.9|251.1→222.3|80.3|yes/silent/yes|
|UVI-AugOrch|8447.3|2158.7|0.0|212.1→209.4|956.1|yes/yes/no|
|Morphology|391.6|173.2|284.0|173.9→169.5|111.1|yes/yes/yes|
|Vista-Harp|406.2|223.6|51.5|114.8→105.2|50.2|yes/yes/yes|
|Solo-Violin|1237.7|902.5|976.1|301.8→221.9|124.0|yes/yes/yes|
|Pacific-Cellos|1216.6|227.5|75.1|149.8→122.1|54.7|yes/yes/yes|
|VWinds-Clarinet|498.8|574.8|1550.4|140.7→144.1|420.2|yes/yes/yes|
|VWinds-Flute|1160.7|5302.9|3303.8|146.2→152.4|548.8|yes/yes/yes|

All14 baseline and W8 loads are admitted. v1 rejects the selected Augmented Orchestra program, so its0ms failed-load field is not a fast load or first-sound result. Barbarian has no mapped audition note and is `not-auditioned`, not a confirmed audio regression. Other admitted audible rows match their shared note plans. W8 improves load_ms on12/14 vs frozen v2, but Clarinet A and Alto Flute regress in this pass. Peak RSS improves12/14 vs frozen v2; Clarinet A and Alto Flute also increase. It remains worse than pinned v1 on all11 Kontakt rows. Scanner peak is a whole-worker metric and cannot be mixed with plugin settled-editor RSS or summed with W3/W9 independent savings.

## Memory attribution and newly found audio failure

| Fixture/build | Sample/stream residency MiB at recorded phase | Authored decoded picture bytes | Whole-worker peak MiB |
|---|---:|---:|---:|
|AnalogStrings/scan-before|242.29|400395208, 14040, 14040|1276.89|
|AnalogStrings/scan-after|24.00|400395208, 14040, 14040|1081.34|
|AnalogStrings/scan-v1|0.00|unknown|561.02|
|VWinds-Clarinet/scan-before|24.96|0|140.65|
|VWinds-Clarinet/scan-after|24.00|0|144.11|
|VWinds-Clarinet/scan-v1|208.78|unknown|420.25|

Picture numbers are per rendered view asset metadata, not an independent RSS delta; overlapping shared pictures cannot be summed into process residency. Clarinet A remains lower than v1 in the matched worker (W8 peak144.11 vs420.25 MiB), with about24–25 MiB streaming residency vs208.8 MiB v1 samples. Analog authored picture decode remains W3-owned; W8 does not claim its381.8 MiB strip problem fixed.

Big Screen has two embedded programs. Controller0 is silent in all versions. Program1 is audible at[60,64] in baseline v2 and v1 but silent in W8; all three use `matched-note-plan`. W8 compile/init succeed, runtime fault records are empty and underruns0. W8 streamed residency is25 MiB vs36.9 MiB baseline; absence of streamed data alone does not establish the cause. A repeat reproduces silence. Fixed-C4 velocity100 stage probes also reproduce it at both205 and4c, narrowing the fault to the earlier once-init/preload checkpoint rather than the numeric cache. This blocks acceptance; investigation continues.

## Callback lowering profile and ownership

Opt-in `KONTRA_AUDIT_LOWER` records numeric context, emitted instruction count and duration only. Conflux301 Plan/UI callback programs emit1,366,942 instructions in31.0 ms;3note programs3318ops/0.088ms,3release1579/.036ms,2controller329/.009ms. `lower::Unit::program` appends reached user functions separately for each callback. Pinned v1 compiles each function unit once into one script code vector, uses `Program.functions` entry offsets, and `Op::Call(f)` jumps through that table (`src/ksp/compile.rs:595`,724; `vm.rs:770`).

Coordinator assigned shared-function lowering to W5 coordination. W8 sent direct evidence and a request for W5 ownership; no competing shared-function rewrite was started. Caller context, callback_type, UI identity, program indices and saved state must remain correct. The current compiler bakes some caller context into function bodies, so blind reuse of one UI callback code is invalid. W8 only removes redundant name/text clones and non-control double folding, plus the opt-in probe; full KSP tests and rootno-run pass. No callbacks are dropped, and no compilation is moved onto audio.

## Big Screen native-init isolation (temporary probe only)

The identical single-real-NCKP initializer produces274 engine writes,49 zero values and44 zero effect-slot values (zero bus-volume writes0). Four controlled runs keep the same scripts, note,24 MiB streaming pool and8 MiB lazy head policy:

| Controlled native-IR change | First audio ms | Peak audio | Outcome |
|---|---:|---:|---|
| None |—|0|silent|
| Omit harvested init writes |372.0|0.02729|audible|
| Disable dynamic group rack admission only |—|0|silent|
| Both controls |399.4|0.02729|audible|

This causally identifies application of the harvested init writes as the muting path, rather than preload/onset handling. It does not yet identify the exact bad parameter. The temporary bypass controls have been removed and were never pushed as a product fix; omitting valid init writes would break authored behavior.

The old W8 evaluator reads `get_engine_par` only from writes made during the current init, returning0 otherwise. W5's current Environment has `engine_values` and `engine_lookups`, but its Kontakt load-side environment currently initializes both empty. Saved native-state readback is therefore a concrete candidate. W8 notified W5 and requested a saved-native parameter seeding/query API; W8 owns the load ordering/field population and will initialize once at the correct backed-state boundary. W5 owns engine semantics. Shared-function lowering is coordinated separately with W5. Combined W3/W5/W8 acceptance remains open.
