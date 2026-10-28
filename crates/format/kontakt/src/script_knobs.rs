//! The laws KSP's engine-parameter knobs go through.

use kontra_ir as ir;

/// What KSP's `$ENGINE_PAR_*` knobs set (value ÷ 1e6 is the knob x), each
/// through Kontakt's law from the knob to the physical value. Sources: the
/// binding tables in docs/re/kontakt-bindings (engine_par_clamp.tsv:
/// threshold −68..−3 dB, ratio knob 0.04^x so ratio 25^x), the effect laws
/// beside each translator (delays.rs, dynamics.rs, drive.rs), the LFO rate
/// measured on ANALOG STRINGS (0.010415·20465^x Hz, within 0.4% over 14
/// knob values), and the Multi weights ANALOG stores for the knobs its
/// script writes (2x − 1). The filter's cutoff and resonance and the
/// Compressor's attack and release depend on the slot's effect: their laws
/// are the slot's own (`Slot`). The delay's composite knobs are not here.
pub fn script_knobs() -> Vec<(&'static str, ir::KnobTarget, ir::Law)> {
    use ir::EffectParam as E;
    use ir::KnobTarget::{Effect, LfoRate, LfoSteps, LfoWeight, LfoWidth, Slot};
    let lin = |lo, hi| ir::Law::Linear { lo, hi };
    let mod_hz = ir::Law::Exp { lo: 0.05, hi: 8.0 };
    vec![
        // The filter in the slot carries these knobs' laws (filter_knobs).
        (
            "$ENGINE_PAR_CUTOFF",
            Slot(ir::SlotKnob::Cutoff),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_RESONANCE",
            Slot(ir::SlotKnob::Resonance),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_FREQ1",
            Slot(ir::SlotKnob::BandHz(0)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_BW1",
            Slot(ir::SlotKnob::BandWidth(0)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_GAIN1",
            Slot(ir::SlotKnob::BandGain(0)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_FREQ2",
            Slot(ir::SlotKnob::BandHz(1)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_BW2",
            Slot(ir::SlotKnob::BandWidth(1)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_GAIN2",
            Slot(ir::SlotKnob::BandGain(1)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_FREQ3",
            Slot(ir::SlotKnob::BandHz(2)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_BW3",
            Slot(ir::SlotKnob::BandWidth(2)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_GAIN3",
            Slot(ir::SlotKnob::BandGain(2)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_COMP_ATTACK",
            Slot(ir::SlotKnob::Attack),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_COMP_DECAY",
            Slot(ir::SlotKnob::Release),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_FORMANT_TALK",
            Effect(E::FormantTalk),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_FORMANT_SHARP",
            Effect(E::FormantSharp),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_FORMANT_SIZE",
            Effect(E::FormantSize),
            lin(0.0, 1.0),
        ),
        ("$ENGINE_PAR_SHAPE", Effect(E::ShaperAmount), lin(-1.0, 1.0)),
        ("$ENGINE_PAR_DRIVE", Effect(E::ShaperAmount), lin(0.0, 1.0)),
        ("$ENGINE_PAR_BITS", Effect(E::CrushBits), lin(0.0, 16.0)),
        ("$ENGINE_PAR_FREQUENCY", Effect(E::CrushRate), lin(0.0, 1.0)),
        (
            "$ENGINE_PAR_THRESHOLD",
            Effect(E::CompressorThreshold),
            lin(-68.0, -3.0),
        ),
        (
            "$ENGINE_PAR_RATIO",
            Effect(E::CompressorRatio),
            ir::Law::Exp { lo: 1.0, hi: 25.0 },
        ),
        ("$ENGINE_PAR_PH_SPEED", Effect(E::ModRate), mod_hz.clone()),
        ("$ENGINE_PAR_CH_SPEED", Effect(E::ModRate), mod_hz.clone()),
        ("$ENGINE_PAR_FL_SPEED", Effect(E::ModRate), mod_hz),
        ("$ENGINE_PAR_PH_PHASE", Effect(E::ModPhase), lin(0.0, 0.25)),
        ("$ENGINE_PAR_CH_PHASE", Effect(E::ModPhase), lin(0.0, 0.25)),
        ("$ENGINE_PAR_FL_PHASE", Effect(E::ModPhase), lin(0.0, 0.25)),
        (
            "$ENGINE_PAR_PH_DEPTH",
            Effect(E::PhaserExponent),
            lin(0.0, 3.0),
        ),
        (
            "$ENGINE_PAR_PH_FEEDBACK",
            Effect(E::PhaserFeedback),
            lin(0.0, 0.85),
        ),
        (
            "$ENGINE_PAR_TR_ATTACK",
            Effect(E::TransientAttack),
            lin(-120.0, 120.0),
        ),
        (
            "$ENGINE_PAR_TR_SUSTAIN",
            Effect(E::TransientSustain),
            lin(-115.0, 115.0),
        ),
        (
            "$ENGINE_PAR_TR_GAIN",
            Effect(E::TransientGain),
            ir::Law::Breakpoints([(0.0, -24.0, 0.0), (0.5, 0.0, 0.0), (1.0, 6.0, 0.0)].into()),
        ),
        (
            "$ENGINE_PAR_INTMOD_FREQUENCY",
            LfoRate,
            ir::Law::Exp {
                lo: 0.010_415,
                hi: 0.010_415 * 20_465.0,
            },
        ),
        // A synced LFO's count, 48 over that Hz (probes/mod_par_laws.lua:
        // x 0, .1, .25, .5, .75, 1 store 4609, 1704, 385, 32, 3, 1).
        (
            "$ENGINE_PAR_INTMOD_FREQUENCY",
            LfoSteps,
            ir::Law::Exp {
                lo: 48.0 / 0.010_415,
                hi: 48.0 / (0.010_415 * 20_465.0),
            },
        ),
        // The saved width for x = 0, .1, .25, .5, .75, .9, 1 is 0.05, 0.14,
        // 0.275, 0.5, 0.725, 0.86, 0.95 (probes/knob_save_laws.lua).
        ("$ENGINE_PAR_INTMOD_PULSEWIDTH", LfoWidth, lin(0.05, 0.95)),
        // The Multi weights in the mixer's order: sine, saw, triangle, pulse, random.
        ("$ENGINE_PAR_LFO_SINE", LfoWeight(0), lin(-1.0, 1.0)),
        ("$ENGINE_PAR_LFO_SAW", LfoWeight(1), lin(-1.0, 1.0)),
        ("$ENGINE_PAR_LFO_TRI", LfoWeight(2), lin(-1.0, 1.0)),
        ("$ENGINE_PAR_LFO_RECT", LfoWeight(3), lin(-1.0, 1.0)),
        ("$ENGINE_PAR_LFO_RAND", LfoWeight(4), lin(-1.0, 1.0)),
    ]
}

/// One field a slot knob moves, and the law it moves it by.
pub(crate) type SlotKnob = (ir::SlotKnob, ir::EffectParam, ir::Law);

/// The cutoff and resonance knobs of filter `kind` through
/// [`crate::laws::filter`]. Cutoff moves only the Hz and resonance only the
/// dampings and gain there, so each knob is sampled with the others at their
/// saved values.
pub(crate) fn filter_knobs(kind: i32, cutoff: f32, resonance: f32, gain: f32) -> Vec<SlotKnob> {
    use ir::EffectParam::{FilterDamping, FilterGain, FilterHz};
    let filter = |u, r| crate::laws::filter(kind, u, r, gain).map(ir::EffectKind::Filter);
    let mut out = sampled(ir::SlotKnob::Cutoff, &[FilterHz], &|u| filter(u, resonance));
    let fields = [
        FilterDamping(0),
        FilterDamping(1),
        FilterDamping(2),
        FilterGain,
    ];
    out.extend(sampled(ir::SlotKnob::Resonance, &fields, &|r| {
        filter(cutoff, r)
    }));
    out
}

/// The Compressor's saved attack and release ms for a knob `x`, `lo + (hi −
/// lo)·(e^(kx) − 1)/(e^k − 1)`: fitted to the saves Kontakt writes for x = 0,
/// .1, .25, .5, .75, .9, 1 (probes/knob_save_laws.lua), within 0.01% / 0.02%.
const COMP_ATTACK_MS: [(f32, f32, f32); 2] = [(0.0, 1.0, 5.811), (1.0, 1000.0, 0.0)];
const COMP_RELEASE_MS: [(f32, f32, f32); 2] = [(0.0, 50.0, 3.874), (1.0, 2500.0, 0.0)];

/// The Compressor's attack and release knobs: the saved ms through
/// [`crate::laws::compressor`], whose law depends on the mode.
pub(crate) fn compressor_knobs(mode: i32, saved: [f32; 4], link: bool) -> Vec<SlotKnob> {
    use ir::EffectParam::{CompressorAttack, CompressorRelease};
    let [t, r, a, rel] = saved;
    let ms = |points: [(f32, f32, f32); 2], x| ir::Law::Breakpoints(points.into()).eval(x);
    let comp = |a, rel| {
        let c = crate::laws::compressor(mode, t, r, a, rel, link);
        Some(ir::EffectKind::Compressor(c))
    };
    let mut out = sampled(ir::SlotKnob::Attack, &[CompressorAttack], &|x| {
        comp(ms(COMP_ATTACK_MS, x), rel)
    });
    out.extend(sampled(ir::SlotKnob::Release, &[CompressorRelease], &|x| {
        comp(a, ms(COMP_RELEASE_MS, x))
    }));
    out
}

/// The 1/2/3-band EQ's saved Hz, octaves and dB for a knob `x`: 20·1000^x
/// Hz, 1/3 + 8x/3 octaves, −18 + 36x dB, exact at x = 0, .1, .25, .5, .75,
/// .9, 1 (probes/eq_knob_laws.lua).
const EQ_SAVED: [ir::Law; 3] = [
    ir::Law::Exp {
        lo: 20.0,
        hi: 20_000.0,
    },
    ir::Law::Linear {
        lo: 1.0 / 3.0,
        hi: 3.0,
    },
    ir::Law::Linear {
        lo: -18.0,
        hi: 18.0,
    },
];

/// Each band's frequency, bandwidth and gain knobs: the saved value through
/// [`crate::laws::eq_band`], the band's other values held.
pub(crate) fn eq_knobs(saved: &[[f32; 3]]) -> Vec<SlotKnob> {
    use ir::EffectParam::{BellGain, BellHz, BellQ};
    let mut out = Vec::new();
    for (n, band) in saved.iter().enumerate() {
        let Ok(b) = u8::try_from(n) else { break };
        let knobs = [
            (ir::SlotKnob::BandHz(b), BellHz(b)),
            (ir::SlotKnob::BandWidth(b), BellQ(b)),
            (ir::SlotKnob::BandGain(b), BellGain(b)),
        ];
        for ((knob, field), (k, law)) in knobs.into_iter().zip(EQ_SAVED.iter().enumerate()) {
            let eq = |x| {
                let mut v = *band;
                *v.get_mut(k)? = law.eval(x);
                let [hz, octaves, db] = v;
                let mut bands = [ir::Bell::FLAT; 4];
                *bands.get_mut(n)? = crate::laws::eq_band(hz, octaves, db)?;
                Some(ir::EffectKind::Eq(bands))
            };
            out.extend(sampled(knob, &[field], &eq));
        }
    }
    out
}

/// Each of `fields` that moves in the effect `build` makes for a knob at 65
/// settings, as a table.
fn sampled(
    knob: ir::SlotKnob,
    fields: &[ir::EffectParam],
    build: &dyn Fn(f32) -> Option<ir::EffectKind>,
) -> Vec<SlotKnob> {
    const STEPS: u8 = 64;
    let kinds = (0..=STEPS).map(|n| build(f32::from(n) / f32::from(STEPS)));
    let Some(mut kinds) = kinds.collect::<Option<Vec<_>>>() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for &field in fields {
        let values = kinds.iter_mut().map(|k| field.field_mut(k).copied());
        let Some(values) = values.collect::<Option<Vec<f32>>>() else {
            continue;
        };
        if values
            .first()
            .is_some_and(|f| values.iter().any(|v| v != f))
        {
            out.push((knob, field, ir::Law::Table(values.into())));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sampled_filter_knobs_land_on_the_filter_law_between_samples() {
        for kind in [6, 31, 52, 70, 101] {
            let knobs = filter_knobs(kind, 0.4, 0.3, 0.0);
            let law = |field| {
                let (_, _, l) = knobs.iter().find(|(_, f, _)| *f == field).unwrap();
                l.clone()
            };
            let hz = crate::laws::filter(kind, 0.637, 0.3, 0.0).unwrap().hz;
            let got = law(ir::EffectParam::FilterHz).eval(0.637);
            assert!((got / hz - 1.0).abs() < 2e-3, "{kind}: {got} vs {hz}");
            let k = crate::laws::filter(kind, 0.4, 0.711, 0.0).unwrap().damping[0];
            let got = law(ir::EffectParam::FilterDamping(0)).eval(0.711);
            assert!((got / k - 1.0).abs() < 2e-2, "{kind}: {got} vs {k}");
        }
    }

    #[test]
    fn the_compressor_knobs_land_on_the_saves_kontakt_writes() {
        // (x, saved attack ms, saved release ms), probes/knob_save_laws.lua.
        let saves = [
            (0.1, 3.364_088, 74.600_525),
            (0.5, 52.827, 358.699_37),
            (0.9, 558.393_4, 1_696.474_4),
        ];
        for mode in 0..3 {
            let knobs = compressor_knobs(mode, [-14.0, 0.5, 10.0, 100.0], false);
            let law = |knob| &knobs.iter().find(|(k, _, _)| *k == knob).unwrap().2;
            for (x, a, rel) in saves {
                let want = crate::laws::compressor(mode, -14.0, 0.5, a, 100.0, false).attack_ms;
                let got = law(ir::SlotKnob::Attack).eval(x);
                assert!(
                    (got / want - 1.0).abs() < 3e-3,
                    "{mode} {x}: {got} vs {want}"
                );
                let want = crate::laws::compressor(mode, -14.0, 0.5, 10.0, rel, false).release_ms;
                let got = law(ir::SlotKnob::Release).eval(x);
                assert!(
                    (got / want - 1.0).abs() < 3e-3,
                    "{mode} {x}: {got} vs {want}"
                );
            }
        }
    }

    #[test]
    fn the_eq_knobs_land_on_the_bands_kontakt_saves() {
        let knobs = eq_knobs(&[[1000.0, 1.0, 0.0], [100.0, 2.0, 6.0]]);
        let law = |knob| &knobs.iter().find(|(k, _, _)| *k == knob).unwrap().2;
        // (x, saved Hz, octaves, dB), probes/eq_knob_laws.lua.
        for (x, hz, octaves, db) in [
            (0.1, 39.905_247, 0.6, -14.4),
            (0.5, 632.455_5, 1.666_666_7, 0.0),
            (0.9, 10_023.743, 2.733_333_3, 14.4),
        ] {
            let want = crate::laws::eq_band(hz, 2.0, 6.0).unwrap().hz;
            let got = law(ir::SlotKnob::BandHz(1)).eval(x);
            assert!((got / want - 1.0).abs() < 3e-3, "{x}: {got} vs {want}");
            let want = crate::laws::eq_band(1000.0, octaves, 0.0).unwrap().q;
            let got = law(ir::SlotKnob::BandWidth(0)).eval(x);
            assert!((got / want - 1.0).abs() < 3e-3, "{x}: {got} vs {want}");
            let want = crate::laws::eq_band(1000.0, 1.0, db).unwrap().gain_db;
            let got = law(ir::SlotKnob::BandGain(0)).eval(x);
            assert!((got - want).abs() < 0.05, "{x}: {got} vs {want}");
        }
    }
}
