//! A group's wet slots (a reverb, a delay, a compressor) on its summed
//! voices. Slots after the group's last per-voice unit (voice_rack.rs) see
//! only linear, fixed stages before them on each voice, so they run once on
//! the group's sum: the voices fold just the slots before that point into
//! their matrix, and the group's own chain runs the rest as a rack.
//!
//! Respects `group.amp_split`: when no active wet slot precedes the amp
//! split, the summed group rack starts at `max(last_voice_unit + 1, amp_split)`
//! so pre-amp linear slots stay before the voice's amp stage while post-amp
//! wet slots run on the post-amp group sum.

use kontra_ir as ir;

use kontra_plan::Group;

use crate::voice_rack::Unit;

/// A kind with a wet path a voice matrix cannot carry.
fn wet(kind: &ir::EffectKind) -> bool {
    matches!(
        kind,
        ir::EffectKind::Reverb(_)
            | ir::EffectKind::Convolution { .. }
            | ir::EffectKind::Delay(_)
            | ir::EffectKind::Scanner(_)
            | ir::EffectKind::Shifter(_)
            | ir::EffectKind::Phaser(_)
            | ir::EffectKind::Compressor(_)
            | ir::EffectKind::Limiter(_)
            | ir::EffectKind::Transient(_)
            | ir::EffectKind::Cascade(_)
            | ir::EffectKind::Clocked(_)
            | ir::EffectKind::RingMod(_)
    )
}

/// The first slot the group's sum runs, when an active wet slot sits at or
/// after it.
pub fn split(group: &Group, units: &[Unit]) -> Option<usize> {
    let after_units = units
        .iter()
        .map(|u| usize::from(u.insert) + 1)
        .max()
        .unwrap_or(0);
    let amp_split = group.amp_split_index();
    let has_wet =
        |slice: &[Option<ir::Effect>]| slice.iter().flatten().any(|e| !e.bypass && wet(&e.kind));
    if after_units < amp_split
        && has_wet(
            group
                .inserts
                .get(after_units..amp_split)
                .unwrap_or_default(),
        )
    {
        return Some(after_units);
    }
    let post = after_units.max(amp_split);
    has_wet(group.inserts.get(post..).unwrap_or_default()).then_some(post)
}

/// The group's slots from `from` on; earlier ones are empty.
pub fn slots(group: &Group, from: usize) -> Vec<Option<ir::Effect>> {
    group
        .inserts
        .iter()
        .enumerate()
        .map(|(n, e)| if n >= from { e.clone() } else { None })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_starts_at_amp_split_when_only_post_amp_slots_are_wet() {
        let mut g = Group::standard();
        g.amp_split = 6;
        g.inserts = vec![None; 8];
        g.inserts[0] = Some(ir::Effect {
            kind: ir::EffectKind::Invert {
                phase: false,
                swap: true,
            },
            bypass: false,
            output: 1.0,
            dry: 0.0,
        });
        g.inserts[6] = Some(ir::Effect {
            kind: ir::EffectKind::Compressor(ir::Compressor {
                threshold_db: -20.0,
                ratio: 4.0,
                knee_db: 0.0,
                attack_ms: 5.0,
                release_ms: 50.0,
                makeup_db: 0.0,
                makeup_slope: 0.0,
                link: true,
                mix: 1.0,
                detector: ir::Detector::Input,
                sidechain_hz: None,
            }),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        });
        assert_eq!(split(&g, &[]), Some(6));
    }
}
