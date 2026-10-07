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
  explicit stop/panic cuts the voice, subject to the optional finite pass count.
- `UntilRelease`: stop wrapping when the effective note gate closes and continue
  in the initial direction toward the selected source end. Ping-pong playback
  completes its return leg before taking that outward exit. Sustain/sostenuto
  therefore defer loop exit just as they defer envelope release.

The first pass includes the material before entering the loop (after the loop
when playing in reverse). Subsequent passes traverse only the loop range. A
one-frame loop is valid. Wrap is deferred until the next read: a release exactly
at a plain wrap boundary enters the source tail immediately, without an extra loop.
Crossfaded wraps use the explicit completion policy below.
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
Native ping-pong motion now extends this contract below; crossfades remain open. Native root-key tracking compiles
into the prepared key index; [live pitch and modulation](MODULATION.md) are now
implemented. The pinned [reference review](REFERENCE_REVIEW.md) records required
fractional crossfade/partner-phase regressions for the remaining loop modes.
Raw loop boundaries are exact, not automatically click-free; no undocumented
smoothing is applied. Host/plugin integration and streaming also remain open.


## Ping-pong topology

`Loop.shape` explicitly separates `Wrap` and `PingPong` from continuous/until-release
lifetime policy. No legacy loop flag or runtime shim is involved. The half-open
range includes its first and last sample frames once at each turn: `[2, 5)` visits
`2, 3, 4, 3, 2, 3, 4, ...`. A one-frame loop holds that frame. Starting in reverse
mirrors this traversal and preserves the selected source view's initial prelude.

Each cursor still advances one monotonic integer-plus-fraction traversal clock.
Integer guard reads map onto the same reflected path as audible reads, so a high
rate can cross multiple turns without an unbounded reflection loop. No floating
absolute source index accumulates drift, no reversed asset is copied, and stereo
channels share the identical cursor. Reflection periods that cannot fit the native
u64 traversal representation are rejected during preparation.

Until-release exits occur at the next outward boundary in the **initial** source
direction. Exact-boundary release exits immediately. Release during the returning
leg completes the turn before reaching the source tail. Previously traversed loop
samples remain available to the interpolation window; future guards switch to the
tail only at the selected exit. Contiguous unity-rate spans are clipped at that exit,
including when a reflected span would otherwise cross it inside one block.

These are explicit native semantics. Architecture §8.3 requires declared boundaries;
the v1 `alternating_loops_match_unrolled_pcm_at_fractional_and_high_rates` fixture
supplies a useful independent endpoint matrix. The pinned Shortcircuit review also
identifies multi-turn overshoot and direction-sensitive release as necessary cases.
No vendor or external engine execution is claimed by these native tests.

Evidence includes literal unity-rate paths for one/two/three-frame loops, both
initial directions and all small block partitions; mid-return and exact-boundary
release; independent unrolled PCM at rates from 1/256 through 16 source frames per
output frame; fractional released interpolation against an analytic sinc reference;
long loops with contiguous windows; muted/restored phase and envelope continuity;
integer guard identity beyond 2^54 traversal frames; malformed period rejection;
and allocation/free checks on execution and terminal retirement. Crossfades and
streaming remain open; exact loops do not imply click-free material.

Final validation: all 181 tests across the four native crates pass in debug,
release and Rust 1.92, along with strict all-target Clippy and both root boundary
tests. Logs use `artifacts/ping-pong-fast-{debug,release,msrv,clippy,boundary}.log`.
The [paired render measurements](RENDER_WORKLOADS.md#ping-pong-source-topology-2026-10-06)
record the initial regression, correction and remaining workload-dependent costs.

## Finite pass counts

`Loop.passes: Option<NonZeroU32>` adds a finite limit to either topology and either
initial direction. `None` is unbounded. `Some(1)` includes the initial traversal of
the loop and then exits directly into the source tail. Each additional wrap pass
adds the loop length; each additional ping-pong pass adds a reflected round trip
between outward exits. A one-frame loop adds one source frame per additional pass.
Frontends translate their own repeat/count conventions explicitly into this native
meaning; this is not a claim that all vendor count fields have identical units.

Preparation computes the final traversal exit once and rejects count multiplication
or total-traversal overflow. Rendering reuses the existing integer exit boundary;
there is no per-block repetition counter, extra event or asset expansion. Fractional
filter guards see the exact same finite traversal and zero outside its final tail.
Until-release can shorten the count at the next outward boundary but cannot extend
the finite limit or restart an already exited loop. The source envelope still owns
its independent release duration.

Finite loop sources may now be used for natural-EOF key/gate release layers without
an explicit duration command. Unbounded release loops still require a duration.
Source EOF retires its family/voice while the original physical key identity remains
until proper release and terminal delivery.

Tests cover literal forward/reverse paths and single-pass behavior, unrolled finite
reference assets through EOF at fractional and multi-turn rates, both shapes and
one-frame loops, independent source views, several block partitions, early/exact/late
release boundary selection, overflow rejection and natural release-layer completion
with zero command slots. Execution and retirement are allocation/deallocation checked.

Validation: all 194 native tests pass in debug, release and Rust 1.92; strict
all-target Clippy and both root boundary tests pass. Logs are retained under
`artifacts/counted-loops-{debug,release,msrv,clippy,boundary}.log`.


## Linear crossfaded wraps

`LoopShape::Crossfade { frames }` keeps the wrap period and blends the outgoing
loop end into the guard immediately preceding the loop. Reverse traversal uses
the guard after the loop. The nonzero fade length must fit both the loop and the
required guard **inside the selected source view**. Missing guard data is rejected,
not replaced by zeros or an unrelated asset region. The final finite pass does not
blend, so it proceeds into the original source tail.

The fade is linear and complementary, at integer source positions from weight 0
at the window start toward 1 at its exclusive boundary. Both channels use the same
weight. The complete blended virtual source is then bandlimited, including every
interpolation guard that crosses a wrap; it is not a second cursor with a mismatched
fractional phase. Unity-rate/integer-phase reads blend directly. Other loop shapes
retain their contiguous or existing resampling paths. No blended asset is allocated.

A sustain-loop release before the fade window exits at the upcoming boundary. At
or inside that window, including the exact wrap, it completes the entered wrap and
exits after the next pass. This preserves every past crossfade guard. Repeated
release never extends a finite exit; an existing pass limit always wins. This is an
explicit native policy, requiring profile comparison before claiming vendor behavior.

One-frame fades/loops, full-loop-length fades, first/final passes, forward/reverse
views and source rates 0.25/1/3.25/16 times output rate match independently preblended
finite assets exactly at blocks 1/7/64. Preparation rejects zero/oversized fades and
missing view guards. Unit checks independently enumerate release boundaries and
compare past/future reads, including fractional positions and finite limits.
Runtime interpolation/retirement checks have zero heap activity.

Still open: reflected/ping-pong crossfades, overlap models that shorten the period,
equal-power or source-specific curves, streaming dual-window demand and vendor
profile fidelity. A linear complementary fade is not an equal-power or spectral
morph implementation.

Validation: 283 native tests pass in debug, release and Rust 1.92; strict all-target
Clippy and both root boundary tests pass (`artifacts/loop-crossfade-*`).


## Per-event attack offsets

Generated events retain a nonnegative source-time offset independently of pitch,
duration and expression. Attack selection converts microseconds to an integer
position plus fractional remainder at each asset's rate, using bounded integer
arithmetic. No source buffer is sliced or copied and no per-frame offset work is
added. The cursor retains the view's interpolation guards and original loop count.

Offsets are directional within the original view. A position at/past the initial
outward loop edge bypasses that loop; an exhausted source is silent and retires
through existing voice/family ownership. Automatic release sources start normally.
The native semantics and KSP evidence/remaining profile work are recorded in
[KSP_FRONTEND.md](KSP_FRONTEND.md#generated-note-source-offsets).
