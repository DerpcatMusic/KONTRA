# Explicit UVI inspector note condition

The preset-switch inspector accepts an optional `uvi` object. Without it the existing fast note condition remains unchanged and its independent UVI owner execution remains unverified. The object is refused for a non-UVI owner. Limits are validated before loading; unknown fields are refused.

```json
{
  "phase_timeout_ms": 1000,
  "total_timeout_ms": 10000,
  "pace_ms": 2,
  "controllers": []
}
```

Timeouts are explicit wall limits from 1 through 10000 ms, with phase no greater than total. Pacing is 1 through 20 ms between PCM blocks. The existing `note_frames` field bounds the measured PCM window to 128 through 96000 frames; the last block may be shorter than 128. The total wall limit includes acknowledgement, sample observation, PCM and pacing. Thresholds are checked between bounded core/UI calls, so the launcher still needs an outer process timeout.

Controllers default to an empty list. At most 32 explicit objects are accepted, each containing `channel` (zero based, 0..15), `cc` (0..127) and `value` (0..127). Entries run in supplied order and their owner/command acknowledgement precedes NoteOn. For example, CC1=100, CC11=127 and CC64=0 would be a **separate explicit input condition**; the inspector never inserts or expands those inputs.

Monitoring is installed on the exact committed bridge and epoch before any probe inputs. The initial phase acknowledges the current owner and records its sample condition without adding clock frames. Controller and note phases record accepted, rejected and executed counter deltas. Both note routes require one owner event: programs with an authored `onNote` use that callback, while programs without it forward the physical attack directly. The report distinguishes the route and `handles_notes`; event execution alone does not prove an authored callback exists. Each owner proof requires both a completed ordinary ScriptThread acknowledgement and consumption of its ordered marker by the real V2Core driver. While polling, ordinary zero-frame begin/render/end and read-only UI service admit commands and sample transactions. These calls do not advance the sample clock; `switch_render(0)` alone would do nothing.

After note acknowledgement, fresh owner observations allow pending sample work to reach a core commit within the phase bound. Sample reports retain requested, successful, failed, cancelled, pending, queued metadata, active worker, ready and submitted counts. Success requires the real core commit reply. Both conservation equations are checked: requested = successful + failed + cancelled + pending, and pending = queued metadata + active worker + ready + submitted. Sample failures, cancellations and unresolved pending work remain partial evidence. An acknowledged owner does not imply all samples are ready.

Paced PCM uses ordinary parent rendering in blocks of at most 128 frames. Reports aggregate peak, frame-weighted RMS across all output channels, nonzero frames/blocks and voice counts. `pcm_block_digest_chain_sha256` hashes a versioned domain, each little-endian u64 block index and frame count, then the block's 64 ASCII hexadecimal PCM digest bytes. It is **a chain of block digests**, not a digest of concatenated PCM. Raw PCM is discarded. Reports retain bounded first/phase-change sample and owner observations, omitted counts, first signal evidence, and a separate final owner/sample acknowledgement.

`observed` means the bounded condition was fully measured with earlier note-owner execution, command drain and sampled sample settlement evidenced. It does not mean all future work completed or the preset passed native parity. `next_due_ms` can still describe a future wait; listeners and background async data can continue. Timeout, service failure, input rejection, stale epoch/bridge, seeded owner and conservation failure retain explicit partial evidence. Dropping or timing out a request abandons that observation and cannot cancel authored Lua or IO.

Zero PCM after proper owner execution may be authored suppression under the selected input/window. Before owner execution is evidenced, zero or absent PCM does not establish instrument silence. Neither case supports a universal library compatibility verdict. This mode does not change RNG, controller defaults, production APIs, source identity, loading policy, UI PNG capture or asset-parity scope, and emits no commercial script/sample/artwork payload.
