# Missing PanLaw default agreement

The serialized Program host needs a numeric `PanLaw=0` fallback for Program,
Layer and Keygroup. Retained authored UVI Workstation 4.0.9 getter observations
return 0 for these three kinds in the loaded-program main and initialization
contexts. Their parameter-definition metadata instead advertises default 1;
that metadata is not the measured loaded-program value. SamplePlayer's native
getter is nil in the same observations.

Cached metadata for the actual original and V2 Clarinet presets omits PanLaw on
one Program, three Layers and 179 Keygroups in each bank: 366 nodes in total.
This establishes a missing-property witness, not a callback or sound comparison.

## Source ownership

The narrow correction belongs to `host::source_parameters`, which supplies both
the Lua parameter baseline and saved-state validation. It inserts 0 only when
the serialized attribute is absent, so explicit attributes retain their value
and type. It creates no initial command or parameter delta. SamplePlayer and
synthetic Part/Synth remain unchanged; the current synthetic parents do not
expose PanLaw.

The renderer already uses 0 when the field is absent. Program/Layer bus code
validates 0/1 but does not pass that selector into its balance helper. Keygroup
mono downmix uses the selector and multichannel PanLaw1 remains gated. This
default correction neither proves broader PanLaw1 fidelity nor changes those
DSP admission rules.

## State and verification limits

The Program fingerprint hashes the original nodes and text, which are unchanged.
Old states without these missing-property deltas retain their identity and
values. Explicit XML overrides retain their baseline. Newly saved overrides of
an omitted PanLaw cannot validate in an older implementation that lacks the
property; no downgrade compatibility is claimed.

The shared baseline lets numeric deltas validate and preload before authored
main/onLoad/onInit. Existing callback precedence and renderer preparation stay
unchanged. Prepared cases cover default reads, explicit values, unsupported
targets, default-free saves, numeric deltas, restore order, callback override
precedence and wrong-type rejection. They remain uncompiled and unexecuted under
the CPU restriction. Retained native observations and the static state audit do
not verify the final source against current bank callbacks or PCM.
