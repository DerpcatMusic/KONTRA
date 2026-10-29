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
/// for any other source, a modulated or inverted connection, or an integer
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
        let mut term = term(program, input, Some(&mut group.lfos))?;
        term.amount *= half;
        blend.terms.push(term);
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

/// An additive connection as a blend's term: its source through its
/// mapper at its ratio, each live connection on that ratio a factor (a
/// constant one folded in). None for a multiplicative one, an inverted bipolar one, a
/// constant source, or a source not carried; an LFO only with `lfos`.
pub(super) fn term(
    program: Node,
    input: Node,
    mut lfos: Option<&mut Vec<ir::Lfo>>,
) -> Option<ir::Term> {
    if flag(input, "ConnectionMode") {
        return None;
    }
    let mut amount = num(input, "Ratio", 1.0).ok()? as f32;
    let mut factors = Vec::new();
    for f in children(input).filter(|f| f.attribute("Destination") == Some("Ratio")) {
        if flag(f, "ConnectionMode") {
            return None;
        }
        let depth = held_ratio(program, f)?.clamp(-1.0, 1.0);
        let factor = ir::Factor {
            input: read_through(program, f, lfos.as_deref_mut())?,
            depth,
        };
        match factor.input.source {
            ir::Source::Constant => amount *= factor.at(factor.input.apply(1.0)),
            _ => factors.push(factor),
        }
    }
    let input = read_through(program, input, lfos)?;
    if input.source == ir::Source::Constant {
        return None;
    }
    Some(ir::Term {
        input,
        amount,
        factors,
    })
}

/// `connection`'s [`transfer`], an inverted one read at `1 − x`: UVI flips
/// a unipolar source before its mapper (0x14142ac00: `1 − x`), a bipolar
/// one to `−x`, which its mapper reads at `(1 − x)/2`: its table reversed
/// too (VWinds' V2 bend on its EQ). Not an unmapped bipolar one, nor a
/// held macro or bipolar script modulation.
fn read_through(
    program: Node,
    connection: Node,
    lfos: Option<&mut Vec<ir::Lfo>>,
) -> Option<ir::Transfer> {
    let mut t = transfer(program, connection, lfos)?;
    if flag(connection, "Inverted") {
        let bipolar = source_node(program, connection).is_some_and(|n| flag(n, "Bipolar"));
        let swing = matches!(t.source, ir::Source::PitchBend | ir::Source::Lfo(_));
        let unipolar =
            !bipolar && matches!(t.source, ir::Source::Controller(_) | ir::Source::Script(_));
        if !unipolar && !(swing && !t.table.is_empty()) {
            return None;
        }
        if t.table.is_empty() {
            t.table = vec![0.0, 1.0];
        }
        t.table.reverse();
    }
    Some(t)
}

/// A connection's ratio with each connection on it scaling it the same
/// multiplicative way; None when one of those is live (not a constant).
pub(super) fn held_ratio(program: Node, connection: Node) -> Option<f32> {
    ratio(program, connection, false)
}

/// [`held_ratio`] passing over the key connections [`super::blend::keyed`] carries.
pub(super) fn unkeyed_ratio(program: Node, connection: Node) -> Option<f32> {
    ratio(program, connection, true)
}

fn ratio(program: Node, connection: Node, keys: bool) -> Option<f32> {
    let mut r = num(connection, "Ratio", 1.0).ok()? as f32;
    for f in children(connection).filter(|f| f.attribute("Destination") == Some("Ratio")) {
        if keys && key_factor(program, f).is_some() {
            continue;
        }
        let input = transfer(program, f, None)?;
        if flag(f, "Inverted") || flag(f, "ConnectionMode") || input.source != ir::Source::Constant
        {
            return None;
        }
        r *= scale(held_ratio(program, f)?.clamp(-1.0, 1.0), input.apply(1.0));
    }
    Some(r)
}

/// A connection from a key on a ratio, as the factor it scales it by: the
/// key read through a table exact at each MIDI key `x` (VoiceParam evaluate
/// 0x14138ab10, `x` the note clamped to 0..=127): `Key` `x/127`, `KeyFollow`
/// `(x/60)² − 1` below 60, else `√((min(x, 120) − 60)/60)`, `LinearKeyFollow`
/// `(x − 60)/60` within ±1. A bipolar one is read at `(k + 1)/2`, its
/// mapper's output as [`centred`]. None for any other connection.
pub(super) fn key_factor(program: Node, connection: Node) -> Option<ir::Factor> {
    if flag(connection, "Inverted") || flag(connection, "ConnectionMode") {
        return None;
    }
    let source = connection.attribute("Source")?;
    let key: fn(f32) -> f32 = match source {
        "@VoiceParam Key" => |x| x / 127.0,
        "@VoiceParam KeyFollow" => |x| {
            let k = if x < 60.0 {
                (x / 60.0).powi(2) - 1.0
            } else {
                ((x.min(120.0) - 60.0) / 60.0).sqrt()
            };
            (k + 1.0) / 2.0
        },
        "@VoiceParam LinearKeyFollow" => |x| (((x - 60.0) / 60.0).clamp(-1.0, 1.0) + 1.0) / 2.0,
        _ => return None,
    };
    let table = centred(connection, mapper(connection)?);
    let at = |x: f32| read(&table, key(x)).unwrap_or(key(x));
    Some(ir::Factor {
        input: ir::Transfer {
            source: ir::Source::Key,
            table: (0..128u8).map(|x| at(f32::from(x))).collect(),
        },
        depth: held_ratio(program, connection)?.clamp(-1.0, 1.0),
    })
}

/// Whether a modulator is bipolar (an LFO is unless it says not).
pub(super) fn bipolar(node: Node) -> bool {
    num(node, "Bipolar", 1.0).is_ok_and(|b| b != 0.0)
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
/// for an inverted, additive or modulated connection or an integer mapper.
fn cell_law(connection: Node) -> Option<(f32, Vec<f32>)> {
    if flag(connection, "Inverted")
        || flag(connection, "ConnectionMode")
        || children(connection).next().is_some()
    {
        return None;
    }
    let r = (num(connection, "Ratio", 1.0).ok()? as f32).clamp(-1.0, 1.0);
    Some((r, centred(connection, mapper(connection)?)))
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
/// zero); empty for none, None for one not found or rounding to integers.
/// A `Discrete` one reads point `⌊(len − 1)·x⌋` of its in-memory table, not
/// between two (0x1411391a6, the flag at +0x130): at each saved point's own
/// `x` that is the point (`⌊n·j/(n − 1)⌋ = j`), so its table is read as it is,
/// exact at every MIDI controller value for the usual 128. ponytail: read
/// linearly between points (a 14-bit bend), within a step of the stepped read.
fn mapper(connection: Node) -> Option<Vec<f32>> {
    if connection.attribute("Mapper").is_none_or(str::is_empty) {
        return Some(Vec::new());
    }
    let mapper = mapper_node(connection)?;
    if flag(mapper, "Integer") {
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

fn mapper_node<'a, 'i>(connection: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    let name = connection.attribute("Mapper").filter(|m| !m.is_empty())?;
    connection
        .ancestors()
        .filter_map(|a| a.children().find(|n| n.has_tag_name("Mappers")))
        .flat_map(|m| m.children())
        .find(|n| n.attribute("Name") == Some(name))
}

/// `table`, a connection's mapper output on a multiplicative destination,
/// as the law reads it: a bipolar mapper's (`Min` < 0) at `(y + 1)/2`, any
/// other's as it is, whatever its source. Read from the libraries, not the
/// binary: VWinds' brightness bells (`Min` 0, a bipolar macro) sum to ~1
/// only as they are; its pitch-bend crossfade (`Min` −1) to 1 and its
/// AttacksMapper (`Min` −1, mostly −1) to an attack that opens only so.
pub(super) fn centred(connection: Node, mut table: Vec<f32>) -> Vec<f32> {
    if centres(connection) {
        table.iter_mut().for_each(|v| *v = (*v + 1.0) / 2.0);
    }
    table
}

/// Whether [`centred`] re-centres the connection's mapper output.
pub(super) fn centres(connection: Node) -> bool {
    mapper_node(connection).is_some_and(|m| num(m, "Min", 0.0).is_ok_and(|v| v < 0.0))
}

/// A connection's source through its mapper. A macro with no inputs of
/// its own is a constant (a one-point table at its value); a bipolar LFO
/// at depth 1 is a copy of it in `lfos`. A bipolar script modulation reads
/// its table at `(x + 1)/2`: its upper half, exactly for x ≥ 0.
/// ponytail: a negative value reads the table's middle (VWinds only ramps
/// 0..1); carry the lower half when a script sends one.
pub(super) fn transfer(
    program: Node,
    connection: Node,
    lfos: Option<&mut Vec<ir::Lfo>>,
) -> Option<ir::Transfer> {
    let table = mapper(connection)?;
    let name = connection.attribute("Source")?;
    let source = if let Some(cc) = name.strip_prefix("@MIDI CC ") {
        ir::Source::Controller(cc.trim().parse().ok()?)
    } else if name == "@PitchBend" {
        ir::Source::PitchBend
    } else {
        let node = source_node(program, connection)?;
        if node.has_tag_name("ScriptEventModulation") {
            let source = ir::Source::Script(node.attribute("EventId")?.parse().ok()?);
            if !flag(node, "Bipolar") || table.len() < 2 {
                return Some(ir::Transfer { source, table });
            }
            // Every point lands on a point or a segment's middle: exact.
            let last = (table.len() - 1) as f32;
            let half = (0..table.len())
                .map(|k| read(&table, 0.5 + 0.5 * k as f32 / last))
                .collect::<Option<_>>()?;
            return Some(ir::Transfer {
                source,
                table: half,
            });
        } else if node.has_tag_name("ConstantModulation") && inputs(node).next().is_none() {
            let x = position(node, num(node, "Value", 0.0).ok()? as f32)?;
            let value = read(&table, x).unwrap_or(x);
            return Some(ir::Transfer {
                source: ir::Source::Constant,
                table: vec![value],
            });
        } else if let (Some((lfo, 1.0)), Some(lfos)) = (modulation::lfo(node), lfos) {
            ir::Source::Lfo(index_of(lfos, lfo))
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
pub(super) fn children<'a, 'input>(
    node: Node<'a, 'input>,
) -> impl Iterator<Item = Node<'a, 'input>> {
    node.children()
        .filter(|c| c.has_tag_name("Connections"))
        .flat_map(|c| c.children())
        .filter(|c| c.has_tag_name("SignalConnection") && !flag(*c, "Bypass"))
}
