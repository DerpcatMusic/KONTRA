# W15: real EQ gain owners and physical band routing

This records gain step `fdf6d4e0`; subsequent frequency/bandwidth owners are
documented in [W15_EQ_KNOBS.md](W15_EQ_KNOBS.md).

Kontakt `eqGain1..3` now address retained physical bands through the shared
processor route resolver. Flat bands stay present when a script or authored EQ
gain route can make them nonflat; their following band identities do not shift.
Static, unbound EQ retains the existing RBJ arithmetic and flat-band elision.
Invalid live bands reject the slot instead of renumbering later bands.

Port from v1 `0cb7a8a0:src/engine/filter.rs`: `Proto::bell`,
`Section::coefficients`, scalar `Section::process_body`, normalized gain depth,
knob saturation and flat-band state suspension. The dedicated owner in
`sampler-core/src/dsp/eq.rs` adapts that float32 TPT recurrence to planar float64
I/O, per-voice histories and the existing sample-clock control lanes. Frequency
and bandwidth are constant in this slice. Coefficients are reused at settled
gain and remain voice-local. A flat interval skips the stage without resetting
its histories. EQ can remain in a lane chain, so adding a flat owner does not
force unrelated stages out of their lane kernels; active EQ calls the same
scalar recurrence with bounded stack scratch for each real lane voice.
W6's shared Biquad recurrence is unchanged.

Each band owns a real Continuous dB control with range -18..18. Generic velocity,
constant, AHDSR, LFO, CC and script sources use the same existing projection;
there is no EQ-specific source pairing. `SourceControlAlias` binds the native
physical GAIN1/2/3 address to that actual control. Native 0..1M writes map
linearly into the control's declared range. Aliases must resolve to continuous
processor bindings, cannot use layer sentinel slots, and conflicting native
addresses fail preparation. Saved getter metadata precedes script initialization;
init writes become the real owner's default. Saved outliers saturate like v1.

Registry descriptors retain the shared shape: name, unit, range, default, law,
scope and display/group order. EQ gain is dB, labelled `EQ Band N Gain`, with
native Linear(-18,18) aliases. The signal trace publishes `v1_peaking_eq` and
its actual frequency, bandwidth and gain lane values.

Failing-first contracts cover unsupported core ownership, flat physical-band
retention, all three gain targets, missing native aliases, lane eligibility and
saved-gain saturation. Runtime witnesses exercise a flat band's +12 dB velocity
modulation, then a native +6 dB base edit whose held modulation clamps at +18 dB.
They make zero allocation/deallocation calls on trigger, edit and render.
The lane witness checks PCM and histories across 8/48/192 kHz, three frequencies,
two widths, zero/partial/full blocks, positive/flat/negative gains and short voices.

Validation: 83 core unit and 27 lowering tests pass; 78 Kontakt library tests
passed before the last outlier fixture, with the EQ tests rechecked afterwards.
Area compile-only checks cover sampler-core, sampler-ir and sampler-kontakt.
Numeric receipts and exact binary identity are under
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w15-eq-gain/`.

Offline A/B uses gate item Afflatus 2 Horns KS, authored zone 8426, key 60,
velocity 64/127, 0.5 s at 48 kHz stereo. The saved EQ slot is bypassed; both
sides explicitly activate it through the native bypass service. Authored gain
routes are then enabled versus removed; scripts are disabled for isolation.
The second EQ band's trace changes from -6 dB to +13.64656949 dB, with identical
stage input. Output energy changes +1.790663767 dB; difference energy relative
to the route-off render is -3.181930197 dB. Both renders are audible, finite and
heap-free. PCM stays in RAM; no WAV or decrypted payload is persisted. The A/B
binary predates the saved-gain outlier clamp, which cannot change these in-range
saved values; the outlier change has its own failing-first targeted check.

This is explicitly the v1 fallback, not native Kontakt EQ parity. The approved
native saved/coefficient/scalar/wrapper vectors in KONTAKT_DSP_LAWS.md still
leave wrapper gain scaling and modulation cadence open. Quiet performance is
UNKNOWN; no CPU improvement or no-regression claim follows from contended runs.
Frequency/bandwidth target adapters, full native-host A/B and W12 recount remain
open. This gain step does not claim complete DSP coverage or the release gate.

NEXT: normalized EQ frequency/bandwidth lanes, preserving physical band owners.
