# W9 v1 voice shape checkpoint

Base: stable381 `54d9a5c5`. Reference: `0cb7a8a0:src/engine/filter.rs`, literal `Section::process_body`.

The full Horns profile identified held EQ controls, scalar EQ recurrence and rack mix control reads. Held EQ now tunes once outside the sample loop and uses v1's two-frame SSE section, adapting only planar f64 input/output to its f32 arithmetic. The generic moving-control path retains its scalar recurrence. Scalar and lane rack mixing compute held dry/wet/bypass gains once, then fuse gain and mixing. Existing bypass state suspension and traces remain at their original boundary.

| Target reads / 32 frames | RED | GREEN |
|---|---:|---:|
| EQ | 97 | 4 |
| Scalar rack mix | 194 | 5 |
| Lane rack mix | not reached after scalar RED | 4 |

Baseline `bb96bc7f` reproduces all three assertions: EQ budget, scalar mix budget, and v1 section null. GREEN null checks output and histories against an independent literal v1 SSE arithmetic transcription for 1/2/3/7/31/32/33/63/64-frame fragments, three consecutive blocks each. The core DSP block capacity is 64; host fragments up to 256 are also checked through the runtime.

Validation: 39 targeted DSP unit tests pass, including frozen scalar/lane PCM/state oracles, moving rack mix, masked/flat EQ lanes and SVF. A runtime EQ+mix render/recycle fixture reports zero audio allocation or deallocation across 1/3/7/32/64/127/256 host fragments. Six control timeline tests and sampler-simd dispatch pass. Sampler-core ci no-run passes. An initial heap fixture installed chains before control definitions and was corrected; it is not counted as the failing-first result.

Receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w9-v1-whole-voice-20261009/SHAPE_{RED,GREEN}.json`, logs and `SHAPE_SHA256SUMS`.

Source GREEN, **whole-cell CPU HOLD**. No instrument timing or full-instrument output acceptance is claimed. The paired section intentionally follows v1 rounding rather than old v2 scalar rounding. W6 control module and raw native preparation/whole voice consumers remain to integrate.

NEXT: raw typed native control preparation and whole-voice integration, then quiet CPU/output A/B.
