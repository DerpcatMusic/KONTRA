//! A keygroup's filters. A `OnePole` (UVIWorkstation's factory names its modes
//! `lpf_1p` and `hpf_1p`, 0x14136aa50) is a one-pole at `Freq` Hz, 20 Hz to
//! 20 kHz. A connection on `Freq` moves it along that range's log scale:
//! `Freq × 1000^(Ratio × s)`, `s` the source (0..=1), `1 − s` when
//! `Inverted` (the parameter tables pair 20 and 20000, 0x14258b220; v1's
//! native renders, crates/sampler-uvi/src/modulation.rs). Any source the
//! shared routes carry moves it the same way, `s` bipolar for an LFO.
//!
//! An `XpanderFilter` on Algorithm 0 with the soft saturator
//! (DistortionType 0) is the explicit ladder, its `Freq` on the same range:
//! Mode 0..=11 picks the taps (low 1..4 poles, high 1..4, band 2 and 4,
//! notch 2 and 4; the table filled by 0x140842a10), `Q` is the resonance
//! as is and `Drive` the input gain in dB (the setters around
//! 0x1412bef10, `10^(dB/20)`); `Fat` carries through.

use kontra_ir as ir;
use roxmltree::Node;

use super::{Library, Translator, flag, modulation, num};

impl<L: Library> Translator<'_, '_, L> {
    /// The routes on a keygroup's carried filters' `Freq`, as cutoffs. A
    /// bypassed one moves nothing until a script turns it on.
    pub(super) fn cutoff_routes(&mut self, group: &mut ir::Group, kg: Node) {
        let program = self.program;
        let cutoffs = freq_routes(kg)
            .into_iter()
            .filter_map(|(insert, c)| {
                let tag = c.parent()?.parent()?.tag_name().name();
                let mut r = modulation::reach(program, c, (tag, "Freq"))?;
                if flag(c, "Bypass") {
                    r.ratio = 0.0;
                }
                Some((insert, c, r))
            })
            .collect::<Vec<_>>();
        for (insert, c, r) in cutoffs {
            self.add_route(group, c, r, |source, depth| ir::ModKind::Cutoff {
                source,
                insert,
                depth,
            });
        }
    }
}

/// Octaves of the `Freq` range a connection's ratio of 1 spans: 20 Hz to 20 kHz.
const FREQ_OCTAVES: f32 = 9.965_784;

/// A keygroup insert this crate carries: an active `OnePole` or `XpanderFilter`
/// (as above) with no key tracking.
pub(super) fn carried(node: Node) -> bool {
    let xpander = node.has_tag_name("XpanderFilter")
        && num(node, "Algorithm", 0.0) == Ok(0.0)
        && num(node, "DistortionType", 0.0) == Ok(0.0)
        && num(node, "Mode", 3.0).is_ok_and(|m| (0.0..=11.0).contains(&m) && m.fract() == 0.0);
    (node.has_tag_name("OnePole") || xpander)
        && !flag(node, "Bypass")
        && !flag(node, "KeyTracking")
        && node
            .parent()
            .filter(|p| p.has_tag_name("Inserts"))
            .and_then(|p| p.parent())
            .is_some_and(|p| p.has_tag_name("Keygroup"))
}

/// A keygroup's carried filters' connections, each with its filter's slot.
fn freq_routes<'a, 'i>(kg: Node<'a, 'i>) -> Vec<(u8, Node<'a, 'i>)> {
    kg.children()
        .filter(|n| n.has_tag_name("Inserts"))
        .flat_map(|n| n.children())
        .filter(Node::is_element)
        .enumerate()
        .filter(|(_, node)| carried(*node))
        .filter_map(|(n, node)| {
            let insert = u8::try_from(n).ok()?;
            Some(
                node.children()
                    .filter(|c| c.has_tag_name("Connections"))
                    .flat_map(|c| c.children())
                    .filter(move |c| c.is_element())
                    .map(move |c| (insert, c)),
            )
        })
        .flatten()
        .collect()
}

/// A keygroup's inserts by slot (a slot not carried passes its input).
pub(super) fn keygroup(kg: Node) -> Vec<Option<ir::Effect>> {
    let mut inserts = Vec::new();
    let slots = kg
        .children()
        .filter(|n| n.has_tag_name("Inserts"))
        .flat_map(|n| n.children())
        .filter(Node::is_element);
    for node in slots {
        if !carried(node) {
            inserts.push(None);
            continue;
        }
        let hz = num(node, "Freq", 20_000.0).unwrap_or(20_000.0) as f32;
        let mode = num(node, "Mode", 0.0).unwrap_or(0.0);
        let mut filter = ir::Filter {
            pass: if mode == 1.0 {
                ir::Pass::High
            } else {
                ir::Pass::Low
            },
            hz: hz.clamp(20.0, 20_000.0),
            stages: 0,
            ladder: None,
            damping: [2.0; 3],
            gain: 1.0,
            octaves: FREQ_OCTAVES,
            q_octaves: 0.0,
        };
        if node.has_tag_name("XpanderFilter") {
            let q = num(node, "Q", 0.0).unwrap_or(0.0).clamp(0.0, 1.0) as f32;
            let drive = num(node, "Drive", 0.0).unwrap_or(0.0).clamp(-20.0, 20.0) as f32;
            filter.ladder = Some(ir::Ladder::Explicit {
                taps: mode as u8 + 1,
                fat: num(node, "Fat", 1.0).unwrap_or(1.0).clamp(0.0, 1.0) as f32,
            });
            filter.damping = [2.0 * (1.0 - q); 3];
            filter.gain = 10f32.powf(drive / 20.0);
        }
        inserts.push(Some(ir::Effect {
            kind: ir::EffectKind::Filter(filter),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        }));
    }
    inserts
}
