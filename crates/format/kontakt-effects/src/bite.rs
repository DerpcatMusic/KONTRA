//! Bite (module 0x66) onto the engine's reducer as a converter pair
//! (`ir::Converter`). The laws are the native core's (process
//! [ref], setters read through v2 sampler-core dsp/color/bite.rs,
//! which matched vectors of the emulated core to 3e-5), confirmed by
//! Kontakt 8.13.1 renders (fx-measured/freak-bite.md): FREQUENCY is
//! `100·441^x` Hz, the HF droop at 44.1 kHz is the band-limited hold's,
//! PREFILTER 0 a 50 Hz four-pole Butterworth, HPF 1/2 one-poles at
//! 100/200 Hz, MIX a crossfade (−6 dB dry at 0.5), SATURATE 0 the measured
//! 0.9 dB of compression at full scale (`y·(1 − 0.05·|y|)`).

use kontra_ir as ir;
use kontra_law::kontakt::PITCH_INDEX;

/// The controls as a script sets them, each 0..=1 but `hpf` (0, 1, 2:
/// 5, 100, 200 Hz) and `dc` (0 or 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knobs {
    pub frequency: f32,
    pub bits: f32,
    pub jitter: f32,
    pub crunch: f32,
    pub dither: f32,
    pub expand: f32,
    pub saturate: f32,
    pub prefilter: f32,
    pub postfilter: f32,
    pub mix: f32,
    pub hpf: f32,
    pub dc: f32,
}

/// A fresh unit as a script loads it (Kontakt 8.13.1 read-back).
pub const FRESH: Knobs = Knobs {
    frequency: 1.0,
    bits: 1.0,
    jitter: 0.0,
    crunch: 0.0,
    dither: 0.0,
    expand: 1.0,
    saturate: 0.0,
    prefilter: 0.5,
    postfilter: 0.5,
    mix: 1.0,
    hpf: 0.0,
    dc: 0.0,
};

/// The native pitch index (`2^(i/60 − 20)`, so 1200 is 1).
fn index(i: f32) -> f32 {
    PITCH_INDEX.eval_unclamped(i)
}

/// dB as a pitch-index offset (`log2(10)/20·60`).
const DB: f32 = 9.965_784;

/// The reducer with its converter at `k`.
#[must_use]
pub fn bite(k: Knobs) -> ir::EffectKind {
    let unit = |x: f32| x.clamp(0.0, 1.0);
    let frequency = unit(k.frequency);
    let pitch = frequency * 105.415_62 + 43.349_957;
    let levels = (2 + (unit(k.bits) * 15.0) as i32).clamp(2, 16);
    let step = (1 - levels) as f32;
    let step = step.exp2();
    // Fewer bits come out quieter (the native table, by bit count from 2).
    let bitgain = [0.6, 0.7, 0.75, 0.8, 0.85, 0.9, 0.95]
        .get((levels - 2) as usize)
        .copied()
        .unwrap_or(1.0);
    let crunch_db = unit(k.crunch) * 18.0 * step.log10();
    let saturate = unit(k.saturate);
    let mix = unit(k.mix);
    let wet = if mix > 0.5 {
        4.0 * mix - 2.0 * mix * mix - 1.0
    } else {
        2.0 * mix * mix
    };
    // Each filter's corner: half the clock at 0.5, toward 50 Hz and 22 kHz.
    let corner = |x: f32| {
        let x = unit(x);
        let (a, b) = if x < 0.5 {
            (1.0 - 2.0 * x, 43.349_957 - pitch)
        } else {
            (2.0 * x - 1.0, 148.765_58 - pitch)
        };
        index((a * b + pitch) * 5.0 + 1_381.881_6) * 0.5
    };
    ir::EffectKind::Crush(ir::Crush {
        rate: 1.0,
        bits: 16.0,
        noise: 0.0,
        noise_tone: 1.0,
        converter: Some(ir::Converter {
            clock_hz: index(frequency * 527.078_1 + 1_598.631_3),
            jitter: unit(k.jitter).powi(2) * 0.9,
            pre_hz: corner(k.prefilter),
            post_hz: corner(k.postfilter),
            highpass_hz: [5.0, 100.0, 200.0]
                .get(k.hpf.clamp(0.0, 2.0) as usize)
                .copied()
                .unwrap_or(5.0),
            step,
            mu: (8.0 - unit(k.expand) * 7.9).exp2() - 1.0,
            midrise: k.dc >= 0.5,
            dither: unit(k.dither).powi(2),
            drive_in: index(crunch_db * DB + 1_200.0),
            drive_out: index((24.0 * saturate - crunch_db) * DB + 1_200.0) * bitgain,
            soft: 0.2 * saturate + 0.05,
            trim: index(1_200.0 - 79.726_27 * saturate),
            wet,
            dry: 1.0 - wet,
        }),
    })
}

/// The saved record (apply [ref], field order from v2's reader): HPF
/// as an i32, FREQUENCY, BITS, JITTER, CRUNCH, DITHER, EXPAND, SATURATE,
/// PREFILTER, POSTFILTER and MIX as floats, DC_QUANT as a byte.
#[must_use]
pub fn saved(public: &[u8]) -> Option<Knobs> {
    let f = |n: usize| {
        Some(f32::from_le_bytes(public.get(n..n + 4)?.try_into().ok()?)).filter(|v| v.is_finite())
    };
    let hpf = i32::from_le_bytes(public.get(0..4)?.try_into().ok()?);
    if !(0..=2).contains(&hpf) {
        return None;
    }
    Some(Knobs {
        hpf: hpf as f32,
        frequency: f(4)?,
        bits: f(8)?,
        jitter: f(12)?,
        crunch: f(16)?,
        dither: f(20)?,
        expand: f(24)?,
        saturate: f(28)?,
        prefilter: f(32)?,
        postfilter: f(36)?,
        mix: f(40)?,
        dc: if *public.get(44)? != 0 { 1.0 } else { 0.0 },
    })
}

/// Single-encoding module descriptor for Bite (`0x66`).
pub static MODULE: crate::module::Module = crate::module::Module {
    id: crate::module::ids::BITE,
    module_id: 0x66,
    versions: (0, u32::MAX),
    name: "Bite",
    script_type: 52,
    mix: crate::module::MixMode::OwnMix,
    knobs: crate::module::KNOBS.split_at(12).0,
    build: |p| {
        let [
            frequency,
            bits,
            jitter,
            crunch,
            dither,
            expand,
            saturate,
            prefilter,
            postfilter,
            mix,
            hpf,
            dc,
        ] = *p
        else {
            return None;
        };
        Some(bite(Knobs {
            frequency,
            bits,
            jitter,
            crunch,
            dither,
            expand,
            saturate,
            prefilter,
            postfilter,
            mix,
            hpf,
            dc,
        }))
    },
};

/// A unit as a script moves it, from `k`, in [`rows`] order.
#[must_use]
pub fn model(k: Knobs) -> ir::KnobModel {
    let saved = [
        k.frequency,
        k.bits,
        k.jitter,
        k.crunch,
        k.dither,
        k.expand,
        k.saturate,
        k.prefilter,
        k.postfilter,
        k.mix,
        k.hpf,
        k.dc,
    ];
    MODULE.knob_model(&saved)
}

/// The KSP names of [`model`]'s knobs: the floats as set, HPF and DC_QUANT
/// by their index.
pub fn rows() -> [(&'static str, ir::KnobTarget, ir::Law); 12] {
    use ir::KnobTarget::Slot;
    use ir::SlotKnob::Knob;
    let unit = || ir::Law::Linear { lo: 0.0, hi: 1.0 };
    let index = || ir::Law::Linear {
        lo: 0.0,
        hi: 1_000_000.0,
    };
    [
        ("$ENGINE_PAR_BITE_FREQUENCY", Slot(Knob(0)), unit()),
        ("$ENGINE_PAR_BITE_BITS", Slot(Knob(1)), unit()),
        ("$ENGINE_PAR_BITE_JITTER", Slot(Knob(2)), unit()),
        ("$ENGINE_PAR_BITE_CRUNCH", Slot(Knob(3)), unit()),
        ("$ENGINE_PAR_BITE_DITHER", Slot(Knob(4)), unit()),
        ("$ENGINE_PAR_BITE_EXPAND", Slot(Knob(5)), unit()),
        ("$ENGINE_PAR_BITE_SATURATE", Slot(Knob(6)), unit()),
        ("$ENGINE_PAR_BITE_PREFILTER", Slot(Knob(7)), unit()),
        ("$ENGINE_PAR_BITE_POSTFILTER", Slot(Knob(8)), unit()),
        ("$ENGINE_PAR_BITE_MIX", Slot(Knob(9)), unit()),
        ("$ENGINE_PAR_BITE_HPF", Slot(Knob(10)), index()),
        ("$ENGINE_PAR_BITE_DC_QUANT", Slot(Knob(11)), index()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fresh_unit_samples_at_44k1_with_16_bits_and_filters_at_half() {
        let ir::EffectKind::Crush(ir::Crush {
            converter: Some(c), ..
        }) = bite(FRESH)
        else {
            panic!()
        };
        assert!((c.clock_hz / 44_100.0 - 1.0).abs() < 1e-3, "{}", c.clock_hz);
        assert!((c.pre_hz / 22_050.0 - 1.0).abs() < 1e-3, "{}", c.pre_hz);
        assert_eq!(
            (c.step, c.wet, c.dry, c.midrise),
            (2f32.powi(-15), 1.0, 0.0, false)
        );
        assert!((c.mu - 0.0718).abs() < 1e-3 && (c.trim - 1.0).abs() < 1e-6);
        let low = Knobs {
            frequency: 0.0,
            prefilter: 0.0,
            mix: 0.5,
            ..FRESH
        };
        let ir::EffectKind::Crush(ir::Crush {
            converter: Some(c), ..
        }) = bite(low)
        else {
            panic!()
        };
        assert!((c.clock_hz / 100.0 - 1.0).abs() < 1e-3 && (c.pre_hz / 50.0 - 1.0).abs() < 1e-2);
        assert!((c.wet - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_saved_record_reads_in_field_order() {
        let mut public = 2i32.to_le_bytes().to_vec();
        for i in 1..=10 {
            public.extend((i as f32 / 10.0).to_le_bytes());
        }
        public.push(0x5a);
        let k = saved(&public).unwrap();
        assert_eq!((k.hpf, k.frequency, k.mix, k.dc), (2.0, 0.1, 1.0, 1.0));
        public[0] = 3;
        assert_eq!(saved(&public), None);
    }
}
