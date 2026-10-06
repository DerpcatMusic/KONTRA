//! What the rack shows of a part below its header: why it is silent or
//! incomplete, and what its load decoded and could not translate.
//!
//! TODO(v2 UI): the part's script interface (`sampler-ui-ir`) goes here once
//! the v2 UI draws it; until then a part shows its load report.

use super::{Cx, theme::*};
use moose::mui::mui::prelude::*;

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
