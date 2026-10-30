//! The laws KSP's engine-parameter knobs go through.

use kontra_ir as ir;
use kontra_law::common::BIPOLAR;

/// What KSP's `$ENGINE_PAR_*` knobs set (value ÷ 1e6 is the knob x), each
/// through Kontakt's law from the knob to the physical value. Sources: the
/// binding tables in docs/re/kontakt-bindings (engine_par_clamp.tsv:
/// threshold −68..−3 dB, ratio knob 0.04^x so ratio 25^x), the effect laws
/// beside each translator (delays.rs, dynamics.rs, drive.rs), the LFO rate
/// measured on ANALOG STRINGS (0.010415·20465^x Hz, within 0.4% over 14
/// knob values), and the Multi weights ANALOG stores for the knobs its
/// script writes (2x − 1). The filter's cutoff and resonance and the
/// Compressor's attack and release depend on the slot's effect: their laws
/// are the slot's own (`Slot`), as are the legacy Delay's.
pub fn script_knobs() -> Vec<(&'static str, ir::KnobTarget, ir::Law)> {
    use ir::EffectParam as E;
    use ir::KnobTarget::{Effect, LfoRate, LfoSteps, LfoWeight, LfoWidth, Slot};
    let lin = |lo, hi| ir::Law::Linear { lo, hi };
    let mod_hz = ir::Law::Exp { lo: 0.05, hi: 8.0 };
    let mut rows = vec![
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
            "$ENGINE_PAR_RV_PREDELAY",
            Slot(ir::SlotKnob::ReverbPredelay),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_RV_SIZE",
            Slot(ir::SlotKnob::ReverbSize),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_RV_COLOUR",
            Slot(ir::SlotKnob::ReverbColour),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_RV_STEREO",
            Slot(ir::SlotKnob::ReverbStereo),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_RV_DAMPING",
            Slot(ir::SlotKnob::ReverbDamping),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_DL_TIME",
            Slot(ir::SlotKnob::DelayTime),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_DL_FEEDBACK",
            Slot(ir::SlotKnob::DelayFeedback),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_DL_PAN",
            Slot(ir::SlotKnob::DelayPan),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_DL_DAMPING",
            Slot(ir::SlotKnob::DelayDamping),
            lin(0.0, 1.0),
        ),
        // Replika (crate::replika).
        (
            "$ENGINE_PAR_RDL_TIME",
            Slot(ir::SlotKnob::DelayTime),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_RDL_FEEDBACK",
            Slot(ir::SlotKnob::DelayFeedback),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_RDL_LOWCUT",
            Slot(ir::SlotKnob::DelayLowCut),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_RDL_HIGHCUT",
            Slot(ir::SlotKnob::DelayHighCut),
            lin(0.0, 1.0),
        ),
        // The Limiter: input −24 + 48x dB, release its saved-ms law
        // (probes/fx_knob_laws.lua).
        (
            "$ENGINE_PAR_LIM_IN_GAIN",
            Effect(E::LimiterInput),
            lin(-24.0, 24.0),
        ),
        (
            "$ENGINE_PAR_LIM_RELEASE",
            Effect(E::LimiterRelease),
            ir::Law::Breakpoints(LIMITER_RELEASE_MS.into()),
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
        // Solid G-EQ: its slot carries the bands' laws (solid_knobs).
        (
            "$ENGINE_PAR_SEQ_LF_GAIN",
            Slot(ir::SlotKnob::BandGain(0)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_SEQ_LF_FREQ",
            Slot(ir::SlotKnob::BandHz(0)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_SEQ_LMF_GAIN",
            Slot(ir::SlotKnob::BandGain(1)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_SEQ_LMF_FREQ",
            Slot(ir::SlotKnob::BandHz(1)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_SEQ_LMF_Q",
            Slot(ir::SlotKnob::BandWidth(1)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_SEQ_HMF_GAIN",
            Slot(ir::SlotKnob::BandGain(2)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_SEQ_HMF_FREQ",
            Slot(ir::SlotKnob::BandHz(2)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_SEQ_HMF_Q",
            Slot(ir::SlotKnob::BandWidth(2)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_SEQ_HF_GAIN",
            Slot(ir::SlotKnob::BandGain(3)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_SEQ_HF_FREQ",
            Slot(ir::SlotKnob::BandHz(3)),
            lin(0.0, 1.0),
        ),
        // Solid Bus Comp: its slot carries the switches' laws (bus_knobs).
        (
            "$ENGINE_PAR_SCOMP_THRESHOLD",
            Slot(ir::SlotKnob::Threshold),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_SCOMP_RATIO",
            Slot(ir::SlotKnob::Ratio),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_SCOMP_ATTACK",
            Slot(ir::SlotKnob::Attack),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_SCOMP_RELEASE",
            Slot(ir::SlotKnob::Release),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_SCOMP_MAKEUP",
            Effect(E::CompressorMakeup),
            lin(-5.25, 21.0),
        ),
        (
            "$ENGINE_PAR_SCOMP_MIX",
            Effect(E::CompressorMix),
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
        ("$ENGINE_PAR_SHAPE", Effect(E::ShaperAmount), BIPOLAR),
        ("$ENGINE_PAR_DRIVE", Effect(E::ShaperAmount), lin(0.0, 1.0)),
        // The Distortion saves damping as set (fx_dynamics_dist_tape.tsv).
        (
            "$ENGINE_PAR_DAMPING",
            Effect(E::ShaperLowpassHz),
            ir::Law::Table(
                (0..=64u8)
                    .map(|n| crate::drive::damping_hz(f32::from(n) / 64.0))
                    .collect(),
            ),
        ),
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
        ("$ENGINE_PAR_LFO_SINE", LfoWeight(0), BIPOLAR),
        ("$ENGINE_PAR_LFO_SAW", LfoWeight(1), BIPOLAR),
        ("$ENGINE_PAR_LFO_TRI", LfoWeight(2), BIPOLAR),
        ("$ENGINE_PAR_LFO_RECT", LfoWeight(3), BIPOLAR),
        ("$ENGINE_PAR_LFO_RAND", LfoWeight(4), BIPOLAR),
        (
            "$ENGINE_PAR_FILTER_SHIFTB",
            Slot(ir::SlotKnob::StageShift(1)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_FILTER_SHIFTC",
            Slot(ir::SlotKnob::StageShift(2)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_FILTER_RESB",
            Slot(ir::SlotKnob::StageResonance(1)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_FILTER_RESC",
            Slot(ir::SlotKnob::StageResonance(2)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_FILTER_TYPEA",
            Slot(ir::SlotKnob::StageMorph(0)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_FILTER_TYPEB",
            Slot(ir::SlotKnob::StageMorph(1)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_FILTER_TYPEC",
            Slot(ir::SlotKnob::StageMorph(2)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_FILTER_BYPA",
            Slot(ir::SlotKnob::StageWet(0)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_FILTER_BYPB",
            Slot(ir::SlotKnob::StageWet(1)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_FILTER_BYPC",
            Slot(ir::SlotKnob::StageWet(2)),
            lin(0.0, 1.0),
        ),
        (
            "$ENGINE_PAR_FILTER_GAIN",
            Slot(ir::SlotKnob::OutputGain),
            lin(0.0, 1.0),
        ),
    ];
    rows.extend(crate::impulse::script_knobs());
    rows
}

/// One field a slot knob moves, and the law it moves it by.
pub(crate) type SlotKnob = (ir::SlotKnob, ir::EffectParam, ir::Law);

/// The cutoff and resonance knobs of filter `kind` through
/// [`crate::laws::filter`]. Cutoff moves only the Hz and resonance only the
/// dampings and gain there, so each knob is sampled with the others at their
/// saved values.
pub(crate) fn filter_knobs(kind: i32, cutoff: f32, resonance: f32, gain: f32) -> Vec<SlotKnob> {
    use ir::EffectParam::{FilterDamping, FilterGain, FilterHz, FilterLastDamping};
    let filter = |u, r| crate::laws::filter(kind, u, r, gain).map(ir::EffectKind::Filter);
    let mut out = sampled(ir::SlotKnob::Cutoff, &[FilterHz], &|u| filter(u, resonance));
    // The last stage by position, so the knob follows a switch of shape.
    let stages = crate::laws::filter(kind, cutoff, resonance, gain).map_or(0, |f| f.stages);
    let mut fields = vec![FilterLastDamping, FilterGain];
    fields.extend((0..3).filter(|&n| n + 1 != stages).map(FilterDamping));
    out.extend(sampled(ir::SlotKnob::Resonance, &fields, &|r| {
        filter(cutoff, r)
    }));
    out
}

/// Filter shape `kind`'s own cutoff and resonance laws: its Hz, and every
/// damping and the gain, kept even where constant, so a switch to it sets
/// them all. Built once for [`crate::laws::SWITCHABLE`]; slots share them.
pub(crate) fn shape_laws(kind: i32) -> std::sync::Arc<[SlotKnob]> {
    static LAWS: std::sync::LazyLock<Vec<(i32, std::sync::Arc<[SlotKnob]>)>> =
        std::sync::LazyLock::new(|| {
            let ids = crate::laws::SWITCHABLE.into_iter().flatten();
            ids.map(|k| (k, own_laws(k).into())).collect()
        });
    let found = LAWS.iter().find(|(k, _)| *k == kind);
    found.map_or_else(|| [].into(), |(_, laws)| laws.clone())
}

fn own_laws(kind: i32) -> Vec<SlotKnob> {
    use ir::EffectParam::{FilterDamping, FilterGain, FilterHz};
    use ir::SlotKnob::{Cutoff, Resonance};
    let rows = [
        (Cutoff, FilterHz),
        (Resonance, FilterDamping(0)),
        (Resonance, FilterDamping(1)),
        (Resonance, FilterDamping(2)),
        (Resonance, FilterGain),
    ];
    let table = |(knob, field): (ir::SlotKnob, ir::EffectParam)| {
        let at = |n: u8| {
            let x = f32::from(n) / 64.0;
            let (u, r) = if knob == Cutoff { (x, 0.5) } else { (0.5, x) };
            let f = crate::laws::filter(kind, u, r, 0.0)?;
            field.field_mut(&mut ir::EffectKind::Filter(f)).copied()
        };
        let values = (0..=64).map(at).collect::<Option<Vec<f32>>>()?;
        Some((knob, field, ir::Law::Table(values.into())))
    };
    rows.into_iter().filter_map(table).collect()
}

/// The Compressor's saved attack and release ms for a knob `x`, `lo + (hi −
/// lo)·(e^(kx) − 1)/(e^k − 1)`: fitted to the saves Kontakt writes for x = 0,
/// .1, .25, .5, .75, .9, 1 (probes/knob_save_laws.lua), within 0.01% / 0.02%;
/// the release k also gives the texts Kontakt shows (readouts.rs).
pub(crate) const COMP_ATTACK_MS: [(f32, f32, f32); 2] = [(0.0, 1.0, 5.811), (1.0, 1000.0, 0.0)];
pub(crate) const COMP_RELEASE_MS: [(f32, f32, f32); 2] = [(0.0, 50.0, 3.8734), (1.0, 2500.0, 0.0)];

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

/// Solid Bus Comp's knobs through [`crate::laws::bus_compressor`], the
/// others at their saved values. KSP saves a switch (ratio, attack,
/// release) at position `min(⌊6x⌋, 5)/6` (fx_dynamics_dist_tape.tsv), which
/// the compressor reads as `⌊5·saved + ½⌋`: measured natively, ratio
/// positions 3 and 4 both compress 4:1 and position 5 is 5:1. The ratio
/// also moves the threshold's offset.
pub(crate) fn bus_knobs(saved: [f32; 6], link: bool) -> Vec<SlotKnob> {
    use ir::EffectParam::{
        CompressorAttack, CompressorRatio, CompressorRelease, CompressorThreshold,
    };
    use ir::SlotKnob::{Attack, Ratio, Release, Threshold};
    let comp = |n: usize, x: f32| {
        let mut knobs = saved;
        *knobs.get_mut(n)? = x;
        let c = crate::laws::bus_compressor(knobs, link).0;
        Some(ir::EffectKind::Compressor(c))
    };
    let mut out = sampled(Threshold, &[CompressorThreshold], &|x| comp(0, x));
    out.extend(switched(Attack, &[CompressorAttack], &|x| comp(2, x)));
    out.extend(switched(Release, &[CompressorRelease], &|x| comp(3, x)));
    let ratio = [CompressorRatio, CompressorThreshold];
    out.extend(switched(Ratio, &ratio, &|x| comp(4, x)));
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

/// What `set_engine_par` of `$ENGINE_PAR_EFFECT_TYPE`, or of an
/// `EFFECT_SUBTYPE` no filter switches to, loads into a slot: Kontakt 8.13's
/// new units, read back from an empty insert slot in `on init` (ANALOG,
/// oracle probe fxdef): nothing for `$EFFECT_TYPE_NONE` (0); for
/// `$EFFECT_TYPE_FILTER` (7) an AR LP2 (100), cutoff and resonance 0; for
/// `$FILTER_TYPE_EQ3BAND` (24) bands at knobs .3656, .6201 and .858 (250 Hz,
/// 1.45 kHz, 7.5 kHz), gain .5 (0 dB), bandwidth .0625 (0.5 octaves); for
/// `$FILTER_TYPE_VERSATILE` (19) [`crate::versatile::DEFAULT`]; for
/// `$FILTER_TYPE_PHASER` (13) and Vowel A and B (14, 15) cutoff and
/// resonance 0 (Vowel's defaults unmeasured); for `$EFFECT_TYPE_REVERB2`
/// (39) Room, time, size, damping, modulation and diffusion .5, predelay,
/// high cut and low shelf 0, stereo 1, output and dry at unity (an insert's;
/// a send slot's dry starts at 0, the engine's rule); for
/// `$EFFECT_TYPE_SEND_LEVELS` (11) every level at unity (630000; oracle
/// probe rvdef, 2026-10-29); for `$EFFECT_TYPE_REPLIKA` (40) [`crate::replika::INIT`].
// ponytail: Kontakt carries the filter's cutoff and resonance into a
// subtype it switches to (oracle: 0.3 and 0.7 stay); a loaded subtype here
// starts at its defaults. The corpus scripts write both after loading.
pub(crate) fn slot_types() -> Vec<ir::SlotType> {
    let unit = |kind| ir::Effect {
        kind,
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    let filter = crate::laws::filter(100, 0.0, 0.0, 0.0).map(|f| unit(ir::EffectKind::Filter(f)));
    let saved = [0.3656, 0.6201, 0.858].map(|x| {
        let [hz, octaves, db] = &EQ_SAVED;
        [hz.eval(x), octaves.eval(0.0625), db.eval(0.5)]
    });
    let mut bands = [ir::Bell::FLAT; 4];
    for (band, &[hz, octaves, db]) in bands.iter_mut().zip(&saved) {
        *band = crate::laws::eq_band(hz, octaves, db).unwrap_or(ir::Bell::FLAT);
    }
    let ty = |id, subtype, reads, effect, laws| ir::SlotType {
        id,
        subtype,
        reads,
        effect,
        laws,
    };
    let eq = Some(unit(ir::EffectKind::Eq(bands)));
    let clocked = |kind| {
        let c = crate::clocked::clocked(kind, 0.0, 0.0)?;
        Some(unit(ir::EffectKind::Clocked(c)))
    };
    let versatile = Some(unit(ir::EffectKind::Cascade(crate::versatile::cascade(
        crate::versatile::DEFAULT,
    ))));
    let reverb = crate::reverb::reverb(0, [0.5, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 1.0]);
    let reverb = Some(ir::Effect {
        dry: 1.0,
        ..unit(ir::EffectKind::Reverb(reverb))
    });
    let sends = Some(unit(ir::EffectKind::Sends([1.0; 8])));
    let replika = crate::replika::delay(crate::replika::INIT).map(|d| ir::Effect {
        dry: 1.0,
        ..unit(ir::EffectKind::Delay(d))
    });
    // ponytail: an empty slot's subtype readback is unmeasured; 0, as for
    // the reverb and send levels.
    vec![
        ty(39, false, 0, reverb, [].into()),
        ty(11, false, 0, sends, [].into()),
        ty(
            40,
            false,
            0,
            replika,
            crate::replika::knobs(crate::replika::INIT).into(),
        ),
        ty(0, false, 0, None, [].into()),
        ty(7, false, 100, filter, shape_laws(100)),
        ty(24, true, 24, eq, eq_knobs(&saved).into()),
        ty(19, true, 19, versatile, crate::versatile::knobs().into()),
        ty(13, true, 13, clocked(13), crate::clocked::knobs().into()),
        ty(14, true, 14, clocked(14), crate::clocked::knobs().into()),
        ty(15, true, 15, clocked(15), crate::clocked::knobs().into()),
    ]
}

/// Saved ms for a knob `x`, `lo + (hi − lo)·(e^(kx) − 1)/(e^k − 1)`, fitted
/// to Kontakt's saves at x = 0, .1, .25, .5, .75, .9, 1 (probes/fx_knob_laws.lua):
/// the legacy Reverb's predelay 0..180 ms and the Limiter's release
/// 10..1000 ms, both within 0.01%; the release k also gives the texts
/// Kontakt shows (readouts.rs).
pub(crate) const REVERB_PREDELAY_MS: [(f32, f32, f32); 2] = [(0.0, 0.0, 4.511), (1.0, 180.0, 0.0)];
pub(crate) const LIMITER_RELEASE_MS: [(f32, f32, f32); 2] =
    [(0.0, 10.0, 4.4242), (1.0, 1000.0, 0.0)];

/// The legacy Reverb's knobs through [`crate::legacy_reverb::reverb`]: the
/// predelay knob through its saved-ms law, the others saved as set.
pub(crate) fn legacy_reverb_knobs((predelay_ms, saved): (f32, [f32; 4])) -> Vec<SlotKnob> {
    use ir::EffectParam as E;
    use ir::SlotKnob as K;
    let build = |p, k: [f32; 4]| Some(ir::EffectKind::Reverb(crate::legacy_reverb::reverb(p, k)));
    let predelay = ir::Law::Breakpoints(REVERB_PREDELAY_MS.into());
    let mut out = sampled(K::ReverbPredelay, &[E::ReverbPredelay], &|x| {
        build(predelay.eval(x), saved)
    });
    let fields = [
        E::ReverbSeconds,
        E::ReverbDamping,
        E::ReverbStereo,
        E::ReverbHighCut,
    ];
    for (n, knob) in [
        K::ReverbSize,
        K::ReverbColour,
        K::ReverbStereo,
        K::ReverbDamping,
    ]
    .into_iter()
    .enumerate()
    {
        out.extend(sampled(knob, &fields, &|x| {
            let mut k = saved;
            *k.get_mut(n)? = x;
            build(predelay_ms, k)
        }));
    }
    out
}

/// The legacy Delay's saved ms for a knob `x`, 5..2900 ms through the
/// curve above: fitted to Kontakt's saves at x = .1 .. .9
/// (docs/re/kontakt-bindings/fx_legacy_delay_reverb_mod.tsv), within 0.001%.
/// Feedback, pan and damping save as set.
const DELAY_MS: [(f32, f32, f32); 2] = [(0.0, 5.0, 6.027), (1.0, 2900.0, 0.0)];

/// The legacy Delay's time, feedback, pan and damping knobs through
/// [`crate::delays::delay`], the others held at their saved values.
/// ponytail: feedback and pan both scale the matrices, so each knob's table
/// holds the other at its saved value; build the matrices at run time if a
/// script moves both.
pub(crate) fn delay_knobs(saved: [f32; 5]) -> Vec<SlotKnob> {
    use ir::EffectParam as E;
    use ir::SlotKnob as K;
    let time = ir::Law::Breakpoints(DELAY_MS.into());
    let cells: Vec<_> = (0..4)
        .flat_map(|n| [E::DelayInput(n), E::DelayLoop(n)])
        .collect();
    let knobs = [
        (K::DelayTime, &[E::DelayMs][..]),
        (K::DelayFeedback, &cells[..]),
        (K::DelayPan, &cells[..]),
        (K::DelayDamping, &[E::DelayDampingHz][..]),
    ];
    let mut out = Vec::new();
    for (n, (knob, fields)) in knobs.into_iter().enumerate() {
        out.extend(sampled(knob, fields, &|x| {
            let mut k = saved;
            *k.get_mut(n)? = if n == 0 { time.eval(x) } else { x };
            let [ms, feedback, pan, damping, unit] = k;
            let d = crate::delays::delay(ms, feedback, pan, damping, unit);
            Some(ir::EffectKind::Delay(d))
        }));
    }
    out
}

/// Each of `fields` that moves in the effect `build` makes for a knob at 65
/// settings, as a table.
pub(crate) fn sampled(
    knob: ir::SlotKnob,
    fields: &[ir::EffectParam],
    build: &dyn Fn(f32) -> Option<ir::EffectKind>,
) -> Vec<SlotKnob> {
    const STEPS: u8 = 64;
    let at = (0..=STEPS).map(|n| f32::from(n) / f32::from(STEPS));
    moving(at, fields, build)
        .map(|(field, values)| (knob, field, ir::Law::Table(values.into())))
        .collect()
}

/// [`sampled`] for a six-position switch at `⌊6x⌋`, `build` given the
/// saved `k/6`: steps that jump at each k/6 (KSP's knob is in millionths).
fn switched(
    knob: ir::SlotKnob,
    fields: &[ir::EffectParam],
    build: &dyn Fn(f32) -> Option<ir::EffectKind>,
) -> Vec<SlotKnob> {
    let at = (0..6u8).map(|k| f32::from(k) / 6.0);
    let steps = |values: Vec<f32>| {
        let ends = values.iter().zip(0..6u8).flat_map(|(&v, k)| {
            let end = if k == 5 {
                1.0
            } else {
                f32::from(k + 1) / 6.0 - 2e-7
            };
            [(f32::from(k) / 6.0, v, 0.0), (end, v, 0.0)]
        });
        ir::Law::Breakpoints(ends.collect())
    };
    moving(at, fields, build)
        .map(|(field, values)| (knob, field, steps(values)))
        .collect()
}

/// Each of `fields` with its values in what `build` makes at `at`, if it
/// moves.
fn moving(
    at: impl Iterator<Item = f32>,
    fields: &[ir::EffectParam],
    build: &dyn Fn(f32) -> Option<ir::EffectKind>,
) -> impl Iterator<Item = (ir::EffectParam, Vec<f32>)> {
    let kinds = at
        .map(build)
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default();
    fields.iter().filter_map(move |&field| {
        let mut kinds = kinds.clone();
        let values = kinds.iter_mut().map(|k| field.field_mut(k).copied());
        let values = values.collect::<Option<Vec<f32>>>()?;
        let first = values.first()?;
        values.iter().any(|v| v != first).then_some((field, values))
    })
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
            let got = law(ir::EffectParam::FilterLastDamping).eval(0.711);
            assert!((got / k - 1.0).abs() < 2e-2, "{kind}: {got} vs {k}");
        }
    }

    /// Measured natively (KSP writes, a 1 kHz tone 0..−30 dB): ratio knob
    /// .55 and .7 compress 4:1, .9 compresses 5:1.
    #[test]
    fn the_bus_compressor_switches_step_where_kontakt_saves_them() {
        let saved = [0.3, 0.3, 1.0 / 6.0, 1.0 / 6.0, 1.0 / 6.0, 0.3];
        let knobs = bus_knobs(saved, true);
        let law = |knob, field| {
            let k = knobs.iter().find(|k| k.0 == knob && k.1 == field);
            k.map(|k| k.2.clone()).unwrap()
        };
        use ir::EffectParam::{CompressorAttack, CompressorRatio, CompressorThreshold};
        let ratio = law(ir::SlotKnob::Ratio, CompressorRatio);
        let got = [0.0, 0.2, 0.4, 0.55, 0.7, 0.9, 1.0].map(|x| ratio.eval(x));
        assert_eq!(got, [1.5, 2.0, 3.0, 4.0, 4.0, 5.0, 5.0]);
        // The threshold's offset follows the ratio: 4:1 is 1 dB above 3:1.
        let t = law(ir::SlotKnob::Ratio, CompressorThreshold);
        assert!((t.eval(0.55) - t.eval(0.4) - 1.0).abs() < 1e-4);
        // KSP 166666 is still the first position, 166667 the second.
        let attack = law(ir::SlotKnob::Attack, CompressorAttack);
        assert_eq!([attack.eval(0.166_666), attack.eval(0.166_667)], [0.1, 0.3]);
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

    #[test]
    fn the_delay_knobs_land_on_the_saves_kontakt_writes() {
        // ANALOG STRINGS' send delay: 250 ms, feedback .31459, pan 1, damping .5.
        let knobs = delay_knobs([250.0, 0.314_59, 1.0, 0.5, 0.0]);
        let law = |knob, field| {
            let row = knobs.iter().find(|(k, f, _)| *k == knob && *f == field);
            &row.unwrap().2
        };
        // (x, saved ms), fx_legacy_delay_reverb_mod.tsv; ANALOG writes 0.594553 for its 250.
        for (x, ms) in [
            (0.1, 10.789_63),
            (0.5, 140.527_2),
            (0.9, 1_586.318),
            (0.594_553, 250.0),
        ] {
            let got = law(ir::SlotKnob::DelayTime, ir::EffectParam::DelayMs).eval(x);
            assert!((got / ms - 1.0).abs() < 2e-3, "{x}: {got} vs {ms}");
        }
        let fed = law(ir::SlotKnob::DelayFeedback, ir::EffectParam::DelayInput(0)).eval(0.4);
        assert!((fed - 0.98 * 0.4).abs() < 1e-5, "{fed}");
        let crossed = law(ir::SlotKnob::DelayPan, ir::EffectParam::DelayLoop(1)).eval(0.25);
        assert!((crossed - 0.25 * 0.98 * 0.314_59).abs() < 1e-5, "{crossed}");
        assert!(
            delay_knobs([1.0, 0.5, 0.5, 0.5, 0.25])
                .iter()
                .all(|(k, _, _)| *k != ir::SlotKnob::DelayTime)
        );
    }

    #[test]
    fn a_loaded_reverb_and_send_levels_start_where_kontakt_puts_them() {
        let types = slot_types();
        let effect = |id: i32| {
            let found = types.iter().find(|t| t.id == id && !t.subtype);
            found.and_then(|t| t.effect)
        };
        let Some(ir::Effect {
            kind: ir::EffectKind::Reverb(r),
            dry,
            output,
            ..
        }) = effect(39)
        else {
            panic!("no reverb type");
        };
        // RV2_TIME .5: 0.5·e^(3.7·0.5) s; Room; stereo 1.
        assert!((r.seconds - 0.5 * 1.85f32.exp()).abs() < 1e-4);
        let [room, _] = crate::reverb_networks();
        assert_eq!(
            (r.network, r.stereo, dry, output),
            (Some(room), 1.0, 1.0, 1.0)
        );
        let sends = effect(11).map(|e| e.kind);
        assert_eq!(sends, Some(ir::EffectKind::Sends([1.0; 8])));
    }

    #[test]
    fn the_reverb_and_limiter_knobs_land_on_the_saves_kontakt_writes() {
        let knobs = legacy_reverb_knobs((10.0, [0.5; 4]));
        let law = |knob| &knobs.iter().find(|(k, _, _)| *k == knob).unwrap().2;
        // (x, saved predelay ms), probes/fx_knob_laws.lua.
        for (x, ms) in [(0.1, 1.140_032_3), (0.5, 17.078_783), (0.9, 113.922_356)] {
            let got = law(ir::SlotKnob::ReverbPredelay).eval(x);
            assert!((got / ms - 1.0).abs() < 3e-3, "{x}: {got} vs {ms}");
        }
        let size = crate::legacy_reverb::reverb(10.0, [0.25, 0.5, 0.5, 0.5]).seconds;
        assert!((law(ir::SlotKnob::ReverbSize).eval(0.25) / size - 1.0).abs() < 1e-3);
        let rows = script_knobs();
        let row = |name| &rows.iter().find(|(n, _, _)| *n == name).unwrap().2;
        for x in [0.0, 0.3, 0.754_65, 1.0] {
            let hz = crate::drive::damping_hz(x);
            assert!((row("$ENGINE_PAR_DAMPING").eval(x) / hz - 1.0).abs() < 2e-3);
        }
        for (x, db, ms) in [
            (0.1, -19.2, 16.678_839),
            (0.5, 0.0, 107.654_01),
            (0.9, 19.2, 641.722_96),
        ] {
            assert!((row("$ENGINE_PAR_LIM_IN_GAIN").eval(x) - db).abs() < 1e-3);
            let got = row("$ENGINE_PAR_LIM_RELEASE").eval(x);
            assert!((got / ms - 1.0).abs() < 1e-3, "{x}: {got} vs {ms}");
        }
    }
}
