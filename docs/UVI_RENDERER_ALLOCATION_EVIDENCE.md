# UVI renderer layer scratch reuse

## Scope

The renderer reuses the allocation for its per-frame layer mix map. It still
accumulates voices in their existing vector order and consumes layers in the
immutable program order. It never iterates the map. The map retains empty
capacity only: its values are fixed `[f32; 12]` frames, with no sample, processor
or script ownership.

Reuse occurs only when every accumulated entry was consumed. Existing accepted
structures with a non-layer Keygroup parent still discard their unconsumed map
at frame end. Failed in-flight frames also drop their scratch as before. Capacity
grows on demand and is bounded by reached IDs in the immutable layer set plus
hash-table bucket rounding; no eager allocation was introduced.

## Functional evidence

The guarded candidate was compared with source `bb60218` using a private actual
Alto Flute replay at 48 kHz over 211,200 frames. PCM, command roots, commands,
completions, host commands and saved state hashes all matched. Both variants had
8,074 commands, 367 host commands, four retained voices, finite output and
406,012 nonzero scalar samples. Four focused checks cover malformed-parent
discard, layer routing, sibling controls and render partitions/error behavior.

The private helper used the same copied UVI source on both sides and unchanged
external audio/fx SDK glue, except the same-commit convolution source and copied
diagnostic excerpt helper. It was not a GUI, full-plugin or native-PCM comparison.

## Allocation evidence and limits

Counting allocations only during rendering on the valid-frame reuse predecessor
measured 98,304 fewer requests over 98,304 held frames: one 244-byte requested
allocation removed per frame. The final guard adds no allocation and preserves
that valid-frame path; its exact replay and malformed-parent check were run
separately. Other renderer allocations remain.

This establishes bounded allocation reduction, not CPU speed, deadline compliance,
sustainable playback or a cause of the reported blackout. Exploratory instruction
counts differed by less than run variation, and CPU measurements were confounded
by external work. The installed checkpoint remains `bb60218` until a later
package is explicitly verified and installed.
