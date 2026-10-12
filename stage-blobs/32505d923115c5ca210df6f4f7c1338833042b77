//! The sound editor: a group's envelope and filter/EQ response drawn from
//! the engine's laws, with handles that edit the part's override layer
//! (never the library), the group's zones, and the voices playing now.

use super::viz::Phase;
use super::viz::{self, Handle, Model};
use super::{Cx, chain, spectrum, theme::*};
use crate::sound::edits::Tap;
use crate::sound::edits::{Override, Param};
use moose::mui::mui::geometry::Path as DrawPath;
use moose::mui::mui::prelude::*;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;

/// The two graphs with handles.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Graph {
    Envelope = 0,
    Response = 1,
}

impl Graph {
    fn id(self) -> &'static str {
        match self {
            Self::Envelope => "edit-envelope",
            Self::Response => "edit-response",
        }
    }
}

/// What the expanded editor shows under the graphs.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Lower {
    #[default]
    Zones,
    Modulation,
    Effects,
}

/// The editor's state across frames.
#[derive(Default)]
pub struct State {
    lower: Lower,
    /// A value being typed in: which, and the text so far.
    typing: Option<(Param, String)>,
    /// Edits apply to the shown group alone, not to every group.
    one_group: bool,
    /// Curves alone, without readouts and zones.
    compact: bool,
    /// The handle being dragged.
    grab: Option<(Graph, usize)>,
    /// Each graph's handle the readouts show.
    focus: [usize; 2],
    /// The curves as last computed, and what they were computed from.
    curves: Option<(Key, Arc<Curves>)>,
    envelope_cache: CanvasCache<(Key, Color)>,
    response_cache: CanvasCache<(Key, Color)>,
}

/// What the curves depend on: the instrument, group, and every value.
type Key = (usize, u32, Vec<u32>);

/// A group's pictures, computed when their inputs change.
struct Curves {
    model: Model,
    envelope: Option<viz::EnvelopeShape>,
    /// The library's envelope, when an edit moved it.
    envelope_ghost: Option<viz::EnvelopeShape>,
    envelope_handles: Vec<Handle>,
    response: Vec<[f32; 2]>,
    response_ghost: Option<Vec<[f32; 2]>>,
    response_handles: Vec<Handle>,
}

impl Curves {
    fn new(model: Model) -> Self {
        let edited = |p: Param| p.read(&model.playing) != p.read(&model.base);
        let env_edited = Param::ENVELOPE.iter().any(|&p| edited(p));
        let filter_edited = model
            .params
            .iter()
            .any(|&(_, p)| !Param::ENVELOPE.contains(&p) && edited(p));
        // The library's envelope under the edited one, both over the longer.
        let ghost = model.base.envelope.as_ref().filter(|_| env_edited);
        let total = [model.playing.envelope.as_ref(), ghost]
            .into_iter()
            .flatten()
            .map(viz::envelope_width)
            .fold(0., f32::max);
        let envelope = model
            .playing
            .envelope
            .as_ref()
            .map(|e| viz::envelope_over(e, total));
        let envelope_handles = match (&envelope, &model.playing.envelope) {
            (Some(shape), Some(env)) => viz::envelope_handles(shape, env)
                .into_iter()
                .filter(|h| h.params().all(|p| p.read(&model.base).is_some()))
                .collect(),
            _ => Vec::new(),
        };
        Self {
            envelope_ghost: ghost.map(|e| viz::envelope_over(e, total)),
            envelope,
            envelope_handles,
            response: viz::response(&model.playing),
            response_ghost: filter_edited.then(|| viz::response(&model.base)),
            response_handles: viz::filter_handles(&model.playing),
            model,
        }
    }

    fn handles(&self, graph: Graph) -> &[Handle] {
        match graph {
            Graph::Envelope => &self.envelope_handles,
            Graph::Response => &self.response_handles,
        }
    }
}

/// Unit coordinates to a canvas of `s`, inset so handles are not clipped.
fn place(s: Size, [x, y]: [f32; 2]) -> Point {
    let pad = SPACE;
    Point::new(
        pad + f64::from(x) * (s.width - 2. * pad).max(1.),
        pad + (1. - f64::from(y)) * (s.height - 2. * pad).max(1.),
    )
}

fn line(s: Size, points: &[[f32; 2]]) -> DrawPath {
    DrawPath::polyline(points.iter().map(|&p| place(s, p)), false)
}

/// `points` closed down to the floor.
fn area(s: Size, points: &[[f32; 2]]) -> DrawPath {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return DrawPath::polyline([], true);
    };
    let floor = [[last[0], 0.], [first[0], 0.]];
    DrawPath::polyline(points.iter().chain(&floor).map(|&p| place(s, p)), true)
}

fn handle_square(at: Point, r: f64) -> DrawPath {
    rect(at.x - r, at.y - r, 2. * r, 2. * r)
}

/// The shown group: the part's, within the instrument.
fn group_of(cx: &Cx, slot: usize) -> Option<(usize, u32, Arc<sampler_ir::Instrument>)> {
    let i = cx.view.parts.get(slot)?.instrument.clone()?;
    let group = Some(cx.selection.parts.get(slot)?.group)
        .filter(|&g| (g as usize) < i.groups.len())
        .unwrap_or(0);
    (!i.groups.is_empty()).then_some((slot, group, i))
}

/// The shown group's curves, computed again only when a value they draw
/// moves.
fn curves(
    cx: &mut Cx,
    slot: usize,
    group: u32,
    instrument: &Arc<sampler_ir::Instrument>,
) -> (Key, Arc<Curves>) {
    let atoms = cx.p.shared.part(slot).expect("loaded editor part");
    let bindings = atoms.engine_bindings.lock().unwrap().clone();
    let base = atoms.control_values();
    let edits = &cx.selection.parts[slot].edits;
    let model = Model::new(
        instrument,
        group as usize,
        edits,
        &bindings,
        &base,
        cx.p.shared.rate(),
    );
    let values: Vec<u32> = (model.params.iter())
        .flat_map(|&(_, p)| [p.read(&model.playing), p.read(&model.base)])
        .map(|v| v.map_or(u32::MAX, f32::to_bits))
        .collect();
    let key: Key = (Arc::as_ptr(instrument) as usize, group, values);
    let state = &mut cx.state.editor;
    match &state.curves {
        Some((k, c)) if *k == key => (key, c.clone()),
        _ => {
            let c = Arc::new(Curves::new(model));
            state.curves = Some((key.clone(), c.clone()));
            (key, c)
        }
    }
}

pub fn view(ui: &mut Ui, cx: &mut Cx, slot: usize) -> El {
    cx.p.shared.editor_watch.store(slot, Relaxed);
    let Some((slot, group, instrument)) = group_of(cx, slot) else {
        return col![
            caption("This instrument has no groups to edit.")
                .fill(secondary())
                .lines(2)
        ]
        .pad(INSET)
        .flex(1);
    };

    let (_, curves) = curves(cx, slot, group, &instrument);
    // An edit shows this frame, not the next.
    let (key, curves) = if interact(ui, cx, slot, curves.model.group, &curves) {
        self::curves(cx, slot, group, &instrument)
    } else {
        (
            cx.state
                .editor
                .curves
                .as_ref()
                .map(|(k, _)| k.clone())
                .unwrap_or_default(),
            curves,
        )
    };

    let library = cx.library_of(Path::new(&cx.selection.parts[slot].path));
    let tint = cx.tint(&library).unwrap_or(part_color(slot));
    let compact = cx.state.editor.compact;
    let toolbar = toolbar(ui, cx, slot, group, &instrument);
    let envelope = panel(
        ui,
        cx,
        slot,
        curves.model.group,
        Graph::Envelope,
        &curves,
        tint,
        key.clone(),
        None,
    );
    // The part's own output, after its effects and fader, behind the response.
    let heard = cx.spectrum(slot + 1);
    let response = panel(
        ui,
        cx,
        slot,
        curves.model.group,
        Graph::Response,
        &curves,
        tint,
        key,
        Some(heard),
    );
    let mut rows = vec![
        toolbar,
        rule(),
        row![
            envelope.flex(1).min_w(0),
            vrule(),
            response.flex(1).min_w(0)
        ]
        .gap(0)
        .flex(1)
        .min_h(0),
    ];
    if !compact {
        rows.push(rule());
        rows.push(lower(ui, cx, slot, group, &instrument, tint));
    }
    col(rows)
        .gap(0)
        .flex(1)
        .min_h(0)
        .min_w(0)
        .id("sound-editor")
}

fn toolbar(
    ui: &mut Ui,
    cx: &mut Cx,
    slot: usize,
    group: u32,
    instrument: &sampler_ir::Instrument,
) -> El {
    let count = instrument.groups.len() as u32;
    let name = match &instrument.groups[group as usize].name {
        n if n.is_empty() => format!("Group {}", group + 1),
        n => n.clone(),
    };
    let (prev, prev_el) = icon_button(ui, "edit-group-prev", Icon::Left, "Previous group", false);
    let (next, next_el) = icon_button(ui, "edit-group-next", Icon::Right, "Next group", false);
    let part = &mut cx.selection.parts[slot];
    if prev {
        part.group = group.checked_sub(1).unwrap_or(count - 1);
    }
    if next {
        part.group = (group + 1) % count;
    }
    let state = &mut cx.state.editor;
    let (all, all_el) = latch(
        ui,
        "edit-scope-all",
        "All groups",
        "Edits apply to every group",
        !state.one_group,
    );
    let (one, one_el) = latch(
        ui,
        "edit-scope-one",
        "This group",
        "Edits apply to this group alone",
        state.one_group,
    );
    if all || one {
        state.one_group = one;
    }
    let (compact, compact_el) = latch(ui, "edit-compact", "Compact", "Curves alone", state.compact);
    let (expanded, expanded_el) = latch(
        ui,
        "edit-expanded",
        "Expanded",
        "Curves with values and zones",
        !state.compact,
    );
    if compact || expanded {
        state.compact = compact;
    }
    let edited = !cx.selection.parts[slot].edits.0.is_empty();
    let mut items = vec![
        cluster(vec![
            prev_el,
            body(name)
                .text_size(TEXT)
                .lines(1)
                .min_w(0)
                .max_size(Size::new(TEXT * 16., CONTROL)),
            next_el,
        ]),
        segmented(vec![all_el, one_el]),
        spacer(),
        segmented(vec![compact_el, expanded_el]),
    ];
    if edited {
        let (reset, el) = action(ui, "edit-reset-part", "Reset part", false);
        if reset {
            cx.selection.parts[slot].edits = Default::default();
        }
        items.push(el.tip("Play every group as the library has it".to_owned()));
    }
    strip(items).pad((INSET, TIGHT))
}

/// Presses, drags, the wheel and double-clicks on the graphs; true when
/// the part's edits changed.
fn interact(ui: &mut Ui, cx: &mut Cx, slot: usize, group: u16, curves: &Curves) -> bool {
    let scope = cx.state.editor.one_group.then_some(group);
    let mut changed = false;
    for graph in [Graph::Envelope, Graph::Response] {
        let id = graph.id();
        let r = ui.get(id);
        let size = ui.scene().and_then(|s| s.surface(id)).map(|s| s.frame.size);
        let handles = curves.handles(graph);
        let nearest = || {
            let (at, size) = (ui.local(id)?, size?);
            handles
                .iter()
                .enumerate()
                .map(|(i, h)| {
                    let p = place(size, h.at);
                    (i, (p.x - at.x).hypot(p.y - at.y))
                })
                .filter(|&(_, d)| d <= CONTROL)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i)
        };
        let state = &mut cx.state.editor;
        if r.pressed {
            state.grab = nearest().map(|i| (graph, i));
            if let Some((_, i)) = state.grab {
                state.focus[graph as usize] = i;
            }
        }
        let grabbed = state
            .grab
            .filter(|(g, _)| *g == graph)
            .and_then(|(_, i)| handles.get(i));
        let edits = &mut cx.selection.parts[slot].edits;
        let model = &curves.model;
        let mut nudge = |p: Param, by: f32| {
            let Some(base) = p.read(&model.base) else {
                return;
            };
            let to = p.norm(base) + edits.offset(group, p) + by;
            changed |= set_norm(edits, scope, group, p, base, to);
        };
        if let (Some(h), true, Some(size)) = (grabbed, r.dragged, size) {
            let fine = if r.mods.shift { 0.1 } else { 1. };
            let (w, hgt) = (
                (size.width - 2. * SPACE).max(1.),
                (size.height - 2. * SPACE).max(1.),
            );
            if let Some((p, k)) = h.x {
                nudge(p, (r.drag_delta.x / w) as f32 * k * fine);
            }
            if let Some((p, k)) = h.y {
                nudge(p, (-r.drag_delta.y / hgt) as f32 * k * fine);
            }
        }
        if r.wheel.y != 0.
            && let Some(p) = nearest().and_then(|i| handles[i].wheel)
        {
            nudge(p, if r.wheel.y < 0. { 0.02 } else { -0.02 });
        }
        if r.double_clicked
            && let Some(i) = nearest()
        {
            for p in handles[i].params() {
                changed |= reset(edits, p, group);
            }
        }
        if r.released {
            cx.state.editor.grab = None;
        }
    }
    changed
}

/// Edit `p` (library value `base`) so `group` plays it at normalized `to`,
/// through the edit of `scope` (one group, or all).
fn set_norm(
    edits: &mut crate::sound::edits::Edits,
    scope: Option<u16>,
    group: u16,
    p: Param,
    base: f32,
    to: f32,
) -> bool {
    let n = p.norm(base);
    let total = edits.offset(group, p);
    let to = (to - n).clamp(-n, 1. - n);
    let own = edits.get(scope, p) + (to - total);
    edits.set(Override {
        group: scope,
        param: p,
        offset: if own.abs() < 1e-6 { 0. } else { own },
    })
}

/// A value's readout; double-clicked, a field to type it in. Enter or
/// leaving the field sets it (a value the readout could show; anything
/// else is ignored), Escape leaves it be.
#[allow(clippy::too_many_arguments)]
fn value_field(
    ui: &mut Ui,
    cx: &mut Cx,
    slot: usize,
    group: u16,
    p: Param,
    v: f32,
    changed: bool,
    model: &Model,
    graph: Graph,
) -> El {
    let id = format!("{}-value-{p:?}", graph.id());
    let edit_id = format!("{id}-edit");
    let state = &mut cx.state.editor;
    if ui.get(id.as_str()).double_clicked {
        state.typing = Some((p, viz::readout(p, model.display(p, v))));
    }
    if let Some((_, text)) = state.typing.as_mut().filter(|(t, _)| *t == p) {
        let existed = ui.scene().and_then(|s| s.surface(&edit_id)).is_some();
        if !existed {
            ui.focus(edit_id.as_str());
        }
        let field = text_edit(ui, edit_id.as_str(), text, TextOpts::default());
        let cancel = ui
            .keys(edit_id.as_str())
            .iter()
            .any(|k| k.key == moose::mui::mui::prelude::Key::Escape);
        let done = field.changed.submitted || (existed && !ui.focused(edit_id.as_str()));
        let el = field
            .el
            .h(CONTROL - TIGHT)
            .w(TEXT * 6.)
            .radius(0)
            .shrink(0)
            .named(format!("{} value", viz::label(p)));
        if cancel || done {
            let text = state.typing.take().map(|(_, t)| t).unwrap_or_default();
            let scope = state.one_group.then_some(group);
            if done
                && !cancel
                && let (Some(to), Some(base)) = (model.typed(p, &text), p.read(&model.base))
            {
                set_norm(
                    &mut cx.selection.parts[slot].edits,
                    scope,
                    group,
                    p,
                    base,
                    to,
                );
            }
        }
        return el;
    }
    caption(viz::readout(p, model.display(p, v)))
        .text_size(TEXT)
        .fill(if changed {
            Fill::from(Role::Ink)
        } else {
            secondary()
        })
        .lines(1)
        .shrink(0)
        .reserve(viz::widest(p).to_owned())
        .tip(format!("{}: double-click to type a value", viz::label(p)))
        .id(id)
}

/// Undo `p`'s edits that reach `group`: its own and the all-groups one.
fn reset(edits: &mut crate::sound::edits::Edits, p: Param, group: u16) -> bool {
    let mut changed = false;
    for scope in [None, Some(group)] {
        changed |= edits.set(Override {
            group: scope,
            param: p,
            offset: 0.,
        });
    }
    changed
}

#[allow(clippy::too_many_arguments)]
fn panel(
    ui: &mut Ui,
    cx: &mut Cx,
    slot: usize,
    group: u16,
    graph: Graph,
    curves: &Arc<Curves>,
    tint: Color,
    key: Key,
    heard: Option<Arc<spectrum::Shape>>,
) -> El {
    let model = &curves.model;
    let edited = |p: Param| p.read(&model.playing) != p.read(&model.base);
    let handles = curves.handles(graph);
    let (title, empty) = match graph {
        Graph::Envelope => (
            "Envelope",
            "This group has no volume envelope; notes start and stop at once.",
        ),
        Graph::Response => ("Filter · EQ", "No filter or EQ · flat response"),
    };
    // The panel's reset, when any of its values is edited.
    let params: Vec<Param> = handles
        .iter()
        .flat_map(|h| h.params().collect::<Vec<_>>())
        .collect();
    let mut actions = Vec::new();
    // Says what the grey behind the curve is.
    if heard.is_some() {
        actions.push(
            caption("Grey: the part's output")
                .fill(secondary())
                .lines(1)
                .min_w(0)
                .pad(edges(0., INSET - TIGHT, 0., 0.)),
        );
    }
    if params.iter().any(|&p| edited(p)) {
        let (hit, el) = action(ui, format!("{}-reset", graph.id()), "Reset", false);
        if hit {
            let edits = &mut cx.selection.parts[slot].edits;
            for &p in &params {
                reset(edits, p, group);
            }
        }
        actions.push(el.tip(format!(
            "Play the {} as the library has it",
            title.to_lowercase()
        )));
    }
    // Bars keep one height with or without their reset, so the graphs line up.
    let mut rows = vec![section_bar(title, actions).min_h(CONTROL + 2. * TIGHT)];
    let note = || {
        caption(empty)
            .fill(secondary())
            .lines(3)
            .pad((INSET, SPACE))
            .min_w(0)
    };
    // No envelope has no graph; no filter still shows the flat response.
    if handles.is_empty() && graph == Graph::Envelope {
        rows.push(note().id(graph.id()));
        return col(rows).gap(0).min_h(0);
    }
    let focus = cx.state.editor.focus[graph as usize].min(handles.len().saturating_sub(1));
    let grabbed = cx
        .state
        .editor
        .grab
        .filter(|(g, _)| *g == graph)
        .map(|(_, i)| i);
    let c = curves.clone();
    let cache = match graph {
        Graph::Envelope => cx.state.editor.envelope_cache.clone(),
        Graph::Response => cx.state.editor.response_cache.clone(),
    };
    let drawn = canvas_keyed(&cache, (key, tint), move |s| draw(&c, graph, s, tint))
        .w(Len::Pct(100.))
        .h(Len::Pct(100.));
    // Handles and playheads change without the curves: drawn over them.
    let c = curves.clone();
    let atoms = cx.p.shared.part(slot);
    let over = canvas(move |s| {
        let mut out = Vec::new();
        for (i, h) in c.handles(graph).iter().enumerate() {
            let at = place(s, h.at);
            let r = if Some(i) == grabbed { 4.5 } else { 3.5 };
            let fill: Fill = if !h.active {
                Role::Field.into()
            } else if Some(i) == grabbed {
                Role::Ink.into()
            } else {
                tint.into()
            };
            out.push(Draw::fill(handle_square(at, r), fill));
            let ring: Fill = if i == focus {
                Role::Ink.into()
            } else {
                Role::Ink.alpha(0.5)
            };
            out.push(Draw::stroke(handle_square(at, r), ring, 1.));
        }

        if graph == Graph::Envelope
            && let (Some(atoms), Some(shape)) = (&atoms, &c.envelope)
        {
            for t in atoms
                .editor_taps
                .iter()
                .filter_map(|v| Tap::unpack(v.load(Relaxed)))
                .filter(|t| u32::from(t.group) == c.model.runtime_group)
            {
                if let Some(at) = viz::playhead(shape, Phase::from_u8(t.phase), t.level) {
                    let at = place(s, at);
                    out.push(Draw::fill(
                        rect(at.x - 0.5, SPACE, 1., s.height - 2. * SPACE),
                        Role::Ink.alpha(0.25),
                    ));
                    out.push(Draw::fill(circle(at.x, at.y, 3.), Role::Ink));
                }
            }
        }
        out
    })
    .w(Len::Pct(100.))
    .h(Len::Pct(100.));
    let hint = match graph {
        Graph::Envelope => "Drag a handle to shape the envelope; double-click to restore it",
        Graph::Response => {
            "Drag a handle: across for frequency, up for gain or resonance; the wheel sets an EQ band's width; double-click to restore it. Behind: what the part plays now"
        }
    };
    let mut layers = Vec::new();
    if let Some(shape) = heard {
        layers.push(
            canvas(move |s| {
                let mut out = Vec::new();
                spectrum::draw(&mut out, &shape, |p| place(s, p));
                out
            })
            .w(Len::Pct(100.))
            .h(Len::Pct(100.)),
        );
    }
    layers.extend([drawn, over]);
    rows.push(
        col![
            stack(layers)
                .flex(1)
                .min_h(CONTROL * 3.)
                .fill(Role::Field)
                .clip()
                .cursor(Cursor::Grab)
                .tracks_pointer()
                .captures_wheel()
                .tip(hint.to_owned())
                .named(format!("{title} graph"))
                .id(graph.id())
        ]
        .pad((INSET, 0))
        .flex(1)
        .min_h(0),
    );
    if graph == Graph::Response {
        rows.push(
            row![
                caption("20 Hz").fill(secondary()),
                spacer(),
                caption("1 kHz").fill(secondary()),
                spacer(),
                caption("20 kHz").fill(secondary())
            ]
            .pad((INSET, TIGHT))
            .shrink(0),
        );
    }
    if handles.is_empty() {
        rows.push(note());
    } else if !cx.state.editor.compact {
        // The envelope's values all; the response's focused handle's.
        let shown: Vec<Param> = match graph {
            Graph::Envelope => Param::ENVELOPE.to_vec(),
            Graph::Response => handles[focus].params().collect(),
        };
        let mut cells = Vec::new();
        for p in shown {
            let Some(v) = p.read(&model.playing) else {
                continue;
            };
            let changed = edited(p);
            let id = format!("{}-reset-{p:?}", graph.id());
            // A small cross beside an edited value; its room kept otherwise.
            let side = TEXT + TIGHT;
            let reset_el = if changed {
                if ui.get(id.as_str()).activated() {
                    reset(&mut cx.selection.parts[slot].edits, p, group);
                }
                let name = format!("Reset {} to the library's", viz::label(p).to_lowercase());
                interactive(
                    stack![glyph(Icon::Close, SMALL, Role::Ink.alpha(1.)).centered()]
                        .square(side)
                        .focusable()
                        .a11y(A11y::Button)
                        .named(name.clone())
                        .tip(name)
                        .id(id),
                    false,
                )
            } else {
                block(side, side).shrink(0)
            };
            let value_el = value_field(ui, cx, slot, group, p, v, changed, &curves.model, graph);
            cells.push(
                col![
                    section(viz::label(p)),
                    row![value_el, reset_el].gap(TIGHT).align(Align::Center),
                ]
                .gap(0)
                .align(Align::Start),
            );
        }
        let columns = 3;
        let title = match (graph, handles[focus].x) {
            (Graph::Response, Some((Param::Cutoff(s), _))) => format!("Filter · slot {}", s + 1),
            (Graph::Response, Some((Param::Freq(s, b), _))) => {
                format!("EQ · slot {} · band {}", s + 1, b + 1)
            }
            _ => String::new(),
        };
        if !title.is_empty() {
            rows.push(row![section(&title), spacer()].pad(edges(SPACE, INSET, 0., INSET)));
        }
        rows.push(
            grid(columns, cells)
                .gap(INSET)
                .pad((INSET, TIGHT))
                .shrink(0),
        );
    }
    col(rows)
        .gap(0)
        .align(Align::Stretch)
        .min_h(0)
        .pad(edges(0., 0., SPACE, 0.))
}

/// The curves under the handles.
fn draw(c: &Curves, graph: Graph, s: Size, tint: Color) -> Vec<Draw> {
    let mut out = Vec::new();
    let grid = |out: &mut Vec<Draw>, x: f32| {
        let at = place(s, [x, 0.]);
        out.push(Draw::fill(rect(at.x, 0., 1., s.height), hairline()));
    };
    match graph {
        Graph::Envelope => {
            let Some(shape) = &c.envelope else { return out };
            for r in &shape.stages[1..] {
                grid(&mut out, shape.line[r.start][0]);
            }
            out.push(Draw::fill(area(s, &shape.line), tint.with_alpha(0.14)));
            if let Some(ghost) = &c.envelope_ghost {
                out.push(Draw::stroke(line(s, &ghost.line), Role::Ink.alpha(0.3), 1.));
            }
            out.push(Draw::stroke(line(s, &shape.line), tint, 1.5));
        }
        Graph::Response => {
            for hz in [100., 1000., 10_000.] {
                grid(&mut out, viz::freq_x(hz));
            }
            for db in [-24., -12., 12.] {
                let y = place(s, [0., viz::db_y(db)]).y;
                out.push(Draw::fill(rect(0., y, s.width, 1.), hairline()));
            }
            let zero = place(s, [0., viz::db_y(0.)]).y;
            out.push(Draw::fill(
                rect(0., zero, s.width, 1.),
                Role::Ink.alpha(0.18),
            ));
            if c.response.is_empty() {
                return out;
            }
            // No filter: the flat line in ink, no color to suggest one.
            if c.handles(Graph::Response).is_empty() {
                out.push(Draw::stroke(
                    line(s, &c.response),
                    Role::Ink.alpha(0.45),
                    1.5,
                ));
                return out;
            }
            out.push(Draw::fill(area(s, &c.response), tint.with_alpha(0.14)));
            if let Some(ghost) = &c.response_ghost {
                out.push(Draw::stroke(line(s, ghost), Role::Ink.alpha(0.3), 1.));
            }
            out.push(Draw::stroke(line(s, &c.response), tint, 1.5));
        }
    }
    out
}

/// Under the graphs: the group's zones, what modulates it, or its effects.
fn lower(
    ui: &mut Ui,
    cx: &mut Cx,
    slot: usize,
    group: u32,
    instrument: &sampler_ir::Instrument,
    tint: Color,
) -> El {
    let now = cx.state.editor.lower;
    let mut latches = Vec::new();
    for (which, label, name) in [
        (
            Lower::Zones,
            "Zones",
            "The group's zones and the voices playing",
        ),
        (
            Lower::Modulation,
            "Modulation",
            "What modulates the group, and by how much now",
        ),
        (
            Lower::Effects,
            "Effects",
            "The group's and the instrument's effects",
        ),
    ] {
        let (hit, el) = latch(
            ui,
            format!("edit-lower-{which:?}"),
            label,
            name,
            now == which,
        );
        if hit {
            cx.state.editor.lower = which;
        }
        latches.push(el);
    }
    let bar = row![segmented(latches), spacer()]
        .pad(edges(TIGHT, INSET, TIGHT, INSET))
        .shrink(0);
    let body = match now {
        Lower::Zones => {
            return col![bar, zones(cx, slot, group, instrument, tint)]
                .gap(0)
                .shrink(0);
        }
        Lower::Modulation => chain::modulation(cx.p, instrument, group as usize),
        Lower::Effects => chain::effects(instrument, group as usize),
    };
    col![
        bar,
        col![body]
            .pad(edges(0., INSET, SPACE, INSET))
            .max_size(Size::new(1e5, CONTROL * 5.))
            .scroll()
    ]
    .gap(0)
    .shrink(0)
}

/// The group's zones on a key × velocity strip, with every voice of the
/// part as a dot: the shown group's inked, the others' faint.
fn zones(cx: &Cx, slot: usize, group: u32, instrument: &sampler_ir::Instrument, tint: Color) -> El {
    let zones: Vec<_> = (instrument.zones.iter())
        .filter(|z| z.group == Some(sampler_ir::GroupRef(group as usize)))
        .map(|z| (z.keys.low, z.keys.high, z.velocities.low, z.velocities.high))
        .collect();
    let atoms = cx.p.shared.part(slot);
    let map = canvas(move |s| {
        let mut draw = Vec::new();
        for n in (0..128).step_by(12) {
            draw.push(Draw::fill(
                rect(f64::from(n) / 128. * s.width, 0., 1., s.height),
                hairline(),
            ));
        }
        for &(lo, hi, lv, hv) in &zones {
            let x = f64::from(lo) / 128. * s.width;
            let y = f64::from(127 - hv) / 128. * s.height;
            let w = f64::from(hi.saturating_sub(lo) + 1) / 128. * s.width;
            let h = f64::from(hv.saturating_sub(lv) + 1) / 128. * s.height;
            draw.push(Draw::fill(
                rect(x, y, (w - 1.).max(1.), (h - 1.).max(1.)),
                tint.with_alpha(0.3),
            ));
        }

        if let Some(atoms) = &atoms {
            for t in atoms
                .editor_taps
                .iter()
                .filter_map(|v| Tap::unpack(v.load(Relaxed)))
            {
                let x = (f64::from(t.note) + 0.5) / 128. * s.width;
                let y = f64::from(127 - t.velocity) / 128. * s.height;
                let ink: Fill = if u32::from(t.group) == group {
                    Role::Ink.into()
                } else {
                    Role::Ink.alpha(0.35)
                };
                draw.push(Draw::fill(circle(x, y, 2. + 2. * f64::from(t.level)), ink));
            }
        }
        draw
    })
    .flex(1)
    .min_w(0)
    .h(CONTROL * 2.)
    .fill(Role::Field)
    .clip()
    .named("Group key and velocity zones with the voices playing");
    col![
        col![
            map,
            row![
                caption(note_name(0)).fill(secondary()),
                spacer(),
                caption(note_name(127)).fill(secondary())
            ]
            .shrink(0)
        ]
        .gap(TIGHT)
        .pad(edges(0., INSET, SPACE, INSET))
    ]
    .gap(0)
    .shrink(0)
}
