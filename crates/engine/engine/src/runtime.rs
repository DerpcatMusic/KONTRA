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
            | Action::FsNavigate
            | Action::SetControlParStrArr
            | Action::WillNeverTerminate
            | Action::HideInstIcon
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
            // Time Machine Pro's own voice pools: KONTRA's stretch draws on
            // the engine's voices, so the change is done at once.
            Action::SetVoiceLimit => HostValue::Int(self.start_async(Exit::Done)),
            // No file is picked in a browser KONTRA does not show.
            Action::FsGetFilename => {
                *call.text_out = Text::EMPTY;
                HostValue::Text
            }
            _ => return None,
        })
    }
}
