# CPU audit through the frozen v1 CLAP

Build a host, never v1. `tools/cpu-audit-native.py` loads the verified
`~/.cache/kontra-v1/plugin/KONTRA.clap` (0.3.152) through the existing native
CLAP test host. It uses the frozen CLI only to export a native state envelope;
the probe's existing v1-only keyed state author selects the original NKI.
The CLI's export command validates the instrument outside the timed audition;
cold eviction runs after that export.
No v1 source, binary, manifest or lockfile changes are needed. Native state,
plugin logs and any authored text stay in tmpfs; retained diagnostics contain
hashes and numeric evidence. No PCM is saved.

The host's new `--cpu-audit` mode uses the audit's original three libraries,
48 kHz, 32/64/256 frames, 1,000 idle warmup blocks, four-second paced audition,
keys/velocity/CCs/pedals, block-start event quantization and 0.25–1.0 s steady
interval. The floor-index p50/p99 definition matches `cpu-audit-common.rs`.
Timing includes a stereo main-output peak scan. It also reports audio-thread
CPU time, deadlines, streaming I/O, underrun diagnostics and native selection
readback. Silent steady intervals, absent underrun evidence, wrong readback,
incomplete events and machine contention cannot produce a MEASURED receipt.

This measures the **complete loaded CLAP callback**, including rack/adapter
work. Run v2 through the same host for a matched comparison. Do not divide
these results by the old v2 source-seam times, or overwrite the `73e6089b`
UNKNOWN gate with them. That gate needs its own pinned v2 CLAP artifact and
a new logged comparison. Frozen plugin 0.3.152 and source baseline `0cb7a8a0`
are distinct references. Voice-normalized costs and allocation/free counts
are unavailable through this host and remain unclaimed.

## Build and check only the host

Use a cache/run directory you own; the SDK already exists on this machine.

```sh
sdk=/home/derpcat/.cache/kontakto-audit-cpu/clap-sdk
out=/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w9-frozen-clap-cpu-20261009
mkdir -p "$out"
g++ -O2 -std=c++17 -Wall -Wextra -Werror -pedantic \
  -I "$sdk/include" vendor/moose-clap/tests/live_performance.cpp \
  -ldl -pthread -o "$out/clap-cpu-host"
"$out/clap-cpu-host" --self-check
python3 tools/cpu-audit-native.py --check
python3 tools/kontra-gate/check-live-host.py
```

## Run a matched cell

First obtain your own quiet request/grant through the machine queue. Supply
the exact `owner` string from both files; the driver rejects another owner's
window or an expired grant. Run one cell per heavy slot, with fresh output
directories. No measurements from another owner's window are valid.

```sh
owner='YOUR EXACT GRANTED OWNER STRING'
KONTRA_QUIET_OWNER=1 ~/.cache/kontakto-heavy python3 tools/cpu-audit-native.py \
  piano 64 "$out/v1-piano-64-cold" --host "$out/clap-cpu-host" \
  --quiet-owner "$owner" --cold
KONTRA_QUIET_OWNER=1 ~/.cache/kontakto-heavy python3 tools/cpu-audit-native.py \
  piano 64 "$out/v1-piano-64-warm" --host "$out/clap-cpu-host" \
  --quiet-owner "$owner"
KONTRA_QUIET_OWNER=1 ~/.cache/kontakto-heavy python3 tools/cpu-audit-native.py \
  piano 64 "$out/v2-piano-64-cold" --host "$out/clap-cpu-host" \
  --quiet-owner "$owner" --cold --version v2 \
  --plugin /ABSOLUTE/PINNED/KONTRA.clap --cli /ABSOLUTE/MATCHING/kontakto
```

Repeat for `strings` and `fx`, at 32/64/256 frames, alternating engine order
between repeats. Cold evicts only original files in the selected library and
retains a `mincore` receipt. The immediate subsequent unforced-cache run is
the warm repeat; the driver labels it `unforced-source-cache` rather than
claiming every source page is resident. Product parsed-cache writes are
disabled in both versions. Results are in each cell's `metrics.json`:
`cpu_audit.steady.p50_us/p99_us`, `steady_thread_cpu`, `deadline_misses`,
`underruns`, `contention`, hashes, readback and event counts.

For attribution, add `--profile` in a separate fresh run. This records only
audio-TID leaf IPs using `perf cpu-clock:u`, with realtime timestamps and no
stack/PCM capture. `audio.perf.data` excludes loading, warmup and teardown;
filter to `cpu_audit.profile_pace_unix_ns + [0.25, 1.0)` seconds for the
steady-note interval. The driver saves that filtered IP/module metadata as
`audio-steady.perf.txt` and reports `profile_samples_steady`. A failed recorder,
unreadable perf data or empty steady sample set makes the receipt UNKNOWN;
the recorder's normal controlled SIGINT exit is accepted after decoding.
Score CPU parity from **unprofiled** repeats. Frozen v1 is stripped, so its
internal attribution may require module offsets rather than symbol names;
do not rebuild it to obtain symbols.

Checks: host builds with warnings as errors; C++ self-check covers audit
quantiles and the 256-frame note-off offset; Python checks cover all three
original schedules and existing native-state/readiness rejection cases.
The generated-tone smoke test exercises the actual frozen CLAP at all three
block sizes, including native state readback, every MIDI event, steady-window
counts and an audio-TID perf recording at 64 frames. Its receipts are explicitly
UNSCORED-SMOKE; run it outside other owners' quiet windows:

```sh
python3 tools/check-cpu-audit-native.py "$out/clap-cpu-host" "$out/smoke"
```

Scored real-library profiles await W9's next machine slot.

NEXT: run the cold/warm matched CLAP cells in the granted W9 slot.
