//! A keygroup GainMatrix's cells and the macros that crossfade them.
//!
//! UVI scales a cell by each multiplicative connection on it
//! (`0x141429a80`): `(r < 0 ? 1 : 1 - r) + r * m`, `r` the ratio clamped to
//! ±1 and `m` its source through the connection's mapper. A macro
//! (`ConstantModulation`) is its `Value` plus each additive connection on
//! it (`0x141429650`: `value + r * m`), each of those ratios scaled the
//! same multiplicative way. The mapper (`0x1411390c0`) reads a bipolar
//! source at `(x + 1) / 2` of its table, linearly between points, and
//! scales by `Max` (or `-Min` below zero). It indexes `len - 2` of its
//! in-memory table; read here as `len - 1` of the saved one, assuming a
//! guard copy of the last point (saved bells peak on their last point, so
//! it is reachable). A variant crossfade (VWinds' "main brightness") is one
//! bell-shaped mapper per variant over the macro's range.

use kontra_ir as ir;
use roxmltree::Node;

use super::{flag, index_of, modulation, num, source_node};

/// The factor a cell's connection holds it at: a macro at its saved value,
/// one with live inputs only when `saved` ([`live`] carries those). None
/// for any other source, a modulated or inverted connection, or a stepped
/// mapper.
pub(super) fn held(program: Node, connection: Node, saved: bool) -> Option<f32> {
    if flag(connection, "Bypass") {
        return Some(1.0);
    }
    let source = source_node(program, connection)?;
    if !source.has_tag_name("ConstantModulation") || (!saved && inputs(source).next().is_some()) {
        return None;
    }
    let (r, table) = cell_law(connection)?;
    let x = position(source, num(source, "Value", 0.0).ok()? as f32)?;
    Some(scale(r, read(&table, x).unwrap_or(x)))
}

/// A cell's connection from a macro that has live inputs, as the source of
/// its live factor: the macro as a blend of its inputs, read through the
/// connection's mapper and law. None when an input is not carried.
pub(super) fn live(program: Node, connection: Node, group: &mut ir::Group) -> Option<ir::Source> {
    let source = source_node(program, connection)?;
    if flag(connection, "Bypass") || !source.has_tag_name("ConstantModulation") {
        return None;
    }
    let (r, table) = cell_law(connection)?;
    // A bipolar macro is read at `(x + 1) / 2`: the blend sums positions.
    let half = if flag(source, "Bipolar") { 0.5 } else { 1.0 };
    let mut blend = ir::Blend {
        base: position(source, num(source, "Value", 0.0).ok()? as f32)?,
        terms: Vec::new(),
    };
    for input in inputs(source) {
        if num(input, "Ratio", 1.0).ok()? == 0.0 {
            continue;
        }
        if flag(input, "Inverted") || flag(input, "ConnectionMode") {
            return None;
        }
        let mut amount = num(input, "Ratio", 1.0).ok()? as f32 * half;
        let mut factors = Vec::new();
        for f in children(input).filter(|f| f.attribute("Destination") == Some("Ratio")) {
            if flag(f, "Inverted") || flag(f, "ConnectionMode") || children(f).next().is_some() {
                return None;
            }
            let depth = (num(f, "Ratio", 1.0).ok()? as f32).clamp(-1.0, 1.0);
            let factor = ir::Factor {
                input: transfer(program, f, group)?,
                depth,
            };
            match factor.input.source {
                ir::Source::Constant => amount *= factor.at(factor.input.apply(1.0)),
                _ => factors.push(factor),
            }
        }
        let input = transfer(program, input, group)?;
        if input.source == ir::Source::Constant {
            return None;
        }
        blend.terms.push(ir::Term {
            input,
            amount,
            factors,
        });
    }
    let blend = ir::Source::Blend(index_of(&mut group.blends, blend));
    // The cell's factor over the macro's position, at the mapper's points.
    let points = if table.is_empty() {
        vec![0.0, 1.0]
    } else {
        table
    };
    let table = points.iter().map(|&m| scale(r, m)).collect();
    Some(ir::Source::Shaped(index_of(
        &mut group.shapers,
        ir::Transfer {
            source: blend,
            table,
        },
    )))
}

/// The multiplier `m` gives at ratio `r`.
fn scale(r: f32, m: f32) -> f32 {
    if r < 0.0 {
        1.0 + r * m
    } else {
        1.0 - r + r * m
    }
}

/// A cell connection's clamped ratio and mapper table (empty: none). None
/// for an inverted, additive or modulated connection or a stepped mapper.
fn cell_law(connection: Node) -> Option<(f32, Vec<f32>)> {
    if flag(connection, "Inverted")
        || flag(connection, "ConnectionMode")
        || children(connection).next().is_some()
    {
        return None;
    }
    let r = (num(connection, "Ratio", 1.0).ok()? as f32).clamp(-1.0, 1.0);
    Some((r, mapper(connection)?))
}

/// `x` (0..=1) on a mapper table, linearly between points; None for no table.
fn read(table: &[f32], x: f32) -> Option<f32> {
    (!table.is_empty()).then(|| {
        ir::Transfer {
            source: ir::Source::Constant,
            table: table.to_vec(),
        }
        .apply(x)
    })
}

/// A macro's `value` as a table position: a bipolar one at `(x + 1) / 2`.
fn position(source: Node, value: f32) -> Option<f32> {
    Some(if flag(source, "Bipolar") {
        (value + 1.0) / 2.0
    } else {
        value
    })
}

/// The connection's mapper as a table scaled by `Max` (or `-Min` below
/// zero); empty for none, None for one not found or stepped.
fn mapper(connection: Node) -> Option<Vec<f32>> {
    let Some(name) = connection.attribute("Mapper").filter(|m| !m.is_empty()) else {
        return Some(Vec::new());
    };
    let mapper = connection
        .ancestors()
        .filter_map(|a| a.children().find(|n| n.has_tag_name("Mappers")))
        .flat_map(|m| m.children())
        .find(|n| n.attribute("Name") == Some(name))?;
    if flag(mapper, "Discrete") || flag(mapper, "Integer") {
        return None;
    }
    let (max, min) = (
        num(mapper, "Max", 1.0).ok()? as f32,
        num(mapper, "Min", 0.0).ok()? as f32,
    );
    mapper
        .text()
        .unwrap_or_default()
        .split_whitespace()
        .map(|p| {
            let v: f32 = p.replace(',', ".").parse().ok()?;
            Some(if v >= 0.0 { v * max } else { -v * min })
        })
        .collect()
}

/// A connection's source through its mapper. A macro with no inputs of
/// its own is a constant (a one-point table at its value); a bipolar LFO
/// at depth 1 is the group's copy of it.
fn transfer(program: Node, connection: Node, group: &mut ir::Group) -> Option<ir::Transfer> {
    let table = mapper(connection)?;
    let name = connection.attribute("Source")?;
    let source = if let Some(cc) = name.strip_prefix("@MIDI CC ") {
        ir::Source::Controller(cc.trim().parse().ok()?)
    } else {
        let node = source_node(program, connection)?;
        if node.has_tag_name("ScriptEventModulation") && !flag(node, "Bipolar") {
            ir::Source::Script(node.attribute("EventId")?.parse().ok()?)
        } else if node.has_tag_name("ConstantModulation") && inputs(node).next().is_none() {
            let x = position(node, num(node, "Value", 0.0).ok()? as f32)?;
            let value = read(&table, x).unwrap_or(x);
            return Some(ir::Transfer {
                source: ir::Source::Constant,
                table: vec![value],
            });
        } else if let Some((lfo, 1.0)) = modulation::lfo(node) {
            ir::Source::Lfo(index_of(&mut group.lfos, lfo))
        } else if node.has_tag_name("LFO") || node.has_tag_name("MultiLFO") {
            // ponytail: an LFO not carried (VWinds' growl: a random wave whose
            // depth a script opens) is held at its rest, 0, read at the
            // middle of the table; carry it when its wave is.
            return Some(ir::Transfer {
                source: ir::Source::Constant,
                table: vec![read(&table, 0.5).unwrap_or(0.0)],
            });
        } else {
            return None;
        }
    };
    Some(ir::Transfer { source, table })
}

/// A macro's live inputs: connections onto its `Value`.
fn inputs<'a, 'input>(source: Node<'a, 'input>) -> impl Iterator<Item = Node<'a, 'input>> {
    children(source).filter(|c| c.attribute("Destination") == Some("Value"))
}

/// A node's active connections.
fn children<'a, 'input>(node: Node<'a, 'input>) -> impl Iterator<Item = Node<'a, 'input>> {
    node.children()
        .filter(|c| c.has_tag_name("Connections"))
        .flat_map(|c| c.children())
        .filter(|c| c.has_tag_name("SignalConnection") && !flag(*c, "Bypass"))
}
