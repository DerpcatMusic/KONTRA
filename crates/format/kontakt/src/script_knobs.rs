//! The laws KSP's engine-parameter knobs go through.

use kontra_ir as ir;

/// What KSP's `$ENGINE_PAR_*` knobs set (value ÷ 1e6 is the knob x), each
/// through Kontakt's law from the knob to the physical value. Sources: the
/// binding tables in docs/re/kontakt-bindings (engine_par_clamp.tsv:
/// threshold −68..−3 dB, ratio knob 0.04^x so ratio 25^x), the effect laws
/// beside each translator (delays.rs, dynamics.rs, drive.rs), the LFO rate
/// measured on ANALOG STRINGS (0.010415·20465^x Hz, within 0.4% over 14
/// knob values), and the Multi weights ANALOG stores for the knobs its
/// script writes (2x − 1). The Compressor's attack and release, its Pro
/// make-up and the delay's composite knobs depend on more than the knob;
/// they are not here.
pub fn script_knobs() -> Vec<(&'static str, ir::KnobTarget, ir::Law)> {
    use ir::EffectParam as E;
    use ir::KnobTarget::{Effect, LfoRate, LfoWeight, Slot};
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

/// The cutoff and resonance knobs of filter `kind`, sampled from
/// [`crate::laws::filter`] at 65 settings each. Cutoff moves only the Hz and
/// resonance only the dampings and gain there, so each knob is sampled with
/// the others at their saved values.
pub(crate) fn filter_knobs(kind: i32, cutoff: f32, resonance: f32, gain: f32) -> Vec<SlotKnob> {
    const STEPS: u8 = 64;
    let table = |at: &dyn Fn(f32) -> Option<f32>| {
        let points = (0..=STEPS).map(|n| at(f32::from(n) / f32::from(STEPS)));
        points
            .collect::<Option<Vec<f32>>>()
            .map(|v| ir::Law::Table(v.into()))
    };
    let mut out = Vec::new();
    let hz = table(&|u| Some(crate::laws::filter(kind, u, resonance, gain)?.hz));
    out.extend(hz.map(|law| (ir::SlotKnob::Cutoff, ir::EffectParam::FilterHz, law)));
    let stages = crate::laws::filter(kind, cutoff, resonance, gain).map_or(0, |f| f.stages);
    for n in 0..stages.min(3) {
        let at = |r| {
            crate::laws::filter(kind, cutoff, r, gain)?
                .damping
                .get(usize::from(n))
                .copied()
        };
        let law = table(&at);
        out.extend(law.map(|law| {
            (
                ir::SlotKnob::Resonance,
                ir::EffectParam::FilterDamping(n),
                law,
            )
        }));
    }
    let law = table(&|r| Some(crate::laws::filter(kind, cutoff, r, gain)?.gain));
    out.extend(law.map(|law| (ir::SlotKnob::Resonance, ir::EffectParam::FilterGain, law)));
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
}
