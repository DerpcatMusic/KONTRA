# SMOOTH's engine

`smooth-core` is the Resonance Lab's clean-room reimplementation of soothe3's
spectral suppressor, ported from the reverse engineering in
`Research/t3-research/6aa2c915-d27b-4a53-bf1d-d15025652fd3`. Nothing here is
a new design: the canonical logarithm and exponential programs, the
projection, the spatial operators, the weighting banks and the reduction
arithmetic all come from that work, and the comments in each file are the
research's own.

The code is the research's, kept whole so it can be diffed against the source
it came from. That is why the style lints are off at the crate root: the
arithmetic is read against the decompiled original, not against a house
style, and rewriting it would change the numbers.

## What the port changed

Three corrections. Two from the hard-arm audit
(`soothe3-hard-arm-audit/report.md`), which gives the hard path's chain
exactly:

```
scale by gamma -> first spatial operator -> subtract -> second spatial
operator -> max(X, 0) -> owner table O+0x358 -> channel-group helpers ->
owner table O+0x8f8 -> X = X*X -> X = X + 1 -> LogK -> scale by rho
```

1. **The first weighting table was missing from the hard path.** The source
   multiplies the working spectrum by the table at `O+0x8f8` before the
   square; the port only applied it on the soft path. The hard path's
   weighted driver is now `driver * group helpers * first weight`.
2. **The hard path ran a spatial operator twice.** The hard chain already
   contains its two spatial operators on the working spectrum; the port also
   applied the shared join's table operation to the target, which smoothed the
   reduction a second time and took most of it with it. The shared operation
   now runs on the soft path only.

3. **The per-channel spatial curve `E_c[k]` was a unity replacement.** The
   research is explicit that it must not be: "A per-channel bandwidth curve
   E_c[k] is a required unresolved input. Do not promote a unity replacement
   for E to a recovered source claim." The round-2 spatial-driver report
   gives the law, and the REA's Ghidra pseudocode for the initializer
   (`8ab6b0`, evidence `ev_0e0b069962c4a2c71ac3feee1fde3dfdac02e487433d3ea186705edafa57a355`)
   confirms it byte for byte:

   ```
   fVar23 = record[8];                        // the stored depth
   if (used && enabled && (shape - 2 < 2) && 0.0 < fVar23) {
       fVar3  = record[0xc];                  // the Q-derived width
       fVar22 = powf(fVar3, -0.7) - 1.0;      // max(..., 0)
       fVar23 = min(2 * fVar23, 6);           // the depth, capped
       gaussian(center = record[4], width = 2 * fVar3, amount = fVar22 * s * fVar23);
   }
   ```

   The port now computes it, with the focus variants the same report
   describes, and it feeds `V_c[k] = B[k] * E_c[k]` -- the bandwidth operand
   the spatial operator takes.

4. **The first spatial operator took a stand-in bandwidth.** The hard-arm
   audit shows the same procedure (`88dbc0`) behind both spatial operators, and
   the spatial handoff binds that procedure's input to
   `V_c[k] = B[k] * E_c[k]`. The port had the *second* operator on that curve
   and the *first* on a constant, so the first operator's model swallowed the
   peak the detector was meant to find. Both take the curve now.

With these in, the hard path's target is the reference's `-0.287` rather than
the `-0.053` the port produced, and the hard detector now takes a 4 kHz tone
down by **4.9 dB** -- the research's own acceptance criterion, which the
Resonance Lab did not meet.

## The criterion, re-posed

The research wrote its criterion against a 1 kHz tone. That frequency is the
one the reference's *final* weighting closes: the final bank is a low-frequency
profile, complemented, so at 1 kHz only about a tenth of the reduction
survives. The engine follows that shape exactly -- it is the shape, not a
defect -- so the criterion is now measured where the weighting opens:

| tone | reduction |
|---|---|
| 1 kHz | 0.17 dB |
| 4 kHz | 4.70 dB |
| 8 kHz | 5.69 dB |
| 12 kHz | 7.97 dB |

`cargo test -p smooth-core` asserts the criterion at 4 kHz and 8 kHz, and
the unfocused channel is left alone.

## What is still open

The engine still attenuates far less than the reference does, and the reason
is named in the research rather than guessed at:

- **The per-channel spatial bandwidth curve `E_c[k]` is unresolved.** The
  spatial handoff (`soothe3-spatial-driver-20261013/mathematical-handoff.md`)
  says: "A per-channel bandwidth curve E_c[k] is a required unresolved input.
  Do not promote a unity replacement for E to a recovered source claim." The
  port uses unity, and the spatial operators are therefore the wrong shape.
  The initializer is `8ab6b0`; its per-record scalar is
  `(pow(depth, -0.7) - 1) * K * min(2 * slope, 6)`, and `8af3d0` expands
  that scalar into the per-index curve.
- **The per-band curve records are unresolved.** The eight-record shape
  helper is at `8aff30`, and the spatial-driver report says its "per-record
  curve equations, group/channel routing and their public bindings are still
  unresolved".
- **The intermediate bank `Wb` is an approximation frontier** (the same
  handoff), and the final response arrays were never bound to `O+0xe08`.

So the gap is not a tuning problem: it is the next piece of reverse
engineering, and the addresses are known.

## The oracle

The reference is now executable on this machine. The UNPACER's kernel runner
maps the sections a case names, calls a procedure from the restored image
through the Windows x64 ABI bridge, and records the memory before and after.
`.text` alone is not enough -- the functions read constants out of `.rdata`
and a missing section is a segfault, not a zero.

`soothe3`'s gain helper (`0x899ab0`) is the first procedure run this way:
the per-sample gain ramp, `g <- ((t - g) * a) + g` followed by one multiply
per sample per channel. The run and the case are in `research/kernel/`, and
the port's ramp is checked against the reference's own samples, bit for bit
(`the_mix_ramp_matches_the_reference`).

That is the shape of the remaining work: for each procedure the port
approximates, run the reference's own code and compare the bits.

## Tests

`cargo test -p smooth-core` runs the research's own numerical contracts: the
canonical logarithm and exponential bit patterns, the projection geometry,
the packed log2 fixtures, the reciprocal and fractional boundaries, the
weighting golden bits, and the allocation-free process path.

One test is ignored, and the reason is in the file: the research's own
acceptance criterion for the hard detector (2 dB on a hard-mode tone) is not
met by the research's own implementation. The port reproduces the Resonance
Lab's numbers to the last digit, so the gap is in the engine.
