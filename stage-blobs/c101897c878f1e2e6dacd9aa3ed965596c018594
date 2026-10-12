# Comb ConstantModulation held-Value boundary

The caller-owned adapter consumes physical held Value points from the upstream
property manager and generates bounded Freq points for the measured Comb leaf.
The property manager retains its own Value RC state; this helper retains only
ConstantModulation source and Freq RC states. It neither duplicates the upstream
RC nor invents graph scheduling. Calls require 32/44.1/48/96 kHz, unipolar
ConstantModulation, Mode 1, Ratio 1, Offset 0, no inversion/mapper and at most
4096 frames. The existing consistent cold initialization remains required.

Sixty-four native point/audio comparisons cover four rates, both Comb modes,
bypass and four targets with 17/65/33-frame fragments. Four focused checks cover
native vectors and rejection without state/output mutation. The unchanged native
fixture executes Value edit/prepare/RC, source, Freq and audio; Rust consumes the
observed upstream held Value points. This verifies the adapter boundary rather
than the complete property manager or graph lifecycle.

The subsequent `CombEditionValueProperty` generator supplies those upstream
points and their per-block static flag for this same bounded route. It shares
the adapter's measured RC arithmetic; the separate `ConstantClock` models a
source clock with different snap and ownership rules. This closes the measured
three-RC composition without duplicating that source clock. Native comparisons
cover 160 scalar cases, 16 constructor cases and 64 consumed Value → edition →
Freq/Comb point/audio cases, including cold edits, fragmented blocks, bypass and
signed zero. Authored static initial negative zero retains its bits; computed
zero after a dynamic ramp is canonical positive zero.

The generator and adapter require one call per source owner's host block.
Native serial/offset coverage can avoid advancing shared sources twice, and
unaligned overlapping extensions are not equivalent to one whole-block call.
That coverage cache remains above these leaves. Caller-owned output is bounded
to 4096 frames; invalid inputs preserve state and output. Five focused checks
cover retained native vectors and these boundaries. This is a leaf-composition
proof, not integration of the native XML loader, graph ownership or active
FeedbackMachine audio.

The active Bartok Comb belongs to Aux3's FeedbackMachine, while its source macro
belongs to the same container. Graph binding, voice cloning/cache behavior and
broader dynamic connection mixing remain unverified. Program admission stays
closed. Private evidence and the reproducer remain under
`~/.cache/kontakto-uvi-comb-private/`; no purchased preset payload is included.
