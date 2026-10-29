//! The editor: [`kontra_ui::app::view`] in a moose window. It draws from a
//! [`Snapshot`] of the rack that [`crate::Tick`] refreshes, so a frame never
//! waits on a load, and it hands what the user does back as [`Edit`]s for
//! the tick to carry out.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use kontra_host::rack::{Mix, Rack};
use kontra_protocol::Stats;
use kontra_ui::app::{Action, App, Part};
use moose::mui::MuiEditor;
use moose::prelude::*;

use crate::{KontraParams, P, Request, lock};

/// Report lines kept; older ones scroll away.
const REPORT: usize = 200;

/// One rack part as the editor shows it.
#[derive(Clone, Debug, PartialEq)]
struct PartView {
    slot: usize,
    name: String,
    mix: Mix,
    solo: bool,
    mute: bool,
    level: f32,
}

/// The rack as the editor last saw it.
#[derive(Debug, Default)]
pub struct Snapshot {
    parts: Vec<PartView>,
    level: f32,
    stats: Stats,
    report: Vec<String>,
}

impl Snapshot {
    /// Copies what the editor shows from `rack`, with this tick's `report`.
    pub fn refresh(&mut self, rack: &Rack, report: Vec<String>) {
        let meters = rack.meters();
        let level = |slot: usize| {
            let m = meters.and_then(|m| m.parts.get(slot).copied());
            m.map_or(0.0, |[l, r]| l.max(r))
        };
        self.parts = (rack.parts().iter().enumerate())
            .filter_map(|(slot, p)| {
                let p = p.as_ref()?;
                Some(PartView {
                    slot,
                    name: p.name.clone(),
                    mix: p.mix,
                    solo: p.solo,
                    mute: p.mute,
                    level: level(slot),
                })
            })
            .collect();
        if let Some(m) = meters {
            self.level = m.master[0].max(m.master[1]);
        }
        if let Some(s) = rack.stats() {
            self.stats = s;
        }
        self.report.extend(report);
        let over = self.report.len().saturating_sub(REPORT);
        self.report.drain(..over);
    }

    /// Fills `app` for a frame, keeping each part's own view state.
    fn fill(&self, app: &mut App) {
        let parts = self.parts.iter().enumerate().map(|(n, p)| {
            let (open, tab) = app
                .parts
                .get(n)
                .map_or((true, Default::default()), |old| (old.open, old.tab));
            Part {
                name: p.name.clone(),
                channel: p.mix.channel,
                output: p.mix.output + 1,
                volume: f64::from(linear_to_db(p.mix.gain)),
                pan: f64::from(p.mix.pan),
                level: f64::from(p.level),
                solo: p.solo,
                mute: p.mute,
                keys: (0, 127),
                open,
                tab,
                ..Part::default()
            }
        });
        app.parts = parts.collect();
        app.level = f64::from(self.level);
        app.stats.cpu = f64::from(self.stats.load) * 100.0;
        app.stats.voices = self.stats.voices;
        app.report.clone_from(&self.report);
    }
}

/// A user's edit to one rack part, or to the whole rack.
pub struct Edit {
    slot: Option<usize>,
    action: Action,
}

/// Carries out `edit` on the rack.
pub fn apply(rack: &mut Rack, edit: Edit) {
    let part = edit
        .slot
        .and_then(|s| Some((s, rack.parts().get(s)?.as_ref()?)));
    let mix = |(slot, p): (usize, &kontra_host::rack::Part)| (slot, p.mix, p.solo, p.mute);
    let Some((slot, mut m, solo, mute)) = part.map(mix) else {
        if edit.action == Action::Panic {
            rack.panic();
        }
        return;
    };
    match edit.action {
        Action::Volume(_, db) => m.gain = db_to_linear(db as f32),
        Action::Pan(_, pan) => m.pan = pan as f32,
        Action::Channel(_, c) => m.channel = c.min(16),
        // Output 0 lets the host pick, which is the first pair.
        Action::Output(_, o) => m.output = o.saturating_sub(1),
        Action::Solo(_, on) => return rack.set_solo_mute(slot, on, mute),
        Action::Mute(_, on) => return rack.set_solo_mute(slot, solo, on),
        Action::Remove(_) => return rack.remove(slot),
        _ => return,
    }
    rack.set_mix(slot, m);
}

/// The rack part an action names, by its place in the editor's list.
fn part_of(action: &Action) -> Option<usize> {
    match *action {
        Action::Volume(n, _)
        | Action::Pan(n, _)
        | Action::Channel(n, _)
        | Action::Output(n, _)
        | Action::Solo(n, _)
        | Action::Mute(n, _)
        | Action::Remove(n) => Some(n),
        _ => None,
    }
}

pub fn editor(params: Arc<KontraParams>) -> Box<dyn Editor> {
    let p = Arc::clone(&params);
    let watched = Arc::clone(&params);
    let mut app = App::default();
    MuiEditor::new(params, kontra_ui::ui(), (1180, 780), move |ui, bridge| {
        let slots: Vec<usize> = {
            let view = lock(&p.shared.view);
            view.fill(&mut app);
            view.parts.iter().map(|v| v.slot).collect()
        };
        app.master = f64::from(p.volume.value());
        let response = kontra_ui::app::view(ui, &mut app);
        for action in response.changed {
            if let Action::Master(db) = action {
                bridge.drag_gesture(P::Volume, db);
                continue;
            }
            let slot = part_of(&action).and_then(|n| slots.get(n).copied());
            p.shared.request(Request::Ui(Edit { slot, action }));
        }
        response.el
    })
    .resizable((900, 600))
    .changed(move || watched.shared.dirty.swap(false, Ordering::AcqRel))
    .into_editor()
}
