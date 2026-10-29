//! The instrument's output stage and the controllers that move it.

/// The instrument's output fader and pan, after every effect. A move glides:
/// the gain as a one-pole of `volume_tau_ms`, each channel's balance gain at
/// most 1 per `pan_slew_ms`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Output {
    pub volume_db: f32,
    /// -1..=1 balance.
    pub pan: f32,
    pub volume_tau_ms: f32,
    pub pan_slew_ms: f32,
    /// Controllers that set the fader and pan; `None` when nothing does.
    pub controllers: Option<OutputControllers>,
}

impl Output {
    /// 0 dB, centred, gliding as natively measured (37.5 ms, 1 per 50 ms).
    pub const UNITY: Self = Self {
        volume_db: 0.0,
        pan: 0.0,
        volume_tau_ms: 37.5,
        pan_slew_ms: 50.0,
        controllers: None,
    };
}

/// The controllers that set the output fader and pan. The volume controller
/// value v sets the gain `top·(v/127)³` (`top_db` at 127); the pan
/// controller sets the balance `(v − 63.5)/63.5`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputControllers {
    pub volume: u8,
    pub pan: u8,
    pub top_db: f32,
}
