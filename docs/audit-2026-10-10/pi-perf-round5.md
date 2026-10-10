# Performance admission ROUND5: executable own-input plan, not acceptance

## Disposition

Base **366515c96b583747d1bd77c881b1f6909e86cf0d**, tree **a3a49bd4427b9b6941ebdaf14948bfc844197365**. Exclusive checkout `/home/derpcat/.t3/worktrees/KONTAKTO/pi-perf-acceptance-round5`, branch `pi/perf-acceptance-round5`. Only evidence documentation and an input-byte generator are added. No engine, setter, loader, collector or admission-policy patch.

The missing-current-release prerequisite is resolved for **OWN artifacts only**. Read-only `live_host.artifact_receipt` and the extra CPU release/source checks pass for the copied ROUND4 artifacts. This does not turn their build receipt into comparative admission. Frozen v1 profile/source and Kontakt CPU/RAM remain **UNKNOWN**. The both-reference CPU AND RAM goal is **UNACHIEVED**.

`pi-perf-round5-evidence.json` contains exact paths, sizes and SHA256s, the bounded absent-path inventory, native receipt checks and authored input hashes. No host/CLI/plugin/audio/Rust execution was performed. The only generated PCM is our integer waveform **input**, not exported audio.

## Frozen v1: precise custody result and next action

- `~/.cache/kontra-v1/SHA256SUMS`: `ccc7b9f464ae99a217550efba0d4b29fb0f2fd767d35368bb1b08ccde32f4fbc`; all seven entries verified read-only. Inventory contains seven binaries plus README and SHA256SUMS, no build log/receipt.
- Frozen CLAP and `/home/derpcat/.cache/kontra-plugin-backup/v1-20261005/clap/KONTRA.clap` are both 43,168,144 bytes, SHA256 `20ff6b471069d6891d2847e72a4f863db5b50ce24265fda82d083b713b3496de`. Backup has only that CLAP file. Byte identity is established, build lineage is not.
- The historical registered shipping checkout `/mnt/Windows11/DEV_PROJECTS/Artifacts/KONTAKTO-20261005-e50b9147-build-cache/shipping-checkpoint-0.3.152-d097c36` and its entire parent build-cache directory are absent. Git's worktree inventory still lists detached `d097c3630508957e96177a783d38b79f8d009aae` with `prunable gitdir file points to non-existent location`. This is stale registration, **not** a recovered build receipt. Do not prune it.
- Frozen root/adjacent `BUILD.json` and backup root `BUILD.json` are absent. README SHA256 `136d701b7710e0f87948bb6398d865888982fdf7c155cb3afa6131b1e5034c0c` describes backup/version, scanner source and CLI purpose but supplies no plugin profile, features, cargo command, compiler artifact or hash-to-source linkage.
- `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w9-frozen-clap-cpu-20261009/BUILD.json`, SHA256 `b674c89994965609adf3384fb6fc76a84a3e58dc6f7fffc09b88eba49dcb829a`, binds source `49401fce...`, flags `-O2 ... -pthread` and compiler16.2.1 to **the C++ host**, while only recording the frozen plugin's path/hash and `frozen_v1_rebuilt=false`. Do not assign its source or flags to the CLAP.
- Its `frozen-state-check.json`, SHA256 `64a89013e92cf1e5e9aafd0173c31555119fe0229a51f23a365c4b812f80c290`, says frozen CLI export/keyed state passed with an original generated tone; it attests neither CLAP flags nor workload equivalence.
- `docs/audit-2026-10-08/cpu-evidence/provenance.json`, SHA256 `ddffabdd1dca21649ce8754a7613169323164597845ef457d465dc0d3caff1f9`, calls `0cb7a8a0...` the v1 baseline while separately hashing this backup CLAP; it does not bind that source to the plugin. It also warns initial matrix binaries were not retained byte-for-byte. Historical CPU rows cannot describe 3665.
- Read-only `readelf -n` and `readelf -p .comment` find build ID `124812b720d8b7dce75460d7d9a2c4330f36fc3f`, rustc1.99.0 `(b940084d7 2026-09-28)`, GCC16.2.1 and LLD23.1.1. These are useful archive lookup keys, **not** build-profile/source attestations. The repository's `d097c363...:Cargo.toml` has release ThinLTO/strip policy; policy existence does not prove this binary used it.

Search was bounded to the complete frozen/backup directories, historical registered artifact path, W9 frozen CLAP receipts, existing CPU provenance and the available performance takeover histories. This establishes absence **in those locations**, not absence from every possible archive or an external build system. Native/reference sample payloads were not opened.

**Minimal central baseline recovery (not executed):**

1. Request the original shipping build manifest/log/compiler-artifact record by the exact CLAP SHA256/build ID above, not by filename or version. Recover to a new receipt directory, never overwrite frozen files. It must bind full source/tree, dirty-source status, profile, features, compiler/config/flags and emitted binary hash. A retrospectively invented BUILD.json is not admissible.
2. If that record cannot be recovered, integration can include one **separate source-built v1 baseline** in the next combined manifest. First pin/review the intended v1 source snapshot; `d097c363...` is only a candidate recovered from registration, not proven binary source. Build in a new private checkout/output archive with the same admitted release policy/compiler/config as the v2 comparison. No worker-local build, wrapper override or changes to frozen v1/reference checkout. Do not install it or overwrite a frozen binary.
3. An independently reproduced build that is byte-identical to frozen SHA256, with full input/toolchain custody, supplies new reproducibility evidence for that source/profile. A nonidentical output is only a **source-built baseline**, not the frozen CLAP. Compare it under that label; frozen-v1 performance/profile acceptance stays UNKNOWN unless original custody is recovered. Even this source-baseline pair cannot prove the original both-reference goal.

No reconstruction build is requested merely to repeat an unknown claim. The first authorized useful execution can measure the existing own release artifacts below without any build.

## Artifact-bound own workload

Copied directory `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/pi-integration-round4/release`:

| File | SHA256 |
|---|---|
| `BUILD.json` | `62e3cd9b33ef759a161078c94192ed35433c3e3799c8252ec040a9df8721a59b` |
| `KONTRA.clap` | `acb651b0dd81e56d684df3178bccaf06f53a8aad7439596ba1d74c3ed608009c` |
| `kontakto` | `6a03aafbbd7e7df162f722e90faff7635dca80c54fc058732ea25afeadab36e1` |
| `clap-cpu-host` | `b2b7abfe56f197ed950d7d33eb7d82164d21b6a7061741aa552bce5ad1a3fbdd` |

BUILD binds full source/tree, `profile=release`, compiler artifacts `opt_level=3`, assertions off, the serialized cargo log/config/manifest and matching CPU-host source SHA256 `76c617e404794084ee08ca7a8497c58f1810a684718efd5f2595ce5cf0d16a85`. Custody checks additionally reverify FINAL-VERDICT `624302e22df71c026cdb038de5bef13e3b8d13bf93101acb8bd9448e443cc3ce` and FINAL-CUSTODY `7494d0ca4443f8407039988176765ae791c0cb4bd49a5e5c1d6e36de28c88452`.

Preparation command, safe without runtime authorization:

```sh
cd /home/derpcat/.t3/worktrees/KONTAKTO/pi-perf-acceptance-round5
PYTHONDONTWRITEBYTECODE=1 python3 docs/audit-2026-10-10/pi-perf-round5-prepare-inputs.py
```

It authors only `.cache/pi-perf-round5-inputs/` in this checkout, refuses to replace changed files, and verifies the WAV header/frame bytes and event shape. Two identical preparations passed. These generated files are intentionally **untracked**; the generator and hashes are committed. Preserve this checkout/path for the plan: moving it changes the multi's absolute path and hash and requires re-review.

| Input | Bytes | SHA256 |
|---|---:|---|
| `samples/tone.wav` | 1536044 | `119cafbc0105181507877e7cce21fca9e2715fc461ea59ea4bb4f35c416428f7` |
| `input.kontra-multi` | 336 | `4a61a14d34ed2adcb5e33ec62ab51b198e6e23c4801aae84cdb6440ab209f95f` |
| `events.tsv` | 390 | `21a4a6da474757177cc3232b442e9dbdbfeff2c6a25cf4d6e1401735e3f64860` |

Input: 8-second, stereo PCM16 integer triangle, 48kHz, period200 frames, amplitude±4096. One WAV part/program0, output0/0dB, no aux, channel wildcard. Existing `audit_events('strings')` generates the identical28 events: CC1=110, CC11=127, pedal down and12 notes48–59/velocity100 at frame0, note-offs48000, pedal-up144000. Audition is192000frames/4seconds at48kHz. No existing library files are inputs. No SFZ/UVI/NI container, reader, Wine, official process, installation, protected library or audio export is involved.

**Scope restriction:** `src/sound/v2.rs:3195–3267` at3665 reads WAV into a resident `Pcm` and creates a128-voice plan, not a Kontakt streamer. This is a real CLAP callback/process-RSS witness for the current product's resident-WAV path, **not** a high-asset-count/DFD/UVI/trim workload or matching Kontakt library benchmark.

## Root-reviewed execution recipe — NOT RUN / authorization required

Do not run the historical `cpu-audit-native.py` main: it selects protected library scenarios. Import only its event/check helpers. Do not run generic gate/all14/probe scripts. Reuse **the existing** `live_host.observe`; no collector is added.

Root must grant a new named quiet window, verify existing request/granted JSON owner and freshness (<45min), and set `KONTRA_QUIET_OWNER=1` plus `KONTRA_GATE_REQUIRE_QUIET=1` for the authorized invocation. This lane did not create/renew either flag. `kontakto-heavy` is currently absent from this environment's PATH, so no runnable wrapper is assumed: root must choose its actual admitted execution mechanism or directly own a documented quiet window. The existing Activity observer still samples before/during/after and rejects nonquiet runs; flags alone never prove quiet. End the window after the bounded run, no watcher/server/timer.

Approval unit: first **one block64/unforced-source-cache cell** for admission smoke. If valid, separately approve the bounded matrix: five repeats × blocks32/64/256 × source-cold-before-load/source-read-warmed-before-load =30 fresh plugin processes. Same exact input bytes, host/source/config/duration, main/audio threads, zero profiling/family capture/scheduling diagnostics, and fresh tmpfs native state/logs. Fix root's CPU-affinity/scheduler/machine context for the entire run and record it before/after; no scheduler/system tuning here. Report all repeats/spread and invalid attempts; do not cherry-pick the best timing. Cold/warm order is cold then warm per block/repeat. The names describe **source cache before load**, not callback temperature.

The following is the proposed exact Python body after root grants the window. It is documentation, not an executed driver. `LIMIT=1` is the initial approval unit; root may explicitly change to30 for the matrix. Output location is owned `.cache/pi-perf-round5-measurements`, not any frozen run.

```python
import hashlib, json, os, runpy, subprocess, sys, tempfile, time
from pathlib import Path
ROOT = Path('/home/derpcat/.t3/worktrees/KONTAKTO/pi-perf-acceptance-round5')
SOURCE = '366515c96b583747d1bd77c881b1f6909e86cf0d'
RELEASE = Path('/mnt/Windows11/DEV_WORKSPACE/kontra-runs/pi-integration-round4/release')
INPUT = ROOT / '.cache/pi-perf-round5-inputs'
OUT = ROOT / '.cache/pi-perf-round5-measurements'
OWNER = 'pi-perf-round5-own-synthetic'  # root must grant this exact owner
LIMIT = 1  # root review required before changing to30
sys.path.insert(0, str(ROOT / 'tools/kontra-gate'))
from live_host import artifact_receipt, observe, private_settings, sha
assert os.environ.get('KONTRA_QUIET_OWNER') == '1'
for leaf in ('request', 'granted'):
    flag = Path.home() / ('.cache/kontra-quiet-' + leaf)
    assert json.loads(flag.read_text())['owner'] == OWNER
    assert 0 <= time.time() - flag.stat().st_mtime < 45 * 60
assert not OUT.exists(), 'every approval attempt needs a fresh output root'
os.environ['KONTRA_GATE_REQUIRE_QUIET'] = '1'
# Start from root-reviewed environment; reject inherited instrumentation/readers.
for key in ('KONTRA_UVI_READER', 'KONTAKTO_UVI_READER', 'KONTRA_THREADS',
            'KONTRA_FAMILY_AUDIO', 'KONTRA_HOST_SCHED_DIAGNOSTIC',
            'KONTRA_GATE_ACTIVITY', 'KONTRA_GATE_ITEM_CACHE', 'KONTRA_LOAD_HOST',
            'KONTRA_UVI_AUDIT_SEED', 'KONTRA_SIGNAL_TRACE', 'PROBE_ALLOCS',
            'CPU_AUDIT_READY', 'CPU_AUDIT_FINISHED'):
    assert key not in os.environ, 'unreviewed inherited setting: ' + key
receipt = json.loads((ROOT / 'docs/audit-2026-10-10/pi-perf-round5-evidence.json').read_text())
for row in receipt['files'] + receipt['inputs']:
    assert sha(row['path']) == row['sha256'], 'frozen evidence/input changed'
plugin, cli, host = [RELEASE / n for n in ('KONTRA.clap', 'kontakto', 'clap-cpu-host')]
build = artifact_receipt(plugin, cli, host)
assert build['profile'] == 'release' and build['source_sha'] == SOURCE
assert build['host_source_sha256'] == sha(ROOT / 'vendor/moose-clap/tests/live_performance.cpp')
audit = runpy.run_path(str(ROOT / 'tools/cpu-audit-native.py'), run_name='perf_plan')
plan = audit['audit_events']('strings')
assert (INPUT / 'events.tsv').read_text() == ''.join('\t'.join(map(str, e)) + '\n' for e in plan)
OUT.mkdir()
with tempfile.TemporaryDirectory(prefix='pi-perf5-state-', dir='/dev/shm') as tmp:
    tmp = Path(tmp)
    private_settings(tmp / 'config')  # imported/uvi_imported=true, roots=[]
    env = dict(os.environ, XDG_CONFIG_HOME=str(tmp / 'config'),
               XDG_DATA_HOME=str(tmp / 'data'), XDG_CACHE_HOME='/dev/null',
               KONTRA_DISABLE_NETWORK='1', KONTRA_LOG_DIR=str(tmp / 'logs'),
               KONTRA_REPORT_DIR=str(tmp / 'reports'))
    native = tmp / 'input.state'
    # Exact CLI argv; the generated native state is only an OWN WAV selection.
    subprocess.run([str(cli), 'export-multi-state', str(INPUT / 'input.kontra-multi'),
                    str(native)], env=env, check=True)
    state = native.read_bytes()
    cells = [(64, 'unforced-source-cache', 0)] if LIMIT == 1 else [
        (block, condition, repeat) for repeat in range(5) for block in (32, 64, 256)
        for condition in ('source-cold-before-load', 'source-read-warmed-before-load')]
    assert LIMIT in (1, 30) and len(cells) == LIMIT
    for block, condition, repeat in cells:
        folder = OUT / f'b{block}-{condition}-r{repeat}'
        folder.mkdir()
        cache = None
        if condition == 'source-cold-before-load':
            # Evicts only this owned samples directory; no global cache flush.
            cold = subprocess.check_output([sys.executable, str(ROOT / 'tools/cpu-audit-cold.py'),
                                            str(INPUT / 'samples')])
            (folder / 'cache.json').write_bytes(cold)
            cache = audit['cold_receipt'](folder / 'cache.json')  # requires pages_after=0
        elif condition == 'source-read-warmed-before-load':
            assert hashlib.sha256((INPUT / 'samples/tone.wav').read_bytes()).hexdigest() == receipt['inputs'][0]['sha256']
            # Explicit read, not proof pages remain resident under external pressure.
        cell = observe(host, plugin, state, plan, block, 4, folder, 'v2',
                       load_probe=True, cpu_audit=True, profile=False)
        cell.update(source_sha=SOURCE, artifact_receipt=build,
                    input_receipts=receipt['inputs'], cli_sha256=sha(cli),
                    host_source_sha256=build['host_source_sha256'],
                    driver_sha256=sha(ROOT / 'tools/kontra-gate/live_host.py'),
                    cache_condition=condition, cache=cache, repeat=repeat,
                    scope='OWN resident-WAV warm CLAP; callback-plus-main-output-scan CPU; whole-host RSS; no comparison')
        (folder / 'metrics.json').write_text(json.dumps(cell, indent=2) + '\n')
        assert cell['status'] == 'MEASURED' and cell['load_probe']['complete'], 'do not score invalid cell'
        assert cell['underruns'] == 0 and cell['deadline_misses'] == 0 and cell['wake_deadline_misses'] == 0, 'retain failure; do not claim healthy workload'
        assert cell['cpu_audit']['steady_thread_cpu']['p50_us'] > 0
# Native state/readback/raw plugin logs removed by tmpfs contexts; numeric receipts retained.
```

Internal host argv generated by `live_host.observe:311–312` (3665) is precisely:

```text
/mnt/Windows11/DEV_WORKSPACE/kontra-runs/pi-integration-round4/release/clap-cpu-host
/mnt/Windows11/DEV_WORKSPACE/kontra-runs/pi-integration-round4/release/KONTRA.clap
/dev/shm/kontra-live-<unique>/session.state
<32|64|256> 4
/dev/shm/kontra-live-<unique>/ready
/dev/shm/kontra-live-<unique>/events.tsv 1
/dev/shm/kontra-live-<unique>/readback.state --cpu-audit
```

`<unique>` is existing harness temporary naming, not an unpinned binary/input. State and schedule SHA256s are retained by observe; native state bytes are not persisted. Private Settings selects Single/default rendering (record `render_threads_setting` and unset `KONTRA_THREADS`), master is set to0dB by the host. The log-based ready flag gates both observed RSS and processing; no pre-touched ready flag.

### Metric semantics / omissions

All cited spans below are pinned to3665 and file hashes in the evidence JSON:

- `live_performance.cpp:271–287`: after ready, audit waits `warm > 1000*block`, i.e. the counter reaches **1001 blocks** before MIDI begins. This is a threshold, not a precise elapsed-duration observation: the transition iteration is already measured, readiness detection is asynchronous and pacing can slip. Non-audit mode uses the **4800frames/100ms threshold** after ready. Therefore these cells are **warm callbacks** even when the source was evicted before load. Neither mode measures immediate production cold onset; `first_audio_wall_ms` includes load, readiness polling and host warmup. Audit steady window is frames12000≤at<48000 (250–1000ms after audition starts).
- `:303–323`: actual audio worker thread brackets `p->process` with `CLOCK_THREAD_CPUTIME_ID` and monotonic `Clock::now`. In audit mode the main-output finite/peak scan runs **inside** both brackets. Events are block-start quantized in audit mode. Names `cpu_p50_us`/`cpu_p99_us` denote **wall**, not CPU; `thread_cpu_*`/`steady_thread_cpu` denote thread CPU. Report both, blocks/counts and full per-repeat spread, not a GUI-smoothed number or percent normalized by voices.
- `:322,396–405`: separate callback wall-over-period misses, wake-late-over-period misses, event completeness and nonfinite counts. `perf_view` provides numeric sample-memory/voices/underruns; sample-memory is not RSS. MEASURED alone does not mean zero misses/underruns, hence the recipe's additional health assertions. Attribution `--profile` and context-switch diagnostic runs are excluded.
- `:210–212,358,374–379`: whole **host process** VmRSS before state load (already initialized plugin/state buffer), at diagnostic readiness, and after audition; VmHWM and swap after audition. Includes host/plugin threads and allocator retention, excludes CLI/Python, external DAW and a presented editor. HWM is lifetime peak, not peak during the steady interval. No post-destroy/RSS-retirement claim. `load_probe.complete` must be true. No total-process CPU counter exists in this host: callback thread CPU is not loader/decoder/UI total CPU.
- `:276,336,399–403`: process logical/physical read deltas span audition, exclude load/warmup and state save. Resident WAV has no streamer and cannot expose storage underrun stress or head-budget trim cost. Warm-source explicit read is a preparation label, not persistent residency proof. Cold `pages_after=0` proves only own source file cache at eviction time, not global OS coldness, drive cache or postload sampler-coldness.
- Exact input, native selection/readback identity, all hashes, QUIET activity, scheduler policy/priority, host return0, positive finite audible peak/steady peak, dispatched=planned, complete process RSS and available underrun counters are required. Retain invalid cells as UNKNOWN/failed admission, with their reason. A successful own matrix is operational evidence, not parity or significantly-lower acceptance.

## Control trim, IO overlap and retirement: minimal separate witnesses

The WAV matrix **cannot** trigger streamed trim: `wav(...):3267+` has no stream. Use the existing synthetic/controlled tests, not a protected library or new collector:

- Stream serialization path: `crates/sampler-kontakt/src/stream.rs:745–850` holds `head_mutation` across trim and reload IO/publication. Regression `reload_serializes_publication_with_control_side_trim:1127–1160` pauses an own `PausedHead` reader, proves `try_lock` fails during IO, releases it and checks final head bytes0. Concurrent reload budget, packed replacement accounting, transient failure/trim rescan and wavetable pin tests complement it. ROUND4 `stream-neighbors.log` hash `a307cac12f632d1a1930ed73801a9faf00f4ad28eab040e16aeb4ffdbed61da6` reports11 PASS/1ignored. **Behavior tested, elapsed lock/trim/RSS latency not measured.** Existing try-lock witness does not assert the second trim thread reached the lock before the IO resume.
- Actual product seam: `src/plugin.rs:1842–1860::trim_streams` clones streamed parts, calls `src/sound/mod.rs:238–245::Stream::trim`, then refreshes atomic resident/freed counters. A future centrally owned small synthetic test can reuse PausedHead, signal trim's entry, timestamp before/after the real trim call on the control thread, and record head/resident/freed counters plus whole-process RSS before/after outside audio clocks. Include a bounded barrier and release-on-error. This is a following combined-manifest test proposal, **not authored or run here**. No lock splitting/allocator trimming patch is proposed without this latency evidence and preserved budget/publication constraints.
- Retirement path: `crates/sampler-core/src/ownership.rs:558–604::flush_ended/retire_note_chain` traverses live notes and accepts terminal notifications without repeated capacity scans. Existing `crates/sampler-core/src/tests.rs:32,86::ownership_survives_source_end_children_and_rejected_terminal_delivery/cleanup_does_not_need_queue_space_and_no_source_notes_retry` are correctness witnesses. Control retirement tests are `crates/sampler-core/tests/plans.rs:257,322`. ROUND4 `core-plans.log` SHA256 `976b23652c89f37d16df09f347c8e2b61d58990933474a593f634ab4f79e8761` reports6 PASS, including actual control-thread destruction and shared-PCM retention across plan adoption/retirement. No standalone rerun is requested. Runtime terminal voice/freed counters in the WAV plan may support diagnosis but cannot prove Drop thread, total retained owners or post-destroy RSS. Extend an existing control-thread destruction fixture only if central validation needs an elapsed/Drop-order witness; do not add a second observer.

## Corresponding native evidence and limitations

Equivalent evidence, read-only and rehashed this round: `pi-head-reload-reference.json` SHA256 `ee85cad917a4f69afde8a112ad5a33cad0b2dad0dee608b18517bed7d07bd384`; immutable REA/static `check-results.json` SHA256 `f56987ad0ee1cf1783c0774ea91d503e93852341ebf0d0d23855491f7ac598e2` and `sources.json` SHA256 `4cfbf652b7aca9e475ac9b2bb8386635dcd52cb1930ab76fdfb487fdf7f419cd`. Kontakt standalone8.13.1 SHA256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`, public reader VA `0x140d0d4b0`, serialized DFD receiver word `+0x21b60`. No proprietary code was run/copied. These receipts correspond to preload configuration, **not** native lock, callback, allocator or performance implementation proof.

Authoritative [Kontakt manual DFD Tab](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view#dfd-tab), prior retained full-HTML SHA256 `846a09243ad7d17d8daa92bf5105418501ebb25cd358ba8d09108ffb5d4ae7c4`: DFD keeps small sections in RAM, preload size trades memory for dropouts, and background loading can produce audible artifacts. This supports measuring memory **with** underruns/readiness, not accepting memory reduction alone. Full HTML was not newly fetched here; the retained excerpt package and native JSONs were reverified. No new builtin/UVI feature or native parity claim is made. Native timing remains unavailable under the existing execution constraints: **UNKNOWN**, not PASS.

## Checks and handback

Observed no-host checks:7/7 mocked CPU admission fixtures; `cpu-audit-native.py --check`;7/7 frozen manifest entries; own release plugin/CLI/host/build/source hashes; two immutable native JSON hashes; authored input waveform/header/event assertions and repeat-stable preparation; input-byte writer idempotence and negative changed-byte rejection with original preserved. No new product logic exists to validate. Runtime recipe received compile/AST syntax-only checking, not execution.

**NEXT for root:** review the exact smoke argv/environment/input hashes above, authorize one OWN block64 cell in a new quiet window if appropriate, then review its numeric admission before approving the30-cell matrix. In parallel, recover the original frozen CLAP build record by hash/build ID; otherwise separately label a centrally built source baseline in the next combined manifest. Do not replace frozen outputs or infer plugin lineage from version/scanner source. No comparative verdict is possible until frozen and native prerequisites are actually resolved.
