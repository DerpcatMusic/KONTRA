# Native source views and loop boundaries

Each native region now owns a `Playback` value: a start frame, optional exclusive
end, forward/reverse direction, and optional loop range. Offsets are absolute PCM
frame indices. Preparation rejects empty/out-of-source ranges, empty loops, and
loops outside the selected range. Manual source admission applies the same
validator before taking a voice slot or queue entry. No playback metadata is
written into the shared sample asset.

Each voice owns its cursor. Multiple regions can use the same immutable PCM with
different views, directions, envelopes and loop settings. Forward playback begins
at start; reverse playback begins at end minus one. Both traverse the selected
range without skipping or duplicating the endpoints.

Loops use half-open ranges and two explicit native policies:

- `Continuous`: repeat through the release tail until the envelope ends or an
  explicit stop/panic cuts the voice.
- `UntilRelease`: stop wrapping when the effective note gate closes and continue
  in the current direction toward the selected source end. Sustain/sostenuto
  therefore defer loop exit just as they defer envelope release.

The first pass includes the material before entering the loop (after the loop
when playing in reverse). Subsequent passes repeat only the loop range. A
one-frame loop is valid. Wrap is deferred until the next read: a release exactly
at the loop boundary enters the source tail immediately, without an extra loop.
Source EOF still retires audio independently of a held logical note; NOTE_END
continues to require gate closure and complete ownership retirement.

Rendering splits a voice's work into contiguous source spans, with a reversed
iterator for reverse playback. Each iteration consumes at least one output frame;
work is bounded by the supplied block even for one-frame loops. Envelopes advance
in playback time, never source-position order. Sums retain stable voice-slot
ordering. No source copies, allocations, frees, locks or file calls are added to
the callback path.

## Evidence

Four independent source tests cover explicit forward/reverse sequences under
all block sizes 1 through 10 plus zero-length calls; continuous and one-frame
loops; exact-boundary release exits in both directions; two independently moving
views into one asset; allocation/free-free looping release and terminal delivery;
and malformed ranges rejected without partial voice/queue admission.

All 29 new-crate tests pass in debug and shipping release. Rust 1.92.0 tests and
strict all-target Clippy pass. The full root CI-profile suite also passes
(601 library, two CLI, 89 playback and two historical v2 integration tests;
34 existing tests remain ignored). The new-core Rust Doctor scan is complete and
authoritative at **90**, with no error-level findings and all rules retained.
Logs and reports are under ignored `artifacts/architecture-v2/source-*`.

The resident benchmark now supports `--loop` (127-frame repeating span). One local
48 kHz / 64-frame run produced:

| Active / reserved voices | Full source p50 / p99 us | Loop p50 / p99 us |
| --- | --- | --- |
| 16 / 64 | 0.460 / 0.760 | 0.510 / 0.830 |
| 16 / 4096 | 2.400 / 4.581 | 2.590 / 6.700 |
| 256 / 256 | 6.470 / 10.521 | 6.900 / 13.520 |

Checksums match the previous constant-source benchmark; no deadlines were missed.
The general cursor adds measurable overhead against the earlier unity-only path
(~5.32 us median at 256 voices). These uncontrolled microbenchmarks guide further
optimization; they do not establish worst-case deadlines or a competitor ranking.

The original evidence above covers integer-position unity-rate playback.
[Fractional playback and rate conversion](RESAMPLING.md) now extend this contract.
Crossfades and ping-pong loops remain open. Native root-key tracking compiles
into the prepared key index; [live pitch and modulation](MODULATION.md) are now
implemented. The pinned [reference review](REFERENCE_REVIEW.md) records required
fractional crossfade/partner-phase regressions for the remaining loop modes.
Raw loop boundaries are exact, not automatically click-free; no undocumented
smoothing is applied. Host/plugin integration and streaming also remain open.
