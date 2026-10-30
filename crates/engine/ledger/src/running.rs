//! The script callback being run, kept across its waits.

use crate::NoteRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Init,
    Note,
    Release,
    Controller,
    Listener,
    Pgs,
    UiControl,
    UiControls,
    AsyncComplete,
    /// After a wait: what the callback was no longer matters.
    Resumed,
}

/// The callback being run and what it decided about its event.
#[derive(Clone, Copy, Debug)]
pub struct Running {
    pub kind: Kind,
    pub event: Option<NoteRef>,
    pub controller: u8,
    pub ignore_release: bool,
    pub ignore_controller: bool,
    /// The callback's type number scripts read, kept across waits.
    pub callback_type: u8,
    /// Unique per callback run, kept across waits.
    pub callback_id: u32,
    /// The script slot running it.
    pub slot: u8,
    /// The signal a listener callback answers.
    pub signal: u8,
    /// The control id of a UI callback; 0 otherwise.
    pub ui: i32,
}

/// Marks a UI id in the low bits of a coroutine context; event ids never set it.
const UI_BIT: u32 = 1 << 31;

impl Running {
    pub const IDLE: Self = Self {
        kind: Kind::Resumed,
        event: None,
        controller: 0,
        ignore_release: false,
        ignore_controller: false,
        callback_type: 0,
        callback_id: 0,
        slot: 0,
        signal: 0,
        ui: 0,
    };

    pub fn new(kind: Kind, event: Option<NoteRef>) -> Self {
        // The script's callback type numbers (measured, probes/callback_constants.lua).
        let callback_type = match kind {
            Kind::Init | Kind::Resumed => 5,
            Kind::Note => 1,
            Kind::Release => 2,
            Kind::Controller => 3,
            Kind::Listener => 12,
            Kind::Pgs => 8,
            Kind::UiControl => 6,
            Kind::UiControls => 15,
            Kind::AsyncComplete => 13,
        };
        Self {
            kind,
            event,
            callback_type,
            ..Self::IDLE
        }
    }

    /// The coroutine context, so a resumed callback still knows its event
    /// or UI id (low 32 bits, a UI id with the top bit set), callback type
    /// (8), callback id (20) and listener signal (4).
    pub fn context(&self) -> u64 {
        let low = match self.event {
            Some(n) => n.event_id() as u32,
            None => self.ui as u32 | UI_BIT,
        };
        u64::from(low)
            | u64::from(self.callback_type) << 32
            | u64::from(self.callback_id & 0xF_FFFF) << 40
            | u64::from(self.signal & 0xF) << 60
    }

    pub fn resumed(context: u64) -> Self {
        let low = context as u32;
        Self {
            ui: if low & UI_BIT != 0 {
                (low & !UI_BIT) as i32
            } else {
                0
            },
            kind: Kind::Resumed,
            event: NoteRef::from_event_id(context as u32 as i32),
            callback_type: (context >> 32) as u8,
            callback_id: (context >> 40) as u32 & 0xF_FFFF,
            signal: (context >> 60) as u8,
            ..Self::IDLE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resumed_callback_keeps_its_event_or_control() {
        let event = NoteRef::from_event_id(0x0003_0002);
        let mut note = Running::new(Kind::Listener, event);
        (note.callback_id, note.signal) = (0xA_BCDE, 9);
        let mut ui = Running::new(Kind::UiControl, None);
        (ui.ui, ui.callback_id) = (77, 1);
        for run in [note, ui] {
            let back = Running::resumed(run.context());
            let fields = |r: Running| (r.event, r.ui, r.callback_type, r.callback_id, r.signal);
            assert_eq!(fields(back), fields(run));
            assert_eq!(back.kind, Kind::Resumed);
        }
    }
}
