# v1 runtime capacity port

Base: exact integration 3f49a9a109220c95546596e37204b5b2faa829b7 plus shared-text step 6f51d817dc389e33d92208ea6a12fbc29caa785d. Measured candidate aabc26ba2d0e366ec056df501db7940aa42205ee.

Copied v1 0cb7a8a0:src/engine/mod.rs MAX_VOICES=1024 and src/ksp/runtime.rs EVENT_CAPACITY=4096 into production startup sizing. Notes and expression owners use 4096 slots; families/decisions cannot exceed that event ownership budget, and PerformanceState follows note capacity. Per-voice DSP/modulation capacity follows the smaller voice pool. Existing v2 release-voice reservations and control-thread doubling to eight times the initial voice pool remain. No stream sizing or DSP arithmetic changed.

| Preset | Load RSS text / capacities (MiB) | Editor RSS text / capacities (MiB) | Trimmed RSS text / capacities (MiB) | HWM text / capacities (MiB) |
|---|---:|---:|---:|---:|
| conflux | 254.67 / 192.23 | 330.36 / 266.56 | 285.86 / 224.09 | 329.82 / 265.63 |
| pacific | 219.14 / 165.15 | 241.43 / 187.61 | 234.41 / 180.87 | 242.04 / 188.52 |
| analog | 1047.81 / 1025.27 | 1067.70 / 1055.57 | 1037.97 / 999.34 | 1078.77 / 1056.26 |

| Preset | Exact 3f49 editor RSS | After text + capacities | Total reduction (MiB) |
|---|---:|---:|---:|
| conflux | 338.82 | 266.56 | 72.26 |
| pacific | 240.36 | 187.61 | 52.75 |
| analog | 1086.40 | 1055.57 | 30.83 |

Debug production-path, editor-open measurements; normal heavy work may overlap and these are not quiet timing or release acceptance. Requested capacity saving and RSS differ because untouched pages and allocator reclamation vary. Analog remains above 1 GiB: these cuts do not resolve its entire RSS or load-time gap. Source/library plaintext is not retained in receipts.

Validation: both core allocation-free voice/note growth tests; root lib --no-run; production loader asserts 1024 starting voices/4096 note parameters; ownership-policy test asserts event/expression/family/decision budgets and 8192 voice growth ceiling; all three preset probes. Stream heads remain 0 bytes and pool remains 25165824 bytes. Numeric receipts ~/.cache/kontakto-fix-load/rss-owners/3f49-{text,arena}-rss.json; frozen test binaries and exact BUILD.json hashes under /mnt/Windows11/DEV_WORKSPACE/kontra-runs/w8-rss-20261008/. Probe flags match rss-text-sizing.md.

Compatibility limit: production owns at most 4096 concurrent events, matching frozen v1. Core callers retain configurable capacities and paged control-thread note growth; v2 voice growth and release reservations remain. No audio-thread allocation is introduced.
