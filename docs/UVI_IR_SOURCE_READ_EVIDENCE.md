# Impulse reconstruction source-read reuse

The rational FIR reconstruction retains its native-measured coefficient generation,
source traversal, double-precision multiply/add order, output rounding and bounds.
Adjacent windows repeatedly read immutable PCM. A per-call, 2048-entry tagged
cache reuses successful decoded source values without retaining any bank, sample,
voice or prepared impulse. Scratch storage is fixed at 32 KiB. Equal-rate copying
returns before cache initialization. Failed reads are never published; the first
read still performs the existing bounds and finite-value checks.

The original native IR fixture and an authored cache-slot-wrap impulse pass.
A private original/candidate matrix compares 144 reconstruction attempts across
eight rate pairs, packed and float storage, one/two channels and 1/2049/8193-frame
inputs: successful output bits and rejected-case errors match. Four effect checks
also pass. The actual Alto Flute corpus retains all six PCM, command, host,
root, completion and state hashes plus resource/event/voice counts.

In an alternating baseline/candidate/candidate/baseline corpus capture, instructions
fall 1.047%, but whole-loop thread CPU does not improve. This is not a sustained
throughput or polyphony claim. The worst Renderer CPU packet changes from
37.63/38.42 ms to 20.63/21.43 ms.

A separate paced baseline-10s/candidate-10s/candidate-20s/baseline-10s comparison
finishes with finite audio, Ready status, no worker errors, endpoint failure or
underrun. Startup packet CPU at frame zero changes from 36.87/37.98 ms to
21.59/25.07 ms; frame 2304 changes from 37.68/36.51 ms to 18.62/18.14 ms.
Backpressure submission attempts are 0/185 for the baseline and 0/0 for the
candidate. These bounded runs do not establish sustainable operation on user
hardware or explain the earlier unreproduced 175 ms wall-time stall.

Both private builds differ only in reconstruction plus identical timing guards;
the newer production Transport beat/playing changes are absent in both.
Unchanged external SDK glue is identified in the private receipts. Current-root
integration needs its own functional checkpoint. Sanitized receipts live at
`~/.cache/kontakto-uvi-alto-realtime-private/resample-perf-assessment-safe.json`
and `resample-paced-assessment-safe.json`; reconstruction boundary checks live at
`~/.cache/kontakto-uvi-render-speed-private/resample-source-read-cache-audit-safe.json`.
No vendor assets, executable code, access state or arithmetic hooks are bundled.
