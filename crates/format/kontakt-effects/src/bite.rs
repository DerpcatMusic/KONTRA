//! Bite (module `0x66`) onto the engine's reducer as a converter pair
//! ([`ir::Converter`]).

use kontra_ir as ir;
use kontra_law::kontakt::PITCH_INDEX;

use crate::module::{Cursor, MixMode, RAW_LAW, bool_f, ids, unit};

const DB: f32 = 9.965_784;

fn index(i: f32) -> f32 {
    PITCH_INDEX.eval_unclamped(i)
}

crate::define_effect! {
    struct Knobs,
    init: FRESH,
    id: ids::BITE,
    module_id: 0x66,
    versions: (0, u32::MAX),
    name: "Bite",
    script_type: 52,
    mix: MixMode::OwnMix,
    build: bite,
    fields: [
        (frequency:  f32 = 1.0, "$ENGINE_PAR_BITE_FREQUENCY"),
        (bits:       f32 = 1.0, "$ENGINE_PAR_BITE_BITS"),
        (jitter:     f32 = 0.0, "$ENGINE_PAR_BITE_JITTER"),
        (crunch:     f32 = 0.0, "$ENGINE_PAR_BITE_CRUNCH"),
        (dither:     f32 = 0.0, "$ENGINE_PAR_BITE_DITHER"),
        (expand:     f32 = 1.0, "$ENGINE_PAR_BITE_EXPAND"),
        (saturate:   f32 = 0.0, "$ENGINE_PAR_BITE_SATURATE"),
        (prefilter:  f32 = 0.5, "$ENGINE_PAR_BITE_PREFILTER"),
        (postfilter: f32 = 0.5, "$ENGINE_PAR_BITE_POSTFILTER"),
        (mix:        f32 = 1.0, "$ENGINE_PAR_BITE_MIX"),
        (hpf:        f32 = 0.0, "$ENGINE_PAR_BITE_HPF", RAW_LAW),
        (dc:         f32 = 0.0, "$ENGINE_PAR_BITE_DC_QUANT", RAW_LAW),
    ]
}

/// The reducer with its converter at `k`.
#[must_use]
pub fn bite(k: Knobs) -> ir::EffectKind {
    let frequency = unit(k.frequency);
    let pitch = frequency * 105.415_62 + 43.349_957;
    let levels = (2 + (unit(k.bits) * 15.0) as i32).clamp(2, 16);
    let step = ((1 - levels) as f32).exp2();
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

/// Decodes a saved Bite record: HPF `i32`, 10 floats, DC_QUANT `u8`.
#[must_use]
pub fn saved(public: &[u8]) -> Option<Knobs> {
    let mut cur = Cursor::new(public);
    let hpf = cur.i32()?;
    if !(0..=2).contains(&hpf) {
        return None;
    }
    Some(Knobs {
        hpf: hpf as f32,
        frequency: cur.f32()?,
        bits: cur.f32()?,
        jitter: cur.f32()?,
        crunch: cur.f32()?,
        dither: cur.f32()?,
        expand: cur.f32()?,
        saturate: cur.f32()?,
        prefilter: cur.f32()?,
        postfilter: cur.f32()?,
        mix: cur.f32()?,
        dc: bool_f(cur.bool()?),
    })
}
