# Saved native control domains and installed-library loading

The installed-library sweep found legacy Kontakt filter cutoff records containing
8.5, 9.7 and 10. These are finite saved values, including in bypassed slots. The
normalized-only IR validator rejected the whole instrument before lowering.

The original ten BFilter setters store the raw cutoff at +0x1e4, then saturate the
coefficient input to 0..1. `tools/re/legacy_cutoff_boundary.py` invokes each isolated
original prepare/setter under the existing restricted Unicorn harness. At 48 kHz,
resonance .25, all 40 comparisons (-8.5, 8.5, 9.7, 10 against the appropriate
boundary) produced byte-identical coefficient state at +0x200..+0x1200. The base
reset virtual call is an explicit no-op; this is setter-state evidence, not a
native host, modulation-clock, or complete audio-parity measurement. The numeric
receipt records the exact original binary hash; it contains no binary payload.

Admission now retains finite-f32 cutoff bases for precisely the ten original
legacy classes. The coefficient law saturates after adding modulation; the control
clock now retains raw cutoff state instead of clipping its target before slewing.
The coefficient math is unchanged. Clipping the saved base during translation would change a later
negative modulation delta; the runtime regression explicitly distinguishes 8.5
plus -8 from a prematurely clipped 1 plus -8. Other filter families, resonance,
extra knobs, NaN/infinity and values outside finite-f32 range remain checked.
There is no preset-name condition or bypass-based deletion.

That unblocked a second independent failure: automatic compressor editor
registration used one dB threshold/normalized-ratio domain for every native law.
A feedback compressor instead consumes positive linear input drive, ratio 1..20
and positive time constants. Editors now follow the law's actual physical units;
limiter editors expose only their used knobs, and cores with different/unused
knobs do not receive invented generic controls. Existing native engine bindings
and authored controls retain their identities. These domains follow the recovered
DSP contracts and `docs/re/kontakt-bindings/fx_dynamics_dist_tape.tsv`; this does
not add unproved normalized Kontakt bindings for feedback controls.

Focused validation: 12 legacy core/IR tests passed, including the native vector
checks; generic and six native compressor/editor cases lower with their actual
domains. Installed Una Corda Pure now renders (113 authored groups, 4,402 zones,
53 selected zones, finite nonzero peak .75519), and its production streamed
binding/lookup test passes (1,417 bindings, 272 lookups). An outdated ownership
oracle was broadened to check each processor's declared exact native lane rather
than recognizing only Color/Ladder families.

The translated-instrument cache hashes sampler-ir/core/importer sources, so these
semantic changes invalidate previous cached translations automatically. An early
cancellation fix separately ensures a canceled path load does not read or
translate a container before returning Canceled.

An additional isolated original-wrapper probe (0x140ae9f90, branch global zero,
zero offset, no modulation buffers, no-op audio callback, 128 frames) confirms
raw clock state survives: an 8.5 -> .5 target produces setter arguments
8.41489315, 8.32978630, 8.24467945, 8.15957260, 8.07446575 at resonance .25.
The negative case is symmetric. The runtime regression checks its four
32-frame updates against the first four original values. The original wrapper
also performs a boundary update in this setup; that boundary phase has not
been calibrated in the runtime. Such out-of-range authored cutoffs therefore
retain a specific partial-fidelity warning. This is not a claim of complete
wrapper-clock parity.
