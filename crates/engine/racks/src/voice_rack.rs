//! A group's inserts that run on each voice alone. A group insert acts on
//! each voice; a linear one with fixed settings commutes with the sum,
//! so it stays on the shared chain (filters.rs). An insert runs here when a
//! route moves it (it differs per note), when it sits at or before the
//! group's last drive effect (a nonlinear stage sees one voice, not the
//! sum), or when it is a gain/stereo/matrix slot whose position relative to
//! a drive effect prevents commuting it into the voice's amp matrix. A
//! bypassed insert is built too and skipped while bypassed, so a script may
//! switch it on. Each group's units are built once, after the init callback;
//! a note copies them into its slot's preallocated rack, so the audio thread
//! never allocates, and their state ends with the voice.

use std::sync::Arc;

use kontra_dsp::delay::{Comb, line_frames};
use kontra_dsp::drive::{ColorSat, Core, Crush, Outer, Overdrive};
use kontra_dsp::dynamics::Compressor;
use kontra_dsp::filter::{Clocked, ClockedShape, Formant, SpectralMorph, Svf, Wah};
use kontra_effects::processors::{
    color_sat, compressor, reduction, shaper, spectral_morph, voicing, wah,
};
use kontra_effects::summed::{VoiceEq, shape, vowels};
use kontra_ir as ir;
use kontra_plan::route_depth;

/// ponytail: the lowest damping (`1/Q`) a resonance route may reach; the
/// stages go unstable below it.
pub const MIN_DAMPING: f32 = 0.01;

/// Frames between a drive unit's steps toward its next refresh: Kontakt's
/// modulated Saturation and Lo-Fi re-read their controls every 32 frames. Held for the whole 256-frame refresh, a
/// 5 Hz LFO on Saturation differs from that by −24 dB RMS (−12.6 dB at
/// 20 Hz); stepping linearly between refreshes costs no extra spans.
///
/// ponytail: Kontakt also smooths Saturation's tick values with a one-pole
/// (coefficient at +0x1d4, not read); add it if a native render shows it.
const TICK: u32 = 32;

/// One insert of a voice's rack, and the group insert slot it runs.
#[derive(Clone)]
pub struct Unit {
    pub insert: u8,
    pub kind: Kind,
    /// A shaper's base-rate filters; its oversampled core is the kind (the
    /// two together outgrow the other kinds).
    pub outer: Outer,
    /// Which of the voice's delay lines a comb runs on ([`lines`]).
    pub line: usize,
    /// A drive unit's settings now and at the next refresh, `left` frames
    /// away; it steps between them every [`TICK`] frames.
    pub ramp: Option<Ramp>,
    /// The effect settings the unit last took, when no ramp moved it since:
    /// the same again changes nothing, so a refresh skips it.
    pub tuned: Option<ir::EffectKind>,
}

#[derive(Clone)]
pub struct Ramp {
    pub from: ir::EffectKind,
    pub to: ir::EffectKind,
    pub at: u32,
    pub len: u32,
}

#[derive(Clone)]
pub enum Kind {
    Filter(Svf),
    Clocked(Clocked),
    Eq(VoiceEq),
    Formant(Formant),
    Wah(Wah),
    SpectralMorph(SpectralMorph),
    Shaper(Core),
    /// A compressor into a shaper's core ([`ir::Strip`]).
    Strip(Compressor, Core),
    Crush(Crush),
    Overdrive(Overdrive),
    ColorSat(ColorSat),
    Comb(Comb),
    /// A stateless 2×2 gain/pan/stereo/invert slot that cannot commute past a
    /// drive effect into the voice's amp matrix; its matrix is read live from
    /// `inserts[u.insert]` on each chunk.
    Matrix,
}

fn is_drive(kind: &ir::EffectKind) -> bool {
    matches!(
        kind,
        ir::EffectKind::Shaper(_)
            | ir::EffectKind::Strip(_)
            | ir::EffectKind::Crush(_)
            | ir::EffectKind::Overdrive(_)
            | ir::EffectKind::ColorSat(_)
            | ir::EffectKind::Comb(_)
    )
}

fn is_matrix_effect(kind: &ir::EffectKind) -> bool {
    kontra_effects::matrix::utility_matrix(kind).is_some()
}

/// The unit for an insert, at its saved settings, and a shaper's base-rate
/// filters.
fn unit(kind: &ir::EffectKind, rate: u32) -> Option<(Kind, Outer)> {
    if let ir::EffectKind::Shaper(s) = kind {
        let s = shaper(s, rate);
        return Some((Kind::Shaper(s.core), s.outer));
    }
    if let ir::EffectKind::Strip(s) = kind {
        let c = Compressor::new(compressor(s.compressor), rate);
        let s = shaper(&s.shaper, rate);
        return Some((Kind::Strip(c, s.core), s.outer));
    }
    let kind = match kind {
        ir::EffectKind::Filter(f) => {
            let mut svf = Svf::new(shape(f));
            svf.set_cutoff(f.hz, rate);
            Kind::Filter(svf)
        }
        ir::EffectKind::Clocked(c) => {
            let shape = match c.shape {
                ir::ClockedShape::Allpass => ClockedShape::Allpass,
                ir::ClockedShape::VowelLow => ClockedShape::VowelLow,
                ir::ClockedShape::VowelHigh => ClockedShape::VowelHigh,
            };
            Kind::Clocked(Clocked::new(shape, [c.cutoff, c.resonance], rate))
        }
        ir::EffectKind::Eq(bells) => Kind::Eq(VoiceEq::new(*bells, rate)),
        ir::EffectKind::EqPass {
            bands,
            hp_hz,
            lp_hz,
        } => Kind::Eq(VoiceEq::with_pass(*bands, *hp_hz, *lp_hz, rate)),
        ir::EffectKind::Formant(f) => Kind::Formant(Formant::new(vowels(*f), rate)),
        ir::EffectKind::Wah(w) => Kind::Wah(Wah::new(wah(*w), rate)),
        ir::EffectKind::SpectralMorph(m) => {
            Kind::SpectralMorph(SpectralMorph::new(spectral_morph(*m), rate))
        }
        ir::EffectKind::Crush(c) => Kind::Crush(Crush::new(reduction(*c, rate), 1)),
        ir::EffectKind::Overdrive(o) => Kind::Overdrive(Overdrive::new(voicing(o), rate)),
        ir::EffectKind::ColorSat(c) => Kind::ColorSat(ColorSat::new(color_sat(*c), rate)),
        ir::EffectKind::Comb(c) => Kind::Comb(Comb::new(c.hz, c.feedback, c.forward, rate)),
        _ => return None,
    };
    Some((kind, Outer::empty()))
}

/// A group's per-voice units in slot order; empty when all of it is shared.
pub fn template(group: &kontra_plan::Group, rate: u32) -> Arc<[Unit]> {
    let split = group.amp_split_index();
    let last_drive = group
        .inserts
        .iter()
        .rposition(|e| e.as_ref().is_some_and(|e| is_drive(&e.kind)));
    let last_pre_drive = group
        .inserts
        .get(..split)
        .unwrap_or_default()
        .iter()
        .rposition(|e| e.as_ref().is_some_and(|e| is_drive(&e.kind)));
    let last_post_drive = group
        .inserts
        .get(split..)
        .unwrap_or_default()
        .iter()
        .rposition(|e| e.as_ref().is_some_and(|e| is_drive(&e.kind)))
        .map(|rel| split + rel);
    let mut combs = 0;
    group
        .inserts
        .iter()
        .enumerate()
        .filter_map(|(n, e)| {
            let e = e.as_ref()?;
            if is_matrix_effect(&e.kind) {
                let keep_in_rack = (n < split && last_pre_drive.is_some_and(|d| n < d))
                    || (n >= split && last_post_drive.is_some());
                if !keep_in_rack {
                    return None;
                }
                return Some(Unit {
                    insert: u8::try_from(n).ok()?,
                    kind: Kind::Matrix,
                    outer: Outer::empty(),
                    line: combs,
                    ramp: None,
                    tuned: None,
                });
            }
            if !(last_drive.is_some_and(|l| n <= l) || route_depth::moved(&group.mods, n)) {
                return None;
            }
            let (kind, outer) = unit(&e.kind, rate)?;
            let line = combs;
            combs += usize::from(matches!(kind, Kind::Comb(_)));
            Some(Unit {
                insert: u8::try_from(n).ok()?,
                kind,
                outer,
                line,
                ramp: None,
                tuned: None,
            })
        })
        .collect()
}

/// Frames of delay line each comb of a voice needs.
#[must_use]
pub fn comb_line(rate: u32) -> usize {
    line_frames(rate, ir::COMB_LOWEST_HZ)
}

/// How many combs a template has.
#[must_use]
pub fn combs(units: &[Unit]) -> usize {
    units
        .iter()
        .filter(|u| matches!(u.kind, Kind::Comb(_)))
        .count()
}

/// Starts a voice's combs: their positions and delay lines cleared.
pub fn start(rack: &mut [Unit], lines: &mut [[f32; 2]]) {
    lines.fill([0.0; 2]);
    for u in rack {
        if let Kind::Comb(c) = &mut u.kind {
            c.reset();
        }
    }
}

/// Runs a voice's rack on `io` (before its envelope and gain), skipping
/// inserts a script has bypassed; a drive effect's output and dry levels
/// follow it, `spare` holding the dry (filters' fold into the matrix).
pub fn process(
    rack: &mut [Unit],
    inserts: &[Option<ir::Effect>],
    io: &mut [[f32; 2]],
    spare: &mut [[f32; 2]],
    lines: &mut [[f32; 2]],
    rate: u32,
) {
    let each = comb_line(rate);
    for u in rack {
        let Some(e) = inserts.get(usize::from(u.insert)).and_then(Option::as_ref) else {
            continue;
        };
        if e.bypass {
            continue;
        }
        let mut rest = &mut io[..];
        while !rest.is_empty() {
            let n = match &u.ramp {
                Some(r) if r.at < r.len => rest.len().min((TICK - r.at % TICK) as usize),
                _ => rest.len(),
            };
            let (chunk, tail) = rest.split_at_mut(n);
            match &mut u.kind {
                Kind::Filter(f) => f.process(chunk),
                Kind::Clocked(c) => c.process(chunk),
                Kind::Eq(q) => q.process(chunk),
                Kind::Formant(f) => f.process(chunk),
                Kind::Wah(w) => drive(chunk, spare, e, |io| w.process(io)),
                Kind::SpectralMorph(m) => drive(chunk, spare, e, |io| m.process(io)),
                Kind::Shaper(s) => drive(chunk, spare, e, |io| {
                    u.outer.before(io);
                    s.process(io);
                    u.outer.after(io);
                }),
                Kind::Strip(c, s) => drive(chunk, spare, e, |io| {
                    c.process(io);
                    u.outer.before(io);
                    s.process(io);
                    u.outer.after(io);
                }),
                Kind::Crush(c) => drive(chunk, spare, e, |io| c.process(io)),
                Kind::Overdrive(o) => drive(chunk, spare, e, |io| o.process(io)),
                Kind::ColorSat(c) => drive(chunk, spare, e, |io| c.process(io)),
                Kind::Comb(c) => {
                    let line = lines.get_mut(u.line * each..(u.line + 1) * each);
                    drive(chunk, spare, e, |io| {
                        if let Some(line) = line {
                            c.process(io, line);
                        }
                    });
                }
                Kind::Matrix => {
                    let [[ll, lr], [rl, rr]] = kontra_effects::matrix::matrix(e);
                    for [l, r] in chunk.iter_mut() {
                        let (in_l, in_r) = (*l, *r);
                        *l = ll * in_l + lr * in_r;
                        *r = rl * in_l + rr * in_r;
                    }
                }
            }
            if let Some(r) = &mut u.ramp
                && r.at < r.len
            {
                r.at += n as u32;
                if r.at.is_multiple_of(TICK) || r.at >= r.len {
                    let k =
                        route_depth::lerp(&r.from, &r.to, (r.at as f32 / r.len as f32).min(1.0));
                    tune(&mut u.kind, &mut u.outer, Tune::Effect(&k), rate);
                }
            }
            rest = tail;
        }
    }
}

fn drive(
    io: &mut [[f32; 2]],
    spare: &mut [[f32; 2]],
    e: &ir::Effect,
    f: impl FnOnce(&mut [[f32; 2]]),
) {
    let mut dry = spare.get_mut(..io.len()).filter(|_| e.dry != 0.0);
    if let Some(dry) = &mut dry {
        dry.copy_from_slice(io);
    }
    f(io);
    if e.output != 1.0 {
        for x in io.iter_mut().flatten() {
            *x *= e.output;
        }
    }
    if let Some(dry) = dry {
        for (x, d) in io.iter_mut().flatten().zip(dry.iter().flatten()) {
            *x += d * e.dry;
        }
    }
}

pub fn tune_unit(u: &mut Unit, t: Tune<'_>, ramp: Option<Ramp>, rate: u32) {
    let unchanged = match (&u.tuned, &t) {
        (Some(prev), Tune::Effect(e)) => ramp.is_none() && u.ramp.is_none() && prev == *e,
        _ => false,
    };
    u.ramp = ramp;
    if unchanged {
        return;
    }
    u.tuned = match &t {
        Tune::Effect(e) if u.ramp.is_none() => Some((*e).clone()),
        _ => None,
    };
    tune(&mut u.kind, &mut u.outer, t, rate);
}

fn tune(kind: &mut Kind, outer: &mut Outer, t: Tune<'_>, rate: u32) {
    match (kind, t) {
        (Kind::Filter(f), Tune::Filter(hz, k)) => {
            f.set_cutoff(hz, rate);
            f.set_damping(k);
        }
        (Kind::Clocked(c), Tune::Clocked(knobs)) => c.set(knobs),
        (Kind::Eq(q), Tune::Eq(bells)) => q.retune(bells, rate),
        (Kind::Eq(q), Tune::EqPass(bells, hp_hz, lp_hz)) => {
            q.retune_pass(bells, hp_hz, lp_hz, rate);
        }
        (
            Kind::Eq(q),
            Tune::Effect(ir::EffectKind::EqPass {
                bands,
                hp_hz,
                lp_hz,
            }),
        ) => q.retune_pass(*bands, *hp_hz, *lp_hz, rate),
        (Kind::Comb(c), Tune::Comb(hz, feedback)) => c.set(hz, feedback, rate),
        (Kind::Formant(f), Tune::Effect(ir::EffectKind::Formant(v))) => f.set(vowels(*v), rate),
        (Kind::Wah(w), Tune::Effect(ir::EffectKind::Wah(v))) => w.set(wah(*v)),
        (Kind::SpectralMorph(m), Tune::Effect(ir::EffectKind::SpectralMorph(v))) => {
            m.set(spectral_morph(*v));
        }
        (Kind::Shaper(s), Tune::Effect(ir::EffectKind::Shaper(v))) => {
            let new = shaper(v, rate);
            s.retune(&new.core);
            outer.retune(&new.outer);
        }
        // Coefficients only: a script moving COMPRESS allocates nothing.
        (Kind::Strip(c, s), Tune::Effect(ir::EffectKind::Strip(v))) => {
            c.set(compressor(v.compressor), rate);
            let new = shaper(&v.shaper, rate);
            s.retune(&new.core);
            outer.retune(&new.outer);
        }
        (Kind::Crush(c), Tune::Effect(ir::EffectKind::Crush(v))) => c.set(reduction(*v, rate)),
        (Kind::Overdrive(o), Tune::Effect(ir::EffectKind::Overdrive(v))) => o.set(voicing(v), rate),
        (Kind::ColorSat(c), Tune::Effect(ir::EffectKind::ColorSat(v))) => c.set(color_sat(*v)),
        _ => {}
    }
}

/// A moved unit's new settings.
pub enum Tune<'a> {
    Filter(f32, [f32; 3]),
    Clocked([f32; 2]),
    Eq([ir::Bell; 4]),
    EqPass([ir::Bell; 4], Option<f32>, Option<f32>),
    /// A comb's pitch in Hz and feedback.
    Comb(f32, f32),
    /// Borrowed: an effect's settings outweigh the other moves.
    Effect(&'a ir::EffectKind),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(threshold_db: f32) -> ir::EffectKind {
        ir::EffectKind::Strip(ir::Strip {
            compressor: ir::Compressor {
                threshold_db,
                ratio: 8.0,
                knee_db: 0.0,
                attack_ms: 1.0,
                release_ms: 50.0,
                makeup_db: 0.0,
                makeup_slope: 0.0,
                link: true,
                mix: 1.0,
                detector: ir::Detector::Input,
                sidechain_hz: None,
            },
            shaper: ir::Shaper {
                pre_lowpass_hz: None,
                curve: ir::DriveCurve::Soft(0.0),
                lowpass_hz: None,
                highpass_hz: None,
                sections: ir::Sections::NONE,
            },
        })
    }

    /// A script moving a group rack's strip on a sounding voice (COMPRESS
    /// up: the threshold down) retunes the voice's own unit.
    #[test]
    fn a_sounding_voice_takes_its_strips_new_settings() {
        let rate = 48_000;
        let open = strip(0.0);
        let Some((kind, outer)) = unit(&open, rate) else {
            panic!("no strip unit");
        };
        let mut rack = [Unit {
            insert: 0,
            kind,
            outer,
            line: 0,
            ramp: None,
            tuned: None,
        }];
        let mut inserts = [Some(ir::Effect {
            kind: open,
            bypass: false,
            output: 1.0,
            dry: 0.0,
        })];
        let mut spare = vec![[0.0; 2]; 256];
        let mut peak = |rack: &mut [Unit], inserts: &[Option<ir::Effect>]| {
            let mut top = 0.0f32;
            for block in 0..100 {
                let mut io: Vec<[f32; 2]> = (0..256)
                    .map(|i| {
                        let t = (block * 256 + i) as f32 / rate as f32;
                        [0.5 * (std::f32::consts::TAU * 1000.0 * t).sin(); 2]
                    })
                    .collect();
                process(rack, inserts, &mut io, &mut spare, &mut [], rate);
                if block > 80 {
                    top = io.iter().fold(top, |p, f| p.max(f[0].abs()));
                }
            }
            top
        };
        let before = peak(&mut rack, &inserts);
        let squeezed = strip(-30.0);
        inserts[0] = Some(ir::Effect {
            kind: squeezed.clone(),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        });
        tune_unit(&mut rack[0], Tune::Effect(&squeezed), None, rate);
        let after = peak(&mut rack, &inserts);
        assert!(after < 0.5 * before, "not retuned: {before} then {after}");
    }

    /// KT-21: a group clocked phaser or vowel filter placed before a drive
    /// runs per-voice in true signal order inside the voice rack and retunes
    /// live without allocation.
    #[test]
    fn kt21_clocked_filter_before_drive_runs_per_voice_in_signal_order() {
        let rate = 48_000;
        let mut g = kontra_plan::Group::standard();
        g.inserts = vec![
            Some(ir::Effect {
                kind: ir::EffectKind::Clocked(ir::Clocked {
                    shape: ir::ClockedShape::Allpass,
                    cutoff: 0.2,
                    resonance: 0.8,
                }),
                bypass: false,
                output: 1.0,
                dry: 0.0,
            }),
            Some(ir::Effect {
                kind: ir::EffectKind::Shaper(ir::Shaper {
                    pre_lowpass_hz: None,
                    curve: ir::DriveCurve::Soft(0.75),
                    lowpass_hz: None,
                    highpass_hz: None,
                    sections: ir::Sections::NONE,
                }),
                bypass: false,
                output: 1.0,
                dry: 0.0,
            }),
        ];
        let tmpl = template(&g, rate);
        assert_eq!(tmpl.len(), 2, "clocked + shaper must both run per-voice");
        assert!(matches!(tmpl[0].kind, Kind::Clocked(_)) && tmpl[0].insert == 0);
        assert!(matches!(tmpl[1].kind, Kind::Shaper(_)) && tmpl[1].insert == 1);
        assert_eq!(crate::group_racks::split(&g, &tmpl), None);

        let mut rack: Vec<Unit> = tmpl.iter().cloned().collect();
        let mut spare = vec![[0.0; 2]; 256];
        let mut run = |rack: &mut [Unit]| -> f32 {
            let mut io: Vec<[f32; 2]> = (0..256)
                .map(|i| {
                    let t = i as f32 / rate as f32;
                    [0.6 * (std::f32::consts::TAU * 600.0 * t).sin(); 2]
                })
                .collect();
            process(rack, &g.inserts, &mut io, &mut spare, &mut [], rate);
            io.iter().map(|f| f[0] * f[0]).sum::<f32>().sqrt()
        };
        let r0 = run(&mut rack);
        tune_unit(&mut rack[0], Tune::Clocked([0.9, 0.8]), None, rate);
        let r1 = run(&mut rack);
        assert!(
            r0 > 0.1 && r1 > 0.1 && (r0 - r1).abs() > 1e-3,
            "{r0} vs {r1}"
        );
    }

    #[test]
    fn voice_rack_runs_eq_pass_wah_spectral_morph_and_color_sat_per_voice() {
        let rate = 48_000;
        let mut g = kontra_plan::Group::standard();
        g.inserts = vec![
            Some(ir::Effect {
                kind: ir::EffectKind::EqPass {
                    bands: [ir::Bell::FLAT; 4],
                    hp_hz: Some(250.0),
                    lp_hz: Some(6_000.0),
                },
                bypass: false,
                output: 1.0,
                dry: 0.0,
            }),
            Some(ir::Effect {
                kind: ir::EffectKind::Wah(ir::Wah {
                    peak_hz: 1200.0,
                    peak_q: 4.0,
                    peak_gain: 2.0,
                    low_hz: 200.0,
                    low_gain: 0.5,
                    mid_hz: 1500.0,
                    mid_gain: 0.2,
                    high_hz: 3200.0,
                    high_gain: 4.0,
                    mono: false,
                }),
                bypass: false,
                output: 1.0,
                dry: 0.0,
            }),
            Some(ir::Effect {
                kind: ir::EffectKind::SpectralMorph(ir::SpectralMorph {
                    morph: 0.5,
                    amount: 0.8,
                    bank: 0,
                    tracking: false,
                }),
                bypass: false,
                output: 1.0,
                dry: 0.0,
            }),
            Some(ir::Effect {
                kind: ir::EffectKind::ColorSat(ir::ColorSat {
                    character: 2,
                    drive: 0.7,
                    boost: false,
                    boost_db: 0.0,
                    bass_saver: 0.2,
                    lpf_hz: 16_000.0,
                    hpf_hz: 40.0,
                    bass_db: 0.0,
                    mid_db: 0.0,
                    mid_hz: 1000.0,
                    mid_q: 1.0,
                    treble_db: 0.0,
                    mix: 1.0,
                }),
                bypass: false,
                output: 1.0,
                dry: 0.0,
            }),
        ];
        let tmpl = template(&g, rate);
        assert_eq!(
            tmpl.len(),
            4,
            "all 4 slots up to ColorSat must run per-voice"
        );
        assert!(matches!(tmpl.first().map(|u| &u.kind), Some(Kind::Eq(_))));
        assert!(matches!(tmpl.get(1).map(|u| &u.kind), Some(Kind::Wah(_))));
        assert!(matches!(
            tmpl.get(2).map(|u| &u.kind),
            Some(Kind::SpectralMorph(_))
        ));
        assert!(matches!(
            tmpl.get(3).map(|u| &u.kind),
            Some(Kind::ColorSat(_))
        ));
        assert_eq!(crate::group_racks::split(&g, &tmpl), None);

        let mut rack: Vec<Unit> = tmpl.iter().cloned().collect();
        let mut spare = vec![[0.0; 2]; 256];
        let mut io: Vec<[f32; 2]> = (0..256)
            .map(|i| [(std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin() * 0.4; 2])
            .collect();
        let dry = io.clone();
        process(&mut rack, &g.inserts, &mut io, &mut spare, &mut [], rate);
        assert!(io.iter().flatten().all(|v| v.is_finite()));
        assert_ne!(io, dry);
    }
}
