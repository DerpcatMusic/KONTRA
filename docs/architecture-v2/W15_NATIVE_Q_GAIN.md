# W15: addressed native Q and Gain routes

The physical filter-slot lookup now admits `filterQ` and `Gain` for retained
Ladder LP4 and Daft processors. Both use normalized depth, through the shared
route transforms, lag and source scaling. Unsupported processors retain their
existing report. Generic SVF cutoff/Q laws remain unchanged.

Port from v1 0cb7a8a0:src/engine/filter.rs (VoiceFilter::process, row additions
and Ladder operation): accumulate knob deltas before conversion, saturate
cutoff/Q, set enabled modulation for all three knobs even at zero depth, and
clamp Ladder Gain to 0..1 only when a Gain route is enabled. Static signed
Ladder Gain stays signed. This adapts the v1 row behavior to preallocated v2
addressed storage: cutoff/Q/Gain deltas plus an enabled bitmask. No render hot
path files are changed. Daft keeps its documented approximate audio kernel.

Failing-first receipts: w15-native-qgain-red.log (translation drops target),
w15-native-qgain-lower-red.log (normalized Resonance lowering unsupported).

Targeted tests pass: native translation, native clock tests, dense projection
cancellation/flags/reused scratch, fragmented native knob equivalence and clamps.
The render check makes zero audio-thread allocation/deallocation calls. Saved
Gain without a route stays signed; enabled zero-depth Gain clamps as in v1.

Gate-item A/B, numeric-only receipt w15-native-qgain-ab.log:
Conflux Ladder Q: level −4.913465280 dB, residual −7.205043993 dB;
Analog Daft Q: level +4.071657164 dB, residual −2.936857336 dB.
Exercised Gain on those same saved blocks/sources: +4.353596641 dB and
+4.800084648 dB respectively. Q uses saved Q targets; Gain projects the saved
cutoff source onto Gain because those selected groups have no retained authored
Gain source. The log records that distinction. Enabled amounts are exercised
for audibility; this is not native-host PCM parity. All PCM stays in memory;
no WAVs are written. Quiet CPU and W12 recount are pending.

NEXT: approved generic address/lane plumbing, preserving existing PCM first.
