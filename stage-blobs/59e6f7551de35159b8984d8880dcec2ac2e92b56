# Compressor editor controls

Kontakt and UVI translation now register threshold, ratio, attack and release
controls in the existing IR control table. `Instrument::register_compressor_controls`
retains any authored binding; repeated calls preserve its address and value.
Defaults remain physical dB, ratio and seconds. Editor ranges are −60..0 dB,
1..20, 0..1 s and 0..5 s, expanded to include authored defaults. These are
editor ranges, not recovered native normalized laws.

`ParameterDescriptor::role` names the semantic field independently of labels.
`sampler_core::ParameterRole` re-exports `sampler_ir::ProcessorParameter`;
`Threshold`, `Ratio`, `Attack` and `Release` resolve through the same address
and `ControlId` used by `Runtime::edit_controls`. W14 should select the exact
chain/processor descriptor slice and role, and write its physical value.

The existing compressor recurrence and linking detector remain in use. Constant
settings prepare once; held control values cache coefficients per processor
state, and 10 ms edits follow the shared sample-clock ramps without allocations.
Signal traces expose the physical control owners rather than fixed coefficients.

The failing-first API fixture, active-voice edits, held-control recurrence,
invalid-domain rejection, registry checks and UVI importer fixture pass. The
isolated Analog Strings owner (chain 483, processor 5) and sample (zone 11340)
produce level changes of +1.423 dB, −1.509 dB, +1.076 dB and −0.801 dB for
threshold/ratio/attack/release edits, with zero heap calls/nonfinite frames.
This probe scales its sample peak to 0.8 and disables scripts; it establishes
control audibility, not complete-program or native compressor parity. PCM stays
in RAM. Receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w15-compressor-controls/`.

Native threshold/ratio/time laws and quiet CPU remain unverified. No release or
installation was performed. The separate Una Corda piano investigation remains
parked; this change does not claim to fix it.
