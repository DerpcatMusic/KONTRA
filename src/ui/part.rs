//! What the rack shows of a part below its header: why it is silent or
//! incomplete, then the library's own interface (drawn from its UI IR, as
//! authored or with our controls) or, without one, what its load decoded and
//! could not translate.

use super::{Cx, ir_view, pictures, theme::*};
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
fn interface(ui: &mut Ui, cx: &mut Cx, slot: usize) -> Option<El> {
    let from = cx.view.parts.get(slot)?.interfaces.clone();
    let main = main_face(&from)?;
    let path = std::path::PathBuf::from(&cx.selection.parts[slot].path);
    let stale = cx.state.faces.get(&slot).is_none_or(|f| !Arc::ptr_eq(&f.from, &from));
    if stale {
        // ponytail: reads and decodes on the UI thread, once per load; move to the
        // loader's worker when big libraries make the first frame stall.
        cx.state.faces.insert(slot, Face::new(&path, from.clone(), main, Presentation::Bitmap));
    }
    let face = cx.state.faces.get_mut(&slot)?;

    // Which script's view, when several have one, and how it is drawn.
    let mut bar = Vec::new();
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
    let view = ir_view::view(ui, &face.face, ir::PageRef(0), &face.assets, face.presentation, scale, &mut face.values);
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

/// Lines of the missing list a part shows before "+N more".
const MISSING: usize = 6;

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

/// The part's interface, else its load report: what was decoded, and what
/// was not translated.
pub fn stage(ui: &mut Ui, cx: &mut Cx, slot: usize) -> El {
    if let Some(face) = interface(ui, cx, slot) {
        return face;
    }
    let v = &cx.view.parts[slot];
    let mut lines = Vec::new();
    if let Some(r) = &v.report {
        let d = &r.decoded;
        lines.push(
            caption(format!(
                "{} · {} zones · {} groups · {} samples · {} scripts",
                d.format, d.zones, d.groups, d.samples, d.scripts
            ))
            .fill(secondary())
            .lines(1)
            .min_w(0),
        );
        for m in r.missing.iter().take(MISSING) {
            let text = format!("Not translated: {} {} at {}", m.feature, m.value, m.location);
            lines.push(caption(text.clone()).fill(secondary()).lines(1).min_w(0).tip(text));
        }
        if r.missing.len() > MISSING {
            lines.push(caption(format!("+{} more not translated", r.missing.len() - MISSING)).fill(secondary()).lines(1));
        }
    }
    col(lines).gap(TIGHT).align(Align::Start).pad((SPACE, INSET)).w(Len::Pct(100.)).shrink(0).id(format!("stage-{slot}"))
}
