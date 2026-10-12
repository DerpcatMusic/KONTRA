# W13: EQ graph editing parity

One row from the 84-axis settings inventory: **Editable EQ band frequency/bandwidth/gain per slot**. The broad Present label missed a Worse interaction in clean 608a20a1: each gain handle followed the whole serial response, and vertical motion ignored the band's gain range.

Port from v1 0cb7a8a0:src/ui/viz.rs, `filter_handles`: position each EQ handle at its own gain and scale vertical motion by graph dB / knob dB. Adapt the conversion to v2's admitted native binding law, shared with typed readout. Frequency and bandwidth keep their existing addressed controls.

The failing-first `ui::v2_tests::v1_eq_handles_use_each_band_gain_and_graph_drag_scale` failed at the independent band-gain position on 608a20a1 and passed after this port (1/1). Its two-band fixture includes another serial filter and both ±18 dB and ±24 dB native ranges. It checks typed gain, a 10% graph drag producing +6 dB, bandwidth wheel ownership, unchanged script base, and reset. Validation uses the `ci` profile through `kontakto-heavy`.

Root `cargo test --profile ci --no-run` passed through the wrapper. This is an editor interaction fix with synthetic native bindings. It does not establish Kontakt EQ DSP admission, audio parity, or a presented DAW frame. Those retain their separate owners and gate verdicts. The frozen 0.3.326 artifact does not contain this follow-up.

Suggested release note: “EQ handles now show each band's gain and follow vertical drags accurately.”

NEXT: restore native group inserts in the Sound editor's Effects list; pause for W6's host-window handoff.
