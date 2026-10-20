//! The editor's continuous controls, as one list.
//!
//! A slot is a control the host owns: it carries a *normalised* value (0..1,
//! the form a host automates) and the words that value reads as, which the
//! host fills in because only the parameter knows its own law (linear, skewed,
//! decibels). The view draws a slot and edits its `norm`; how the edit reaches
//! the host is the [`Bind`]'s business, so one call site works in a preview
//! and in a plug-in.

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
}

impl Slot {
    pub const COUNT: usize = 21;
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
