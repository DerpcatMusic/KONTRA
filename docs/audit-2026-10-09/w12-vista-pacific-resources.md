# Vista/Pacific sample-resource inventory

On `54d9a5c5da45ada122694bde78e3b4aaf36dc731` (381), every installed NKI in both libraries translates without a missing sample or failed impulse-resource report. There are **no reproduced failing patch/member pairs** to classify as path case, nested archive membership or alias failures.

| Library | Installed patches checked | Translated zone references | Sum of per-patch assets | Sample/IR resource failures |
|---|---:|---:|---:|---:|
| Vista | 7/7 | 47,780 | 21,066 | 0 |
| Pacific | 49/49 | 85,718 | 79,228 | 0 |

Frozen v1 CLI integrity passes. Its `inspect` output, reduced in RAM, confirms Vista `Bonus/Vista - Harp.nki` has 2,000/2,000 available zones and Pacific `10 Cellos/Pacific - Ens Strings - 10 Cellos - Legato Sustains.nki` has 6,136/6,136. Both have an empty `missing_samples` list; v2 resolves the same zone totals. Frozen v1 was not rebuilt. Its cache writes were disabled with `XDG_CACHE_HOME=/dev/null`; no source, sample or PCM payload was persisted.

Historical `decoded.missing` values 49/1,416 counted **all untranslated features**, not sample resources (`LoadReport::missing` derives from `Instrument::unsupported`). Current Harp/Cellos translation reports 32/112 other feature entries. The audit receipt now adds explicit untranslated-feature, sample-resource and impulse-resource counts while retaining the legacy total. This is a diagnostic classification repair, not a resolver/playback port.

V1's `0cb7a8a0:src/import.rs:918` includes an NKR fallback for `Resources/...` impulse references that v2's ordinary sample resolver lacks. It was not needed by these patches and was not ported without a reproduced failure.

Reproduce the census with `cargo build -p sampler-kontakt --example resource_inventory --profile ci`, then run that example on each installed NKI. Both build and library probes must go through `~/.cache/kontakto-heavy`. Numeric per-patch receipts, commands and binary hashes are under `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w12-vista-pacific-resources/`; `RESOURCE_CENSUS.json` contains the empty failing-patch/member lists.

Limits: directory resolution and translated resource reports do not prove every monolith byte offset, sample payload or playback path. Full audio/family parity remains UNKNOWN; the separate Vista/Pacific artwork lookup issue is not a sample failure. No timing claim.

NEXT: confirmed sample/header or playback failure; keep family comparison separate.
