//! What the rack shows of a part below its header: why it is silent or
//! incomplete, then the library's own interface (drawn from its UI IR, as
//! authored or with our controls) or, without one, what its load decoded and
//! could not translate.

use super::{Cx, inside, ir_view, pictures, theme::*};
use moose::mui::mui::prelude::*;
use sampler_ui_ir::{self as ir, Presentation};
use std::sync::Arc;

/// A part's interface as drawn: resolved, with the pictures it needs.
pub struct Face {
    /// The interfaces it was made from, to notice a reload.
    from: Arc<[ir::Interface]>,
    /// Which of them is shown.
    pub shown: usize,
    face: ir::Interface,
    source: pictures::Source,
    assets: ir_view::Assets,
    pub presentation: Presentation,
    values: ir_view::Values,
}

impl Face {
    fn new(path: &std::path::Path, from: Arc<[ir::Interface]>, shown: usize, presentation: Presentation) -> Self {
        let source = pictures::Source::of(path);
        let face = ir_view::resolved(&from[shown]);
        let mut out = Self { from, shown, face, source, assets: Default::default(), presentation, values: Default::default() };
        out.sync();
        out
    }

    /// Decoded picture bytes the view keeps.
    pub fn bytes(&self) -> usize {
        self.assets.bytes()
    }

    fn sync(&mut self) {
        let source = &mut self.source;
        self.assets.sync(&self.face, self.presentation, |a| source.load(a));
    }
}

/// The interface the rack opens on: the script with the most controls.
fn main_face(faces: &[ir::Interface]) -> Option<usize> {
    (0..faces.len()).filter(|&n| !faces[n].widgets.is_empty()).max_by_key(|&n| faces[n].widgets.len())
}

/// `slot`'s library interface, when its scripts declare one.
fn interface(ui: &mut Ui, cx: &mut Cx, slot: usize, lead: Option<El>) -> Option<El> {
    let from = cx.view.parts.get(slot)?.interfaces.clone();
    let main = main_face(&from)?;
    let path = std::path::PathBuf::from(&cx.selection.parts[slot].path);
    let stale = cx.state.faces.get(&slot).is_none_or(|f| !Arc::ptr_eq(&f.from, &from));
    if stale {
        // ponytail: reads and decodes on the UI thread, once per load; move to the
        // loader's worker when big libraries make the first frame stall.
        // Vector unless the source needed something we only approximate.
        let start = if from[main].unsupported.is_empty() { Presentation::Vector } else { Presentation::Bitmap };
        cx.state.faces.insert(slot, Face::new(&path, from.clone(), main, start));
    }
    let face = cx.state.faces.get_mut(&slot)?;

    // Which script's view, when several have one, and how it is drawn.
    let mut bar: Vec<El> = lead.into_iter().collect();
    let with: Vec<usize> = (0..from.len()).filter(|&n| !from[n].widgets.is_empty()).collect();
    let mut pick = None;
    if with.len() > 1 {
        let tabs: Vec<El> = with
            .iter()
            .map(|&n| {
                let label = match from[n].source {
                    ir::Source::Ksp { slot } => format!("Script {}", slot + 1),
                    _ => format!("View {}", n + 1),
                };
                let (hit, el) = latch(ui, format!("face-{slot}-{n}"), &label, "Show this script's view", face.shown == n);
                if hit {
                    pick = Some(n);
                }
                el
            })
            .collect();
        bar.push(segmented(tabs));
    }
    bar.push(spacer());
    let original = face.presentation == Presentation::Bitmap;
    let (a, orig) = latch(ui, format!("face-original-{slot}"), "Original", "The library's own artwork", original);
    let (b, vect) = latch(ui, format!("face-vector-{slot}"), "Vector", "The library's background with KONTRA's controls; frees the control pictures", !original);
    bar.push(segmented(vec![orig, vect]));
    if let Some(n) = pick.filter(|&n| n != face.shown) {
        let presentation = face.presentation;
        *face = Face::new(&path, from.clone(), n, presentation);
    }
    if a || b {
        face.presentation = if b { Presentation::Vector } else { Presentation::Bitmap };
        face.sync();
    }

    let held = face.assets.bytes() as f64 / (1024. * 1024.);
    bar.insert(bar.len() - 1, caption(format!("{held:.1} MB pictures")).fill(secondary()).lines(1).tip("Decoded artwork this view keeps in memory"));
    let page = &face.face.pages[0];
    let avail = ui.scene().and_then(|s| s.surface(&format!("face-{slot}"))).map_or(f64::from(page.size.width), |s| s.frame.size.width);
    let scale = (avail / f64::from(page.size.width.max(1))).clamp(0.5, 1.0);
    // The core's values (scripts change them too); edits go back as widget edits.
    let shared = cx.p.shared.part(slot);
    let current: Vec<_> = shared.as_ref().map(|p| p.control_values()).unwrap_or_default();
    face.values.extend(current.iter().copied());
    let view = ir_view::view(ui, &face.face, ir::PageRef(0), &face.assets, face.presentation, scale, &mut face.values);
    for &(id, was) in &current {
        if let Some(&now) = face.values.get(&id)
            && now != was
        {
            cx.p.shared.set_control(slot, id, now);
        }
    }
    Some(
        col![
            row(bar).gap(SPACE).align(Align::Center).pad((TIGHT, INSET)).w(Len::Pct(100.)).shrink(0),
            row![spacer(), view, spacer()].w(Len::Pct(100.)).shrink(0).id(format!("face-{slot}"))
        ]
        .gap(0)
        .align(Align::Stretch)
        .w(Len::Pct(100.))
        .shrink(0),
    )
}

/// The empty rack.
pub fn welcome(cx: &Cx) -> El {
    let mut lines = vec![
        title("Pick an instrument").text_weight(Weight::SEMIBOLD),
        body("Choose a library on the left and click an instrument, or drag it onto the rack. Multis load the whole rack.")
            .fill(secondary())
            .lines(3)
            .max_size(Size::new(TEXT * 35., CONTROL * 3.)),
    ];
    if !cx.view.multi_status.is_empty() {
        lines.push(caption(cx.view.multi_status.clone()).fill(secondary()).lines(2));
    }
    col![spacer(), col(lines).gap(SPACE).align(Align::Start), spacer()]
        .align(Align::Center)
        .pad(INSET * 3.)
        .flex(1)
        .min_h(0)
}

/// Why `slot` is silent or incomplete, in words a player can act on.
pub fn notices(cx: &Cx, slot: usize) -> Option<El> {
    let v = &cx.view.parts[slot];
    let mut out = Vec::new();
    if let Some(reason) = v.status.strip_prefix("Load failed: ") {
        out.push(banner(Role::Danger, format!("This instrument could not be loaded: {reason}.")));
    }
    if let Some(p) = v.report.as_ref().map(|r| r.runtime) {
        if p.capacity_drops > 0 {
            out.push(banner(Role::Warning, format!("{} notes were dropped: the part ran out of voices.", p.capacity_drops)));
        }
        if p.ignored_input > 0 {
            out.push(banner(Role::Warning, format!("{} MIDI messages this part does not play yet were ignored.", p.ignored_input)));
        }
    }
    (!out.is_empty()).then(|| col(out).gap(0).align(Align::Stretch).shrink(0))
}

/// The instrument volume as the part's volume: the saved level until CC7
/// arrives, then CC7 cubed (`sampler_ir::HostVolume`).
pub fn volume_text(inst: &sampler_ir::Instrument) -> Option<String> {
    let v = inst.host_volume?;
    let db = if v.saved > 0. { 20. * v.saved.log10() } else { f64::NEG_INFINITY };
    Some(if db.is_finite() { format!("CC{} {db:+.1} dB", v.controller) } else { format!("CC{} off", v.controller) })
}

/// The dynamics badge: the picked start (what the next load uses) wins over
/// the last load's value; at the default start it warns when the part waits.
pub fn badge_text(controllers: &str, picked: i16, loaded: u8, needs: bool) -> String {
    match u8::try_from(picked) {
        Ok(start) => format!("{controllers} starts at {start}"),
        Err(_) if needs => format!("Needs {controllers}"),
        Err(_) => format!("{controllers} starts at {loaded}"),
    }
}

/// What the part plays and listens to, in one line under its header: the
/// articulation (with its switch keys), the instrument volume, the dynamics
/// controller it waits for (one click sets where it starts) and MPE.
pub fn performance(ui: &mut Ui, cx: &mut Cx, slot: usize) -> Option<El> {
    let v = cx.view.parts.get(slot)?;
    let (inst, report) = (v.instrument.clone(), v.report.clone());
    if inst.is_none() && report.is_none() {
        return None;
    }
    let mut items = Vec::new();
    if let Some(inst) = inst.as_deref() {
        if let Some(n) = inside::active(cx, slot).filter(|&n| n < inst.articulations.len()) {
            let a = &inst.articulations[n];
            let keys = a.switch_keys.iter().map(|&k| note_name(k)).collect::<Vec<_>>().join(" ");
            let text = if keys.is_empty() { a.name.clone() } else { format!("{} · {keys}", a.name) };
            items.push(row![caption("Articulation").fill(secondary()), body(text).lines(1)].gap(SPACE).align(Align::Center).named("Articulation").id(format!("perf-art-{slot}")));
        }
        if let Some(text) = volume_text(inst) {
            items.push(
                row![caption("Volume").fill(secondary()), body(text).lines(1)]
                    .gap(SPACE)
                    .align(Align::Center)
                    .tip("The instrument's saved volume until CC7 arrives; then CC7 cubed")
                    .id(format!("perf-vol-{slot}")),
            );
        }
    }
    let dynamics = report.as_ref().map(|r| r.decoded.dynamics.clone()).unwrap_or_default();
    let moving: Vec<_> = dynamics.iter().filter(|&&(cc, _)| cc != 11).map(|&(cc, _)| format!("CC{cc}")).collect();
    if !moving.is_empty() {
        let now = cx.selection.parts[slot].dynamics;
        let mut picked = now;
        let tabs = [(-1, "Kontakt"), (64, "64"), (127, "127")]
            .into_iter()
            .map(|(value, label)| {
                let (hit, el) = latch(ui, format!("dyn-{slot}-{value}"), label, &format!("Start {} at {label}", moving.join("/")), now == value);
                if hit {
                    picked = value;
                }
                el
            })
            .collect();
        cx.selection.parts[slot].dynamics = picked;
        // The picked start is what the next load uses; the report shows the last load's.
        let loaded = dynamics.iter().find(|&&(cc, _)| cc != 11).map_or(0, |d| d.1);
        let needs = report.as_ref().is_some_and(|r| r.decoded.needs_controller);
        let words = badge_text(&moving.join("/"), picked, loaded, needs);
        let badge = if words.starts_with("Needs") {
            caption(words).fill(Role::Warning).tip("Near-silent until the controller moves; set where it starts")
        } else {
            caption(words).fill(secondary())
        };
        items.push(row![badge.id(format!("perf-needs-{slot}")), segmented(tabs)].gap(SPACE).align(Align::Center));
    }
    Some(row(items).gap(SPACE * 2.).align(Align::Center).pad((SPACE, TIGHT)).w(Len::Pct(100.)).shrink(0).id(format!("perf-{slot}")))
}

/// The part's view switch, then the view: its interface, articulations,
/// mapping, sound or info.
pub fn stage(ui: &mut Ui, cx: &mut Cx, slot: usize) -> El {
    let (view, tabs) = inside::bar(ui, cx, slot);
    let perf = performance(ui, cx, slot);
    if view == inside::View::Interface
        && let Some(face) = interface(ui, cx, slot, tabs.clone())
    {
        return col(perf.into_iter().chain([face]).collect::<Vec<_>>()).gap(0).align(Align::Stretch).w(Len::Pct(100.)).shrink(0);
    }
    let tabs = tabs.map(|t| row![t, spacer()].align(Align::Center).pad((TIGHT, INSET)).w(Len::Pct(100.)).shrink(0));
    let body = inside::view(ui, cx, slot, view).unwrap_or_else(|| spacer().h(0));
    col(perf.into_iter().chain(tabs).chain([body]).collect::<Vec<_>>()).gap(0).align(Align::Stretch).w(Len::Pct(100.)).shrink(0).id(format!("stage-{slot}"))
}
