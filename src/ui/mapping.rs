//! Read-only sample mapping from the worker-published IR; audition uses the keyboard queue.
use super::{Cx, theme::*};
use moose::mui::mui::prelude::*;
use sampler_ir as ir;
use std::{
    collections::HashSet,
    sync::{Arc, Weak},
};

#[derive(Default)]
pub(super) struct State {
    source: Option<Arc<ir::Instrument>>,
    counts: Vec<usize>,
    links: Vec<Vec<usize>>,
    source_ids: Vec<u32>,
    bounds: (u8, u8),
    rectangles: Arc<[(usize, u8, u8, u8, u8)]>,
    selected: usize,
    query: String,
    stack: Vec<usize>,
    audition: Option<Audition>,
    cell: Option<(u8, u8)>,
    wave: Option<(usize, (u64, u64))>,
}
struct Audition {
    params: Weak<crate::plugin::SamplerParams>,
    note: u8,
    surface: String,
}
impl Drop for Audition {
    fn drop(&mut self) {
        if let Some(p) = self.params.upgrade() {
            p.shared.release_key(self.note);
        }
    }
}

/// Runs even when a part closes or another chrome view hides the map.
pub(super) fn release(ui: &Ui, cx: &mut Cx) {
    for state in cx.state.inside.values_mut() {
        if state
            .mapping
            .audition
            .as_ref()
            .is_some_and(|a| !ui.get(a.surface.as_str()).held)
        {
            state.mapping.audition = None;
        }
    }
}
fn group(z: &ir::Zone, inst: &ir::Instrument) -> usize {
    z.group.map_or(inst.groups.len(), |g| g.0)
}
fn color(g: usize) -> Color {
    Color::oklch(0.72, 0.09, golden_hue(200., g))
}
fn sample(inst: &ir::Instrument, z: &ir::Zone) -> String {
    match inst.assets.get(z.asset.0).map(|a| &a.location) {
        Some(ir::AssetLocation::Path(p)) => p.rsplit(['/', '\\']).next().unwrap_or(p).to_owned(),
        Some(ir::AssetLocation::KontaktFile { id }) => format!("Container sample {id}"),
        None => "Sample unavailable".into(),
    }
}
/// Reference links only: composed predicates remain the core's selection authority.
fn links(inst: &ir::Instrument) -> Vec<Vec<usize>> {
    let mut links: Vec<Vec<usize>> = inst.groups.iter().map(|group| {
        inst.articulations.iter().enumerate().filter(|(_, a)| group.start.iter().any(|s| matches!(s.test, ir::StartTest::Key {low, high} if a.switch_keys.iter().any(|k| (low..=high).contains(k))))).map(|(n, _)| n).collect()
    }).chain(std::iter::once(Vec::new())).collect();
    for z in &inst.zones {
        if let Some(a) = z.articulation
            && a.0 < inst.articulations.len()
        {
            let list = &mut links[group(z, inst)];
            if !list.contains(&a.0) {
                list.push(a.0);
            }
        }
    }
    links
}
fn takes(inst: &ir::Instrument, z: &ir::Zone) -> String {
    let mut labels = Vec::new();
    if let Some(s) = z.selection
        && let Some(seq) = inst.sequences.get(s.sequence.0)
    {
        let mode = match seq.policy {
            ir::SequencePolicy::RoundRobin => "RR",
            ir::SequencePolicy::Random => "Random",
            ir::SequencePolicy::RandomNoRepeat => "Random, no repeat",
        };
        labels.push(match s.take {
            ir::Take::Index(n) => format!("{mode} {}/{total}", n + 1, total = seq.takes),
            ir::Take::Probability { low, high } => format!("{mode} {low:.2}–{high:.2}"),
        });
    }
    if let Some(g) = z.group.and_then(|g| inst.groups.get(g.0)) {
        for s in &g.start {
            if let ir::StartTest::RoundRobin(n) = s.test {
                labels.push(format!("Native RR {n}"));
            }
        }
    }
    for pick in &z.axes {
        if let Some(axis) = inst.axes.get(pick.axis)
            && let Some(choice) = axis.choices.get(pick.choice)
        {
            labels.push(format!("{}: {}", axis.name, choice.name));
        }
    }
    if labels.is_empty() {
        "Layer".into()
    } else {
        labels.join(" · ")
    }
}
fn stack(inst: &ir::Instrument, picked: Option<usize>, key: u8, vel: u8) -> Vec<usize> {
    inst.zones
        .iter()
        .enumerate()
        .filter(|(_, z)| {
            picked.is_none_or(|g| group(z, inst) == g)
                && (z.keys.low..=z.keys.high).contains(&key)
                && (z.velocities.low..=z.velocities.high).contains(&vel)
        })
        .map(|(n, _)| n)
        .collect()
}
fn midpoint(z: &ir::Zone) -> (u8, u8) {
    (
        ((u16::from(z.keys.low) + u16::from(z.keys.high)) / 2) as u8,
        ((u16::from(z.velocities.low) + u16::from(z.velocities.high)) / 2) as u8,
    )
}
fn point(at: Point, size: Size, bounds: (u8, u8)) -> Option<(u8, u8)> {
    if size.width <= 0.
        || size.height <= 0.
        || !(0. ..size.width).contains(&at.x)
        || !(0. ..size.height).contains(&at.y)
    {
        return None;
    }
    let key = bounds.0 + (at.x / size.width * f64::from(bounds.1 - bounds.0 + 1)).floor() as u8;
    let vel = ((1. - at.y / size.height) * 128.).floor().clamp(1., 127.) as u8;
    Some((key, vel))
}

fn fit(inst: &ir::Instrument, picked: Option<usize>) -> (u8, u8) {
    let (mut low, mut high) = (127, 0);
    for z in inst
        .zones
        .iter()
        .filter(|z| picked.is_none_or(|g| group(z, inst) == g))
    {
        low = low.min(z.keys.low);
        high = high.max(z.keys.high);
    }
    if low > high {
        (0, 127)
    } else {
        (
            low / 12 * 12,
            (u16::from(high) / 12 * 12 + 11).min(127) as u8,
        )
    }
}
fn window(center: u8, width: u16) -> (u8, u8) {
    let width = width.clamp(12, 128) as i16;
    let low = (i16::from(center) - width / 2).clamp(0, 128 - width);
    (low as u8, (low + width - 1) as u8)
}
fn pan(bounds: (u8, u8), direction: i16) -> (u8, u8) {
    let width = i16::from(bounds.1) - i16::from(bounds.0) + 1;
    let low = (i16::from(bounds.0) + direction * width / 2).clamp(0, 128 - width);
    (low as u8, (low + width - 1) as u8)
}
fn rectangles(inst: &ir::Instrument) -> Arc<[(usize, u8, u8, u8, u8)]> {
    // Overlapping takes share paint geometry; their identities remain in the IR and stack.
    let mut rectangles = inst
        .zones
        .iter()
        .map(|z| {
            (
                group(z, inst),
                z.keys.low,
                z.keys.high,
                z.velocities.low,
                z.velocities.high,
            )
        })
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    rectangles.sort_unstable();
    rectangles.into()
}

pub(super) fn view(ui: &mut Ui, cx: &mut Cx, slot: usize, inst: &Arc<ir::Instrument>) -> El {
    let compact = ui
        .scene()
        .and_then(|s| s.surface("editor-root"))
        .is_some_and(|s| s.frame.size.height < 700.);
    // Keep the inspector inside both the resized part and the keyboard boundary.
    let map_height = ui
        .scene()
        .and_then(|scene| {
            let map = scene.surface(&format!("map-{slot}"))?.frame;
            let status = scene.surface(&format!("map-wave-status-{slot}"))?.frame;
            let part = scene.surface(&format!("part-{slot}"))?.frame;
            let rack = scene.surface("rack-view")?.frame;
            let bottom = (part.y + part.size.height - 1.).min(rack.y + rack.size.height) - INSET;
            Some(map.size.height + bottom - (status.y + status.size.height.max(SMALL + 2.)))
        })
        .unwrap_or(if compact { 64. } else { 144. })
        .clamp(32., if compact { 64. } else { 144. });
    let changed = cx
        .state
        .inside
        .entry(slot)
        .or_default()
        .mapping
        .source
        .as_ref()
        .is_none_or(|old| !Arc::ptr_eq(old, inst));
    if changed {
        let mut counts = vec![0; inst.groups.len() + 1];
        for z in &inst.zones {
            counts[group(z, inst)] += 1;
        }
        let bounds = fit(inst, None);
        let st = cx.state.inside.entry(slot).or_default();
        st.group = None;
        st.mapping = State {
            source: Some(inst.clone()),
            counts,
            links: links(inst),
            source_ids: crate::sound::waveform::source_ids(inst),
            rectangles: rectangles(inst),
            bounds,
            ..Default::default()
        };
        ui.set_scroll(format!("map-stack-{slot}"), [0., 0.]);
        if let Some(z) = inst.zones.first() {
            let (k, v) = midpoint(z);
            st.mapping.stack = stack(inst, None, k, v);
        }
    }
    let st = &mut cx.state.inside.get_mut(&slot).unwrap().mapping;
    let before = st.query.clone();
    let search = super::browser::search_field(
        ui,
        &format!("map-search-{slot}"),
        &mut st.query,
        "Filter groups",
        "Filter groups by group or articulation name",
    )
    .w(156.);
    if st.query != before {
        ui.set_scroll(format!("map-groups-{slot}"), [0., 0.]);
    }
    let query = st.query.trim().to_lowercase();
    let active = super::inside::active(cx, slot);
    let mut picked = cx.state.inside[&slot].group;
    let (all, all_el) = latch(
        ui,
        format!("map-all-{slot}"),
        "All groups",
        "Show every group",
        picked.is_none(),
    );
    if all {
        picked = None;
    }
    let mut groups = vec![all_el.h(CONTROL)];
    let ids = crate::sound::articulation::identities(&inst.articulations);
    for g in 0..=inst.groups.len() {
        let count = cx.state.inside[&slot].mapping.counts[g];
        if count == 0 {
            continue;
        }
        let name = inst
            .groups
            .get(g)
            .filter(|g| !g.name.is_empty())
            .map_or_else(
                || {
                    if g == inst.groups.len() {
                        "No group".into()
                    } else {
                        format!("Group {}", g + 1)
                    }
                },
                |g| g.name.clone(),
            );
        if !query.is_empty()
            && !name.to_lowercase().contains(&query)
            && !cx.state.inside[&slot].mapping.links[g]
                .iter()
                .any(|&n| inst.articulations[n].name.to_lowercase().contains(&query))
        {
            continue;
        }
        let id = format!("map-group-{slot}-{g}");
        let on = picked == Some(g);
        if ui.get(id.as_str()).activated() {
            picked = if on { None } else { Some(g) };
        }
        groups.push(interactive(
            row![
                block(3., 16.).fill(color(g)).shrink(0),
                body(name.clone()).lines(1).flex(1).min_w(0),
                caption(count.to_string()).fill(secondary())
            ]
            .gap(TIGHT)
            .h(CONTROL)
            .pad((0, TIGHT))
            .align(Align::Center)
            .focusable()
            .a11y(A11y::Toggle { on })
            .named(name.clone())
            .tip(name)
            .id(id),
            on,
        ));
        if let Some(group) = inst.groups.get(g) {
            for n in cx.state.inside[&slot].mapping.links[g].clone() {
                let a = &inst.articulations[n];
                let id = format!("map-art-{slot}-{g}-{n}");
                if ui.get(id.as_str()).activated() {
                    cx.p.shared.select_articulation(slot, n);
                    cx.state.inside.get_mut(&slot).unwrap().active = Some(n);
                }
                let on = active == Some(n);
                let c = super::inside::articulation_color(cx, slot, &ids[n], a);
                let input = cx.selection.parts[slot].articulation_overlay.input(
                    &ids[n],
                    a,
                    super::inside::mode(cx, slot, inst),
                );
                let label = format!("{} · {}", a.name, super::inside::input_label(&input));
                groups.push(interactive(
                    row![
                        block(3., 12.).fill(c).shrink(0),
                        caption(label.clone()).lines(1).min_w(0).flex(1)
                    ]
                    .gap(TIGHT)
                    .pad((0, SPACE))
                    .h(20.)
                    .focusable()
                    .a11y(A11y::Toggle { on })
                    .when(on, |e| e.fill(Role::Raised))
                    .named(format!("Select {} articulation", a.name))
                    .tip(format!("{label}; authored group conditions still apply"))
                    .id(id),
                    on,
                ));
            }
            let conditions = group
                .start
                .iter()
                .map(|s| match s.test {
                    ir::StartTest::Key { low, high } => {
                        format!("Native KS {}–{}", note_name(low), note_name(high))
                    }
                    ir::StartTest::Controller {
                        controller,
                        low,
                        high,
                    } => format!("CC{controller} {low}–{high}"),
                    ir::StartTest::RoundRobin(n) => format!("RR {n}"),
                    ir::StartTest::Random => "Random".into(),
                })
                .collect::<Vec<_>>()
                .join(" · ");
            if !conditions.is_empty() {
                groups.push(
                    caption(conditions.clone())
                        .fill(secondary())
                        .lines(1)
                        .tip(format!("Authored conditions: {:?}", group.start))
                        .min_w(0),
                );
            }
        }
    }
    if groups.len() == 1 && !query.is_empty() {
        groups.push(
            caption("No groups match")
                .fill(secondary())
                .lines(1)
                .h(CONTROL)
                .tip("Clear the group search to show every group")
                .id(format!("map-no-groups-{slot}")),
        );
    }
    if picked != cx.state.inside[&slot].group {
        ui.set_scroll(format!("map-stack-{slot}"), [0., 0.]);
        let st = cx.state.inside.get_mut(&slot).unwrap();
        st.group = picked;
        st.mapping.audition = None;
        st.mapping.cell = None;
        st.mapping.bounds = fit(inst, picked);
        if let Some((n, z)) = inst
            .zones
            .iter()
            .enumerate()
            .find(|(_, z)| picked.is_none_or(|g| group(z, inst) == g))
        {
            st.mapping.selected = n;
            let (k, v) = midpoint(z);
            st.mapping.stack = stack(inst, picked, k, v);
        }
    }
    let mut navigation = Vec::new();
    for (suffix, label, name) in [
        ("pan-left", "<", "Pan keys left"),
        ("zoom-in", "+", "Zoom keys in"),
        ("zoom-out", "−", "Zoom keys out"),
        ("pan-right", ">", "Pan keys right"),
        ("fit", "Fit", "Fit the visible groups"),
    ] {
        let st = &mut cx.state.inside.get_mut(&slot).unwrap().mapping;
        let width = u16::from(st.bounds.1) - u16::from(st.bounds.0) + 1;
        let disabled = match suffix {
            "zoom-in" => width <= 12,
            "zoom-out" => width == 128,
            "pan-left" => st.bounds.0 == 0,
            "pan-right" => st.bounds.1 == 127,
            _ => false,
        };
        let (hit, el) = match suffix {
            "pan-left" => icon_button(ui, format!("map-{suffix}-{slot}"), Icon::Left, name, false),
            "pan-right" => {
                icon_button(ui, format!("map-{suffix}-{slot}"), Icon::Right, name, false)
            }
            _ => super::theme::action(ui, format!("map-{suffix}-{slot}"), label, false),
        };
        if hit && !disabled {
            let center = st
                .cell
                .filter(|c| (st.bounds.0..=st.bounds.1).contains(&c.0))
                .map_or((u16::from(st.bounds.0) + width / 2).min(127) as u8, |c| c.0);
            st.bounds = match suffix {
                "zoom-in" => window(center, width / 2),
                "zoom-out" => window(center, width * 2),
                "pan-left" => pan(st.bounds, -1),
                "pan-right" => pan(st.bounds, 1),
                _ => fit(inst, picked),
            };
        }
        navigation.push(el.named(name).tip(name).when(disabled, |e| e.disabled()));
    }
    let navigation = row(navigation).gap(0).shrink(0);
    let map_id = format!("map-{slot}");
    let bounds = cx.state.inside[&slot].mapping.bounds;
    let r = ui.get(map_id.as_str());
    if r.pressed
        && r.button == Some(Button::Primary)
        && let Some(at) = ui.local(map_id.as_str())
        && let Some(size) = ui
            .scene()
            .and_then(|s| s.surface(&map_id))
            .map(|s| s.frame.size)
        && let Some((key, vel)) = point(at, size, bounds)
    {
        let hit = stack(inst, picked, key, vel);
        if !hit.is_empty() {
            let st = &mut cx.state.inside.get_mut(&slot).unwrap().mapping;
            let n = if hit == st.stack && st.cell == Some((key, vel)) {
                hit.iter()
                    .position(|&n| n == st.selected)
                    .map_or(0, |n| (n + 1) % hit.len())
            } else {
                0
            };
            ui.set_scroll(format!("map-stack-{slot}"), [0., (n % 32) as f64 * 22.]);
            st.cell = Some((key, vel));
            st.selected = hit[n];
            st.stack = hit;
            st.audition = None;
            cx.p.shared.press_key(slot, key, vel);
            st.audition = Some(Audition {
                params: Arc::downgrade(cx.p),
                note: key,
                surface: map_id.clone(),
            });
        }
    }
    let selected = cx.state.inside[&slot].mapping.selected;
    let geometry = cx.state.inside[&slot].mapping.rectangles.clone();
    let selected_zone = inst
        .zones
        .get(selected)
        .map(|z| (z.keys.low, z.keys.high, z.velocities.low, z.velocities.high));
    let map = canvas(move |s| {
        let keys = f64::from(bounds.1 - bounds.0 + 1);
        let x = |k: u16| (f64::from(k) - f64::from(bounds.0)) / keys * s.width;
        let y = |v: u16| (1. - f64::from(v) / 128.) * s.height;
        let mut out = vec![Draw::fill(rect(0., 0., s.width, s.height), Role::Field)];
        for k in bounds.0..=bounds.1 {
            if matches!(k % 12, 1 | 3 | 6 | 8 | 10) {
                out.push(Draw::fill(
                    rect(x(k.into()), 0., s.width / keys, s.height),
                    Role::Ink.alpha(0.025),
                ));
            }
            if k % 12 == 0 {
                out.push(Draw::fill(
                    rect(x(k.into()).round(), 0., 1., s.height),
                    hairline(),
                ));
            }
        }
        for v in [32, 64, 96] {
            out.push(Draw::fill(rect(0., y(v).round(), s.width, 1.), hairline()));
        }
        let box_ = |low: u8, high: u8, bottom: u8, top: u8| {
            rect(
                x(low.into()),
                y(u16::from(top) + 1),
                x(u16::from(high) + 1) - x(low.into()),
                y(bottom.into()) - y(u16::from(top) + 1),
            )
        };
        for &(g, low, high, bottom, top) in geometry
            .iter()
            .filter(|r| r.2 >= bounds.0 && r.1 <= bounds.1)
        {
            let shown = picked.is_none_or(|p| p == g);
            let r = box_(low, high, bottom, top);
            out.push(Draw::fill(
                r.clone(),
                color(g).with_alpha(if shown { 0.18 } else { 0.025 }),
            ));
            if shown {
                out.push(Draw::stroke(r, color(g).with_alpha(0.65), 1.));
            }
        }
        if let Some((low, high, bottom, top)) = selected_zone {
            out.push(Draw::stroke(box_(low, high, bottom, top), Role::Ink, 2.));
        }

        out
    })
    .h(map_height)
    .w(Len::Pct(100.))
    .min_w(0)
    .clip()
    .cursor(Cursor::Crosshair)
    .named(
        "Key and velocity zone map; click and hold to audition; repeat a cell to cycle its stack",
    )
    .id(map_id);
    let width = u16::from(bounds.1) - u16::from(bounds.0) + 1;
    let first = u16::from(bounds.0).div_ceil(12) * 12;
    let mut ticks = vec![
        block(0., 0.)
            .w(Len::Pct(
                (first - u16::from(bounds.0)) as f64 / f64::from(width) * 100.,
            ))
            .shrink(0),
    ];
    for k in (first..=u16::from(bounds.1)).step_by(12) {
        ticks.push(
            caption(note_name(k as u8))
                .fill(secondary())
                .text_size(SMALL)
                .w(Len::Pct(
                    f64::from((k + 12).min(u16::from(bounds.1) + 1) - k) / f64::from(width) * 100.,
                ))
                .shrink(0)
                .min_w(0)
                .id(format!("map-scale-{slot}-{k}")),
        );
    }
    let scale = row(ticks).gap(0);
    let mut layers = Vec::new();
    let stack = cx.state.inside[&slot].mapping.stack.clone();
    let mut page = stack.iter().position(|&n| n == selected).unwrap_or(0) / 32;
    let mut pages = Vec::new();
    for (suffix, label, next) in [
        ("prev", "Previous", page.saturating_sub(1)),
        ("next", "Next", page + 1),
    ] {
        let disabled = next == page || next * 32 >= stack.len();
        let (hit, el) =
            super::theme::action(ui, format!("map-stack-{suffix}-{slot}"), label, false);
        if hit && !disabled {
            page = next;
            cx.state.inside.get_mut(&slot).unwrap().mapping.selected = stack[page * 32];
            ui.set_scroll(format!("map-stack-{slot}"), [0., 0.]);
        }
        pages.push(el.when(disabled, |e| e.disabled()));
    }
    let selected = cx.state.inside[&slot].mapping.selected;
    for &n in stack.iter().skip(page * 32).take(32) {
        let z = &inst.zones[n];
        let on = n == selected;
        let id = format!("map-zone-{slot}-{n}");
        if ui.get(id.as_str()).activated() {
            cx.state.inside.get_mut(&slot).unwrap().mapping.selected = n;
        }
        let label = format!("{} · {}", sample(inst, z), takes(inst, z));
        layers.push(interactive(
            row![
                block(3., 14.).fill(color(group(z, inst))).shrink(0),
                caption(label.clone()).lines(1).min_w(0).flex(1),
                caption(format!(
                    "{}–{} / {}–{}",
                    note_name(z.keys.low),
                    note_name(z.keys.high),
                    z.velocities.low,
                    z.velocities.high
                ))
                .fill(secondary())
                .lines(1)
            ]
            .gap(TIGHT)
            .h(22.)
            .pad((0, TIGHT))
            .focusable()
            .a11y(A11y::Toggle { on })
            .named(label.clone())
            .tip(label)
            .id(id),
            on,
        ));
    }
    let head = row![
        search,
        caption(format!(
            "{} zones · {} {}",
            inst.zones.len(),
            inst.groups.len(),
            if inst.groups.len() == 1 {
                "group"
            } else {
                "groups"
            }
        ))
        .fill(secondary())
        .lines(1)
        .flex(1)
        .min_w(0)
        .tip(format!(
            "{} zones · {} groups in this instrument",
            inst.zones.len(),
            inst.groups.len()
        )),
        caption(format!("{}–{}", note_name(bounds.0), note_name(bounds.1)))
            .fill(secondary())
            .id(format!("map-range-{slot}")),
        navigation
    ]
    .gap(SPACE)
    .align(Align::Center);
    let instructions = caption("Hold a cell to audition · repeat to cycle overlapping zones")
        .fill(secondary())
        .lines(1)
        .min_w(0);
    let grid = row![
        col(groups)
            .gap(1)
            .w(156.)
            .h(map_height + 28.)
            .scroll()
            .id(format!("map-groups-{slot}"))
            .shrink(0),
        col![
            row![
                col![
                    caption("127").fill(secondary()),
                    spacer(),
                    caption("1").fill(secondary())
                ]
                .w(20.)
                .h(map_height)
                .align(Align::End),
                col![map, scale].gap(TIGHT).flex(1).min_w(0)
            ]
            .gap(TIGHT)
            .align(Align::Start),
            instructions
        ]
        .gap(TIGHT)
        .flex(1)
        .min_w(0)
    ]
    .gap(SPACE)
    .align(Align::Start);
    let stack_head = row![
        caption(format!("Overlapping zones · {}", stack.len())).fill(secondary()),
        spacer(),
        caption(if stack.is_empty() {
            "0".into()
        } else {
            format!("{}–{}", page * 32 + 1, ((page + 1) * 32).min(stack.len()))
        })
        .fill(secondary()),
        row(pages).gap(0)
    ]
    .gap(TIGHT)
    .align(Align::Center);
    let inspector = inspector(ui, cx, slot, inst, compact);
    col![
        head,
        grid,
        stack_head,
        col(layers)
            .gap(0)
            .h(if compact { 22. } else { 44. })
            .scroll()
            .id(format!("map-stack-{slot}")),
        inspector
    ]
    .gap(TIGHT)
    .w(Len::Pct(100.))
    .min_w(0)
}

struct LoopMark {
    slot: usize,
    range: ir::LoopRange,
    release: Option<u64>,
    fades: Option<[(u64, u64); 2]>,
}
fn loop_marks(playback: &ir::Playback, rate: u32) -> Vec<LoopMark> {
    let slots = match playback.looping {
        ir::Looping::Continuous(r) => vec![(1, r, false)],
        ir::Looping::UntilRelease(r) => vec![(1, r, true)],
        ir::Looping::Slots(slots) => slots
            .into_iter()
            .enumerate()
            .filter_map(|(n, s)| s.map(|s| (n + 1, s.range, s.until_release)))
            .collect(),
        _ => vec![],
    };
    slots
        .into_iter()
        .map(|(slot, range, release)| {
            let fade = range.crossfade.frames(f64::from(rate));
            // Same source-rate conversion and ping-pong precedence as core lowering.
            let fades = if fade == 0 || range.alternating {
                None
            } else if playback.reverse {
                range
                    .start
                    .checked_add(fade)
                    .zip(range.end.checked_add(fade))
                    .map(|(a, b)| [(range.start, a), (range.end, b)])
            } else {
                range
                    .end
                    .checked_sub(fade)
                    .zip(range.start.checked_sub(fade))
                    .map(|(a, b)| [(a, range.end), (b, range.start)])
            };
            LoopMark {
                slot,
                range,
                release: release.then_some(if playback.reverse {
                    range.start
                } else {
                    range.end
                }),
                fades,
            }
        })
        .collect()
}
fn sample_window(low: u64, width: u64, total: u64) -> (u64, u64) {
    if total == 0 {
        return (0, 0);
    }
    let width = width.clamp(1, total);
    let low = low.min(total - width);
    (low, low + width)
}
fn sample_zoom(bounds: (u64, u64), total: u64, zoom_in: bool, anchor: f64) -> (u64, u64) {
    let width = bounds.1 - bounds.0;
    let next = if zoom_in {
        width / 2
    } else {
        width.saturating_mul(2)
    }
    .clamp(1, total.max(1));
    let anchor = anchor.clamp(0., 1.);
    let low = (bounds.0 as f64 + width as f64 * anchor - next as f64 * anchor).max(0.) as u64;
    sample_window(low, next, total)
}
fn sample_pan(bounds: (u64, u64), total: u64, delta: i64) -> (u64, u64) {
    sample_window(
        bounds.0.saturating_add_signed(delta),
        bounds.1 - bounds.0,
        total,
    )
}
fn sample_x(frame: u64, bounds: (u64, u64), width: f64) -> Option<f64> {
    ((bounds.0..=bounds.1).contains(&frame) && bounds.0 < bounds.1).then(|| {
        ((frame - bounds.0) as f64 / (bounds.1 - bounds.0) as f64 * width).min((width - 1.).max(0.))
    })
}
fn sample_start(playback: &ir::Playback, frames: u64) -> u64 {
    if playback.reverse {
        playback.end.unwrap_or(frames).saturating_sub(1)
    } else {
        playback.start
    }
}

fn inspector(ui: &mut Ui, cx: &mut Cx, slot: usize, inst: &ir::Instrument, compact: bool) -> El {
    let st = &cx.state.inside[&slot].mapping;
    let n = st.selected;
    let Some(z) = inst.zones.get(n) else {
        return caption("No zones in this instrument").fill(secondary());
    };
    let id = format!("map-wave-{slot}");
    let bins = ui
        .scene()
        .and_then(|s| s.surface(&id))
        .map_or(512, |s| s.frame.size.width.ceil().clamp(1., 4096.) as usize);
    let source = st.source_ids.get(n).copied();
    let part = cx.p.shared.part(slot);
    let epoch = cx.view.parts[slot].generation;
    let full = source.and_then(|zone| part.as_ref()?.zone_waveform(zone, epoch, bins));
    let total = full.as_ref().map_or(0, |e| e.frames);
    let st = &mut cx.state.inside.get_mut(&slot).unwrap().mapping;
    if st.wave.is_none_or(|(zone, _)| zone != n) {
        st.wave = Some((n, (0, total)));
    }
    let (_, mut bounds) = st.wave.unwrap();
    if bounds.0 >= bounds.1 && total > 0 {
        bounds = (0, total);
    }
    let mut navigation = Vec::new();
    for (suffix, label, name) in [
        ("pan-left", "", "Pan sample left"),
        ("zoom-in", "+", "Zoom sample in"),
        ("zoom-out", "−", "Zoom sample out"),
        ("pan-right", "", "Pan sample right"),
        ("fit", "Fit", "Fit the whole sample"),
    ] {
        let width = bounds.1 - bounds.0;
        let disabled = total == 0
            || match suffix {
                "pan-left" => bounds.0 == 0,
                "pan-right" => bounds.1 == total,
                "zoom-in" => width <= 1,
                "zoom-out" => width >= total,
                _ => false,
            };
        let id = format!("map-wave-{suffix}-{slot}");
        let (hit, el) = match suffix {
            "pan-left" => icon_button(ui, id, Icon::Left, name, false),
            "pan-right" => icon_button(ui, id, Icon::Right, name, false),
            _ => super::theme::action(ui, id, label, false),
        };
        if hit && !disabled {
            bounds = match suffix {
                "zoom-in" => sample_zoom(bounds, total, true, 0.5),
                "zoom-out" => sample_zoom(bounds, total, false, 0.5),
                "pan-left" => sample_pan(
                    bounds,
                    total,
                    -((width / 2).max(1).min(i64::MAX as u64) as i64),
                ),
                "pan-right" => sample_pan(
                    bounds,
                    total,
                    (width / 2).max(1).min(i64::MAX as u64) as i64,
                ),
                _ => (0, total),
            };
        }
        navigation.push(el.named(name).tip(name).when(disabled, |e| e.disabled()));
    }
    if let Some(wheel) = ui
        .wheel(id.as_str())
        .filter(|w| w.x.is_finite() && w.y.is_finite())
        && total > 0
    {
        let width = ui
            .scene()
            .and_then(|s| s.surface(&id))
            .map_or(1., |s| s.frame.size.width.max(1.));
        if wheel.x != 0. {
            bounds = sample_pan(
                bounds,
                total,
                (wheel.x / width * (bounds.1 - bounds.0) as f64) as i64,
            );
        }
        if wheel.y != 0. {
            let anchor = ui.local(id.as_str()).map_or(0.5, |p| p.x / width);
            bounds = sample_zoom(bounds, total, wheel.y < 0., anchor);
        }
    }
    st.wave = Some((n, bounds));
    let navigation = row(navigation).gap(0).shrink(0);
    let envelope = if bounds == (0, total) {
        full.clone()
    } else {
        source.and_then(|zone| {
            part.as_ref()?
                .zone_waveform_window(zone, epoch, bins, Some(bounds))
        })
    };
    let envelope = envelope.filter(|e| e.range == bounds);
    let root = match z.pitch {
        ir::KeyTracking::Tracked { root } | ir::KeyTracking::Scaled { root, .. } => Some(root),
        ir::KeyTracking::Fixed => inst.assets.get(z.asset.0).and_then(|a| a.root_key),
    };
    let gain = 20. * z.gain.linear().log10();
    let pan = z.pan.position;
    let pan = if pan.abs() < 0.005 {
        "C".into()
    } else {
        format!(
            "{} {:.0}",
            if pan < 0. { "L" } else { "R" },
            pan.abs() * 100.
        )
    };
    let info = format!(
        "Root {} · Tune {:+.0} ct · Volume {gain:+.1} dB · Pan {pan}",
        root.map_or_else(|| "— (fixed pitch)".into(), note_name),
        z.tune.semitones() * 100.
    );
    let playback = z.playback;
    let ranges = loop_marks(&playback, full.as_ref().map_or(0, |e| e.sample_rate));
    let marks = format!(
        "Start {} · End {}{}{}",
        if total > 0 {
            sample_start(&playback, total)
        } else {
            playback.start
        },
        if playback.reverse {
            playback.start.to_string()
        } else {
            playback
                .end
                .map_or_else(|| "sample end".into(), |e| e.to_string())
        },
        if playback.reverse { " · Reverse" } else { "" },
        if ranges.is_empty() {
            " · No loop".into()
        } else {
            format!(
                " · Loop {}",
                ranges
                    .iter()
                    .map(|r| format!(
                        "{}: {}–{}{}{}",
                        r.slot,
                        r.range.start,
                        r.range.end,
                        r.release
                            .map_or(String::new(), |f| format!("; release exit {f}")),
                        r.fades.map_or(String::new(), |f| format!(
                            "; fade {}–{} + {}–{}",
                            f[0].0, f[0].1, f[1].0, f[1].1
                        ))
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    );
    let status = full
        .as_ref()
        .map_or("Waveform not yet available".into(), |e| {
            format!(
                "View {}–{} frames · {:.2} s · {} Hz{}",
                bounds.0,
                bounds.1,
                e.duration_us as f64 / 1e6,
                e.sample_rate,
                if envelope.is_none() {
                    " · Loading peaks"
                } else {
                    ""
                }
            )
        });
    let marks_tip = format!(
        "{marks}. Trim {}–{} (exclusive upper bound). Authored source frames; white: start/end, purple: loop, green: release exit, shaded: both crossfade legs. Envelope release is note-relative, not a fixed source frame.",
        playback.start,
        playback
            .end
            .map_or_else(|| "sample end".into(), |f| f.to_string())
    );
    let wave = canvas(move |s| {
        let mut draws = vec![
            Draw::fill(rect(0., 0., s.width, s.height), Role::Field),
            Draw::fill(rect(0., s.height / 2., s.width, 1.), hairline()),
        ];
        if let Some(e) = &envelope {
            let height = s.height / 2.;
            let len = e.peaks.len();
            for (n, &(lo, hi)) in e.peaks.iter().enumerate() {
                draws.push(Draw::fill(
                    rect(
                        n as f64 / len as f64 * s.width,
                        height - f64::from(hi) * height,
                        (s.width / len as f64).max(1.),
                        (f64::from(hi - lo) * height).max(1.),
                    ),
                    Role::Ink.alpha(0.6),
                ));
            }
        }
        if total>0 {
            let x=|frame:u64| (frame as f64-bounds.0 as f64)/(bounds.1-bounds.0).max(1) as f64*s.width;
            for r in &ranges {
                draws.push(Draw::fill(rect(x(r.range.start),0.,x(r.range.end)-x(r.range.start),s.height),color(1).with_alpha(0.10)));
                if let Some(fades)=r.fades {for (a,b) in fades {draws.push(Draw::fill(rect(x(a),0.,x(b)-x(a),s.height),color(0).with_alpha(0.25)));}}
                for f in [r.range.start,r.range.end] {if let Some(x)=sample_x(f,bounds,s.width) {draws.push(Draw::fill(rect(x,0.,1.,s.height),color(1)));}}
                if let Some(f)=r.release && let Some(x)=sample_x(f,bounds,s.width) {draws.push(Draw::fill(rect(x,0.,2.,s.height),color(2)));}
                if let Some(fades)=r.fades {for (a,b) in fades {for f in [a,b] {if let Some(x)=sample_x(f,bounds,s.width) {draws.push(Draw::fill(rect(x,0.,1.,s.height),color(0)));}}}}
            }
            for f in [sample_start(&playback,total),if playback.reverse {playback.start} else {playback.end.unwrap_or(total)}] {
                if let Some(x)=sample_x(f,bounds,s.width) {draws.push(Draw::fill(rect(x,0.,1.,s.height),Role::Ink));}
            }
        }

        draws
    })
    .h(if compact { 40. } else { 64. })
    .w(Len::Pct(100.))
    .clip()
    .named("Sample waveform with source-frame playback, loop, release-exit and crossfade boundaries; wheel to zoom, horizontal scroll to pan")
    .id(id);
    let aud_id = format!("map-audition-{slot}");
    let r = ui.get(aud_id.as_str());
    if r.pressed && r.button == Some(Button::Primary) || r.key_activated {
        let (key, vel) = midpoint(z);
        let st = &mut cx.state.inside.get_mut(&slot).unwrap().mapping;
        st.audition = None;
        cx.p.shared.press_key(slot, key, vel);
        st.audition = Some(Audition {
            params: Arc::downgrade(cx.p),
            note: key,
            surface: aud_id.clone(),
        });
    }
    let (_, aud) = latch(
        ui,
        aud_id,
        "Audition",
        "Hold to audition through the instrument's normal articulation and RR routing",
        r.held,
    );
    col![
        row![
            body(format!("{} · {}", sample(inst, z), takes(inst, z)))
                .lines(1)
                .flex(1)
                .min_w(0),
            navigation,
            aud
        ]
        .gap(SPACE)
        .align(Align::Center),
        caption(info).lines(1).min_w(0),
        wave,
        caption(marks)
            .id(format!("map-boundaries-{slot}"))
            .lines(1)
            .min_w(0)
            .tip(marks_tip),
        caption(status.clone())
            .fill(secondary())
            .id(format!("map-wave-status-{slot}"))
            .lines(1)
            .min_w(0)
            .tip(status.clone())
    ]
    .gap(TIGHT)
    .id(format!("map-inspector-{slot}-{n}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mapping_sample_markers_match_source_loop_and_reverse_boundaries() {
        let range = ir::LoopRange {
            start: 12000,
            end: 34000,
            crossfade: ir::Span::Time(ir::Time::Milliseconds(10.)),
            alternating: false,
        };
        let mut p = ir::Playback {
            start: 2000,
            end: Some(45000),
            looping: ir::Looping::UntilRelease(range),
            ..Default::default()
        };
        let a = loop_marks(&p, 48000);
        assert_eq!(a[0].release, Some(34000));
        assert_eq!(a[0].fades, Some([(33520, 34000), (11520, 12000)]));
        assert_eq!(sample_start(&p, 48000), 2000);
        assert_eq!(sample_x(12000, (10000, 34000), 240.), Some(20.));
        assert_eq!(
            sample_x(2000, (10000, 34000), 240.),
            None,
            "out-of-view markers never clamp to a false boundary"
        );
        p.reverse = true;
        let a = loop_marks(&p, 48000);
        assert_eq!(sample_start(&p, 48000), 44999);
        assert_eq!(a[0].release, Some(12000));
        assert_eq!(a[0].fades, Some([(12000, 12480), (34000, 34480)]));
        let mut slots = [None; 8];
        slots[3] = Some(ir::LoopSlot {
            range: ir::LoopRange {
                alternating: true,
                ..range
            },
            count: 0,
            tuning: 1.,
            until_release: true,
        });
        p.looping = ir::Looping::Slots(slots);
        let a = loop_marks(&p, 48000);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].slot, 4);
        assert_eq!(
            a[0].fades, None,
            "ping-pong takes precedence over authored crossfade, as in lowering"
        );
        assert_eq!(a[0].release, Some(12000));
    }
    #[test]
    fn mapping_sample_viewport_retains_exact_frame_anchor_and_clamps_pan() {
        assert_eq!(sample_zoom((0, 48000), 48000, true, 0.5), (12000, 36000));
        assert_eq!(
            sample_zoom((10000, 34000), 48000, true, 0.25),
            (13000, 25000)
        );
        assert_eq!(sample_pan((12000, 36000), 48000, 12000), (24000, 48000));
        assert_eq!(sample_pan((12000, 36000), 48000, -20000), (0, 24000));
        assert_eq!(sample_zoom((3, 4), 8, true, 0.5), (3, 4));
        assert_eq!(sample_window(u64::MAX, u64::MAX, 48000), (0, 48000));
        assert_eq!(sample_window(0, 0, 0), (0, 0));
    }
    #[test]
    fn mapping_zoom_and_pan_keep_every_key_reachable() {
        assert_eq!(window(0, 12), (0, 11));
        assert_eq!(window(127, 12), (116, 127));
        assert_eq!(window(60, 256), (0, 127));
        for center in 0..128 {
            for width in [12, 24, 64, 128] {
                let b = window(center, width);
                assert_eq!(u16::from(b.1) - u16::from(b.0) + 1, width);
                assert!((b.0..=b.1).contains(&center));
                for direction in [-1, 1] {
                    let p = pan(b, direction);
                    assert_eq!(p.1 - p.0, b.1 - b.0);
                }
            }
        }
        assert_eq!(pan((116, 127), 1), (116, 127));
        assert_eq!(pan((0, 11), -1), (0, 11));
        assert_eq!(
            point(Point::new(0., 0.), Size::new(12., 128.), (116, 127)),
            Some((116, 127))
        );
    }
    #[test]
    fn mapping_paint_cache_preserves_all_overlapping_identities() {
        let mut inst = ir::Instrument::default();
        for _ in 0..1024 {
            inst.zones.push(ir::Zone::new(ir::AssetRef(0)));
        }
        assert_eq!(rectangles(&inst).len(), 1);
        assert_eq!(stack(&inst, None, 60, 64), (0..1024).collect::<Vec<_>>());
        inst.groups.push(ir::Group::default());
        inst.zones[1023].group = Some(ir::GroupRef(0));
        inst.zones[1023].keys = ir::KeyRange { low: 60, high: 72 };
        assert_eq!(rectangles(&inst).len(), 2);
        assert_eq!(fit(&inst, Some(0)), (60, 83));
        assert_eq!(stack(&inst, Some(0), 60, 64), vec![1023]);
    }
    #[test]
    fn mapping_group_hues_respect_the_existing_palette_for_native_group_counts() {
        for group in 0..65_536 {
            assert!(!orange(golden_hue(200., group)), "group {group}");
        }
        assert!((golden_hue(200., 7) - 38.76).abs() < 0.01);
    }
    #[test]
    fn map_cell_edges_and_overlapping_identity_are_inclusive() {
        assert_eq!(
            point(Point::new(0., 0.), Size::new(128., 128.), (0, 127)),
            Some((0, 127))
        );
        assert_eq!(
            point(Point::new(127.9, 127.9), Size::new(128., 128.), (0, 127)),
            Some((127, 1))
        );
        assert_eq!(
            point(Point::new(128., 40.), Size::new(128., 128.), (0, 127)),
            None
        );
        let mut i = ir::Instrument::default();
        for _ in 0..2 {
            i.zones.push(ir::Zone::new(ir::AssetRef(0)));
        }
        assert_eq!(stack(&i, None, 60, 64), vec![0, 1]);
    }
}
