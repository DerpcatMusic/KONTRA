//! What the rack shows of a part below its header: why it is silent or
//! incomplete, and what its load decoded and could not translate.
//!
//! A part with a script interface shows it ([`super::ir_view`]).

use super::{Cx, ir_view, theme::*};
use moose::mui::mui::prelude::*;
use sampler_ui_ir::{Interface, PageRef, Presentation};
use std::sync::Arc;

/// A part's interface as drawn: resolved once per load, its pictures and
/// the values its controls show.
pub struct Face {
    source: Arc<[Interface]>,
    face: Interface,
    assets: ir_view::Assets,
    // TODO(v2): control edits reach the script once the core takes them.
    values: ir_view::Values,
}

/// `slot`'s script interface (the one with the most widgets), if it has one.
pub fn interface(ui: &mut Ui, cx: &mut Cx, slot: usize) -> Option<El> {
    let source = cx.view.parts.get(slot)?.interfaces.clone();
    let shown = source.iter().max_by_key(|u| u.widgets.len()).filter(|u| !u.widgets.is_empty())?;
    let path = std::path::PathBuf::from(&cx.selection.parts.get(slot)?.path);
    let face = cx.state.faces.entry(slot).or_insert_with(|| Face {
        source: Arc::from(Vec::new()),
        face: Interface::default(),
        assets: Default::default(),
        values: Default::default(),
    });
    if !Arc::ptr_eq(&face.source, &source) {
        face.face = ir_view::resolved(shown);
        face.assets = Default::default();
        face.values.clear();
        face.source = source.clone();
    }
    face.assets.sync(&face.face, Presentation::Bitmap, |a| crate::artwork::asset(&path, a));
    Some(ir_view::view(ui, &face.face, PageRef(0), &face.assets, Presentation::Bitmap, 1., &mut face.values))
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

/// The part's load report: what was decoded, and what was not translated.
pub fn stage(cx: &Cx, slot: usize) -> El {
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
