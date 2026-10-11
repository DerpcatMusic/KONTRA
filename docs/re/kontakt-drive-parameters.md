# Live Shape, Drive and Damping: registry work deferred

Status: implementation blocked at the [MCP evidence gate](kontakt-fx-2-evidence.md).

Docs first: Kontakt 8 [Effect Reference, Distortion and Saturator](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference);
KSP [Engine Parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters),
`$ENGINE_PAR_SHAPE`, `$ENGINE_PAR_DRIVE`, `$ENGINE_PAR_DAMPING`.

Historical binary lead: BEffectShaper audio `0x140aa4d20`.
Round 1's independent v1-derived kernels and initialization writes are
already present; this round did not independently extract their live
parameter reader, clamp or smoothing functions through MCP.

The current saved/KSP domains are signed Shape in [-1,1] and normalized
Drive/Damping in [0,1]. Those domains alone do not specify when coefficient
or transfer-function updates take effect. Routing a live parameter to a
static kernel without tracing that behavior would not prove parity.

The existing standard path is IR processor parameter roles →
`lower.rs` processor bindings → prepared `dsp/control.rs` parameters.
Shape/Drive/Damping roles and dynamic kernel inputs are still needed.
The modulation agent owns generic routing; no changes were made to
`voice_mod.rs` or `parameter_registry.rs` internals.

Confirmation: current initialization support and registry boundary
inspected; no live-parameter vector or production test. Parameters remain
unconnected during playback. Coordinate any new roles with modulation
after the required binary clamp/smoothing evidence is available.
