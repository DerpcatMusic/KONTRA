//! The nested mixer: every instrument's output tree as strips.
//!
//! ```text
//!  ┌Cellos┐ ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━   ┌Harp──┐
//!  │ ▾ 4  │ ┌Close┐┌Tree─┐┌Room─┐┌Legato┐    │      │
//!  │  ◢   │ │ ◢   ││ ◢   ││ ◢   ││ ▸ 2  │    │  ◢   │
//!  │ ┃ ▌▌ │ │┃ ▌▌ ││┃ ▌▌ ││┃ ▌▌ ││┃ ▌▌  │    │ ┃ ▌▌ │
//!  │ S  M │ │S  M ││S  M ││S  M ││S  M  │    │ S  M │
//!  │▮ 1/2 │ │Up   ││Up   ││▮ 5/6││Up    │    │▮ 3/4 │
//!  └──────┘ └─────┘└─────┘└─────┘└──────┘    └──────┘
//! ```
//!
//! A node's children sit to its right under a bar in the node's colour, the
//! tie between them. Each level is [`STEP`] shorter than its parent and every
//! strip is bottom-aligned, so faders, buttons and output fields line up
//! across depths while the bars stack above. A node with children folds to its
//! own strip.
//!
//! Output is per node: up to its parent (the default below the top) or straight
//! to a host stereo pair. Automatic routing ([`crate::routing`]) gives every
//! instrument its own pair unless the user chose one.

use super::theme::*;
pub use crate::sound::tree::{NodeKind as Kind, NodeOutput as Output};
use moose::mui::mui::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;

/// One mixer node's settings: plain data the core fills and reads back.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    /// Stable across rebuilds of the tree (collapse state is keyed by it).
    pub id: u64,
    pub name: String,
    pub kind: Kind,
    /// Index of the parent in [`Tree::nodes`]; `None` for an instrument.
    pub parent: Option<usize>,
    pub gain_db: f32,
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    pub output: Output,
    /// The user chose `output`; automatic assignment leaves it alone.
    pub output_set: bool,
    /// Insert effect names, in order.
    pub inserts: Vec<String>,
}

impl Node {
    pub fn new(id: u64, name: impl Into<String>, kind: Kind, parent: Option<usize>) -> Self {
        Self {
            id,
            name: name.into(),
            kind,
            parent,
            gain_db: 0.,
            pan: 0.,
            mute: false,
            solo: false,
            output: Output::Parent,
            output_set: false,
            inserts: Vec::new(),
        }
    }
}

fn label(kind: Kind) -> &'static str {
    match kind {
        Kind::Instrument => "Instrument",
        Kind::Group => "Group",
        Kind::Bus => "Bus",
    }
}

/// The whole mixer: nodes in pre-order, every parent before its children.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tree {
    pub nodes: Vec<Node>,
}

impl Tree {
    pub fn children(&self, n: usize) -> impl Iterator<Item = usize> + '_ {
        self.nodes.iter().enumerate().filter(move |(_, c)| c.parent == Some(n)).map(|(i, _)| i)
    }

    pub fn depth(&self, mut n: usize) -> usize {
        let mut d = 0;
        while let Some(p) = self.nodes[n].parent {
            (n, d) = (p, d + 1);
        }
        d
    }
}

/// Each level down is this much shorter than its parent.
pub const STEP: f64 = SPACE * 2.;
const WIDTH: f64 = TEXT * 7.;
/// The tie bar over a node's children.
const BAR: f64 = 3.;
const DB: std::ops::RangeInclusive<f64> = -60.0..=6.0;

/// The mixer's view state across frames.
#[derive(Default)]
pub struct State {
    /// Folded nodes, by [`Node::id`].
    pub folded: HashSet<u64>,
    /// The node whose output list is open.
    picking: Option<u64>,
}

/// A node's level meter, `[left, right]` linear peaks, by node index.
pub type Levels = Arc<dyn Fn(usize) -> [f32; 2] + Send + Sync>;

/// Colours: an instrument's from its rack position, a child's a darker step
/// of its instrument's hue, so a family reads as one.
fn colour(tree: &Tree, n: usize, root_slot: &dyn Fn(usize) -> usize) -> Color {
    let mut root = n;
    while let Some(p) = tree.nodes[root].parent {
        root = p;
    }
    let depth = tree.depth(n) as f32;
    Color::oklch(0.7 - 0.07 * depth, 0.1 - 0.015 * depth, golden_hue(250., root_slot(root)))
}

/// The mixer: `pairs` host stereo pairs; `height` the room for a top strip.
pub fn view(ui: &mut Ui, tree: &mut Tree, state: &mut State, pairs: u8, height: f64, levels: Levels) -> El {
    let roots: Vec<usize> = (0..tree.nodes.len()).filter(|&n| tree.nodes[n].parent.is_none()).collect();
    let slot = |n: usize| roots.iter().position(|&r| r == n).unwrap_or(0);
    let mut families = Vec::new();
    for &r in &roots {
        families.push(family(ui, tree, state, r, pairs, height, &levels, &slot));
    }
    if families.is_empty() {
        families.push(caption("Load an instrument to give it a strip.").fill(secondary()).pad(INSET).shrink(0));
    }
    row(families)
        .gap(SPACE)
        .align(Align::End)
        .pad(SPACE)
        .fill(Role::Background)
        .scroll()
        .flex(1)
        .min_w(0)
        .min_h(0)
        .a11y(A11y::Group)
        .named("Mixer")
        .id("mix-tree")
}

#[allow(clippy::too_many_arguments)]
fn family(
    ui: &mut Ui,
    tree: &mut Tree,
    state: &mut State,
    n: usize,
    pairs: u8,
    height: f64,
    levels: &Levels,
    slot: &dyn Fn(usize) -> usize,
) -> El {
    let children: Vec<usize> = tree.children(n).collect();
    let folded = state.folded.contains(&tree.nodes[n].id);
    let colour = colour(tree, n, slot);
    let mut parts = vec![strip(ui, tree, state, n, pairs, height, colour, children.len(), levels)];
    if !children.is_empty() && !folded {
        let inner: Vec<El> =
            children.iter().map(|&c| family(ui, tree, state, c, pairs, height - STEP, levels, slot)).collect();
        parts.push(
            col![block(Len::Pct(100.), BAR).fill(colour).shrink(0), row(inner).gap(1).align(Align::End)]
                .gap(TIGHT)
                .align(Align::Stretch)
                .shrink(0),
        );
    }
    row(parts).gap(1).align(Align::End).shrink(0)
}

#[allow(clippy::too_many_arguments)]
fn strip(
    ui: &mut Ui,
    tree: &mut Tree,
    state: &mut State,
    n: usize,
    pairs: u8,
    height: f64,
    colour: Color,
    children: usize,
    levels: &Levels,
) -> El {
    let key = tree.nodes[n].id;
    let parent_colour = tree.nodes[n].parent.map(|_| colour);
    // Header: fold toggle with the child count, the name, the kind.
    let mut head = Vec::new();
    if children > 0 {
        let folded = state.folded.contains(&key);
        let id = format!("mt-fold-{key}");
        if ui.get(id.as_str()).activated() && !state.folded.remove(&key) {
            state.folded.insert(key);
        }
        let el = row![
            glyph(if folded { Icon::Right } else { Icon::Down }, TIGHT * 2.5, secondary()),
            caption(children.to_string()).text_size(SMALL).fill(secondary())
        ]
        .gap(2)
        .align(Align::Center)
        .pad((2., 0.))
        .h(STRIP)
        .focusable()
        .a11y(A11y::Toggle { on: !folded })
        .named(if folded { "Show its strips" } else { "Hide its strips" })
        .tip(if folded { "Show its strips" } else { "Hide its strips" })
        .id(id)
        .shrink(0);
        head.push(interactive(el, false));
    }
    let node = &tree.nodes[n];
    head.push(body(node.name.clone()).text_size(SMALL).text_weight(Weight::SEMIBOLD).lines(2).min_w(0));
    let header = col![
        row(head).gap(2).align(Align::Start).min_w(0),
        caption(match node.inserts.len() {
            0 => label(node.kind).to_owned(),
            i => format!("{i} insert{}", if i == 1 { "" } else { "s" }),
        })
        .text_size(SMALL)
        .fill(secondary())
        .lines(1)
    ]
    .gap(0)
    .h(SMALL * 3.6)
    .shrink(0);

    let picking = state.picking == Some(key);
    let out = output_button(ui, tree, state, n, colour);
    let middle = if picking {
        output_list(ui, tree, state, n, pairs)
    } else {
        level(ui, tree, n, levels)
    };
    let node = &mut tree.nodes[n];
    let switches = solo_mute(ui, &format!("mt-{key}"), &mut node.solo, &mut node.mute);

    let mut rows = vec![block(Len::Pct(100.), 2).fill(colour).shrink(0)];
    let body = col![header, middle, row![switches].justify(Justify::Center).shrink(0), out]
        .gap(TIGHT)
        .align(Align::Stretch)
        .pad((TIGHT, TIGHT))
        .flex(1)
        .min_h(0);
    rows.push(body);
    let el = col(rows).gap(0).align(Align::Stretch).w(WIDTH).h(height.max(CONTROL * 8.)).fill(Role::Surface).shrink(0);
    // Below the top, a hairline in the parent's colour down the left edge
    // ties the strip to its bar even when the bar scrolls out of view.
    let el = match parent_colour {
        Some(c) => row![block(1, Len::Pct(100.)).fill(c.with_alpha(0.6)).shrink(0), el].gap(0).shrink(0),
        None => el,
    };
    el.a11y(A11y::Group).named(tree.nodes[n].name.clone()).id(format!("mt-strip-{key}"))
}

/// Pan, the fader with its meter, and the readout.
fn level(ui: &mut Ui, tree: &mut Tree, n: usize, levels: &Levels) -> El {
    let node = &mut tree.nodes[n];
    let key = node.id;
    let mut pan = f64::from(node.pan);
    let pan_el = pan_wedge(ui, &format!("mt-pan-{key}"), &mut pan);
    let id = format!("mt-fader-{key}");
    let travel = ui.scene().and_then(|s| s.surface(&id)).map_or(TRAVEL, |s| s.frame.size.height).max(CONTROL);
    let mut db = f64::from(node.gain_db);
    let held = drive(ui, &id, &mut db, &DB, travel, true, 0.);
    (node.pan, node.gain_db) = (pan as f32, db as f32);
    let lift = ui.state(id.as_str()).hover.max(if held { 1. } else { 0. }) as f32;
    let unit = |db: f64| ((db - DB.start()) / (DB.end() - DB.start())).clamp(0., 1.);
    let fader = fader_face(unit(f64::from(node.gain_db)), 0., Some(unit(0.)), true, lift, ui.focus_visible(&id))
        .w(CONTROL)
        .h(Len::Pct(100.))
        .shrink(0)
        .cursor(Cursor::ResizeV)
        .focusable()
        .a11y(A11y::Slider { value: f64::from(node.gain_db), min: *DB.start(), max: *DB.end() })
        .named("Level")
        .tip("Level: drag, Shift for fine, double-click for 0 dB")
        .id(id);
    let levels = levels.clone();
    let meter = meter_v(move || levels(n));
    col![
        pan_el,
        row![fader, meter].gap(TIGHT).justify(Justify::Center).flex(1).min_h(CONTROL * 2.),
        caption(super::mixer::db_short(f64::from(node.gain_db))).text_size(SMALL).fill(secondary()).lines(1)
    ]
    .gap(TIGHT)
    .align(Align::Center)
    .flex(1)
    .min_h(0)
}

fn output_text(o: Output) -> String {
    match o {
        Output::Parent => "Up".into(),
        Output::Pair(p) => super::mixer::port_text(usize::from(p)),
    }
}

fn output_button(ui: &mut Ui, tree: &Tree, state: &mut State, n: usize, colour: Color) -> El {
    let node = &tree.nodes[n];
    let to = output_text(node.output);
    let name = match node.output {
        Output::Parent => "Output: into its parent".to_owned(),
        Output::Pair(_) => format!("Output: host pair {to}{}", if node.output_set { "" } else { ", automatic" }),
    };
    let (hit, el) = route(ui, format!("mt-out-{}", node.id), Icon::AudioOut, &to, "15/16", &name);
    if hit {
        state.picking = if state.picking == Some(node.id) { None } else { Some(node.id) };
    }
    let chip = block(TIGHT, STRIP).fill(match node.output {
        Output::Parent => Fill::from(colour.with_alpha(0.4)),
        Output::Pair(_) => Fill::from(colour),
    });
    row![chip.shrink(0), el.flex(1).min_w(0)].gap(0).align(Align::Center).shrink(0)
}

/// The output choices in place of the fader while picking.
fn output_list(ui: &mut Ui, tree: &mut Tree, state: &mut State, n: usize, pairs: u8) -> El {
    let key = tree.nodes[n].id;
    let mut choices: Vec<(Option<Output>, String)> = Vec::new();
    if tree.nodes[n].parent.is_some() {
        choices.push((Some(Output::Parent), "Into parent".into()));
    }
    choices.extend((0..pairs).map(|p| (Some(Output::Pair(p)), format!("Host {}", super::mixer::port_text(usize::from(p))))));
    if tree.nodes[n].parent.is_none() {
        choices.push((None, "Automatic".into()));
    }
    let mut rows = Vec::new();
    for (k, (to, label)) in choices.into_iter().enumerate() {
        let node = &tree.nodes[n];
        let current = match to {
            Some(o) => node.output_set && node.output == o || node.parent.is_some() && !node.output_set && o == Output::Parent,
            None => !node.output_set,
        };
        let (hit, el) = action(ui, format!("mt-pick-{key}-{k}"), &label, current);
        if hit {
            let node = &mut tree.nodes[n];
            match to {
                Some(o) => (node.output, node.output_set) = (o, true),
                None => node.output_set = false,
            }
            state.picking = None;
        }
        rows.push(el);
    }
    col(rows).gap(1).align(Align::Stretch).scroll().flex(1).min_h(0).id(format!("mt-picks-{key}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Two instruments; the first has a mic bus with two mics and a group.
    pub(crate) fn sample() -> Tree {
        let mut nodes = vec![
            Node::new(1, "Cellos", Kind::Instrument, None),
            Node::new(2, "Mics", Kind::Bus, Some(0)),
            Node::new(3, "Close", Kind::Bus, Some(1)),
            Node::new(4, "Room", Kind::Bus, Some(1)),
            Node::new(5, "Legato", Kind::Group, Some(0)),
            Node::new(6, "Harp", Kind::Instrument, None),
        ];
        (nodes[0].output, nodes[5].output) = (Output::Pair(0), Output::Pair(1));
        (nodes[3].output, nodes[3].output_set) = (Output::Pair(4), true);
        Tree { nodes }
    }

    #[test]
    fn tree_shape() {
        let t = sample();
        assert_eq!(t.depth(3), 2);
        assert_eq!(t.children(0).collect::<Vec<_>>(), [1, 4]);
    }
}
