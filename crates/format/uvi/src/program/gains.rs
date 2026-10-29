//! A keygroup's and its players' connections on `Gain`: the keygroup's
//! gain law (UVIWorkstation 0x141429a80, [`modulation::factor`]) as group
//! Volume routes, and one a player has alone as a factor on its zones' cells.

use kontra_ir as ir;
use roxmltree::Node;

use super::{Library, Translator, blend, flag, index_of, matrix, modulation, source_node};

impl<L: Library> Translator<'_, '_, L> {
    /// A keygroup's gain routes and those its players have alike
    /// ([`player_gains`]), each as a Volume route on its group.
    pub(super) fn gain_routes(&mut self, group: &mut ir::Group, kg: Node) {
        let program = self.program;
        let mut gains = matrix::children(kg)
            .filter(|c| c.attribute("Destination") == Some("Gain"))
            .filter_map(|c| {
                let r =
                    modulation::gain_route(program, c).or_else(|| macro_gain(program, c, group));
                Some((c, r?, Vec::new()))
            })
            .collect::<Vec<_>>();
        gains.extend(player_gains(program, kg, group));
        for (c, r, twins) in gains {
            let rides = twins.iter().flat_map(|t| blend::carries(*t, &r));
            self.carried.extend(rides.collect::<Vec<_>>());
            self.add_route(group, c, r, |source, intensity| ir::ModKind::Volume {
                source,
                intensity,
                curve: ir::Curve::Signed,
                tau_ms: 0.0,
            });
            if let Some(&key) = self.routed.get(&c.id()) {
                twins
                    .iter()
                    .for_each(|t| _ = self.routed.insert(t.id(), key));
            }
        }
    }
}

impl<L: Library> Translator<'_, '_, L> {
    /// A player's connections on its `Gain` that [`player_gains`] does not
    /// carry, as they differ from its keygroup's other players' (VWinds'
    /// brightness and noise crossfades), each as the factor the gain law
    /// gives it ([`modulation::factor`]): `1 − k + k·x`, or `1 + k·x` below
    /// zero, folded into the source's table (it is linear, so exact).
    pub(super) fn own_gains(&mut self, player: Node, group: &mut ir::Group) -> Vec<ir::Source> {
        let program = self.program;
        let mut own = Vec::new();
        for c in matrix::children(player).filter(|c| c.attribute("Destination") == Some("Gain")) {
            let amp = source_node(program, c).is_some_and(|n| n.has_tag_name("DAHDSR"));
            if amp || self.routed.contains_key(&c.id()) || self.carried.contains(&c.id()) {
                continue;
            }
            let r = modulation::route(program, c, ("SamplePlayer", "Gain"));
            let r = r.and_then(|r| modulation::factor(r, c));
            let Some(r) = r.or_else(|| macro_gain(program, c, group)) else {
                continue;
            };
            self.carried.extend(blend::carries(c, &r));
            self.carried.insert(c.id());
            let k = (r.unit * r.ratio).clamp(-1.0, 1.0);
            let law = |x: f32| {
                if k < 0.0 {
                    1.0 + k * x
                } else {
                    1.0 - k + k * x
                }
            };
            let t = match modulation::group_source(group, r.source, r.timed) {
                ir::Source::Shaped(i) => {
                    let Some(mut t) = group.shapers.get(usize::from(i)).cloned() else {
                        continue;
                    };
                    t.table = if t.table.is_empty() {
                        vec![law(0.0), law(1.0)]
                    } else {
                        t.table.iter().map(|&v| law(v)).collect()
                    };
                    t
                }
                source => ir::Transfer {
                    source,
                    table: vec![law(0.0), law(1.0)],
                },
            };
            own.push(ir::Source::Shaped(index_of(&mut group.shapers, t)));
        }
        own
    }
}

/// [`Translator::own_gains`] on every cell of zone `zone`'s `mix` (over
/// `channels` sample channels), which an unmixed zone gets written out:
/// mono on both sides, else the first two channels left and right.
pub(super) fn own_mix(
    own: &[ir::Source],
    mix: &mut Vec<[f32; 2]>,
    channels: usize,
    zone: u16,
) -> Vec<ir::Mod> {
    if own.is_empty() {
        return Vec::new();
    }
    if mix.is_empty() {
        *mix = (0..channels)
            .map(|c| match (channels, c) {
                (1, _) => [1.0, 1.0],
                (_, 0) => [1.0, 0.0],
                (_, 1) => [0.0, 1.0],
                _ => [0.0, 0.0],
            })
            .collect();
    }
    let cells = mix.iter().enumerate().flat_map(|(row, r)| {
        let on = r.iter().enumerate().filter(|(_, g)| **g != 0.0);
        on.map(move |(side, _)| (row, side))
    });
    let cells: Vec<_> = cells.collect();
    own.iter()
        .flat_map(|&source| {
            cells.iter().map(move |&(row, side)| ir::Mod {
                name: String::new(),
                slot: 0,
                target: 0,
                bypass: false,
                kind: ir::ModKind::Mix {
                    source,
                    zone,
                    row: u8::try_from(row).unwrap_or(u8::MAX),
                    side: side as u8,
                },
            })
        })
        .collect()
}

/// The connections on the `Gain` of a keygroup's enabled players that every
/// one of them has alike, each as the route it makes (the keygroup's gain
/// law, a voice playing one player) and the other players' copies. One that
/// differs between players (VWinds' pitch-bend crossfade) is not carried.
pub(super) fn player_gains<'a, 'i>(
    program: Node,
    kg: Node<'a, 'i>,
    group: &mut ir::Group,
) -> Vec<(Node<'a, 'i>, modulation::Route, Vec<Node<'a, 'i>>)> {
    let players: Vec<_> = kg
        .children()
        .filter(|n| n.has_tag_name("Oscillators"))
        .flat_map(|o| o.children())
        .filter(|n| n.has_tag_name("SamplePlayer") && !flag(*n, "Bypass"))
        .collect();
    let gains = |p: Node<'a, 'i>| {
        matrix::children(p).filter(|c| c.attribute("Destination") == Some("Gain"))
    };
    let Some((first, rest)) = players.split_first() else {
        return Vec::new();
    };
    gains(*first)
        .filter_map(|c| {
            let twin = |p: &Node<'a, 'i>| gains(*p).find(|t| alike(c, *t));
            let twins = rest.iter().map(twin).collect::<Option<Vec<_>>>()?;
            let amp = source_node(program, c).is_some_and(|n| n.has_tag_name("DAHDSR"));
            let r = modulation::route(program, c, ("SamplePlayer", "Gain")).filter(|_| !amp);
            let r = r.and_then(|r| modulation::factor(r, c));
            Some((c, r.or_else(|| macro_gain(program, c, group))?, twins))
        })
        .collect()
}

/// A macro's connection on a `Gain` as the factor the law gives it
/// ([`matrix::live`], VWinds' AttacksMapper), at ratio 1: `1 − 1 + x`.
fn macro_gain(program: Node, c: Node, group: &mut ir::Group) -> Option<modulation::Route> {
    Some(modulation::Route {
        source: matrix::live(program, c, group)?,
        timed: None,
        unit: 1.0,
        ratio: 1.0,
    })
}

/// Whether two connections are the same but for their names.
fn alike(a: Node, b: Node) -> bool {
    let named = |x: &roxmltree::Attribute| x.name() != "Name";
    let kids = |n: Node| n.children().filter(Node::is_element).count();
    a.tag_name() == b.tag_name()
        && a.attributes()
            .filter(named)
            .map(|x| (x.name(), x.value()))
            .eq(b.attributes().filter(named).map(|x| (x.name(), x.value())))
        && kids(a) == kids(b)
        && a.children()
            .filter(Node::is_element)
            .zip(b.children().filter(Node::is_element))
            .all(|(p, q)| alike(p, q))
}
