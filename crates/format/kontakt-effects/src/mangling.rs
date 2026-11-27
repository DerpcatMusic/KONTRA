//! Kontakt 8 time-stretch / slicing / gating modules onto format-neutral IR:
//! Beat Masher (`0x71`), Gater (`0x72`), Beat Slicer (`0x73`), Reverse Grain
//! (`0x74`), and Transpose Stretch (`0x75`).

use kontra_ir as ir;

use crate::module::{MixMode, ids};

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

/// 1. Beat Masher (`0x71`, v`0x01`, `$EFFECT_TYPE_BEATMASHER` = 62).
pub mod beat_masher {
    use super::*;

    const LENGTH_BEATS: [f32; 9] = [4.0, 3.0, 2.0, 1.5, 1.0, 0.75, 0.5, 0.25, 0.125];

    crate::define_effect! {
        struct BeatMasher,
        init: INIT,
        id: ids::BEAT_MASHER,
        module_id: 0x71,
        versions: (1, 1),
        name: "Beat Masher",
        script_type: 62,
        mix: MixMode::OwnMix,
        saved: 1,
        build: |p| ir::EffectKind::BeatMasher(ir_effect(p)),
        fields: [
            (rotate:  f32  = 0.0,   "$ENGINE_PAR_BEATMASHER_ROTATE"),
            (gate:    f32  = 0.5,   "$ENGINE_PAR_BEATMASHER_GATE"),
            (mix:     f32  = 1.0,   "$ENGINE_PAR_BEATMASHER_MIX"),
            (reverse: bool = false, "$ENGINE_PAR_BEATMASHER_REVERSE"),
            (wrap:    bool = true,  "$ENGINE_PAR_BEATMASHER_WRAP"),
            (mash:    bool = false, "$ENGINE_PAR_BEATMASHER_MASH"),
            (length:  i32  = 4,     "$ENGINE_PAR_BEATMASHER_LENGTH"),
        ]
    }

    #[must_use]
    pub fn ir_effect(p: BeatMasher) -> ir::BeatMasher {
        let idx = usize::try_from(p.length.clamp(0, 8)).unwrap_or(4);
        ir::BeatMasher {
            length_beats: LENGTH_BEATS.get(idx).copied().unwrap_or(1.0),
            rotate: p.rotate.clamp(0.0, 1.0),
            gate: (p.gate.clamp(0.0, 1.0) - 0.5) * 2.0,
            mix: p.mix.clamp(0.0, 1.0),
            reverse: p.reverse,
            wrap: p.wrap,
            mash: p.mash,
        }
    }
}

/// 2. Gater (`0x72`, v`0x01`, `$EFFECT_TYPE_GATER` = 63).
pub mod gater {
    use super::*;

    const SYNC_BEATS: [f32; 9] = [4.0, 2.0, 1.5, 1.0, 0.75, 0.5, 0.375, 0.25, 0.125];

    crate::define_effect! {
        struct Gater,
        init: INIT,
        id: ids::GATER,
        module_id: 0x72,
        versions: (1, 1),
        name: "Gater",
        script_type: 63,
        mix: MixMode::OwnMix,
        saved: 1,
        build: |p| ir::EffectKind::Gater(ir_effect(p)),
        fields: [
            (rate:       f32  = 0.5,   "$ENGINE_PAR_GATER_RATE"),
            (shape:      f32  = 0.5,   "$ENGINE_PAR_GATER_SHAPE"),
            (noise:      f32  = 0.0,   "$ENGINE_PAR_GATER_NOISE"),
            (mix:        f32  = 1.0,   "$ENGINE_PAR_GATER_MIX"),
            (stutter:    bool = false, "$ENGINE_PAR_GATER_STUTTER"),
            (mute_input: bool = false, "$ENGINE_PAR_GATER_MUTE_INPUT"),
            (gate:       bool = true,  "$ENGINE_PAR_GATER_GATE"),
            (rate_sync:  bool = true,  "$ENGINE_PAR_GATER_RATE_SYNC"),
        ]
    }

    #[must_use]
    pub fn ir_effect(p: Gater) -> ir::Gater {
        let x = p.rate.clamp(0.0, 1.0);
        let rate = if p.rate_sync {
            let idx =
                ((x * (SYNC_BEATS.len() - 1) as f32).round() as usize).min(SYNC_BEATS.len() - 1);
            ir::Rate::Beats(SYNC_BEATS.get(idx).copied().unwrap_or(0.5))
        } else {
            ir::Rate::Hz((20.0 + 380.0 * x) / 60.0)
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
}

/// 3. Beat Slicer (`0x73`, v`0x01`, `$EFFECT_TYPE_BEATSLICER` = 64).
pub mod beat_slicer {
    use super::*;

    crate::define_effect! {
        struct BeatSlicer,
        init: INIT,
        id: ids::BEAT_SLICER,
        module_id: 0x73,
        versions: (1, 1),
        name: "Beat Slicer",
        script_type: 64,
        mix: MixMode::OwnMix,
        saved: 1,
        build: |p| ir::EffectKind::BeatSlicer(ir_effect(p)),
        fields: [
            (buzz:    f32  = 0.0,   "$ENGINE_PAR_BEATSLICER_BUZZ"),
            (mix:     f32  = 1.0,   "$ENGINE_PAR_BEATSLICER_MIX"),
            (twobars: bool = false, "$ENGINE_PAR_BEATSLICER_TWOBARS"),
            (gate:    bool = false, "$ENGINE_PAR_BEATSLICER_GATE"),
            (slice:   bool = false, "$ENGINE_PAR_BEATSLICER_SLICE"),
            (pattern: i32  = 0,     "$ENGINE_PAR_BEATSLICER_PATTERN"),
            (style:   i32  = 0,     "$ENGINE_PAR_BEATSLICER_STYLE"),
        ]
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
}

/// 4. Reverse Grain (`0x74`, v`0x01`, `$EFFECT_TYPE_REVGRAIN` = 65).
pub mod reverse_grain {
    use super::*;

    crate::define_effect! {
        struct ReverseGrain,
        init: INIT,
        id: ids::REVERSE_GRAIN,
        module_id: 0x74,
        versions: (1, 1),
        name: "Reverse Grain",
        script_type: 65,
        mix: MixMode::OwnMix,
        saved: 1,
        build: |p| ir::EffectKind::ReverseGrain(ir_effect(p)),
        fields: [
            (speed:         f32  = 0.5,   "$ENGINE_PAR_RG_SPEED"),
            (grain:         f32  = 0.5,   "$ENGINE_PAR_RG_GRAIN"),
            (pitch:         f32  = 0.5,   "$ENGINE_PAR_RG_PITCH"),
            (mix:           f32  = 1.0,   "$ENGINE_PAR_RG_MIX"),
            (forward:       bool = false, "$ENGINE_PAR_RG_FORWARD"),
            (invert_grains: bool = false, "$ENGINE_PAR_RG_INVERT_GRAINS"),
            (reverse:       bool = true,  "$ENGINE_PAR_RG_REVERSE"),
        ]
    }

    #[must_use]
    pub fn ir_effect(p: ReverseGrain) -> ir::ReverseGrain {
        ir::ReverseGrain {
            speed: p.speed.clamp(0.0, 1.0),
            grain_ms: 20.0 * 20.0f32.powf(p.grain.clamp(0.0, 1.0)),
            pitch_ratio: (2.0 * p.pitch.clamp(0.0, 1.0) - 1.0).exp2(),
            mix: p.mix.clamp(0.0, 1.0),
            forward: p.forward,
            invert_grains: p.invert_grains,
            reverse: p.reverse,
        }
    }
}

/// 5. Transpose Stretch (`0x75`, v`0x01`, `$EFFECT_TYPE_TRANS_STRETCH` = 66).
pub mod transpose_stretch {
    use super::*;

    crate::define_effect! {
        struct TransposeStretch,
        init: INIT,
        id: ids::TRANSPOSE_STRETCH,
        module_id: 0x75,
        versions: (1, 1),
        name: "Transpose Stretch",
        script_type: 66,
        mix: MixMode::OwnMix,
        saved: 1,
        build: |p| ir::EffectKind::TransposeStretch(ir_effect(p)),
        fields: [
            (amount:  f32  = 0.0,   "$ENGINE_PAR_TS_AMOUNT"),
            (size:    f32  = 0.5,   "$ENGINE_PAR_TS_SIZE"),
            (key:     f32  = 0.5,   "$ENGINE_PAR_TS_KEY"),
            (mix:     f32  = 1.0,   "$ENGINE_PAR_TS_MIX"),
            (stretch: bool = false, "$ENGINE_PAR_TS_STRETCH"),
            (grain:   bool = false, "$ENGINE_PAR_TS_GRAIN"),
            (twobars: bool = false, "$ENGINE_PAR_TS_TWOBARS"),
        ]
    }

    #[must_use]
    pub fn ir_effect(p: TransposeStretch) -> ir::TransposeStretch {
        ir::TransposeStretch {
            semitones: kontra_law::FormulaId::TransStretchKey.eval(p.key.clamp(0.0, 1.0)),
            amount: p.amount.clamp(0.0, 1.0),
            grain_ms: 20.0 + 180.0 * p.size.clamp(0.0, 1.0),
            mix: p.mix.clamp(0.0, 1.0),
            stretch: p.stretch,
            grain_mode: p.grain,
            bars: if p.twobars { 2 } else { 1 },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_round_trips_match_native_field_layouts() {
        let mut bm = Vec::new();
        for f in [0.25f32, 0.75, 0.9] {
            bm.extend(f.to_le_bytes());
        }
        bm.extend([1u8, 0, 1]);
        bm.extend(7i32.to_le_bytes());
        let e = beat_masher::ir_effect(beat_masher::saved(1, &bm).expect("bm"));
        assert_eq!(e.length_beats, 0.25);
        assert!((e.gate - 0.5).abs() < 1e-6 && e.reverse && !e.wrap && e.mash);

        let mut gt = Vec::new();
        for f in [0.5f32, 0.6, 0.2, 0.8] {
            gt.extend(f.to_le_bytes());
        }
        gt.extend([1u8, 0, 1, 0]);
        let ge = gater::ir_effect(gater::saved(1, &gt).expect("gater"));
        assert_eq!(ge.rate, ir::Rate::Hz(210.0 / 60.0));
        assert!(ge.stutter && !ge.mute_input && ge.active);

        let mut bs = Vec::new();
        for f in [0.4f32, 0.85] {
            bs.extend(f.to_le_bytes());
        }
        bs.extend([1u8, 1, 1]);
        bs.extend(3i32.to_le_bytes());
        bs.extend(2i32.to_le_bytes());
        let bse = beat_slicer::ir_effect(beat_slicer::saved(1, &bs).expect("bs"));
        assert_eq!((bse.bars, bse.pattern, bse.style), (2, 3, 2));
        assert!(bse.gate && bse.slice);

        let mut rg = Vec::new();
        for f in [0.3f32, 0.5, 1.0, 0.9] {
            rg.extend(f.to_le_bytes());
        }
        rg.extend([0u8, 1, 1]);
        let rge = reverse_grain::ir_effect(reverse_grain::saved(1, &rg).expect("rg"));
        assert!((rge.pitch_ratio - 2.0).abs() < 1e-5 && rge.invert_grains && rge.reverse);

        let mut ts = Vec::new();
        for f in [0.5f32, 0.25, 1.0, 0.8] {
            ts.extend(f.to_le_bytes());
        }
        ts.extend([1u8, 1, 1]);
        let tse = transpose_stretch::ir_effect(transpose_stretch::saved(1, &ts).expect("ts"));
        assert!((tse.semitones - 12.0).abs() < 1e-5 && tse.bars == 2 && tse.stretch);
    }
}
