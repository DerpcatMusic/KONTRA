//! The instrument's output stage and the controllers that move it.

/// The instrument's output fader and pan, after every effect. A move glides:
/// the gain as a one-pole of `volume_tau_ms`, each channel's balance gain at
/// most 1 per `pan_slew_ms`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Output {
    pub volume_db: f32,
    /// -1..=1 balance.
    pub pan: f32,
    pub balance: Balance,
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
        balance: Balance::Linear,
        volume_tau_ms: 37.5,
        pan_slew_ms: 50.0,
        controllers: None,
    };
}

/// How a balance turns down the channel it pans away from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Balance {
    /// To `1 − |pan|`.
    Linear,
    /// To `(1 + cos π·pan)/2`: unity at the centre, half at ±½.
    Cosine,
}

impl Balance {
    /// The left and right gains of a -1..=1 pan.
    pub fn gains(self, pan: f32) -> [f32; 2] {
        let pan = pan.clamp(-1.0, 1.0);
        let far = match self {
            Self::Linear => 1.0 - pan.abs(),
            Self::Cosine => 0.5 * (1.0 + (std::f32::consts::PI * pan).cos()),
        };
        if pan > 0.0 { [far, 1.0] } else { [1.0, far] }
    }
}

/// The controllers that set the output fader and pan. The volume controller
/// value v sets the gain `top·(v/127)^power` (`top_db` at 127); the pan
/// controller sets the balance `(v − 63.5)/63.5`; the expression
/// controller scales the gain by `v/127` (127 until one arrives).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputControllers {
    pub volume: u8,
    pub pan: u8,
    pub top_db: f32,
    pub power: i32,
    pub expression: Option<u8>,
}

#[cfg(test)]
mod tests {
    use super::Balance;

    #[test]
    fn a_cosine_balance_holds_the_centre_and_halves_the_far_side_at_a_half() {
        assert_eq!(Balance::Cosine.gains(0.0), [1.0, 1.0]);
        let [l, r] = Balance::Cosine.gains(0.5);
        assert!((l - 0.5).abs() < 1e-6 && r == 1.0);
        let [l, r] = Balance::Cosine.gains(-0.25);
        assert!(l == 1.0 && (r - 0.853_553).abs() < 1e-5);
        assert_eq!(Balance::Linear.gains(-0.25), [1.0, 0.75]);
    }
}
