//! Kontakt 8 time-stretch / slicing / gating modules onto format-neutral IR:
//! - **Beat Masher** (`0x71`, v`0x01`, `$EFFECT_TYPE_BEATMASHER = 62`)
//! - **Gater** (`0x72`, v`0x01`, `$EFFECT_TYPE_GATER = 63`)
//! - **Beat Slicer** (`0x73`, v`0x01`, `$EFFECT_TYPE_BEATSLICER = 64`)
//! - **Reverse Grain** (`0x74`, v`0x01`, `$EFFECT_TYPE_REVGRAIN = 65`)
//! - **Transpose Stretch** (`0x75`, v`0x01`, `$EFFECT_TYPE_TRANS_STRETCH = 66`)
//!
//! All five share version `0x01` binary framing, verified field offsets, and
//! single-encoding declarative [`crate::module::Module`] descriptors ([ref]).

use kontra_ir as ir;

#[inline]
fn f32_le(public: &[u8], offset: usize) -> Option<f32> {
    let bytes: [u8; 4] = public.get(offset..offset + 4)?.try_into().ok()?;
    Some(f32::from_le_bytes(bytes)).filter(|v| v.is_finite())
}

#[inline]
fn i32_le(public: &[u8], offset: usize) -> Option<i32> {
    let bytes: [u8; 4] = public.get(offset..offset + 4)?.try_into().ok()?;
    Some(i32::from_le_bytes(bytes))
}

#[inline]
fn flag_at(public: &[u8], offset: usize) -> Option<bool> {
    public.get(offset).map(|&b| b != 0)
}

#[inline]
fn on_f(b: bool) -> f32 {
    if b { 1.0 } else { 0.0 }
}

/// Builds a standard live-loadable [`ir::SlotType`] from a [`crate::module::Module`]
/// and its INIT knob vector.
fn make_slot_type(module: &'static crate::module::Module, saved: &[f32]) -> ir::SlotType {
    let model = module.knob_model(saved);
    let effect = model.build(&model.saved).map(|kind| ir::Effect {
        kind,
        bypass: false,
        output: 1.0,
        dry: 0.0,
    });
    ir::SlotType {
        id: module.script_type,
        subtype: false,
        reads: 0,
        kind: module.script_type,
        effect,
        laws: [].into(),
        model: Some(model),
    }
}

/// Returns all 5 mangling [`ir::SlotType`] entries for live KSP `$ENGINE_PAR_EFFECT_TYPE`.
#[must_use]
pub fn slot_types() -> [ir::SlotType; 5] {
    [
        beat_masher::slot_type(),
        gater::slot_type(),
        beat_slicer::slot_type(),
        reverse_grain::slot_type(),
        transpose_stretch::slot_type(),
    ]
}

/// Returns all 36 KSP `$ENGINE_PAR_*` knob rows across the 5 mangling modules.
#[must_use]
pub fn rows() -> Vec<(&'static str, ir::KnobTarget, ir::Law)> {
    let mut out = Vec::with_capacity(36);
    out.extend(beat_masher::rows());
    out.extend(gater::rows());
    out.extend(beat_slicer::rows());
    out.extend(reverse_grain::rows());
    out.extend(transpose_stretch::rows());
    out
}

// ---------------------------------------------------------------------------
// 1. Beat Masher (0x71, v0x01, script_type 62)
// ---------------------------------------------------------------------------

pub mod beat_masher {
    use super::*;

    /// `$NI_BEATMASHER_LENGTH_*` enum values (0..=8) to quarter-note beats.
    const LENGTH_BEATS: [f32; 9] = [
        4.0,   // 0: BAR
        3.0,   // 1: HALF_DOTTED
        2.0,   // 2: HALF
        1.5,   // 3: QUARTER_DOTTED
        1.0,   // 4: QUARTER
        0.75,  // 5: 8TH_DOTTED
        0.5,   // 6: 8TH
        0.25,  // 7: 16TH
        0.125, // 8: 32ND
    ];

    /// Saved fields of Beat Masher (`FFFBBBI`, 19 bytes).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct BeatMasher {
        pub rotate: f32,
        pub gate: f32,
        pub mix: f32,
        pub reverse: bool,
        pub wrap: bool,
        pub mash: bool,
        pub length: i32,
    }

    pub const INIT: BeatMasher = BeatMasher {
        rotate: 0.0,
        gate: 0.5,
        mix: 1.0,
        reverse: false,
        wrap: true,
        mash: false,
        length: 4, // 1/4 beat
    };

    #[must_use]
    pub fn saved(version: u16, public: &[u8]) -> Option<BeatMasher> {
        if version != 1 {
            return None;
        }
        Some(BeatMasher {
            rotate: f32_le(public, 0)?,
            gate: f32_le(public, 4)?,
            mix: f32_le(public, 8)?,
            reverse: flag_at(public, 12)?,
            wrap: flag_at(public, 13)?,
            mash: flag_at(public, 14)?,
            length: i32_le(public, 15)?.clamp(0, 8),
        })
    }

    #[must_use]
    pub fn ir_effect(p: BeatMasher) -> ir::BeatMasher {
        let idx = usize::try_from(p.length.clamp(0, 8)).unwrap_or(4);
        let length_beats = LENGTH_BEATS.get(idx).copied().unwrap_or(1.0);
        ir::BeatMasher {
            length_beats,
            rotate: p.rotate.clamp(0.0, 1.0),
            // Gate knob 0..1 displays -100..+100% (0.5 -> 0.0 full open).
            gate: (p.gate.clamp(0.0, 1.0) - 0.5) * 2.0,
            mix: p.mix.clamp(0.0, 1.0),
            reverse: p.reverse,
            wrap: p.wrap,
            mash: p.mash,
        }
    }

    pub static MODULE: crate::module::Module = crate::module::Module {
        id: crate::module::ids::BEAT_MASHER,
        module_id: 0x71,
        versions: (1, 1),
        name: "Beat Masher",
        script_type: 62,
        mix: crate::module::MixMode::OwnMix,
        knobs: crate::module::KNOBS.split_at(7).0,
        build: |v| {
            let [rotate, gate, mix, reverse, wrap, mash, length] = *v else {
                return None;
            };
            Some(ir::EffectKind::BeatMasher(ir_effect(BeatMasher {
                rotate,
                gate,
                mix,
                reverse: reverse > 0.5,
                wrap: wrap > 0.5,
                mash: mash > 0.5,
                length: length.round() as i32,
            })))
        },
    };

    #[must_use]
    pub fn model(p: BeatMasher) -> ir::KnobModel {
        MODULE.knob_model(&[
            p.rotate,
            p.gate,
            p.mix,
            on_f(p.reverse),
            on_f(p.wrap),
            on_f(p.mash),
            p.length as f32,
        ])
    }

    #[must_use]
    pub fn slot_type() -> ir::SlotType {
        make_slot_type(
            &MODULE,
            &[
                INIT.rotate,
                INIT.gate,
                INIT.mix,
                on_f(INIT.reverse),
                on_f(INIT.wrap),
                on_f(INIT.mash),
                INIT.length as f32,
            ],
        )
    }

    #[must_use]
    pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 7] {
        use ir::KnobTarget::Slot;
        use ir::SlotKnob::Knob;
        let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
        let raw = || ir::Law::Linear { lo: 0.0, hi: 1e6 };
        [
            ("$ENGINE_PAR_BEATMASHER_ROTATE", Slot(Knob(0)), unit()),
            ("$ENGINE_PAR_BEATMASHER_GATE", Slot(Knob(1)), unit()),
            ("$ENGINE_PAR_BEATMASHER_MIX", Slot(Knob(2)), unit()),
            ("$ENGINE_PAR_BEATMASHER_REVERSE", Slot(Knob(3)), raw()),
            ("$ENGINE_PAR_BEATMASHER_WRAP", Slot(Knob(4)), raw()),
            ("$ENGINE_PAR_BEATMASHER_MASH", Slot(Knob(5)), raw()),
            ("$ENGINE_PAR_BEATMASHER_LENGTH", Slot(Knob(6)), raw()),
        ]
    }
}

// ---------------------------------------------------------------------------
// 2. Gater (0x72, v0x01, script_type 63)
// ---------------------------------------------------------------------------

pub mod gater {
    use super::*;

    /// Synced gate subdivisions mapped across `rate` in `0..=1` (1 bar down to 1/32).
    const SYNC_BEATS: [f32; 9] = [4.0, 2.0, 1.5, 1.0, 0.75, 0.5, 0.375, 0.25, 0.125];

    /// Saved fields of Gater (`FFFFBBBB`, 20 bytes).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Gater {
        pub rate: f32,
        pub shape: f32,
        pub noise: f32,
        pub mix: f32,
        pub stutter: bool,
        pub mute_input: bool,
        pub gate: bool,
        pub rate_sync: bool,
    }

    pub const INIT: Gater = Gater {
        rate: 0.5,
        shape: 0.5,
        noise: 0.0,
        mix: 1.0,
        stutter: false,
        mute_input: false,
        gate: true,
        rate_sync: true,
    };

    #[must_use]
    pub fn saved(version: u16, public: &[u8]) -> Option<Gater> {
        if version != 1 {
            return None;
        }
        Some(Gater {
            rate: f32_le(public, 0)?,
            shape: f32_le(public, 4)?,
            noise: f32_le(public, 8)?,
            mix: f32_le(public, 12)?,
            stutter: flag_at(public, 16)?,
            mute_input: flag_at(public, 17)?,
            gate: flag_at(public, 18)?,
            rate_sync: flag_at(public, 19)?,
        })
    }

    #[must_use]
    pub fn ir_effect(p: Gater) -> ir::Gater {
        let x = p.rate.clamp(0.0, 1.0);
        let rate = if p.rate_sync {
            let idx =
                ((x * (SYNC_BEATS.len() - 1) as f32).round() as usize).min(SYNC_BEATS.len() - 1);
            let beats = SYNC_BEATS.get(idx).copied().unwrap_or(0.5);
            ir::Rate::Beats(beats)
        } else {
            // Free rate readout law: `20 + 380*x` BPM -> quarter-note beats/sec.
            let bpm = 20.0 + 380.0 * x;
            ir::Rate::Hz(bpm / 60.0)
        };
        ir::Gater {
            rate,
            shape: p.shape.clamp(0.0, 1.0),
            noise: p.noise.clamp(0.0, 1.0),
            mix: p.mix.clamp(0.0, 1.0),
            stutter: p.stutter,
            mute_input: p.mute_input,
            active: p.gate,
        }
    }

    pub static MODULE: crate::module::Module = crate::module::Module {
        id: crate::module::ids::GATER,
        module_id: 0x72,
        versions: (1, 1),
        name: "Gater",
        script_type: 63,
        mix: crate::module::MixMode::OwnMix,
        knobs: crate::module::KNOBS.split_at(8).0,
        build: |v| {
            let [
                rate,
                shape,
                noise,
                mix,
                stutter,
                mute_input,
                gate,
                rate_sync,
            ] = *v
            else {
                return None;
            };
            Some(ir::EffectKind::Gater(ir_effect(Gater {
                rate,
                shape,
                noise,
                mix,
                stutter: stutter > 0.5,
                mute_input: mute_input > 0.5,
                gate: gate > 0.5,
                rate_sync: rate_sync > 0.5,
            })))
        },
    };

    #[must_use]
    pub fn model(p: Gater) -> ir::KnobModel {
        MODULE.knob_model(&[
            p.rate,
            p.shape,
            p.noise,
            p.mix,
            on_f(p.stutter),
            on_f(p.mute_input),
            on_f(p.gate),
            on_f(p.rate_sync),
        ])
    }

    #[must_use]
    pub fn slot_type() -> ir::SlotType {
        make_slot_type(
            &MODULE,
            &[
                INIT.rate,
                INIT.shape,
                INIT.noise,
                INIT.mix,
                on_f(INIT.stutter),
                on_f(INIT.mute_input),
                on_f(INIT.gate),
                on_f(INIT.rate_sync),
            ],
        )
    }

    #[must_use]
    pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 8] {
        use ir::KnobTarget::Slot;
        use ir::SlotKnob::Knob;
        let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
        let raw = || ir::Law::Linear { lo: 0.0, hi: 1e6 };
        [
            ("$ENGINE_PAR_GATER_RATE", Slot(Knob(0)), unit()),
            ("$ENGINE_PAR_GATER_SHAPE", Slot(Knob(1)), unit()),
            ("$ENGINE_PAR_GATER_NOISE", Slot(Knob(2)), unit()),
            ("$ENGINE_PAR_GATER_MIX", Slot(Knob(3)), unit()),
            ("$ENGINE_PAR_GATER_STUTTER", Slot(Knob(4)), raw()),
            ("$ENGINE_PAR_GATER_MUTE_INPUT", Slot(Knob(5)), raw()),
            ("$ENGINE_PAR_GATER_GATE", Slot(Knob(6)), raw()),
            ("$ENGINE_PAR_GATER_RATE_SYNC", Slot(Knob(7)), raw()),
        ]
    }
}

// ---------------------------------------------------------------------------
// 3. Beat Slicer (0x73, v0x01, script_type 64)
// ---------------------------------------------------------------------------

pub mod beat_slicer {
    use super::*;

    /// Saved fields of Beat Slicer (`FFBBBII`, 19 bytes).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct BeatSlicer {
        pub buzz: f32,
        pub mix: f32,
        pub twobars: bool,
        pub gate: bool,
        pub slice: bool,
        pub pattern: i32,
        pub style: i32,
    }

    pub const INIT: BeatSlicer = BeatSlicer {
        buzz: 0.0,
        mix: 1.0,
        twobars: false,
        gate: false,
        slice: false,
        pattern: 0,
        style: 0,
    };

    #[must_use]
    pub fn saved(version: u16, public: &[u8]) -> Option<BeatSlicer> {
        if version != 1 {
            return None;
        }
        Some(BeatSlicer {
            buzz: f32_le(public, 0)?,
            mix: f32_le(public, 4)?,
            twobars: flag_at(public, 8)?,
            gate: flag_at(public, 9)?,
            slice: flag_at(public, 10)?,
            pattern: i32_le(public, 11)?.clamp(0, 4),
            style: i32_le(public, 15)?.clamp(0, 4),
        })
    }

    #[must_use]
    pub fn ir_effect(p: BeatSlicer) -> ir::BeatSlicer {
        ir::BeatSlicer {
            bars: if p.twobars { 2 } else { 1 },
            pattern: u8::try_from(p.pattern.clamp(0, 4)).unwrap_or(0),
            style: u8::try_from(p.style.clamp(0, 4)).unwrap_or(0),
            buzz: p.buzz.clamp(0.0, 1.0),
            gate: p.gate,
            slice: p.slice,
            mix: p.mix.clamp(0.0, 1.0),
        }
    }

    pub static MODULE: crate::module::Module = crate::module::Module {
        id: crate::module::ids::BEAT_SLICER,
        module_id: 0x73,
        versions: (1, 1),
        name: "Beat Slicer",
        script_type: 64,
        mix: crate::module::MixMode::OwnMix,
        knobs: crate::module::KNOBS.split_at(7).0,
        build: |v| {
            let [buzz, mix, twobars, gate, slice, pattern, style] = *v else {
                return None;
            };
            Some(ir::EffectKind::BeatSlicer(ir_effect(BeatSlicer {
                buzz,
                mix,
                twobars: twobars > 0.5,
                gate: gate > 0.5,
                slice: slice > 0.5,
                pattern: pattern.round() as i32,
                style: style.round() as i32,
            })))
        },
    };

    #[must_use]
    pub fn model(p: BeatSlicer) -> ir::KnobModel {
        MODULE.knob_model(&[
            p.buzz,
            p.mix,
            on_f(p.twobars),
            on_f(p.gate),
            on_f(p.slice),
            p.pattern as f32,
            p.style as f32,
        ])
    }

    #[must_use]
    pub fn slot_type() -> ir::SlotType {
        make_slot_type(
            &MODULE,
            &[
                INIT.buzz,
                INIT.mix,
                on_f(INIT.twobars),
                on_f(INIT.gate),
                on_f(INIT.slice),
                INIT.pattern as f32,
                INIT.style as f32,
            ],
        )
    }

    #[must_use]
    pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 7] {
        use ir::KnobTarget::Slot;
        use ir::SlotKnob::Knob;
        let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
        let raw = || ir::Law::Linear { lo: 0.0, hi: 1e6 };
        [
            ("$ENGINE_PAR_BEATSLICER_BUZZ", Slot(Knob(0)), unit()),
            ("$ENGINE_PAR_BEATSLICER_MIX", Slot(Knob(1)), unit()),
            ("$ENGINE_PAR_BEATSLICER_TWOBARS", Slot(Knob(2)), raw()),
            ("$ENGINE_PAR_BEATSLICER_GATE", Slot(Knob(3)), raw()),
            ("$ENGINE_PAR_BEATSLICER_SLICE", Slot(Knob(4)), raw()),
            ("$ENGINE_PAR_BEATSLICER_PATTERN", Slot(Knob(5)), raw()),
            ("$ENGINE_PAR_BEATSLICER_STYLE", Slot(Knob(6)), raw()),
        ]
    }
}

// ---------------------------------------------------------------------------
// 4. Reverse Grain (0x74, v0x01, script_type 65)
// ---------------------------------------------------------------------------

pub mod reverse_grain {
    use super::*;

    /// Saved fields of Reverse Grain (`FFFFBBB`, 19 bytes).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct ReverseGrain {
        pub speed: f32,
        pub grain: f32,
        pub pitch: f32,
        pub mix: f32,
        pub forward: bool,
        pub invert_grains: bool,
        pub reverse: bool,
    }

    pub const INIT: ReverseGrain = ReverseGrain {
        speed: 0.5,
        grain: 0.5,
        pitch: 0.5,
        mix: 1.0,
        forward: false,
        invert_grains: false,
        reverse: true,
    };

    #[must_use]
    pub fn saved(version: u16, public: &[u8]) -> Option<ReverseGrain> {
        if version != 1 {
            return None;
        }
        Some(ReverseGrain {
            speed: f32_le(public, 0)?,
            grain: f32_le(public, 4)?,
            pitch: f32_le(public, 8)?,
            mix: f32_le(public, 12)?,
            forward: flag_at(public, 16)?,
            invert_grains: flag_at(public, 17)?,
            reverse: flag_at(public, 18)?,
        })
    }

    #[must_use]
    pub fn ir_effect(p: ReverseGrain) -> ir::ReverseGrain {
        let g = p.grain.clamp(0.0, 1.0);
        // Exponential grain duration from 20 ms to 400 ms.
        let grain_ms = 20.0 * 20.0f32.powf(g);
        // Pitch 0..1 centered at 0.5 = 1.0x, spanning 0.5x (-12 st) to 2.0x (+12 st).
        let pitch_ratio = (2.0 * p.pitch.clamp(0.0, 1.0) - 1.0).exp2();
        ir::ReverseGrain {
            speed: p.speed.clamp(0.0, 1.0),
            grain_ms,
            pitch_ratio,
            mix: p.mix.clamp(0.0, 1.0),
            forward: p.forward,
            invert_grains: p.invert_grains,
            reverse: p.reverse,
        }
    }

    pub static MODULE: crate::module::Module = crate::module::Module {
        id: crate::module::ids::REVERSE_GRAIN,
        module_id: 0x74,
        versions: (1, 1),
        name: "Reverse Grain",
        script_type: 65,
        mix: crate::module::MixMode::OwnMix,
        knobs: crate::module::KNOBS.split_at(7).0,
        build: |v| {
            let [speed, grain, pitch, mix, forward, invert_grains, reverse] = *v else {
                return None;
            };
            Some(ir::EffectKind::ReverseGrain(ir_effect(ReverseGrain {
                speed,
                grain,
                pitch,
                mix,
                forward: forward > 0.5,
                invert_grains: invert_grains > 0.5,
                reverse: reverse > 0.5,
            })))
        },
    };

    #[must_use]
    pub fn model(p: ReverseGrain) -> ir::KnobModel {
        MODULE.knob_model(&[
            p.speed,
            p.grain,
            p.pitch,
            p.mix,
            on_f(p.forward),
            on_f(p.invert_grains),
            on_f(p.reverse),
        ])
    }

    #[must_use]
    pub fn slot_type() -> ir::SlotType {
        make_slot_type(
            &MODULE,
            &[
                INIT.speed,
                INIT.grain,
                INIT.pitch,
                INIT.mix,
                on_f(INIT.forward),
                on_f(INIT.invert_grains),
                on_f(INIT.reverse),
            ],
        )
    }

    #[must_use]
    pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 7] {
        use ir::KnobTarget::Slot;
        use ir::SlotKnob::Knob;
        let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
        let raw = || ir::Law::Linear { lo: 0.0, hi: 1e6 };
        [
            ("$ENGINE_PAR_RG_SPEED", Slot(Knob(0)), unit()),
            ("$ENGINE_PAR_RG_GRAIN", Slot(Knob(1)), unit()),
            ("$ENGINE_PAR_RG_PITCH", Slot(Knob(2)), unit()),
            ("$ENGINE_PAR_RG_MIX", Slot(Knob(3)), unit()),
            ("$ENGINE_PAR_RG_FORWARD", Slot(Knob(4)), raw()),
            ("$ENGINE_PAR_RG_INVERT_GRAINS", Slot(Knob(5)), raw()),
            ("$ENGINE_PAR_RG_REVERSE", Slot(Knob(6)), raw()),
        ]
    }
}

// ---------------------------------------------------------------------------
// 5. Transpose Stretch (0x75, v0x01, script_type 66)
// ---------------------------------------------------------------------------

pub mod transpose_stretch {
    use super::*;

    /// Saved fields of Transpose Stretch (`FFFFBBB`, 19 bytes).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct TransposeStretch {
        pub amount: f32,
        pub size: f32,
        pub key: f32,
        pub mix: f32,
        pub stretch: bool,
        pub grain: bool,
        pub twobars: bool,
    }

    pub const INIT: TransposeStretch = TransposeStretch {
        amount: 0.0,
        size: 0.5,
        key: 0.5,
        mix: 1.0,
        stretch: false,
        grain: false,
        twobars: false,
    };

    #[must_use]
    pub fn saved(version: u16, public: &[u8]) -> Option<TransposeStretch> {
        if version != 1 {
            return None;
        }
        Some(TransposeStretch {
            amount: f32_le(public, 0)?,
            size: f32_le(public, 4)?,
            key: f32_le(public, 8)?,
            mix: f32_le(public, 12)?,
            stretch: flag_at(public, 16)?,
            grain: flag_at(public, 17)?,
            twobars: flag_at(public, 18)?,
        })
    }

    #[must_use]
    pub fn ir_effect(p: TransposeStretch) -> ir::TransposeStretch {
        let key_u = p.key.clamp(0.0, 1.0);
        // Exact `TransStretchKey` law from `kontra_law::FormulaId::TransStretchKey`:
        // `120*x - 60` below 0.5 (-60..0 st), `24*(x - 0.5)` from 0.5 (0..+12 st).
        let semitones = kontra_law::FormulaId::TransStretchKey.eval(key_u);
        let grain_ms = 20.0 + 180.0 * p.size.clamp(0.0, 1.0);
        ir::TransposeStretch {
            semitones,
            amount: p.amount.clamp(0.0, 1.0),
            grain_ms,
            mix: p.mix.clamp(0.0, 1.0),
            stretch: p.stretch,
            grain_mode: p.grain,
            bars: if p.twobars { 2 } else { 1 },
        }
    }

    pub static MODULE: crate::module::Module = crate::module::Module {
        id: crate::module::ids::TRANSPOSE_STRETCH,
        module_id: 0x75,
        versions: (1, 1),
        name: "Transpose Stretch",
        script_type: 66,
        mix: crate::module::MixMode::OwnMix,
        knobs: crate::module::KNOBS.split_at(7).0,
        build: |v| {
            let [amount, size, key, mix, stretch, grain, twobars] = *v else {
                return None;
            };
            Some(ir::EffectKind::TransposeStretch(ir_effect(
                TransposeStretch {
                    amount,
                    size,
                    key,
                    mix,
                    stretch: stretch > 0.5,
                    grain: grain > 0.5,
                    twobars: twobars > 0.5,
                },
            )))
        },
    };

    #[must_use]
    pub fn model(p: TransposeStretch) -> ir::KnobModel {
        MODULE.knob_model(&[
            p.amount,
            p.size,
            p.key,
            p.mix,
            on_f(p.stretch),
            on_f(p.grain),
            on_f(p.twobars),
        ])
    }

    #[must_use]
    pub fn slot_type() -> ir::SlotType {
        make_slot_type(
            &MODULE,
            &[
                INIT.amount,
                INIT.size,
                INIT.key,
                INIT.mix,
                on_f(INIT.stretch),
                on_f(INIT.grain),
                on_f(INIT.twobars),
            ],
        )
    }

    #[must_use]
    pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 7] {
        use ir::KnobTarget::Slot;
        use ir::SlotKnob::Knob;
        let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
        let raw = || ir::Law::Linear { lo: 0.0, hi: 1e6 };
        [
            ("$ENGINE_PAR_TS_AMOUNT", Slot(Knob(0)), unit()),
            ("$ENGINE_PAR_TS_SIZE", Slot(Knob(1)), unit()),
            ("$ENGINE_PAR_TS_KEY", Slot(Knob(2)), unit()),
            ("$ENGINE_PAR_TS_MIX", Slot(Knob(3)), unit()),
            ("$ENGINE_PAR_TS_STRETCH", Slot(Knob(4)), raw()),
            ("$ENGINE_PAR_TS_GRAIN", Slot(Knob(5)), raw()),
            ("$ENGINE_PAR_TS_TWOBARS", Slot(Knob(6)), raw()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_round_trips_match_native_field_layouts() {
        // Beat Masher: FFFBBBI (19 bytes)
        let mut bm = Vec::new();
        for f in [0.25f32, 0.75, 0.9] {
            bm.extend(f.to_le_bytes());
        }
        bm.extend([1u8, 0, 1]);
        bm.extend(7i32.to_le_bytes());
        let d = beat_masher::saved(1, &bm).expect("bm decode");
        let e = beat_masher::ir_effect(d);
        assert_eq!(e.length_beats, 0.25);
        assert!((e.gate - 0.5).abs() < 1e-6);
        assert!(e.reverse && !e.wrap && e.mash);

        // Gater: FFFFBBBB (20 bytes)
        let mut gt = Vec::new();
        for f in [0.5f32, 0.6, 0.2, 0.8] {
            gt.extend(f.to_le_bytes());
        }
        gt.extend([1u8, 0, 1, 0]);
        let gd = gater::saved(1, &gt).expect("gater decode");
        let ge = gater::ir_effect(gd);
        assert_eq!(ge.rate, ir::Rate::Hz(210.0 / 60.0));
        assert!(ge.stutter && !ge.mute_input && ge.active);

        // Beat Slicer: FFBBBII (19 bytes)
        let mut bs = Vec::new();
        for f in [0.4f32, 0.85] {
            bs.extend(f.to_le_bytes());
        }
        bs.extend([1u8, 1, 1]);
        bs.extend(3i32.to_le_bytes());
        bs.extend(2i32.to_le_bytes());
        let bsd = beat_slicer::saved(1, &bs).expect("bs decode");
        let bse = beat_slicer::ir_effect(bsd);
        assert_eq!((bse.bars, bse.pattern, bse.style), (2, 3, 2));
        assert!(bse.gate && bse.slice);

        // Reverse Grain: FFFFBBB (19 bytes)
        let mut rg = Vec::new();
        for f in [0.3f32, 0.5, 1.0, 0.9] {
            rg.extend(f.to_le_bytes());
        }
        rg.extend([0u8, 1, 1]);
        let rgd = reverse_grain::saved(1, &rg).expect("rg decode");
        let rge = reverse_grain::ir_effect(rgd);
        assert!((rge.pitch_ratio - 2.0).abs() < 1e-5);
        assert!(rge.invert_grains && rge.reverse);

        // Transpose Stretch: FFFFBBB (19 bytes)
        let mut ts = Vec::new();
        for f in [0.5f32, 0.25, 1.0, 0.8] {
            ts.extend(f.to_le_bytes());
        }
        ts.extend([1u8, 1, 1]);
        let tsd = transpose_stretch::saved(1, &ts).expect("ts decode");
        let tse = transpose_stretch::ir_effect(tsd);
        assert!((tse.semitones - 12.0).abs() < 1e-5);
        assert_eq!(tse.bars, 2);
        assert!(tse.stretch && tse.grain_mode);
    }
}
