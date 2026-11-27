//! Ring Modulator (module `0x64`, saved v`0x50`, `$EFFECT_TYPE_RINGMOD` = 50)
//! onto [`ir::RingMod`] with [`ir::RingSource::Modulated`].

use kontra_ir as ir;

use crate::module::{Cursor, MixMode, f32_le, ids, sync_beats, sync_unit, unit};

crate::define_effect! {
    struct RingModulator,
    init: INIT,
    id: ids::RINGMOD,
    module_id: 0x64,
    versions: (0x50, 0x50),
    name: "Ring Modulator",
    script_type: 50,
    mix: MixMode::InPlace,
    build: |r| ir::EffectKind::RingMod(ringmod(r)),
    fields: [
        (fast_mode:  bool = false, "$ENGINE_PAR_RINGMOD_FAST_MODE"),
        (ring:       f32  = 0.5,   "$ENGINE_PAR_RINGMOD_RING"),
        (fm:         f32  = 0.0,   "$ENGINE_PAR_RINGMOD_FM"),
        (frequency:  f32  = 0.5,   "$ENGINE_PAR_RINGMOD_FREQUENCY"),
        (edge:       f32  = 0.0,   "$ENGINE_PAR_RINGMOD_EDGE"),
        (lfo_wave:   i32  = 0,     "$ENGINE_PAR_RINGMOD_LFO_WAVE"),
        (lfo_rate:   f32  = 0.5,   "$ENGINE_PAR_RINGMOD_LFO_RATE"),
        (lfo_amount: f32  = 0.0,   "$ENGINE_PAR_RINGMOD_LFO_AMOUNT"),
        (lfo_unit:   i32  = 0,     "$ENGINE_PAR_RINGMOD_LFO_RATE_UNIT"),
        (sync_beats: f32  = -1.0),
    ]
}

#[must_use]
pub fn saved(version: u16, public: &[u8]) -> Option<RingModulator> {
    if version != 0x50 {
        return None;
    }
    let mut cur = Cursor::new(public);
    let fast_mode = cur.bool()?;
    let ring = cur.f32()?;
    let fm = cur.f32()?;
    let frequency = cur.f32()?;
    let edge = cur.f32()?;
    let lfo_wave = cur.i32()?;
    let lfo_rate = cur.f32()?;
    let lfo_amount = cur.f32()?;
    let sync = f32_le(public, 29).unwrap_or(-1.0);
    Some(RingModulator {
        fast_mode,
        ring,
        fm,
        frequency,
        edge,
        lfo_wave,
        lfo_rate,
        lfo_amount,
        lfo_unit: sync_unit(sync),
        sync_beats: if sync > 1e-9 { sync } else { -1.0 },
    })
}

/// Physical IR settings for `r`.
#[must_use]
pub fn ringmod(r: RingModulator) -> ir::RingMod {
    let ring = unit(r.ring);
    let fm = unit(r.fm);
    let freq = unit(r.frequency);
    let edge = unit(r.edge);
    let lfo_rate = unit(r.lfo_rate);
    let amt = unit(r.lfo_amount);

    let mult = if r.fast_mode { 50.0 } else { 1.0 };
    let raw_hz = 0.625 * (7.0 * freq).exp2() * mult;

    let a2 = amt * amt;
    let num = (0.1476 * a2 + 0.5342) * a2 + 1.0;
    let den = (0.1288 * a2 - 0.5342) * a2 + 1.0;
    let ratio = num / den.max(1e-4);
    let base_hz = raw_hz * den.max(1e-4);
    let lfo_depth = (2.0 * (ratio - 1.0)).max(0.0);

    let synced_beats =
        sync_beats(r.lfo_unit).or_else(|| (r.sync_beats > 1e-9).then_some(r.sync_beats));
    let rate = match synced_beats {
        Some(beats) => ir::Rate::Beats(beats),
        None => ir::Rate::Hz(0.031_25 * (10.0 * lfo_rate).exp2()),
    };

    ir::RingMod {
        hz: base_hz,
        spread: 0.0,
        bias: 0.0,
        quadrature: 0.0,
        drive: 1.0,
        highpass_hz: 0.0,
        gate: false,
        feedback: 0.0,
        source: ir::RingSource::Modulated {
            edge,
            fm_ms: 1.28 * fm * fm,
            lfo_rate: rate,
            lfo_depth,
            lfo_square: r.lfo_wave != 0,
        },
        wet: 3.2 * ring,
        dry: 1.0 - 2.6 * ring,
    }
}
