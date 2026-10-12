//! Exact selected raw-source preview; never instrument audition or note dispatch.
use super::theme::*;
use crate::{import::Instrument, plugin::{SamplerParams, preview::Status, sample_preview::Ticket}};
use moose::mui::mui::prelude::*;
use std::sync::{Arc, Weak, atomic::{AtomicU64, Ordering}};

pub(super) const PAGE: usize = 24;
// Control request IDs are below u64::MAX >> 3. These are window lifecycle
// tombstones, never requests or a second monotonic sequence.
const CLOSED: u64 = u64::MAX;
const CANCELED: u64 = u64::MAX - 1;

/// One window's accepted service receipt, shared with close/cancel callbacks.
/// This never allocates request IDs or authorizes a source.
pub(super) struct State {
    receipt: Arc<AtomicU64>,
    target: Option<(usize, Ticket)>,
    rejected: bool,
    stopped: bool,
}

impl State {
    pub fn new(receipt: Arc<AtomicU64>) -> Self {
        Self { receipt, target: None, rejected: false, stopped: false }
    }

    pub fn clear(&mut self, p: &SamplerParams) {
        stop_receipt(p, &self.receipt);
        self.target = None;
        self.rejected = false;
        self.stopped = false;
    }

    pub fn finish_frame(&mut self, p: &SamplerParams, mapping_slot: Option<usize>) {
        if self.target.as_ref().is_some_and(|(slot, _)| Some(*slot) != mapping_slot) {
            self.clear(p);
        }
    }
}

pub(super) fn stop_receipt(p: &SamplerParams, receipt: &AtomicU64) {
    if let Ok(id) = receipt.fetch_update(Ordering::AcqRel, Ordering::Acquire,
        |id| (id != CLOSED && id != CANCELED).then_some(0)) {
        if id != 0 && id != CANCELED { p.stop_sample_preview(id); }
    }
}

pub(super) fn close_receipt(p: &SamplerParams, receipt: &AtomicU64) {
    let id = receipt.swap(CLOSED, Ordering::AcqRel);
    if id != 0 && id != CLOSED && id != CANCELED { p.stop_sample_preview(id); }
}

pub(super) fn cancel_receipt(p: &SamplerParams, receipt: &AtomicU64) {
    if let Ok(id) = receipt.fetch_update(Ordering::AcqRel, Ordering::Acquire,
        |id| (id != CLOSED).then_some(CANCELED)) {
        if id != 0 && id != CANCELED { p.stop_sample_preview(id); }
    }
}

fn accept_receipt(p: &SamplerParams, receipt: &AtomicU64, expected: u64, accepted: u64) -> bool {
    if expected != CLOSED && expected != CANCELED
        && receipt.compare_exchange(expected, accepted, Ordering::AcqRel, Ordering::Acquire).is_ok() {
        true
    } else {
        // Close/cancel raced synchronous admission. Retire only that accepted
        // service request; it cannot become orphaned or stop a newer request.
        p.stop_sample_preview(accepted);
        false
    }
}

pub(super) fn view(ui: &mut Ui, p: &SamplerParams, state: &mut State,
    slot: usize, ticket: Option<Ticket>) -> El {
    // Only a subsequent GUI build resumes a canceled window. A closed window
    // stays closed, including if a late build arrives after native teardown.
    state.receipt.compare_exchange(CANCELED, 0, Ordering::AcqRel, Ordering::Acquire).ok();
    let closed = state.receipt.load(Ordering::Acquire) == CLOSED;
    let same = state.target.as_ref().zip(ticket.as_ref())
        .is_some_and(|((old_slot, old), new)| *old_slot == slot && old.same_target(new));
    if !same {
        state.clear(p);
        state.target = ticket.map(|ticket| (slot, ticket));
    }
    let control = p.selection.preview_control();
    let mut id = state.receipt.load(Ordering::Acquire);
    if id == CLOSED || id == CANCELED { id = 0; }
    if id != 0 && control.status(id).is_none() {
        // Superseded/reset service ownership is never a new GUI request receipt.
        state.receipt.compare_exchange(id, 0, Ordering::AcqRel, Ordering::Acquire).ok();
        id = 0;
    }
    let phase = (id != 0).then(|| control.status(id)).flatten();
    let busy = matches!(phase, Some(Status::Preparing | Status::Ready | Status::Playing));
    let can_play = state.target.is_some() && !busy && !closed;
    let (play, play_el) = action(ui, "sample-preview-play", "Play dry sample", false);
    let (stop, stop_el) = action(ui, "sample-preview-stop", "Stop", false);
    if stop && busy {
        // Successful conditional cancellation is the exact service Stop ack.
        // It tombstones the request, so status(id) then becomes None.
        state.stopped = p.stop_sample_preview(id);
    }
    if play && can_play && !matches!(state.receipt.load(Ordering::Acquire), CLOSED | CANCELED) {
        let expected = state.receipt.load(Ordering::Acquire);
        let (slot, ticket) = state.target.as_ref().unwrap();
        if let Some(accepted) = p.request_sample_preview(*slot, ticket.clone()) {
            if accept_receipt(p, &state.receipt, expected, accepted) {
                id = accepted;
                state.rejected = false;
                state.stopped = false;
            } else {
                id = 0;
            }
        } else {
            state.rejected = true;
        }
    }
    // Read again after the action; Ready/Playing is exclusively service feedback.
    let phase = (id != 0).then(|| control.status(id)).flatten();
    let feedback = (id != 0).then(|| p.sample_preview_feedback(id)).flatten()
        .filter(|feedback| feedback.id == id);
    let status = if state.rejected { "Preview request was not accepted for this selection." }
        else { match phase {
            Some(Status::Preparing) => "Loading selected sample…",
            Some(Status::Ready) => "Sample ready; waiting for audio output.",
            Some(Status::Playing) => "Playing selected sample.",
            Some(Status::Stopped) => "Stopped.",
            Some(Status::Failed) => "Sample preview failed.",
            Some(Status::Unsupported) => "Sample preview is unsupported.",
            None if state.stopped => "Stopped.",
            None if state.target.is_some() => "Select Play dry sample to hear this source.",
            None => "Select a sample zone to preview its source.",
        }};
    let failed = state.rejected || matches!(phase, Some(Status::Failed | Status::Unsupported));
    let mut details = vec![caption(status).named(status).lines(3)
        .fill(if failed { Fill::from(Role::Danger) } else { secondary() }).id("sample-preview-status")];
    if let Some(feedback) = feedback {
        let rate = feedback.source_rate.map_or_else(|| "rate unknown".into(), |n| format!("{n} Hz"));
        let frames = feedback.source_frames.map_or_else(|| "frames unknown".into(), |n| format!("{n} frames"));
        let channels = feedback.source_channels.map_or_else(|| "channels unknown".into(), |n| format!("{n} channels"));
        let mut source = format!("Source · {rate} · {channels} · {frames}");
        if let Some((rate, frames)) = feedback.source_rate.zip(feedback.source_frames).filter(|(rate, _)| *rate > 0) {
            source.push_str(&format!(" · {:.3} s", frames as f64 / f64::from(rate)));
        }
        details.push(caption(source.clone()).named(source).lines(3).id("sample-preview-source"));
        if let Some(frames) = feedback.output_frames {
            details.push(caption(format!("Preview output · {} Hz · {frames} frames", feedback.host_rate)).lines(2));
        }
        if let Some(reason) = &feedback.reason {
            details.push(caption(reason.clone()).named(reason.clone()).fill(Role::Danger).lines(5).id("sample-preview-reason"));
        }
    }
    let busy = matches!(phase, Some(Status::Preparing | Status::Ready | Status::Playing));
    col![row![play_el.when(state.target.is_none() || busy
                || matches!(state.receipt.load(Ordering::Acquire), CLOSED | CANCELED), |el| el.disabled()),
            stop_el.when(!busy, |el| el.disabled()), col(details).gap(TIGHT).flex(1).min_w(SIDEBAR_MIN)]
            .wrap().gap(SPACE).line_gap(TIGHT).align(Align::Start),
        caption("Dry source preview, once. Instrument scripts, zone tuning, loops and effects are not applied.")
            .fill(secondary()).lines(3)]
        .gap(TIGHT).pad(INSET).min_w(0).shrink(0).id("sample-preview")
}

pub(super) struct SampleRow {
    pub index: usize,
    pub title: String,
    pub range: String,
}

pub(super) fn rows(ui: &mut Ui, prefix: &str, rows: Vec<SampleRow>, selected: &mut Option<usize>, empty: &str) -> El {
    // Resolve input for the page before materializing any selected styling.
    // A later row activation must not leave the former row selected this frame.
    for row in &rows {
        if ui.get(format!("{prefix}-{}", row.index)).activated() { *selected = Some(row.index); }
    }
    let mut elements = Vec::new();
    for row in rows {
        let active = *selected == Some(row.index);
        let label = format!("{}\n{}", row.title, row.range);
        elements.push(interactive(col![body(row.title).lines(3).min_w(0), caption(row.range).lines(2).min_w(0)]
            .gap(TIGHT).pad(SPACE).align(Align::Start).min_h(CONTROL)
            .when(active, |el| el.fill(Role::Raised)).focusable().a11y(A11y::Button).named(label)
            .id(format!("{prefix}-{}", row.index)), active).w(Len::Pct(100.)).min_w(0).shrink(0));
    }
    if elements.is_empty() { elements.push(caption(empty.to_owned()).lines(4)); }
    col(elements).gap(1).align(Align::Stretch).flex(1).min_h(0).scroll()
}

pub(super) fn pager(ui: &mut Ui, id: &str, count: usize, page: &mut usize) -> El {
    let pages = count.max(1);
    *page = (*page).min(pages - 1);
    let (previous, previous_el) = action(ui, format!("{id}-previous"), "Previous", false);
    let (next, next_el) = action(ui, format!("{id}-next"), "Next", false);
    let can_previous = *page > 0;
    let can_next = *page + 1 < pages;
    if previous && can_previous { *page -= 1; }
    if next && can_next { *page += 1; }
    row![previous_el.when(!can_previous, |el| el.disabled()), spacer(), caption(format!("{} / {pages}", *page + 1)),
        spacer(), next_el.when(!can_next, |el| el.disabled())].gap(TIGHT).align(Align::Center)
        .w(Len::Pct(100.)).min_w(0).shrink(0)
}

#[derive(Default)]
pub(super) struct KontaktSelection {
    owner: Weak<Instrument>,
    group: Option<usize>,
    pub page: usize,
    pub selected: Option<usize>,
    pub indices: Arc<Vec<usize>>,
}

impl KontaktSelection {
    pub fn bind(&mut self, instrument: &Arc<Instrument>, group: usize) {
        if self.group != Some(group) || !self.owner.upgrade().is_some_and(|old| Arc::ptr_eq(&old, instrument)) {
            *self = Self { owner: Arc::downgrade(instrument), group: Some(group),
                indices: Arc::new(instrument.zones.iter().enumerate().filter_map(|(index, zone)|
                    (zone.group == group).then_some(index)).collect()), ..Default::default() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket() -> Ticket {
        Ticket::kontakt(Arc::new(Instrument {
            path: "/authored/preview.nki".into(),
            zones: vec![crate::import::Zone { sample: "owned.wav".into(), ..Default::default() }],
            ..Default::default()
        }), 0).unwrap()
    }

    fn label(ui: &Ui, id: &str) -> String {
        ui.scene().unwrap().surface(id).unwrap().semantics.as_ref()
            .and_then(|semantics| semantics.label.as_deref()).unwrap_or_default().to_owned()
    }

    #[test]
    fn a_window_stop_receipt_cannot_cancel_another_window_or_newer_start() {
        let p = SamplerParams::new();
        let control = p.selection.preview_control();
        let old = control.begin(0).unwrap();
        let receipt = AtomicU64::new(old);
        let newer = control.begin(1).unwrap();
        stop_receipt(&p, &receipt);
        assert_eq!(receipt.load(Ordering::Acquire), 0);
        assert!(control.current(newer));
        assert_eq!(control.status(newer), Some(Status::Preparing));
        stop_receipt(&p, &receipt);
        assert!(control.current(newer), "close/cancel repeated at ID zero is inert");
    }

    #[test]
    fn close_and_cancel_during_admission_cannot_orphan_a_late_accepted_receipt() {
        let p = SamplerParams::new();
        let receipt = AtomicU64::new(0);
        let expected = receipt.load(Ordering::Acquire);
        let accepted = p.selection.preview_control().begin(0).unwrap();
        close_receipt(&p, &receipt);
        assert!(!accept_receipt(&p, &receipt, expected, accepted));
        assert!(!p.selection.preview_control().current(accepted));
        assert_eq!(receipt.load(Ordering::Acquire), CLOSED);
        stop_receipt(&p, &receipt);
        cancel_receipt(&p, &receipt);
        assert_eq!(receipt.load(Ordering::Acquire), CLOSED, "late clear/cancel cannot reopen a closed lease");
        let late = p.selection.preview_control().begin(0).unwrap();
        assert!(!accept_receipt(&p, &receipt, CLOSED, late));
        assert!(!p.selection.preview_control().current(late));
        assert_eq!(receipt.load(Ordering::Acquire), CLOSED);

        let receipt = AtomicU64::new(0);
        let accepted = p.selection.preview_control().begin(0).unwrap();
        cancel_receipt(&p, &receipt);
        assert!(!accept_receipt(&p, &receipt, 0, accepted));
        assert!(!p.selection.preview_control().current(accepted));
        assert_eq!(receipt.load(Ordering::Acquire), CANCELED);
        let newer = p.selection.preview_control().begin(1).unwrap();
        close_receipt(&p, &receipt);
        assert!(p.selection.preview_control().current(newer), "another window's receipt is never canceled");
    }

    #[test]
    fn target_clear_preserves_cancel_until_the_next_explicit_gui_build() {
        let p = SamplerParams::new();
        let receipt = Arc::new(AtomicU64::new(0));
        let mut state = State::new(receipt.clone());
        let target = ticket();
        state.target = Some((0, target.clone()));
        let id = p.selection.preview_control().begin(0).unwrap();
        receipt.store(id, Ordering::Release);
        // Native cancellation between view's explicit resume and a target
        // replacement must survive that same build's generic clear.
        cancel_receipt(&p, &receipt);
        state.clear(&p);
        assert_eq!(receipt.load(Ordering::Acquire), CANCELED);
        stop_receipt(&p, &receipt);
        assert_eq!(receipt.load(Ordering::Acquire), CANCELED);
        let late = p.selection.preview_control().begin(0).unwrap();
        assert!(!accept_receipt(&p, &receipt, CANCELED, late));
        assert!(!p.selection.preview_control().current(late));
        assert_eq!(receipt.load(Ordering::Acquire), CANCELED);

        let mut ui = super::super::theme::ui();
        let el = view(&mut ui, &p, &mut state, 0, Some(target));
        ui.frame(el, Some(Size::new(480., 220.)), Input::default(), 1.).unwrap();
        assert_eq!(receipt.load(Ordering::Acquire), 0,
            "only the subsequent explicit GUI build resumes cancellation");
        assert!(!ui.scene().unwrap().surface("sample-preview-play").unwrap().disabled);
    }

    #[test]
    fn mapping_tab_and_slot_lifecycle_retire_only_the_window_receipt() {
        let p = SamplerParams::new();
        let receipt = Arc::new(AtomicU64::new(0));
        let mut state = State::new(receipt.clone());
        state.target = Some((0, ticket()));
        let id = p.selection.preview_control().begin(0).unwrap();
        receipt.store(id, Ordering::Release);
        state.finish_frame(&p, Some(0));
        assert!(p.selection.preview_control().current(id));
        state.finish_frame(&p, Some(1));
        assert!(!p.selection.preview_control().current(id));
        assert_eq!(receipt.load(Ordering::Acquire), 0);
        state.target = Some((1, ticket()));
        let id = p.selection.preview_control().begin(1).unwrap();
        receipt.store(id, Ordering::Release);
        state.finish_frame(&p, None);
        assert!(!p.selection.preview_control().current(id), "leaving Mapping stops the matching receipt");
    }

    #[test]
    fn gui_feedback_follows_monotonic_service_phase_and_exact_receipts() {
        // Constructed Control phases test GUI consumption only, not source
        // admission, decode, audible PCM or native/bank playback.
        let p = SamplerParams::new();
        let receipt = Arc::new(AtomicU64::new(0));
        let mut state = State::new(receipt.clone());
        let target = ticket();
        state.target = Some((0, target.clone()));
        let id = p.selection.preview_control().begin(0).unwrap();
        receipt.store(id, Ordering::Release);
        let mut ui = super::super::theme::ui();
        let draw = |ui: &mut Ui, state: &mut State, input| {
            let el = view(ui, &p, state, 0, Some(target.clone()));
            ui.frame(el, Some(Size::new(480., 220.)), input, 1.).unwrap();
        };
        draw(&mut ui, &mut state, Input::default());
        assert!(label(&ui, "sample-preview-status").starts_with("Loading"));
        assert!(ui.scene().unwrap().surface("sample-preview-play").unwrap().disabled);
        assert!(ui.scene().unwrap().surface("sample-preview-source").is_none(), "no fabricated decoded dimensions");
        p.selection.preview_control().set_status(id, Status::Ready);
        draw(&mut ui, &mut state, Input::default());
        assert!(label(&ui, "sample-preview-status").contains("waiting for audio output"));
        p.selection.preview_control().set_status(id, Status::Playing);
        p.selection.preview_control().set_status(id, Status::Ready);
        draw(&mut ui, &mut state, Input::default());
        assert!(label(&ui, "sample-preview-status").starts_with("Playing"), "late Ready must not regress output state");
        ui.focus("sample-preview-stop");
        draw(&mut ui, &mut state, Input { keys: vec![KeyPress { key: Key::Enter, mods: Mods::default() }], ..Default::default() });
        draw(&mut ui, &mut state, Input::default());
        assert_eq!(label(&ui, "sample-preview-status"), "Stopped.");
        assert!(!p.selection.preview_control().current(id));
        assert!(ui.scene().unwrap().surface("sample-preview-stop").unwrap().disabled);
        let newer = p.selection.preview_control().begin(1).unwrap();
        state.clear(&p);
        assert!(p.selection.preview_control().current(newer));
    }

    #[test]
    fn failed_and_unsupported_phases_remain_truthful_without_producer_reason() {
        let p = SamplerParams::new();
        for (phase, expected) in [(Status::Failed, "Sample preview failed."),
            (Status::Unsupported, "Sample preview is unsupported.")] {
            let target = ticket();
            let receipt = Arc::new(AtomicU64::new(0));
            let mut state = State::new(receipt.clone());
            state.target = Some((0, target.clone()));
            let id = p.selection.preview_control().begin(0).unwrap();
            receipt.store(id, Ordering::Release);
            p.selection.preview_control().set_status(id, phase);
            let mut ui = super::super::theme::ui();
            let el = view(&mut ui, &p, &mut state, 0, Some(target));
            ui.frame(el, Some(Size::new(480., 220.)), Input::default(), 1.).unwrap();
            assert_eq!(label(&ui, "sample-preview-status"), expected);
            assert!(ui.scene().unwrap().surface("sample-preview-source").is_none());
            assert!(ui.scene().unwrap().surface("sample-preview-reason").is_none());
        }
    }

    #[test]
    fn exact_kontakt_selection_keeps_duplicate_paths_and_resets_foreign_owner() {
        let instrument = Arc::new(Instrument { zones: (0..26).map(|index| crate::import::Zone {
            group: if index == 25 { 1 } else { 0 }, sample: "same.wav".into(), ..Default::default()
        }).collect(), ..Default::default() });
        let mut selection = KontaktSelection::default();
        selection.bind(&instrument, 0);
        assert_eq!(selection.indices.as_slice(), &(0..25).collect::<Vec<_>>());
        selection.page = 1;
        selection.selected = Some(24);
        selection.bind(&instrument, 0);
        assert_eq!((selection.page, selection.selected), (1, Some(24)));
        let first = Ticket::kontakt(instrument.clone(), 0).unwrap();
        let last = Ticket::kontakt(instrument.clone(), 24).unwrap();
        assert!(!first.same_target(&last), "duplicate paths are distinct selected zones");
        assert!(last.same_target(&last.clone()));
        let foreign = Arc::new(Instrument { zones: instrument.zones.clone(), ..Default::default() });
        selection.bind(&foreign, 0);
        assert_eq!((selection.page, selection.selected), (0, None));
        selection.bind(&foreign, 1);
        assert_eq!(selection.indices.as_slice(), &[25]);
    }

    #[test]
    fn pointer_and_keyboard_select_before_every_row_style_on_the_same_build() {
        use moose::mui::mui::scene::Layer;
        let mut ui = super::super::theme::ui();
        let mut selected = Some(0);
        let draw = |ui: &mut Ui, selected: &mut Option<usize>, input| {
            let row_items = [0, 24, 48].into_iter().map(|index| SampleRow {
                index, title: format!("Zone {index} · same.wav"), range: "C3–C4 · velocity 0–127".into()
            }).collect();
            let el = rows(ui, "fixture-zone", row_items, selected, "No samples.");
            ui.frame(el, Some(Size::new(420., 400.)), input, 1.).unwrap();
        };
        let assert_one = |ui: &Ui, selected: usize| {
            let fills: Vec<_> = ui.scene().unwrap().paint.iter().filter(|paint|
                matches!(paint.layer, Layer::Fill) && paint.paint.solid().alpha() > 0.99 && [0, 24, 48].iter().any(|index|
                    paint.key == Id::from(format!("fixture-zone-{index}")))).collect();
            assert_eq!(fills.len(), 1, "selection must not create two full row fills in one build");
            assert_eq!(fills[0].key, Id::from(format!("fixture-zone-{selected}")));
        };
        for _ in 0..3 { draw(&mut ui, &mut selected, Input::default()); }
        ui.focus("fixture-zone-48");
        draw(&mut ui, &mut selected, Input { keys: vec![KeyPress { key: Key::Enter, mods: Mods::default() }], ..Default::default() });
        draw(&mut ui, &mut selected, Input::default());
        assert_eq!(selected, Some(48));
        assert_one(&ui, 48);
        let frame = ui.scene().unwrap().surface("fixture-zone-24").unwrap().frame;
        let point = Point::new(frame.x + frame.size.width / 2., frame.y + frame.size.height / 2.);
        draw(&mut ui, &mut selected, Input { pointer: PointerInput { pos: Some(point), buttons: Buttons::PRIMARY, ..Default::default() }, ..Default::default() });
        draw(&mut ui, &mut selected, Input { pointer: PointerInput { pos: Some(point), ..Default::default() }, ..Default::default() });
        draw(&mut ui, &mut selected, Input::default());
        assert_eq!(selected, Some(24));
        assert_one(&ui, 24);
    }
}
