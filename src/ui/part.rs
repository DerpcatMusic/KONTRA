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
    path: std::path::PathBuf,
    generation: u64,
    revision: u64,
    patch: ir::InterfacePatch,
    pub page: ir::PageRef,
    /// Which of them is shown.
    pub shown: usize,
    face: ir::Interface,
    source: pictures::Source,
    assets: ir_view::Assets,
    pub presentation: Presentation,
    values: ir_view::Values,
}

impl Face {
    fn new(path: &std::path::Path, generation: u64, from: Arc<[ir::Interface]>, shown: usize, presentation: Presentation) -> Self {
        let source = pictures::Source::of(path);
        let face = ir_view::resolved(&from[shown]);
        let mut out = Self { from, path: path.into(), generation, revision: u64::MAX, patch: Default::default(), page: ir::PageRef(0), shown, face, source, assets: Default::default(), presentation, values: Default::default() };
        out.sync();
        out
    }

    fn update(&mut self, patch: ir::InterfacePatch) {
        if patch == self.patch { return; }
        let mut changed: Vec<_> = self.patch.widgets.iter().chain(&patch.widgets).map(|(n, _)| *n).collect();
        changed.sort_unstable(); changed.dedup();
        let assets_changed = patch.assets != self.patch.assets;
        patch.apply(&self.from[self.shown], &self.patch, &mut self.face);
        if assets_changed { changed = (0..self.face.widgets.len()).collect(); }
        ir_view::resolve_changed(&mut self.face, changed);
        self.patch = patch;
        self.page.0 = self.page.0.min(self.face.pages.len().saturating_sub(1));
        self.sync();
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

/// One resolver for saved v1 overrides and the global preference.
pub fn mode(cx: &Cx, slot: usize) -> crate::library::ViewMode {
    resolve_mode(&cx.selection.parts[slot], &cx.settings)
}

fn resolve_mode(part: &crate::plugin::Part, settings: &crate::library::Settings) -> crate::library::ViewMode {
    use crate::library::ViewMode;
    match part.view { 1 => ViewMode::Original, 2 => ViewMode::Kontra, 3 => ViewMode::Vectorized, _ => settings.instrument_views.get(&part.path).copied().unwrap_or(settings.view_mode) }
}

pub fn available(cx: &Cx, slot: usize) -> bool { main_face(&cx.view.parts[slot].interfaces).is_some() }

pub(super) fn scale_to_fit(room: Size, authored: Size, setting: f32) -> f64 {
    if setting.is_finite() && setting > 0. { return f64::from(setting); }
    if room.width <= 0. || authored.width <= 0. { return 1.; }
    let mut fit = room.width / authored.width;
    if room.height > 0. && authored.height > 0. { fit = fit.min(room.height / authored.height); }
    if fit >= 1. { fit.floor() } else { fit }
}

fn room_height(ui: &Ui, slot: usize) -> f64 {
    let Some(scene) = ui.scene() else { return 0. };
    let (Some(rack), Some(part), Some(stage)) = (scene.surface("rack-view"), scene.surface(&format!("part-{slot}")), scene.surface(&format!("stage-{slot}"))) else { return 0. };
    let tabs = scene.surface(&format!("face-bar-{slot}")).map_or(0., |s| s.frame.size.height);
    let perf = scene.surface(&format!("perf-{slot}")).map_or(0., |s| s.frame.size.height);
    (rack.frame.size.height - (stage.frame.y - part.frame.y).max(0.) - tabs - perf).max(0.)
}

/// `slot`'s library interface, when its scripts declare one.
fn interface(ui: &mut Ui, cx: &mut Cx, slot: usize, lead: Option<El>) -> Option<El> {
    let from = cx.view.parts.get(slot)?.interfaces.clone();
    let main = main_face(&from)?;
    let path = std::path::PathBuf::from(&cx.selection.parts[slot].path);
    let published = &cx.view.parts[slot];
    let generation = published.generation;
    let mode = mode(cx, slot);
    let presentation = if mode == crate::library::ViewMode::Original { Presentation::Bitmap } else { Presentation::Vector };
    let stale = cx.state.faces.get(&slot).is_none_or(|f| f.path != path || f.generation != generation);
    if stale {
        // ponytail: first decode is synchronous; W3 owns moving preparation to the asset worker.
        cx.state.faces.insert(slot, Face::new(&path, generation, from.clone(), main, presentation));
    }
    let face = cx.state.faces.get_mut(&slot)?;
    if !Arc::ptr_eq(&face.from, &from) || face.revision != published.ui_revision {
        if !from.get(face.shown).is_some_and(|f| !f.widgets.is_empty()) { face.shown = main; }
        let patch = published.updates.get(face.shown).cloned().unwrap_or_default();
        if !Arc::ptr_eq(&face.from, &from) {
            face.face = ir_view::resolved(&from[face.shown]);
            face.patch = Default::default();
            face.from = from.clone();
        }
        face.update(patch);
        face.revision = published.ui_revision;
    }
    if face.presentation != presentation { face.presentation = presentation; face.sync(); }

    // Which script's view, when several have one, and how it is drawn.
    let mut bar: Vec<El> = lead.into_iter().collect();
    let with: Vec<usize> = (0..from.len()).filter(|&n| !from[n].widgets.is_empty()).collect();
    let mut pick = None;
    if with.len() > 1 {
        let tabs: Vec<El> = with
            .iter()
            .map(|&n| {
                let label = from[n].pages.first().map(|p| p.name.clone()).filter(|s| !s.is_empty()).unwrap_or_else(|| match from[n].source {
                    ir::Source::Ksp { slot } => format!("Script {}", slot + 1),
                    _ => format!("View {}", n + 1),
                });
                let (hit, el) = latch(ui, format!("face-{slot}-{n}"), &label, "Show this script's view", face.shown == n);
                if hit {
                    pick = Some(n);
                }
                el
            })
            .collect();
        bar.push(segmented(tabs));
    }
    if face.face.pages.len() > 1 {
        let pages = face.face.pages.iter().enumerate().map(|(n, p)| {
            let label = if p.name.is_empty() { format!("Page {}", n + 1) } else { p.name.clone() };
            let (hit, el) = latch(ui, format!("face-page-{slot}-{n}"), &label, "Show this page", face.page.0 == n);
            if hit { face.page = ir::PageRef(n); }
            el
        }).collect();
        bar.push(segmented(pages));
    }
    bar.push(spacer());
    let original = mode == crate::library::ViewMode::Original;
    let (a, orig) = latch(ui, format!("face-original-{slot}"), "Original", "The library's own artwork", original);
    let (b, vect) = latch(ui, format!("face-vector-{slot}"), "Vector", "The library's background with KONTRA's controls; frees the control pictures", mode == crate::library::ViewMode::Vectorized);
    let (c, generated) = latch(ui, format!("face-kontra-{slot}"), "KONTRA", "Readable sections and channel strips", mode == crate::library::ViewMode::Kontra);
    bar.push(segmented(vec![orig, vect, generated]));
    if let Some(n) = pick.filter(|&n| n != face.shown) {
        let presentation = face.presentation;
        face.shown = n;
        face.page = ir::PageRef(0);
        face.face = ir_view::resolved(&from[n]);
        face.patch = Default::default();
        face.update(published.updates.get(n).cloned().unwrap_or_default());
        face.presentation = presentation;
        face.sync();
    }
    if a || b || c {
        cx.selection.parts[slot].view = if b { 3 } else if c { 2 } else { 1 };
        let chosen = if b { crate::library::ViewMode::Vectorized } else if c { crate::library::ViewMode::Kontra } else { crate::library::ViewMode::Original };
        cx.p.shared.libraries.edit(|settings| { settings.instrument_views.insert(path.to_string_lossy().into_owned(), chosen); });
        face.presentation = if a { Presentation::Bitmap } else { Presentation::Vector };
        face.sync();
    }

    let held = face.assets.bytes() as f64 / (1024. * 1024.);
    bar.insert(bar.len() - 1, caption(format!("{held:.1} MB pictures")).fill(secondary()).lines(1).tip("Decoded artwork this view keeps in memory"));
    let mode = if a { crate::library::ViewMode::Original } else if b { crate::library::ViewMode::Vectorized } else if c { crate::library::ViewMode::Kontra } else { mode };
    let page = &face.face.pages[face.page.0];
    let avail = ui.scene().and_then(|s| s.surface(&format!("face-{slot}"))).map_or(f64::from(page.size.width), |s| s.frame.size.width);
    let room = room_height(ui, slot);
    let scale = scale_to_fit(Size::new(avail, room), Size::new(f64::from(page.size.width), f64::from(ir_view::height(&face.face, face.page))), cx.settings.view_scale);
    // The core's values (scripts change them too); edits go back as widget edits.
    let shared = cx.p.shared.part(slot);
    let current: Vec<_> = shared.as_ref().map(|p| p.display_values()).unwrap_or_default();
    face.values.extend(current.iter().copied());
    let namespace = format!("part-{slot}-epoch-{generation}-script-{}-", face.shown);
    let view = if mode == crate::library::ViewMode::Kontra {
        super::generated::view(ui, &namespace, &face.face, face.page, &face.assets, scale, &mut face.values)
    } else {
        ir_view::view(ui, &namespace, &face.face, face.page, &face.assets, face.presentation, scale, &mut face.values)
    };
    for &(id, was) in &current {
        if let Some(&now) = face.values.get(&id)
            && now != was
        {
            cx.p.shared.set_control_at(slot, generation, id, now);
        }
    }
    Some(
        col![
            row(bar).id(format!("face-bar-{slot}")).gap(SPACE).align(Align::Center).pad((TIGHT, INSET)).w(Len::Pct(100.)).shrink(0),
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
    if v.report.as_ref().is_some_and(|r| r.missing.iter().any(|m| m.feature == "native interface")) {
        out.push(banner(Role::Warning, "The requested native performance view is unavailable. Showing the script controls."));
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
        return col(perf.into_iter().chain([face]).collect::<Vec<_>>()).gap(0).align(Align::Stretch).w(Len::Pct(100.)).shrink(0).id(format!("stage-{slot}"));
    }
    let tabs = tabs.map(|t| row![t, spacer()].align(Align::Center).pad((TIGHT, INSET)).w(Len::Pct(100.)).shrink(0));
    let body = inside::view(ui, cx, slot, view).unwrap_or_else(|| spacer().h(0));
    col(perf.into_iter().chain(tabs).chain([body]).collect::<Vec<_>>()).gap(0).align(Align::Stretch).w(Len::Pct(100.)).shrink(0).id(format!("stage-{slot}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_preserves_presentation_script_page_and_values() {
        let script = sampler_ksp::compile("on init make_perfview declare ui_knob $k(0,100,1) end on", 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        let mut authored = script.ui(&|_| None).unwrap();
        authored.pages.push(authored.pages[0].clone());
        let from: Arc<[ir::Interface]> = vec![authored.clone(), authored.clone()].into();
        let mut face = Face::new(std::path::Path::new("/missing/synthetic.nki"), 7, from.clone(), 1, Presentation::Bitmap);
        face.page = ir::PageRef(1);
        face.values.insert(ir::ControlId(9), 31.);
        let mut current = authored.clone();
        current.widgets[0].value_text = Some("Changed".into());
        current.widgets[0].rect.x += 10;
        let patch = ir::InterfacePatch::between(&authored, &current);
        face.update(patch.clone());
        assert_eq!((face.shown, face.page, face.presentation), (1, ir::PageRef(1), Presentation::Bitmap));
        assert!(Arc::ptr_eq(&face.from, &from));
        assert_eq!(face.values[&ir::ControlId(9)], 31.);
        assert_eq!(face.face.widgets[0].value_text.as_deref(), Some("Changed"));
        let first = face.face.clone();
        face.update(patch);
        assert_eq!(face.face, first);
        face.update(Default::default());
        assert_eq!(face.face, ir_view::resolved(&authored), "reverted source properties return to their authored values");
    }

    #[test]
    fn saved_override_wins_and_factory_default_is_original() {
        use crate::library::{Settings, ViewMode};
        let mut settings = Settings::default();
        let mut part = crate::plugin::Part { path: "synthetic.nki".into(), ..Default::default() };
        assert_eq!(resolve_mode(&part, &settings), ViewMode::Original);
        settings.view_mode = ViewMode::Vectorized;
        assert_eq!(resolve_mode(&part, &settings), ViewMode::Vectorized);
        settings.instrument_views.insert(part.path.clone(), ViewMode::Kontra);
        assert_eq!(resolve_mode(&part, &settings), ViewMode::Kontra);
        part.view = 1;
        assert_eq!(resolve_mode(&part, &settings), ViewMode::Original);
        let restored = serde_json::from_str::<crate::plugin::Part>(&serde_json::to_string(&part).unwrap()).unwrap();
        let settings = serde_json::from_str::<Settings>(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(resolve_mode(&restored, &settings), ViewMode::Original);
        part.view = 0;
        assert_eq!(resolve_mode(&part, &settings), ViewMode::Kontra);
    }

    #[test]
    fn fit_uses_both_axes_and_explicit_zoom_is_exact() {
        let page = Size::new(600., 800.);
        assert_eq!(scale_to_fit(Size::new(1200., 400.), page, 0.), 0.5);
        assert_eq!(scale_to_fit(Size::new(300., 1600.), page, 0.), 0.5);
        assert_eq!(scale_to_fit(Size::new(1200., 2000.), page, 0.), 2.);
        for zoom in [1., 1.5, 2.] { assert_eq!(scale_to_fit(Size::new(300., 400.), page, zoom), f64::from(zoom)); }
    }
}
