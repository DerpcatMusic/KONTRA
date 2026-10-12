# W10 authored UVI 32-frame callback diagnostic

The single corrected-target diagnostic completed exit 0 under the explicit W9→W10 DIRECT handoff. The canonical observer recorded five QUIET samples. It found **0 deadline misses in 6,000 blocks**, including 1,125 steady blocks. There were 16 script callbacks, zero Lua faults, overruns or underruns, and zero render/event heap calls. This opt-in scheduling diagnostic is **DIAGNOSTIC-NOT-ACCEPTANCE**.

| Phase | Wall p50 µs | Wall p99 µs | Wall max µs |
| --- | ---: | ---: | ---: |
| All | 24.07 | 56.081 | 219.204 |
| Steady | 24.921 | 54.531 | 96.642 |

Earlier retained rows had steady/sustain deadline misses with low thread CPU compared with wall time. That establishes off-CPU delay, but blocking versus preemption remains unresolved. The new context-switch bracket captured no miss events in this cell; zero misses here do not prove the intermittent issue fixed. No repeat attribution run is requested.

## Frozen provenance

- Compiled source: `b34c08e63ee95cb95fae282bdb82ff22bca92b32`; UI tests above diagnostic product `4912e581f4bd3197f230de6dd16682b390cc6e3a`.
- Frozen binary SHA256: `c9d75120a5e47ad604c0f1917a8035ea2b143ea276b330e4186ee89b6fe6ae08`.
- Clear authored fixture SHA256: `e69a5a175509c39d51456786de86b742903619115e518af7a8bf4932f9853650`; member `/preset.uvip`; declared valid keys, 32-frame blocks, 48 kHz, 16 peak voices.
- Observer source `d9ea398f92ca5ba762d1f574a9329c76a0594c78`, SHA256 `d82d5f7825cdbce3d9f485277b512b17bf6057f9c4f535c1edad2f8ed3a15c36`.
- Receipt: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w10-uvi-scan-344/scheduler-20261009/corrected32-once/RECEIPT.json`; raw `activity.jsonl`, `row.json`, worker log and exit digests retained there.

An earlier launch preflight rejected foreign KURV Cargo PID 3195171 with exit 75 before any timing frames ran. Its raw observation is separately preserved in `corrected32-preflight-rejected-KURV/`. The actual diagnostic began only after a new QUIET check. The owner unit finished inactive/MainPID 0 and W10 removed only its own request/grant flags. Queued ordinary jobs remained under wrapper control.

Installed-bank and native-v1 runtime parity remain UNKNOWN/PARKED. This fixture does not open protected payloads or an official reader.

NEXT: finish class userdata parity; retain callback root cause as open until a real miss supplies switch evidence.
