//! The editor's continuous controls, as one list.
//!
//! A slot is a control the host owns: it carries a *normalised* value (0..1,
//! the form a host automates) and the words that value reads as, which the
//! host fills in because only the parameter knows its own law (linear, skewed,
//! decibels). The view draws a slot and edits its `norm`; how the edit reaches
//! the host is the [`Bind`]'s business, so one call site works in a preview
//! and in a plug-in.
//!
//! Switches are slots too: a mode, a quality or a toggle is a discrete
//! parameter, and its normalised value steps through the same bind, so the
//! host records a switch as a gesture just as it records a knob.

use mui::prelude::*;

/// Every continuous control the editor draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Depth,
    Detail,
    Attack,
    Release,
    Mix,
    Out,
    MaxCut,
    Link,
    Focus,
    DetailLow,
    DetailHigh,
    AttackLow,
    AttackHigh,
    ReleaseLow,
    ReleaseHigh,
    BandFrequency,
    BandDepth,
    BandWidth,
    BandSlope,
    BandBalance,
    BandShape,
    Mode,
    StereoMode,
    Quality,
    LowLatency,
    Bypass,
    Delta,
    BandBypass,
}

impl Slot {
    pub const COUNT: usize = 28;
    pub const ALL: [Slot; Slot::COUNT] = [
        Slot::Depth,
        Slot::Detail,
        Slot::Attack,
        Slot::Release,
        Slot::Mix,
        Slot::Out,
        Slot::MaxCut,
        Slot::Link,
        Slot::Focus,
        Slot::DetailLow,
        Slot::DetailHigh,
        Slot::AttackLow,
        Slot::AttackHigh,
        Slot::ReleaseLow,
        Slot::ReleaseHigh,
        Slot::BandFrequency,
        Slot::BandDepth,
        Slot::BandWidth,
        Slot::BandSlope,
        Slot::BandBalance,
        Slot::BandShape,
        Slot::Mode,
        Slot::StereoMode,
        Slot::Quality,
        Slot::LowLatency,
        Slot::Bypass,
        Slot::Delta,
        Slot::BandBypass,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The widget's name.
    pub fn id(self) -> &'static str {
        match self {
            Slot::Depth => "depth",
            Slot::Detail => "detail",
            Slot::Attack => "attack",
            Slot::Release => "release",
            Slot::Mix => "mix",
            Slot::Out => "out",
            Slot::MaxCut => "max-cut",
            Slot::Link => "link",
            Slot::Focus => "focus",
            Slot::DetailLow => "detail-low",
            Slot::DetailHigh => "detail-high",
            Slot::AttackLow => "attack-low",
            Slot::AttackHigh => "attack-high",
            Slot::ReleaseLow => "release-low",
            Slot::ReleaseHigh => "release-high",
            Slot::BandFrequency => "band-frequency",
            Slot::BandDepth => "band-depth",
            Slot::BandWidth => "band-width",
            Slot::BandSlope => "band-slope",
            Slot::BandBalance => "band-balance",
            Slot::BandShape => "band-shape",
            Slot::Mode => "mode",
            Slot::StereoMode => "stereo-mode",
            Slot::Quality => "quality",
            Slot::LowLatency => "lowlatency",
            Slot::Bypass => "bypass",
            Slot::Delta => "delta",
            Slot::BandBypass => "band-bypass",
        }
    }

    /// Where the slot rests, and what that reads as, before a host says.
    pub fn rest(self) -> (f64, &'static str) {
        match self {
            Slot::Depth => (0.6, "6.0"),
            Slot::Detail => (0.63, "6.3"),
            Slot::Attack => (0.5, "5.0"),
            Slot::Release => (0.3, "3.0"),
            Slot::Mix => (1.0, "100 %"),
            Slot::Out => (0.5, "0.0 dB"),
            Slot::MaxCut => (1.0, "24 dB"),
            Slot::Link => (1.0, "100 %"),
            Slot::Focus => (0.5, "0.50"),
            Slot::BandFrequency => (0.5, "1.0 kHz"),
            Slot::BandDepth => (0.5, "0.0 dB"),
            Slot::BandWidth => (0.5, "0.71"),
            Slot::BandSlope => (0.0, "6 dB/oct"),
            Slot::BandBalance => (0.5, "linked"),
            Slot::BandShape => (2.0 / 7.0, "bell"),
            Slot::Mode => (0.0, "soft"),
            Slot::StereoMode => (0.0, "l/r"),
            Slot::Quality => (0.0, "normal"),
            Slot::LowLatency | Slot::Bypass | Slot::Delta | Slot::BandBypass => (0.0, "off"),
            _ => (0.5, "1.0"),
        }
    }
}

/// A slot's state: where it is and what that reads as.
#[derive(Clone, Debug)]
pub struct Slider {
    pub norm: f64,
    pub text: String,
}

/// The state of every slot, at rest.
pub fn at_rest() -> [Slider; Slot::COUNT] {
    Slot::ALL.map(|slot| {
        let (norm, text) = slot.rest();
        Slider {
            norm,
            text: text.to_string(),
        }
    })
}

/// How a control draws itself and edits a normalised value.
pub type Part<'a> = dyn FnMut(&mut Ui, &str, &mut f64) -> El + 'a;

/// How a slot reaches its value: the part is run on the host's value inside
/// the host's gesture, or on the model's own in a preview.
pub type Bind<'a> = dyn FnMut(&mut Ui, Slot, &mut f64, &mut Part) -> El + 'a;

/// The bind a preview uses: the part, on the model.
pub fn model_bind() -> impl FnMut(&mut Ui, Slot, &mut f64, &mut Part) -> El {
    |ui, slot, norm, part| part(ui, slot.id(), norm)
}

/// A switch of `n` choices, bound to a discrete slot: the part edits the index,
/// and the slot holds it as a step of the normalised range.
pub fn choice(
    ui: &mut Ui,
    bind: &mut Bind,
    slot: Slot,
    state: &mut Slider,
    n: usize,
    part: &mut dyn FnMut(&mut Ui, &str, &mut usize) -> El,
) -> El {
    let span = n.max(2) - 1;
    bind(ui, slot, &mut state.norm, &mut |ui, id, norm| {
        let mut index = (*norm * span as f64).round().clamp(0.0, span as f64) as usize;
        let el = part(ui, id, &mut index);
        *norm = index as f64 / span as f64;
        el
    })
}

/// A toggle bound to a discrete slot: off at 0, on at 1.
pub fn flag(
    ui: &mut Ui,
    bind: &mut Bind,
    slot: Slot,
    state: &mut Slider,
    part: &mut dyn FnMut(&mut Ui, &str, &mut bool) -> El,
) -> El {
    bind(ui, slot, &mut state.norm, &mut |ui, id, norm| {
        let mut on = *norm >= 0.5;
        let el = part(ui, id, &mut on);
        *norm = f64::from(u8::from(on));
        el
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_and_a_flag_hold_their_step_in_the_normalised_range() {
        let mut ui = Ui::default();
        let mut bind = model_bind();
        let mut state = Slider { norm: 0.5, text: String::new() };
        // A three-way choice: 0.5 is the middle option, and the part moves it
        // to the last one, which is the top of the range.
        let _ = choice(&mut ui, &mut bind, Slot::Quality, &mut state, 3, &mut |_, _, index| {
            assert_eq!(*index, 1);
            *index = 2;
            block(0., 0.)
        });
        assert_eq!(state.norm, 1.0);

        let mut state = Slider { norm: 0.0, text: String::new() };
        let _ = flag(&mut ui, &mut bind, Slot::Bypass, &mut state, &mut |_, _, on| {
            assert!(!*on);
            *on = true;
            block(0., 0.)
        });
        assert_eq!(state.norm, 1.0);
    }
}
