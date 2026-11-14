//! Script calls answered from the engine's clock, keys and live events,
//! and the host-only calls KONTRA has no surface for.

use kontra_vm::{Action, HostCall, HostValue, Text};

use crate::Engine;
use crate::queue::Exit;
use crate::script::Kind;

impl Engine {
    /// Answers `call` when it is one of these; `None` for the rest.
    pub(crate) fn runtime_call(&mut self, call: &mut HostCall<'_>) -> Option<HostValue> {
        let int = |i: usize| call.ints.get(i).map_or(0, |&v| v as i32);
        let flag = |b: bool| HostValue::Int(i32::from(b));
        Some(match call.action {
            Action::PolyAt => HostValue::Int(
                usize::try_from(int(0))
                    .ok()
                    .and_then(|k| self.poly_at.get(k))
                    .map_or(0, |&v| i32::from(v)),
            ),
            Action::NcNum => HostValue::Int(match self.running.kind {
                Kind::NoteController => i32::from(self.running.controller),
                _ => 0,
            }),
            Action::NcNote => HostValue::Int(i32::from(self.running.midi[2])),
            Action::NcValue => HostValue::Int(self.running.value),
            Action::MidiCommand => HostValue::Int(i32::from(self.running.midi[0])),
            Action::MidiByte1 => HostValue::Int(i32::from(self.running.midi[2])),
            Action::MidiByte2 => HostValue::Int(i32::from(self.running.midi[3])),
            // Kept across a wait (the resumed callback is still poly_at's).
            Action::PolyAtNum => {
                let key = match self.running.kind {
                    Kind::PolyAt => self.running.controller,
                    _ => 0,
                };
                HostValue::Int(i32::from(key))
            }
            // Microseconds since `reset_ksp_timer`, on the engine's clock: a
            // callback takes no time on it.
            Action::KspTimer => {
                let frames = self.now.0.saturating_sub(self.timer_at.0);
                HostValue::Int((frames * 1_000_000 / u64::from(self.rate)) as i32)
            }
            Action::ResetKspTimer => {
                self.timer_at = self.now;
                HostValue::Nothing
            }
            // ponytail: with no editor, the first group is the one selected
            // (unmeasured); only developer tools in the corpus read it.
            Action::GroupsSelected => flag(int(0) == 0),
            // Every live event id, then a 0 when they do not fill the array.
            Action::GetEventIds => {
                let (base, len) = (usize::try_from(int(0)), usize::try_from(int(1)));
                let (base, len) = (base.unwrap_or(usize::MAX), len.unwrap_or(0));
                let cells = call.cells.get_mut(base..).unwrap_or_default();
                let ids = self.notes.live().map(|(r, _)| i64::from(r.event_id()));
                for (cell, id) in cells.iter_mut().take(len).zip(ids.chain([0])) {
                    *cell = id;
                }
                HostValue::Nothing
            }
            // What only a host, NI hardware or Kontakt's own editor shows:
            // NKS navigation, exposed controls, a native UI, waveform looks, a
            // file browser's place, automation names and the release-trigger
            // system script's never-ending events, the instrument header's icon
            // and a zone's machine mode. KONTRA has none of them.
            Action::SetNksNavName
            | Action::SetNksNavPar
            | Action::ResetNksNav
            | Action::ExposeControls
            | Action::LoadNativeUi
            | Action::SetUiWfProperty
            | Action::AttachZone
            | Action::FsNavigate
            | Action::SetControlParStrArr
            | Action::WillNeverTerminate
            | Action::HideInstIcon
            | Action::SetMapEditorEventColor
            | Action::LoadKompleteUi
            | Action::DontUseMachineMode => HostValue::Nothing,
            // The host reads the script's performance view at load and
            // declares its controls (kontra-host ui.rs); a request for
            // that view is done. Any other name has no view to show.
            Action::LoadPerformanceView => {
                let asked = call.texts.first().map_or("", Text::as_str);
                let prepared = self.script().and_then(|s| s.view.as_deref());
                if !prepared.is_some_and(|v| v.eq_ignore_ascii_case(asked)) {
                    self.count_unhandled(Action::LoadPerformanceView);
                }
                HostValue::Nothing
            }
            // A zone by its name, exactly as spelled; -1 when none has it
            // (Kontakt 8.13.1, Morphology: "warmth" finds nothing).
            Action::FindZone => {
                let name = call.texts.first().map_or("", Text::as_str);
                let found = self.plan.zone_facts.iter().find(|z| &*z.name == name);
                HostValue::Int(found.map_or(-1, |z| i32::try_from(z.id).unwrap_or(-1)))
            }
            // A zone that is not there reads -1 for every parameter (measured).
            Action::GetZonePar => {
                let zone =
                    (self.plan.zone_facts.iter()).find(|z| i64::from(z.id) == i64::from(int(0)));
                match (zone, self.engine_par(int(1))) {
                    (None, _) => HostValue::Int(-1),
                    (Some(z), Some(crate::EnginePar::Zone(par))) => HostValue::Int(z.get(par)),
                    (Some(_), _) => {
                        self.count_unhandled(Action::GetZonePar);
                        HostValue::Int(0)
                    }
                }
            }
            Action::SetZonePar => match (u32::try_from(int(0)), self.engine_par(int(1))) {
                (Ok(zone), Some(crate::EnginePar::Zone(par))) => {
                    HostValue::Int(self.set_zone_par(zone, par, int(2)))
                }
                _ => return None,
            },
            // Time Machine Pro's own voice pools: KONTRA's stretch draws on
            // the engine's voices, so the change is done at once.
            Action::SetVoiceLimit => {
                let slot = usize::from(int(0) == kontra_vm::midi::VL_HQ);
                if let Some(limit) = self.voice_limits.get_mut(slot) {
                    *limit = int(1).max(1);
                }
                HostValue::Int(self.start_async(Exit::Done))
            }
            Action::GetVoiceLimit => HostValue::Int(
                self.voice_limits
                    .get(usize::from(int(0) == kontra_vm::midi::VL_HQ))
                    .copied()
                    .unwrap_or(0),
            ),
            // No file is picked in a browser KONTRA does not show.
            Action::FsGetFilename => {
                *call.text_out = Text::EMPTY;
                HostValue::Text
            }
            _ => return None,
        })
    }
}
