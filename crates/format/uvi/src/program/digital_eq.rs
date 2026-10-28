//! UVI's DigitalEq: sixteen bands, each at `Gain × GainScale` dB
//! (0x14128b838), designed by 0x1412840f0 at the order its Slope index
//! names ({1, 2, 3, 4, 6, 8, 12, 16}, table 0x141e731a8). Types: 0 low
//! pass, 1 high pass, 2 band pass, 3 notch, 4 low shelf, 5 high shelf,
//! 6 peak. A first-order peak is RBJ's with `alpha = tan(Δω/2)`,
//! `Δω = ω0·(2^(BW/2) − 2^(−BW/2))`; a second-order shelf has Q 1.
//! Connections on GainScale add to it (0x141429650: `value + r·m`), so
//! each moves every band's gain by its own `Gain` dB per unit.

use kontra_format::Notes;
use kontra_ir as ir;
use roxmltree::Node;

use super::{flag, index_of, matrix, num};

/// The EQ's carried bands, by slot, and each one's `Gain` at GainScale 1.
pub(super) struct Bands {
    pub(super) effect: ir::Effect,
    pub(super) gains: Vec<(u8, f32)>,
}

/// The enabled peak and shelf bands, at most four.
pub(super) fn digital_eq(node: Node, notes: &mut Notes) -> Result<Bands, String> {
    for name in ["StereoMode", "Transpose", "KeyTracking", "OverallGain"] {
        if num(node, name, 0.0)? != 0.0 {
            notes.add(ir::Impact::Audible, &format!("DigitalEq {name}"), "");
        }
    }
    let scale = num(node, "GainScale", 1.0)? as f32;
    let mut bells = [ir::Bell::FLAT; 4];
    let mut gains = Vec::new();
    for n in 1..=16 {
        let at = |name: &str| format!("{name}{n}");
        if !flag(node, &at("Enabled")) {
            continue;
        }
        if num(node, &at("Channels"), 0.0)? != 0.0 {
            notes.add(ir::Impact::Audible, "DigitalEq band on one channel", "");
        }
        let (kind, slope) = (num(node, &at("Type"), 6.0)?, num(node, &at("Slope"), 0.0)?);
        let shape = match kind as i32 {
            4 => ir::Shape::LowShelf,
            5 => ir::Shape::HighShelf,
            6 => ir::Shape::Peak,
            _ => {
                notes.add(ir::Impact::Audible, "DigitalEq filter band", "");
                continue;
            }
        };
        let width = num(node, &at("Bandwidth"), 1.0)?.max(1e-3) as f32;
        // ponytail: the analog bandwidth law, within 0.5% of the digital
        // one below 1.3 kHz at 48 kHz; orders above these approximated.
        let q = match (shape, slope as i32) {
            (ir::Shape::Peak, 0) => 1.0 / ((width / 2.0).exp2() - (-width / 2.0).exp2()),
            (ir::Shape::LowShelf | ir::Shape::HighShelf, 1) => 1.0,
            _ => {
                notes.add(ir::Impact::Audible, "DigitalEq band order", "");
                if shape == ir::Shape::Peak {
                    1.0 / ((width / 2.0).exp2() - (-width / 2.0).exp2())
                } else {
                    1.0
                }
            }
        };
        let Some(bell) = bells.get_mut(gains.len()) else {
            notes.add(ir::Impact::Audible, "DigitalEq band past the fourth", "");
            continue;
        };
        let gain = num(node, &at("Gain"), 0.0)? as f32;
        *bell = ir::Bell {
            shape,
            hz: num(node, &at("Freq"), 1000.0)?.clamp(20.0, 20_000.0) as f32,
            q,
            gain_db: gain * scale,
        };
        gains.push((gains.len() as u8, gain));
    }
    let effect = ir::Effect {
        kind: ir::EffectKind::Eq(bells),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    Ok(Bands { effect, gains })
}

/// GainScale's connections as routes on `bus`'s insert `insert`: one blend
/// of them moving each band by its `Gain`. Returns the connections carried.
pub(super) fn gain_scale<'a, 'i>(
    program: Node<'a, 'i>,
    node: Node<'a, 'i>,
    bands: &Bands,
    insert: u8,
    bus: &mut ir::Bus,
) -> Vec<Node<'a, 'i>> {
    let connections = node
        .children()
        .filter(|c| c.has_tag_name("Connections"))
        .flat_map(|c| c.children())
        .filter(|c| c.has_tag_name("SignalConnection") && !flag(*c, "Bypass"))
        .filter(|c| c.attribute("Destination") == Some("GainScale"));
    let mut blend = ir::Blend {
        base: 0.0,
        terms: Vec::new(),
    };
    let mut carried = Vec::new();
    for c in connections {
        if let Some(term) = matrix::term(program, c, None) {
            blend.terms.push(term);
            carried.push(c);
        }
    }
    if blend.terms.is_empty() || bands.gains.is_empty() {
        return carried;
    }
    let source = ir::Source::Blend(index_of(&mut bus.blends, blend));
    for &(band, gain) in &bands.gains {
        bus.mods.push(ir::ModKind::EffectParam {
            source,
            insert,
            param: ir::EffectParam::BellGain(band),
            depth: gain,
            span: 1.0,
            unipolar: false,
        });
    }
    carried
}
