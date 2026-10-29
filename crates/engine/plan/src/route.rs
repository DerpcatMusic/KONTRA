//! Source selection for a modulation route.

/// The source read by this route, if it has one.
#[must_use]
pub fn route_source(kind: kontra_ir::ModKind) -> Option<kontra_ir::Source> {
    use kontra_ir::ModKind;
    match kind {
        ModKind::Envelope => None,
        ModKind::Volume { source, .. }
        | ModKind::Pitch { source, .. }
        | ModKind::Pan { source, .. }
        | ModKind::Cutoff { source, .. }
        | ModKind::Resonance { source, .. }
        | ModKind::EqGain { source, .. }
        | ModKind::SampleStart { source, .. }
        | ModKind::Attack { source, .. }
        | ModKind::Release { source, .. }
        | ModKind::LfoPhase { source, .. }
        | ModKind::LfoRate { source, .. }
        | ModKind::LfoHz { source, .. }
        | ModKind::Intensity { source, .. }
        | ModKind::LoopShift { source, .. }
        | ModKind::Mix { source, .. }
        | ModKind::EffectParam { source, .. }
        | ModKind::Send { source, .. } => Some(source),
    }
}
