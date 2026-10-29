//! Routes a group reads as a blend ([`ir::Blend`]): a user-table LFO, whose
//! depth routes scale its output, and a route whose ratio a key scales per
//! voice. Also the unipolar LFO, a table over its wave.

use kontra_ir as ir;
use roxmltree::{Node, NodeId};

use super::modulation::{self, Route, Timed};
use super::{flag, index_of, matrix, num};

/// A route's value: `base` plus each part, a term whose source is the
/// group's copy of its LFO if it has one, all times `keys` (its ratio's key
/// factors, [`keyed`]). `bipolar`: the value is an LFO's `m` on −1..=1.
/// `depths`: the `Depth` connections its factors carry.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Blend {
    base: f32,
    parts: Vec<(Option<ir::Lfo>, ir::Term)>,
    keys: Vec<ir::Factor>,
    bipolar: bool,
    depths: Vec<NodeId>,
}

/// The connections `r` carries besides its own: those on its ratio, and
/// for a blend the `Depth` connections of its LFO.
pub(super) fn carries(connection: Node, r: &Route) -> Vec<NodeId> {
    let ratio =
        matrix::children(connection).filter(|c| c.attribute("Destination") == Some("Ratio"));
    let mut ids: Vec<_> = ratio.map(|c| c.id()).collect();
    if let Some(Timed::Blend(b)) = &r.timed {
        ids.extend(&b.depths);
    }
    ids
}

/// `None` for an LFO read as a wave, else what a route reads of one read
/// through a table (`None` inside when it is not carried):
/// - unipolar: `m = (w + 1)·d/2` (process 0x1410f9b50, `Depth` `d`), as a
///   table over its wave; a route takes `m` itself, inverted `1 − m`
///   (0x141429a80). ponytail: none with a delay or rise (it holds the wave
///   at −1, then pulls it up from there: 0x1410f8ff0, 0x1410f98a0), nor
///   with depth routes; no library routes one.
/// - `WaveFormType` 9, its `UserTable`: 256 points a cycle, the phase's top
///   byte indexing them and its low 24 bits between two (0x1410f9470), here
///   a saw read through them; `m` is `d` times that times each connection on
///   its `Depth` (see [`depths`]), inverted `−m`. Not with a delay or rise.
pub(super) fn table(program: Node, node: Node, inverted: bool) -> Option<Option<Timed>> {
    let user = num(node, "WaveFormType", 0.0).is_ok_and(|w| w == 9.0);
    if !node.has_tag_name("LFO") || (matrix::bipolar(node) && !user) {
        return None;
    }
    let still = !flag(node, "DelayTime") && !flag(node, "RiseTime");
    let shape = still.then(|| modulation::shape(node, user)).flatten();
    if !matrix::bipolar(node) {
        return Some(shape.map(|(lfo, d)| {
            // A negative depth is a rising saw read backwards ([`modulation::shape`]).
            let flip = |v: f32| if inverted { 1.0 - v } else { v };
            let (lo, hi) = if d < 0.0 { (-d, 0.0) } else { (0.0, d) };
            Timed::Steps(lfo, vec![flip(lo), flip(hi)])
        }));
    }
    let user = || {
        let (lfo, d) = shape?;
        let points: Vec<f32> = node
            .children()
            .find(|c| c.has_tag_name("UserTable"))?
            .text()?
            .split_whitespace()
            .map(|v| {
                v.replace(',', ".")
                    .parse()
                    .ok()
                    .filter(|v: &f32| v.is_finite())
            })
            .collect::<Option<_>>()?;
        let n = points.len();
        // The saw reads `1 − p`: entry `j` is the point at phase `1 − j/n`.
        let read = (0..=n).map(|j| points.get((n - j) % n).copied());
        let input = ir::Transfer {
            source: ir::Source::Constant,
            table: read.collect::<Option<_>>().filter(|_| n > 0)?,
        };
        let sign = if inverted { -1.0 } else { 1.0 };
        let (factors, depths) = depths(program, node)?;
        let term = ir::Term {
            input,
            amount: sign * d,
            factors,
        };
        Some(Timed::Blend(Blend {
            base: 0.0,
            parts: vec![(Some(lfo), term)],
            keys: Vec::new(),
            bipolar: true,
            depths,
        }))
    };
    Some(user())
}

/// The connections on an LFO's `Depth` as factors on its output: the
/// connection law (0x141429a80) scales the depth, so `m` (0x1410f9b11),
/// by `1 − r(1 − x)`, or `1 + r·x` below zero, `x` the source through its
/// mapper. None for one from an LFO or with a live ratio.
fn depths(program: Node, node: Node) -> Option<(Vec<ir::Factor>, Vec<NodeId>)> {
    let on_depth = node
        .children()
        .filter(|c| c.has_tag_name("Connections"))
        .flat_map(|c| c.children())
        .filter(|c| c.has_tag_name("SignalConnection") && !flag(*c, "Bypass"))
        .filter(|c| c.attribute("Destination") == Some("Depth"));
    let on_depth: Vec<_> = on_depth
        .filter(|c| !num(*c, "Ratio", 1.0).is_ok_and(|r| r == 0.0))
        .collect();
    let factors = on_depth.iter().map(|&c| {
        let input = matrix::transfer(program, c, None)?;
        let lfo = matches!(input.source, ir::Source::Lfo(_));
        (!lfo && !flag(c, "Inverted") && !flag(c, "ConnectionMode")).then_some(())?;
        Some(ir::Factor {
            input,
            depth: matrix::held_ratio(program, c)?.clamp(-1.0, 1.0),
        })
    });
    Some((
        factors.collect::<Option<_>>()?,
        on_depth.iter().map(|c| c.id()).collect(),
    ))
}

/// `route` with the key connections on its ratio ([`matrix::key_factor`])
/// scaling it per voice: its value as a blend, times each. None when that
/// value is not a source through a table, an LFO or a blend.
pub(super) fn keyed(program: Node, connection: Node, route: Route) -> Option<Route> {
    let keys: Vec<_> = matrix::children(connection)
        .filter(|f| f.attribute("Destination") == Some("Ratio"))
        .filter_map(|f| matrix::key_factor(program, f))
        .collect();
    if keys.is_empty() {
        return Some(route);
    }
    let term = |input, factors| ir::Term {
        input,
        amount: route.unit,
        factors,
    };
    let mut ids = Vec::new();
    let (lfo, term, bipolar) = match route.timed {
        Some(Timed::Blend(mut b)) => {
            b.keys.extend(keys);
            return Some(Route {
                timed: Some(Timed::Blend(b)),
                ..route
            });
        }
        Some(Timed::Mapped(t)) => (None, term(t, Vec::new()), matrix::centres(connection)),
        None => {
            let t = ir::Transfer {
                source: route.source,
                table: Vec::new(),
            };
            (
                None,
                term(t, Vec::new()),
                route.source == ir::Source::PitchBend,
            )
        }
        // The table gives the LFO's `x` back.
        Some(Timed::Lfo(lfo)) => {
            let node = super::source_node(program, connection)?;
            let t = ir::Transfer {
                source: ir::Source::Constant,
                table: vec![-1.0, 1.0],
            };
            let (factors, depths) = depths(program, node)?;
            ids = depths;
            (Some(lfo), term(t, factors), true)
        }
        _ => return None,
    };
    Some(Route {
        source: ir::Source::Constant,
        timed: Some(Timed::Blend(Blend {
            base: 0.0,
            parts: vec![(lfo, term)],
            keys,
            bipolar,
            depths: ids,
        })),
        unit: 1.0,
        ratio: route.ratio,
    })
}

/// `b` as the source `x` of a multiplicative destination at ratio `r`
/// ([`modulation::factor`]): a bipolar value enters at `(m + 1)/2`. The key
/// factors `k` scale the ratio, not `x`: `1 − rk + rk·x` is `1 − r + r·x'`
/// with `x' = 1 − k + k·x` (`k·x` below zero).
pub(super) fn gain(mut b: Blend, r: f32) -> Blend {
    if b.bipolar {
        b.base = (b.base + 1.0) / 2.0;
        b.parts.iter_mut().for_each(|(_, t)| t.amount /= 2.0);
        b.bipolar = false;
    }
    if b.keys.is_empty() {
        return b;
    }
    let lift = if r < 0.0 { 0.0 } else { 1.0 };
    let constant = ir::Term {
        input: ir::Transfer {
            source: ir::Source::Constant,
            table: Vec::new(),
        },
        amount: b.base - lift,
        factors: Vec::new(),
    };
    b.parts.push((None, constant));
    let keys = std::mem::take(&mut b.keys);
    b.parts
        .iter_mut()
        .for_each(|(_, t)| t.factors.extend(keys.clone()));
    b.base = lift;
    b
}

/// `b` in `group`, its LFOs copied onto it.
pub(super) fn lower(group: &mut ir::Group, b: Blend) -> ir::Source {
    let terms = b.parts.into_iter().map(|(lfo, mut t)| {
        if let Some(lfo) = lfo {
            t.input.source = ir::Source::Lfo(index_of(&mut group.lfos, lfo));
        }
        t.factors.extend(b.keys.iter().cloned());
        t
    });
    let blend = ir::Blend {
        base: b.base,
        terms: terms.collect(),
    };
    ir::Source::Blend(index_of(&mut group.blends, blend))
}
