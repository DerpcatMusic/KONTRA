# Vista block-size failure at frozen 64919587

Holding note-end retirement restores Vista's missing final-note replay at 64 frames. Restoring authored bound envelopes does not change the failing result. This is a causal diagnostic of the lifecycle seam, not a production fix or CPU acceptance.

## Checkpoint and plan

Product base: `6491958793f2d9690e2e21bb006f5817f946afa2`, the settled Mix coefficient hoist. The diagnostic branch `v2/diagnose-vista-649` adds only the existing ignored host witness from `91056971` (cherry-picked as `e213b4e6`), numeric observations and trace finalization. No importer, KSP, envelope, source, FX or native Stereo kernel changes.

Public instrument: `/mnt/MAIN_STORAGE/Libraries/Kontakt/Performance Samples Vista/Instruments/Vista - 5 Violins.nki`. The probe follows `examples/cpu_audit.rs` and `tools/cpu-audit-common.rs`: default V2Loader.prepare at 48000 Hz, one installed part, live BlockInfo defaults, no thread override, 1000 unpaced idle blocks, then 192000 paced audio frames. At audition frame 0: CC1=110, CC11=127, CC64=127 and notes 48..59 at velocity100. Note-offs at48000; pedal-off at144000. Each host block renders chunks of at most128 frames, then calls end_block. Environment: XDG_CACHE_HOME=/dev/null, KONTRA_DISABLE_NETWORK=1.

These are warm debug observations with the opt-in shared signal graph. The trace uses scalar rendering and materially increases elapsed time. No cold eviction was performed. Numeric state and typed IR descriptions are recorded; authored script source, saved payloads and PCM are never exported. Direct sample readiness probes run after audition.

## Results

| Host frames | Intervention | Host output peak | Peak voices | Blocks with voices in .25–1s | Key59 replay |
|---:|---|---:|---:|---:|---|
|32|Baseline|0.0023953146301209927|64|0|Absent|
|64|Baseline|0.002848012838512659|64|0|Absent|
|64|Restore authored bound envelope stages|0.002848012838512659|64|0|Absent|
|256|Baseline|0.10827232152223587|68|141|Two at640frames|
|64|Hold note-end retirement|0.10827232152223587|80|562|Two at576frames|

All rows have zero underruns, refused starts and voice drops. The failing 32/64 rows accept64 regions, and their last live voices end at audition frames9056/9280. The 256 baseline and held64 row accept80 regions; voices remain until150784/151104. Both successful rows replay key59 at velocities17/127 and127/127, with eight accepted regions each.

The 64-frame baseline and full-envelope counterfactual have exactly equal output peaks and selection results. The counterfactual writes authored attack/hold/decay/sustain/release through the shared native parameter service after idle and before notes, using this frozen checkpoint's parameter laws. Rendered voice envelopes have sustain1; they are not the zero-sustain bound envelopes observed in Dolce. W5 separately reports that Vista getter ablation changes zero envelope setters and has zero prior-zero setters. Its authored seed therefore has no demonstrated Vista repair mechanism.

Baseline32, baseline64, envelope-restored64 and held64 each report36 callback outcomes, all Finished. No Fault, Cancelled or FuelExhausted outcome occurs. The baseline maximum preemption duration is34frames at32 and66frames at64. A callback failure or one-second yielded-callback watchdog is not supported by these observations.

The held64 counterfactual still drains callback outcomes before the block end, using the same acceptance and Fault accounting as the host. It skips only V2Core.end_block during audition; all1000 idle end_block calls remain. At this checkpoint that leaves runtime.flush_ended and removal of host held-note records undrained. This change alone restores the missing replay and sustained output. It localizes the failure to retirement/key-held lifetime; it does not identify which ownership transition must change. W7 owns that shared lifecycle fix. Permanently suppressing end_block is not a proposed fix.

## Artifacts and validation

Reports and passing targeted logs: `~/.cache/kontakto-w6/vista-649/{outcomes32,outcomes64,restore64,detail256,hold64}` and their adjacent `.log` files. Signal graphs are complete with zero dropped records:26964,20822,20834,243548 and240773 records respectively. Block timestamps in witness.json are audition-relative; selection and graph timestamps include1000 idle blocks.

Frozen diagnostic executable: `~/.cache/kontakto-w6/bin/vista-649-probe`, SHA256 `81ca0c26c26f3c0c2ed7b9d36d4764e70979bca37ca9bcd9b0167ab59a00530d`. Invoke its ignored `sound::v2::dolce_diagnostic::vista_signal_graph` test with an explicit KONTRA_VISTA_PATH, KONTRA_VISTA_BLOCK and fresh KONTRA_REPORT_DIR. KONTRA_VISTA_HOLD_NOTE_END=1 enables the retirement counterfactual; KONTRA_DOLCE_RESTORE_ENVELOPE=1 enables the independent envelope counterfactual. KONTRA_SIGNAL_TRACE=1 is required for graph evidence.

The library area cargo test --no-run passes in `hold-build.log`; each reported targeted witness passes. The trace-disabled original CPU example independently reproduces the64-frame loss of steady voices (peak0.0033577466, peak64voices). Trace-enabled original example also reproduces it. No shared scanner was rebuilt or collected.

W9's original paired649→3a288897 cells remain unscorable at32/64 because steady voices disappear. Its256 cold cell has141 steady blocks but approximately14ms p50 and22ms p99 before retirement optimization, already beyond its deadline. These diagnostic runs provide no CPU, heap, cold-load, native-level or RR-parity acceptance. After W7 repairs the shared lifecycle, rerun the unchanged original CPU matrix with functional output checks before accepting performance.
