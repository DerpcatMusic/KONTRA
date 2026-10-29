//! Routes on an LFO's own parameters: its depth and its rate.

use kontra_ir as ir;
use roxmltree::Node;

use super::modulation::{Route, Timed, route};
use super::{Library, Translator, flag, matrix, source_node};

impl<L: Library> Translator<'_, '_, L> {
    /// The connections on the `Depth` of the LFO `connection` reads, as
    /// [`ir::ModKind::Intensity`] routes on its group copy `of`: the
    /// connection law (UVIWorkstation 0x141429a80) scales the depth by
    /// `1 − r(1 − x)`, or `1 + r·x` below zero, where Intensity reads `1 − x`
    /// at `−r` (unit −1, so a script's ratio keeps its sign). ponytail: a
    /// script turning a depth ratio's sign keeps its inversion.
    pub(super) fn depth_scales(&mut self, group: &mut ir::Group, connection: Node, of: ir::Source) {
        // An LFO read through a table (a gain's `factor`) scales the same way.
        let of = match of {
            ir::Source::Shaped(n) => group.shapers.get(usize::from(n)).map_or(of, |t| t.source),
            of => of,
        };
        if !matches!(of, ir::Source::Lfo(_)) {
            return;
        }
        let Some(lfo) = source_node(self.program, connection) else {
            return;
        };
        // A rate route's own source keeps its rate (no recursion).
        if connection.attribute("Destination") != Some("Freq") {
            self.rate_routes(group, lfo, of);
        }
        // ponytail: an Intensity scales the wave around 0, a unipolar LFO's
        // depth its `m`; carried when a library routes one (none does).
        if lfo.has_tag_name("LFO") && !matrix::bipolar(lfo) {
            self.notes
                .add(ir::Impact::Audible, "unipolar LFO depth route", "");
            return;
        }
        let owner = lfo.tag_name().name();
        let depths = lfo
            .children()
            .filter(|c| c.has_tag_name("Connections"))
            .flat_map(|c| c.children())
            .filter_map(|c| Some((c, depth_route(self.program, c, owner)?)))
            .collect::<Vec<_>>();
        for (c, r) in depths {
            self.add_route(group, c, r, |source, depth| ir::ModKind::Intensity {
                source,
                of,
                depth,
            });
        }
    }

    /// The connections on the `Freq` of the free-running `lfo`, as
    /// [`ir::ModKind::LfoHz`] routes on its group copy `of`. `Freq` is a
    /// normalized 0..=1 parameter (default 0.5, LFO constructor 0x1410f629e)
    /// over 0..=20 Hz (range 0x14258aad8), linear when unsynced (0x14134eeae);
    /// a connection adds `r·m` to it (0x141429650), held within the range.
    /// ponytail: a synced `Freq` rounds to a beat division (0x140dc84e0) and
    /// is reported.
    fn rate_routes(&mut self, group: &mut ir::Group, lfo: Node, of: ir::Source) {
        let ir::Source::Lfo(n) = of else { return };
        let owner = lfo.tag_name().name();
        let rates = lfo
            .children()
            .filter(|c| c.has_tag_name("Connections"))
            .flat_map(|c| c.children())
            .filter_map(|c| Some((c, route(self.program, c, (owner, "Freq"))?)))
            .collect::<Vec<_>>();
        for (c, r) in rates {
            if flag(lfo, "SyncToHost") {
                self.notes
                    .add(ir::Impact::Audible, "synced LFO rate route", "");
                continue;
            }
            self.add_route(group, c, r, |source, depth| ir::ModKind::LfoHz {
                source,
                lfo: n,
                depth,
                range_hz: [0.0, 20.0],
            });
        }
    }
}

/// A connection on an LFO's `Depth` ([`Translator::depth_scales`]): its
/// source on 0..=1, read `1 − x` for a negative ratio, clamped to ±1. An LFO
/// source enters at `(d·x + 1)/2`, its own depth `d` inside. Not an
/// envelope or a rising LFO read backwards.
fn depth_route(program: Node, connection: Node, owner: &str) -> Option<Route> {
    let mut r = route(program, connection, (owner, "Depth"))?;
    r.ratio = r.ratio.clamp(-1.0, 1.0);
    let falling = r.ratio < 0.0;
    let flip = |t: Vec<f32>| -> Vec<f32> {
        if falling {
            t.into_iter().map(|v| 1.0 - v).collect()
        } else {
            t
        }
    };
    r.timed = match r.timed.take() {
        Some(Timed::Lfo(lfo)) => {
            let d = r.unit;
            Some(Timed::Steps(
                lfo,
                flip(vec![(1.0 - d) / 2.0, (1.0 + d) / 2.0]),
            ))
        }
        Some(Timed::Steps(lfo, table)) => Some(Timed::Steps(lfo, flip(table))),
        Some(Timed::Mapped(mut t)) => {
            if t.table.is_empty() {
                t.table = vec![0.0, 1.0];
            }
            t.table = flip(t.table);
            Some(Timed::Mapped(t))
        }
        None if falling => Some(Timed::Mapped(ir::Transfer {
            source: r.source,
            table: vec![1.0, 0.0],
        })),
        None => None,
        Some(Timed::Envelope(_) | Timed::Rising(..) | Timed::Blend(_)) if falling => return None,
        timed => timed,
    };
    if r.timed.is_some() {
        r.source = ir::Source::Constant;
    }
    r.unit = if falling { -1.0 } else { 1.0 };
    Some(r)
}
