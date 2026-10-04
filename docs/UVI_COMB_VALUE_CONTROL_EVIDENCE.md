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

The active Bartok Comb belongs to Aux3's FeedbackMachine, while its source macro
belongs to the same container. Graph binding, voice cloning/cache behavior and
broader dynamic connection mixing remain unverified. Program admission stays
closed. Private evidence and the reproducer remain under
`~/.cache/kontakto-uvi-comb-private/`; no purchased preset payload is included.
