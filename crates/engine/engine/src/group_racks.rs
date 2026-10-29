//! A group's wet slots (a reverb, a delay, a compressor) on its summed
//! voices. Slots after the group's last per-voice unit (voice_rack.rs) see
//! only linear, fixed stages before them on each voice, so they run once on
//! the group's sum: the voices fold just the slots before that point into
//! their matrix, and the group's own chain runs the rest as a rack.
//!
//! ponytail: Kontakt runs group inserts per voice, slots before the group's
//! amp split ahead of its envelope (`fx_idx_amp_split_point`, not carried).
//! The sum here is exact for a linear slot after the split; one before it
//! rings on past the voice where Kontakt cuts it with the envelope. A wet
//! slot at or before a per-voice unit folds to its dry level (reported). A
//! summed tap cannot model that, because the envelope comes after the slot.
//! It would need a per-voice instance, and no corpus library (682 nki, 112
//! libraries, 2026-10-28) has an active group wet slot. Two save a 3x2
//! Versatile in a group (Una Corda Cotton and Felt, the room-hiss group),
//! bypassed and never enabled by their script (2026-10-29).

use kontra_ir as ir;

use crate::plan::Group;
use crate::voice_rack::Unit;

/// A kind with a wet path a voice matrix cannot carry.
fn wet(kind: ir::EffectKind) -> bool {
    matches!(
        kind,
        ir::EffectKind::Reverb(_)
            | ir::EffectKind::Convolution { .. }
            | ir::EffectKind::Delay(_)
            | ir::EffectKind::Phaser(_)
            | ir::EffectKind::Compressor(_)
            | ir::EffectKind::Limiter(_)
            | ir::EffectKind::Transient(_)
            | ir::EffectKind::Cascade(_)
    )
}

/// The first slot the group's sum runs, when an active wet slot sits at or
/// after it.
pub(crate) fn split(group: &Group, units: &[Unit]) -> Option<usize> {
    let from = units
        .iter()
        .map(|u| usize::from(u.insert) + 1)
        .max()
        .unwrap_or(0);
    group
        .inserts
        .get(from..)?
        .iter()
        .flatten()
        .any(|e| !e.bypass && wet(e.kind))
        .then_some(from)
}

/// The group's slots from `from` on; earlier ones are empty.
pub(crate) fn slots(group: &Group, from: usize) -> Vec<Option<ir::Effect>> {
    group
        .inserts
        .iter()
        .enumerate()
        .map(|(n, e)| e.filter(|_| n >= from))
        .collect()
}
