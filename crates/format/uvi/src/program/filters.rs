//! A keygroup's filters. A `OnePole` (UVIWorkstation's factory names its modes
//! `lpf_1p` and `hpf_1p`, 0x14136aa50) is an exponential one-pole at
//! `Freq` Hz, 20 Hz to 20 kHz (coefficient 0x1412b4710, audio 0x1412b40d0).
//! A connection on `Freq` moves it along that range's log scale:
//! `Freq × 1000^(Ratio × s)`, `s` the source (0..=1), `1 − s` when
//! `Inverted` (the parameter tables pair 20 and 20000, 0x14258b220; v1's
//! native renders, crates/sampler-uvi/src/modulation.rs). Any source the
//! shared routes carry moves it the same way, `s` bipolar for an LFO.
//!
//! An `XpanderFilter` on any Algorithm with the soft saturator
//! (DistortionType 0) is the explicit ladder, its `Freq` on the same range:
//! Mode 0..=11 picks the taps (low 1..4 poles, high 1..4, band 2 and 4,
//! notch 2 and 4; the table filled by 0x140842a10), `Q` is the resonance
//! as is and `Drive` the input gain in dB (the setters around
//! 0x1412bef10, `10^(dB/20)`); `Fat` carries through.
//!
//! An `MS20` is the two-pole Sallen-Key on the same `Freq` range: `Morph`
//! mixes low to high, `Q` the resonance as is, `ReferenceVoltage` the drive,
//! and `ResonanceTrim` picks the feedback curve `[a, b]` between eleven
//! pairs (0x141e77630, read by the setup 0x1412ae600 at `10 × trim`).

use kontra_ir as ir;
use kontra_law::common::DB_GAIN;
use roxmltree::Node;

use super::{Library, Translator, flag, modulation, num};

impl<L: Library> Translator<'_, '_, L> {
    /// The routes on a keygroup's carried filters' `Freq`, as cutoffs, and
    /// on their `Q`, as resonance factors. A bypassed one moves nothing
    /// until a script turns it on. ponytail, unverified: `Q` takes the
    /// connection law of `Gain` ([`modulation::factor`]); in the Xpander's
    /// registration "Cutoff" (0x1412bcbe1) and "Q" (0x1412bccf9) share a
    /// constructor (0x140fffcb0) and differ only in Cutoff's `+0x144 = 10`,
    /// beside its `UviFreq` mapping, so Q keeps the default law.
    pub(super) fn cutoff_routes(&mut self, group: &mut ir::Group, kg: Node) {
        let program = self.program;
        let routes = freq_routes(kg)
            .into_iter()
            .filter_map(|(insert, c)| {
                let tag = c.parent()?.parent()?.tag_name().name();
                let q = c.attribute("Destination") == Some("Q") && tag != "OnePole";
                let mut r = if q {
                    modulation::factor(modulation::reach(program, c, (tag, "Q"))?, c)?
                } else {
                    modulation::reach(program, c, (tag, "Freq"))?
                };
                if flag(c, "Bypass") {
                    r.ratio = 0.0;
                }
                Some((insert, c, r, q))
            })
            .collect::<Vec<_>>();
        for (insert, c, r, q) in routes {
            self.add_route(group, c, r, |source, depth| match q {
                true => ir::ModKind::Resonance {
                    source,
                    insert,
                    depth,
                },
                false => ir::ModKind::Cutoff {
                    source,
                    insert,
                    depth,
                },
            });
        }
    }
}

/// Octaves of the `Freq` range a connection's ratio of 1 spans: 20 Hz to 20 kHz.
const FREQ_OCTAVES: f32 = 9.965_784;

/// The MS20 feedback curve `[a, b]` at `ResonanceTrim` 0, 0.1, .. 1.
#[rustfmt::skip]
const TRIM: [[f32; 2]; 11] = [
    [2.063_635, 1.314_278], [2.061_92, 1.385_688], [2.061_411, 1.463_379],
    [2.061_943, 1.548_671], [2.063_361, 1.643_228], [2.065_561, 1.749_079],
    [2.068_483, 1.868_755], [2.072_104, 2.005_497], [2.076_427, 2.163_567],
    [2.081_486, 2.348_711], [2.087_344, 2.568_901],
];

/// The curve at `trim` (0..=1), linear between the pairs.
fn trim_curve(trim: f64) -> [f32; 2] {
    let at = (trim.clamp(0.0, 1.0) * 10.0) as f32;
    let i = (at.floor() as usize).min(9);
    let [lo, hi] = [i, i + 1].map(|j| TRIM.get(j).copied().unwrap_or_default());
    let f = at - i as f32;
    [lo[0] + f * (hi[0] - lo[0]), lo[1] + f * (hi[1] - lo[1])]
}

/// An `XpanderFilter` on the explicit ladder with the soft saturator.
fn ladder_shape(node: Node) -> bool {
    // ponytail: Algorithm 1 (388 hits in Augmented Orchestra) plays as the
    // explicit ladder, the nearest mode here; reported by `report::effects`.
    num(node, "DistortionType", 0.0) == Ok(0.0)
        && num(node, "Mode", 3.0).is_ok_and(|m| (0.0..=11.0).contains(&m) && m.fract() == 0.0)
}

/// A keygroup insert this crate carries: an active `OnePole`, `XpanderFilter`
/// or `MS20` (as above) with no key tracking.
pub(super) fn carried(node: Node) -> bool {
    let xpander = node.has_tag_name("XpanderFilter") && ladder_shape(node);
    let comb = node.has_tag_name("CombFilter")
        && num(node, "Mode", 0.0).is_ok_and(|m| m == 0.0 || m == 1.0);
    (node.has_tag_name("OnePole") || node.has_tag_name("MS20") || xpander || comb)
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

/// An insert on a bus (a layer, aux or chain) that is an explicit ladder: the
/// same filter a keygroup runs.
pub(super) fn on_bus(node: Node) -> bool {
    node.has_tag_name("XpanderFilter") && ladder_shape(node) && !flag(node, "Bypass")
}

/// The filter at `node` (one `carried` or `on_bus` accepts), for slot `slot`.
pub(super) fn effect(node: Node, slot: usize) -> ir::Effect {
    if node.has_tag_name("CombFilter") {
        // One feedback comb at `Freq` Hz, `Q` its loop gain (unverified law,
        // as the bus comb); `Freq` moves on the same 20 Hz to 20 kHz range.
        let comb = ir::Comb {
            hz: (num(node, "Freq", 440.0).unwrap_or(440.0) as f32)
                .clamp(ir::COMB_LOWEST_HZ, 20_000.0),
            feedback: (num(node, "Q", 0.0).unwrap_or(0.0) as f32).clamp(0.0, 0.999),
            forward: num(node, "Mode", 0.0) == Ok(1.0),
            octaves: FREQ_OCTAVES,
        };
        return ir::Effect {
            kind: ir::EffectKind::Comb(comb),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        };
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
    if node.has_tag_name("OnePole") {
        filter.ladder = Some(ir::Ladder::OnePole);
    }
    if node.has_tag_name("XpanderFilter") {
        let q = num(node, "Q", 0.0).unwrap_or(0.0).clamp(0.0, 1.0) as f32;
        let drive = num(node, "Drive", 0.0).unwrap_or(0.0).clamp(-20.0, 20.0) as f32;
        filter.ladder = Some(ir::Ladder::Explicit {
            taps: mode as u8 + 1,
            fat: num(node, "Fat", 1.0).unwrap_or(1.0).clamp(0.0, 1.0) as f32,
        });
        filter.damping = [2.0 * (1.0 - q); 3];
        filter.gain = DB_GAIN.eval_unclamped(drive);
    }
    if node.has_tag_name("MS20") {
        let q = num(node, "Q", 0.0).unwrap_or(0.0).clamp(0.0, 1.0) as f32;
        filter.ladder = Some(ir::Ladder::SallenKey {
            morph: num(node, "Morph", 1.0).unwrap_or(1.0).clamp(0.0, 1.0) as f32,
            curve: trim_curve(num(node, "ResonanceTrim", 0.5).unwrap_or(0.5)),
        });
        filter.damping = [2.0 * (1.0 - q); 3];
        filter.gain = num(node, "ReferenceVoltage", 1.0).unwrap_or(1.0).max(0.01) as f32;
    }
    super::provenance::record("uvi.filter.v1", node, &slot, &filter);
    ir::Effect {
        kind: ir::EffectKind::Filter(filter),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    }
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
        let effect = effect(node, inserts.len());
        inserts.push(Some(effect));
    }
    inserts
}
