//! One preset into the instrument model (SoundFont 2.01 §8–9): every preset
//! zone's instrument zones become voices, the preset zone's generators added
//! onto the instrument zone's (§9.4). Voices that share their settings share
//! a group; each is one zone. What the IR cannot carry is a diagnostic.

use std::collections::HashMap;
use std::path::Path;

use kontra_format::Notes;
use kontra_ir as ir;

use crate::Error;
use crate::gens::{self, Gens};
use crate::laws;
use crate::parser::{self, LEFT, OGG, RIGHT, ROM, SoundFont, ZoneRecords, malformed};

/// Octaves a cutoff route's depth of 1 moves the filter: the generators'
/// ±12000 cents.
const CUTOFF_OCTAVES: f32 = 10.0;

/// One 16-bit bag table's worth of instrument zones, across all references.
const MAX_EXPANDED_ZONES: usize = u16::MAX as usize;
/// Count repeated generator/modulator visits as well as bag visits.
const MAX_EXPANDED_RECORDS: usize = 1 << 20;
/// SoundFont 2.01 §7.10 gives 400 Hz as the lower practical sample rate.
const MIN_PRACTICAL_RATE: u32 = 400;

/// The SF2 default modulators the IR carries as routes or output controls
/// (§8.4.1–8.4.10): velocity → attenuation (960 cB concave, which is
/// `(v/127)²` in amplitude), velocity → cutoff (−2400 cents, from velocity
/// 64 up), CC7/CC11 → attenuation, CC10 → pan, mod wheel and channel
/// pressure → vibrato (50 cents each), pitch wheel → pitch (±2 semitones).
const VELOCITY_CUTOFF_CENTS: f32 = -2_400.0;
const WHEEL_VIBRATO_CENTS: f32 = 50.0;
const BEND_SEMITONES: f32 = 2.0;
const OUTPUT_CONTROLLERS: ir::OutputControllers = ir::OutputControllers {
    volume: 7,
    pan: 10,
    top_db: 0.0,
    power: 2,
    expression: Some(11),
};

/// The preset whose header is `index` (not the terminal one) of `sf`.
pub(crate) fn preset(sf: &SoundFont, index: usize, path: &Path) -> Result<ir::Instrument, Error> {
    let (Some(head), Some(next)) = (sf.presets.get(index), sf.presets.get(index + 1)) else {
        return Err(malformed("phdr", format!("no preset {index}")));
    };
    let mut build = Build {
        sf,
        path,
        samples: Vec::new(),
        sample_index: HashMap::new(),
        groups: Vec::new(),
        classes: Vec::new(),
        notes: Notes::default(),
        zone_id: 0,
        zone_visits: 0,
        record_visits: 0,
    };
    if sf.sm24.is_some() {
        build.notes.add(
            ir::Impact::Cosmetic,
            "sm24 24-bit low bytes",
            "played from the 16-bit words",
        );
    }
    let zones = parser::zones((head.bag, next.bag), &sf.pbag, &sf.pgen, &sf.pmod, "pbag")?;
    let mut global = Gens::default();
    for (i, zone) in zones.iter().enumerate() {
        charge(
            &mut build.record_visits,
            zone.generators.len() + zone.modulators.len(),
            MAX_EXPANDED_RECORDS,
            "preset generator and modulator records",
        )?;
        build.modulators(zone, "preset");
        let (gens, instrument) = Gens::read(zone.generators, gens::INSTRUMENT, &mut build.notes);
        for g in gens.set().filter(|&g| gens::instrument_only(g)) {
            build.notes.add(
                ir::Impact::Cosmetic,
                "instrument-only generator in a preset zone",
                gens::name(g),
            );
        }
        match instrument {
            Some(inst) => build.instrument(inst, &global.over(&gens))?,
            None if i == 0 => global = gens,
            None => build.notes.add(
                ir::Impact::Cosmetic,
                "preset zone without an instrument",
                &head.name,
            ),
        }
    }
    Ok(build.finish(head.name.clone()))
}

struct Build<'a> {
    sf: &'a SoundFont,
    path: &'a Path,
    samples: Vec<ir::Sample>,
    /// Each sample header read so far: its index in `samples`, or `None`
    /// when it cannot play.
    sample_index: HashMap<u16, Option<usize>>,
    /// Each group's settings (no zones) and its zones.
    groups: Vec<(ir::Group, Vec<ir::Zone>)>,
    /// Exclusive class numbers, by voice group index.
    classes: Vec<u16>,
    notes: Notes,
    zone_id: u32,
    zone_visits: usize,
    record_visits: usize,
}

impl Build<'_> {
    /// The voices of instrument `inst` under the preset generators `preset`.
    fn instrument(&mut self, inst: u16, preset: &Gens) -> Result<(), Error> {
        let sf = self.sf;
        let i = usize::from(inst);
        let real = sf.inst.len().saturating_sub(1);
        let (Some(head), Some(next)) = (sf.inst.get(i), sf.inst.get(i + 1)) else {
            return Err(malformed("pgen", format!("instrument {i} of {real}")));
        };
        if i >= real {
            return Err(malformed("pgen", format!("instrument {i} of {real}")));
        }
        let zones = parser::zones((head.bag, next.bag), &sf.ibag, &sf.igen, &sf.imod, "ibag")?;
        charge(
            &mut self.zone_visits,
            zones.len(),
            MAX_EXPANDED_ZONES,
            "instrument zones",
        )?;
        let mut global = Gens::default();
        for (j, zone) in zones.iter().enumerate() {
            charge(
                &mut self.record_visits,
                zone.generators.len() + zone.modulators.len(),
                MAX_EXPANDED_RECORDS,
                "instrument generator and modulator records",
            )?;
            self.modulators(zone, "instrument");
            let (gens, sample) = Gens::read(zone.generators, gens::SAMPLE_ID, &mut self.notes);
            match sample {
                Some(id) => self.voice(&head.name, &global.over(&gens), preset, id)?,
                None if j == 0 => global = gens,
                None => self.notes.add(
                    ir::Impact::Cosmetic,
                    "instrument zone without a sample",
                    &head.name,
                ),
            }
        }
        Ok(())
    }

    /// One voice: instrument zone `inst` with the preset's offsets `preset`.
    fn voice(&mut self, name: &str, inst: &Gens, preset: &Gens, id: u16) -> Result<(), Error> {
        let v = |g| gens::value(inst, preset, g);
        let (Some(keys), Some(velocities)) = (
            range(inst, preset, gens::KEY_RANGE),
            range(inst, preset, gens::VEL_RANGE),
        ) else {
            return Ok(());
        };
        let Some(sample) = self.sample(id)? else {
            return Ok(());
        };
        let sf = self.sf;
        let Some(head) = sf.samples.get(usize::from(id)) else {
            return Ok(());
        };
        for g in gens::UNSUPPORTED {
            if v(g) != gens::default(g) {
                self.notes.add(
                    ir::Impact::Audible,
                    &format!("generator {}", gens::name(g)),
                    v(g),
                );
            }
        }
        if head.kind & (LEFT | RIGHT) != 0 && v(gens::PAN) == 0 {
            // §7.10: a stereo half plays as a mono sample at its zone's pan.
            self.notes.add(
                ir::Impact::Cosmetic,
                "stereo half at centre pan",
                &head.name,
            );
        }

        let frames = i64::from(head.end - head.start);
        let offset = |fine, coarse| i64::from(v(fine)) + 32_768 * i64::from(v(coarse));
        let mut clamp = |at: i64, lo: i64| {
            let held = at.clamp(lo, frames);
            if held != at {
                self.notes
                    .add(ir::Impact::Cosmetic, "sample offset outside its sample", at);
            }
            held as u64
        };
        let start = clamp(offset(gens::START_OFFSET, gens::START_COARSE_OFFSET), 0);
        let end = clamp(
            frames + offset(gens::END_OFFSET, gens::END_COARSE_OFFSET),
            start as i64,
        );
        let loop_start = clamp(
            i64::from(head.loop_start) - i64::from(head.start)
                + offset(gens::START_LOOP_OFFSET, gens::START_LOOP_COARSE_OFFSET),
            0,
        );
        let loop_end = clamp(
            i64::from(head.loop_end) - i64::from(head.start)
                + offset(gens::END_LOOP_OFFSET, gens::END_LOOP_COARSE_OFFSET),
            0,
        );
        let loop_start = loop_start.clamp(start, end);
        let loop_end = loop_end.clamp(start, end);
        let loops = match v(gens::SAMPLE_MODES) & 3 {
            mode @ (1 | 3) if loop_end > loop_start => vec![ir::Loop {
                start: loop_start,
                end: loop_end,
                until_release: mode == 3,
                ..ir::Loop::default()
            }],
            1 | 3 => {
                self.notes
                    .add(ir::Impact::Audible, "empty loop", &head.name);
                Vec::new()
            }
            _ => Vec::new(),
        };
        let root = match v(gens::OVERRIDING_ROOT_KEY) {
            k @ 0..=127 => k as u8,
            _ if head.original_pitch <= 127 => head.original_pitch,
            _ => 60,
        };
        let (settings, swing_db) = self.settings(name, &v);
        let zone = ir::Zone {
            id: self.zone_id,
            sample,
            keys,
            velocities,
            velocity_fade: (0, 0),
            key_fade: (0, 0),
            crossfades: Vec::new(),
            root,
            tune: f64::from(v(gens::COARSE_TUNE))
                + f64::from(v(gens::FINE_TUNE) + i32::from(head.pitch_correction)) / 100.0,
            volume_db: laws::attenuation_db(v(gens::INITIAL_ATTENUATION)) + swing_db,
            pan: laws::pan(v(gens::PAN)),
            start,
            end,
            start_mod: 0,
            loops,
            reverse: false,
            mix: Vec::new(),
            slot: 0,
            envelopes: Vec::new(),
        };
        self.zone_id += 1;
        match self.groups.iter_mut().find(|(s, _)| *s == settings) {
            Some((_, zones)) => zones.push(zone),
            None => self.groups.push((settings, vec![zone])),
        }
        Ok(())
    }

    /// The group-level settings of a voice: envelopes, oscillators, filter,
    /// routes and exclusive class; and the dB the volume LFO's swing is
    /// centred by.
    fn settings(&mut self, name: &str, v: &impl Fn(u16) -> i32) -> (ir::Group, f32) {
        let mut g = ir::Group {
            name: name.to_owned(),
            keytrack: f64::from(v(gens::SCALE_TUNING)) / 100.0,
            envelope: ir::Envelope {
                attack: laws::delay(v(gens::DELAY_VOL_ENV))
                    + laws::timecents(v(gens::ATTACK_VOL_ENV)),
                lag: laws::delay(v(gens::DELAY_VOL_ENV)),
                hold: laws::delay(v(gens::HOLD_VOL_ENV)),
                decay: laws::timecents(v(gens::DECAY_VOL_ENV)),
                sustain: laws::sustain(v(gens::SUSTAIN_VOL_ENV)),
                release: laws::timecents(v(gens::RELEASE_VOL_ENV)),
                attack_shape: 0.0,
                decay_shape: laws::FALL_SHAPE,
                release_shape: laws::FALL_SHAPE,
                floor: 0.0,
                depth: 1.0,
                // ponytail: the SF2 delay still rides attack + lag as before;
                // move it to `delay` when a bank shows the difference.
                delay: 0.0,
                one_shot: false,
            },
            ..ir::Group::DEFAULT
        };
        g.mods.push(route(
            "velocity",
            ir::ModKind::Volume {
                source: ir::Source::Velocity,
                intensity: 1.0,
                curve: ir::Curve::Squared,
                tau_ms: 0.0,
            },
        ));
        g.mods.push(route(
            "pitch wheel",
            ir::ModKind::Pitch {
                source: ir::Source::PitchBend,
                semitones: BEND_SEMITONES,
                depth: 1.0,
                tau_ms: 0.0,
            },
        ));

        // The modulation LFO, when anything reads it.
        let (lfo_pitch, lfo_cutoff, lfo_volume) = (
            v(gens::MOD_LFO_TO_PITCH),
            v(gens::MOD_LFO_TO_FILTER_FC),
            v(gens::MOD_LFO_TO_VOLUME),
        );
        let mut swing_db = 0.0;
        let mod_lfo = (lfo_pitch != 0 || lfo_cutoff != 0 || lfo_volume != 0)
            .then(|| self.lfo(&mut g, v(gens::FREQ_MOD_LFO), v(gens::DELAY_MOD_LFO)));
        if let Some(lfo) = mod_lfo {
            if lfo_pitch != 0 {
                g.mods.push(pitch("mod LFO pitch", lfo, lfo_pitch));
            }
            if lfo_volume != 0 {
                // The signed dB curve adds -2A at source 0 and 0 at source 1.
                // A signed baseline leaves the endpoints at -A and +A.
                let a = laws::swing_db(lfo_volume);
                swing_db = a;
                g.mods.push(route(
                    "mod LFO volume",
                    ir::ModKind::Volume {
                        source: lfo,
                        intensity: a.signum(),
                        curve: ir::Curve::Decibels(2.0 * a.abs()),
                        tau_ms: 0.0,
                    },
                ));
            }
        }

        // The vibrato LFO, always: the mod wheel and pressure deepen it.
        let vib = self.lfo(&mut g, v(gens::FREQ_VIB_LFO), v(gens::DELAY_VIB_LFO));
        if v(gens::VIB_LFO_TO_PITCH) != 0 {
            g.mods
                .push(pitch("vibrato LFO pitch", vib, v(gens::VIB_LFO_TO_PITCH)));
        }
        let wheel = |source| ir::Term {
            input: ir::Transfer {
                source: vib,
                table: vec![-1.0, 1.0],
                ..ir::Transfer::default()
            },
            amount: 1.0,
            factors: vec![ir::Factor {
                input: ir::Transfer {
                    source,
                    ..ir::Transfer::default()
                },
                depth: 1.0,
            }],
        };
        g.blends.push(ir::Blend {
            base: 0.0,
            terms: vec![
                wheel(ir::Source::Controller(1)),
                wheel(ir::Source::Pressure),
            ],
        });
        g.mods.push(route(
            "mod wheel vibrato",
            ir::ModKind::Pitch {
                source: ir::Source::Blend(index(g.blends.len() - 1)),
                semitones: WHEEL_VIBRATO_CENTS / 100.0,
                depth: 1.0,
                tau_ms: 0.0,
            },
        ));

        // The modulation envelope, when anything reads it.
        let (env_pitch, env_cutoff) = (v(gens::MOD_ENV_TO_PITCH), v(gens::MOD_ENV_TO_FILTER_FC));
        let mod_env = (env_pitch != 0 || env_cutoff != 0).then(|| {
            g.envelopes.push(mod_envelope(v));
            ir::Source::Envelope(index(g.envelopes.len() - 1))
        });
        if let (Some(env), true) = (mod_env, env_pitch != 0) {
            g.mods.push(pitch("mod envelope pitch", env, env_pitch));
        }

        // The low-pass, when it is below the top or something moves it.
        let fc = v(gens::INITIAL_FILTER_FC);
        if fc < 13_500 || lfo_cutoff != 0 || env_cutoff != 0 {
            g.inserts.push(Some(ir::Effect {
                kind: ir::EffectKind::Filter(ir::Filter {
                    pass: ir::Pass::Low,
                    hz: laws::cutoff_hz(fc),
                    stages: 1,
                    ladder: None,
                    damping: [laws::damping(v(gens::INITIAL_FILTER_Q)); 3],
                    gain: 1.0,
                    octaves: CUTOFF_OCTAVES,
                    q_octaves: 0.0,
                }),
                bypass: false,
                output: 1.0,
                dry: 0.0,
            }));
            for (label, source, cents) in [
                ("mod LFO cutoff", mod_lfo, lfo_cutoff),
                ("mod envelope cutoff", mod_env, env_cutoff),
            ] {
                if let (Some(source), true) = (source, cents != 0) {
                    g.mods.push(cutoff(label, source, cents as f32));
                }
            }
            // §8.4.2: velocity below 64 leaves the cutoff; from 64 it falls
            // by 2400·(1 − v/127) cents.
            g.shapers.push(ir::Transfer {
                source: ir::Source::Velocity,
                table: (0..128)
                    .map(|i| if i < 64 { 0.0 } else { 1.0 - i as f32 / 127.0 })
                    .collect(),
                ..ir::Transfer::default()
            });
            let shaped = ir::Source::Shaped(index(g.shapers.len() - 1));
            g.mods
                .push(cutoff("velocity cutoff", shaped, VELOCITY_CUTOFF_CENTS));
        }

        let class = v(gens::EXCLUSIVE_CLASS);
        if let Ok(class @ 1..) = u16::try_from(class) {
            let at = (self.classes.iter().position(|&c| c == class)).unwrap_or_else(|| {
                self.classes.push(class);
                self.classes.len() - 1
            });
            g.voice_group = Some(at);
        }
        (g, swing_db)
    }

    /// A triangle LFO at `freq` cents after `delay` timecents, added to `g`.
    fn lfo(&mut self, g: &mut ir::Group, freq: i32, delay: i32) -> ir::Source {
        let delay_s = laws::delay(delay);
        if delay_s > 0.0 {
            self.notes
                .add(ir::Impact::Cosmetic, "LFO delay as a fade-in", delay);
        }
        // ponytail: the IR's LFO has no delay, so it fades in over the
        // delay; an `Lfo::delay_ms` when a bank shows the difference.
        g.lfos.push(ir::Lfo {
            wave: ir::Wave::Triangle,
            rate: ir::Rate::Hz(laws::lfo_hz(freq)),
            phase: 0.0,
            fade_ms: delay_s * 1000.0,
            clock: ir::Clock::Voice,
        });
        ir::Source::Lfo(index(g.lfos.len() - 1))
    }

    /// The sample of header `id` (not the terminal one), added on first use;
    /// `None`, reported, when it cannot play.
    fn sample(&mut self, id: u16) -> Result<Option<usize>, Error> {
        if let Some(&known) = self.sample_index.get(&id) {
            return Ok(known);
        }
        let real = self.sf.samples.len().saturating_sub(1);
        let head = (self.sf.samples.get(usize::from(id)))
            .filter(|_| usize::from(id) < real)
            .ok_or_else(|| malformed("igen", format!("sample {id} of {real}")))?;
        let words = self.sf.smpl.map_or(0, |s| s.len / 2);
        let playable = if head.kind & ROM != 0 {
            self.notes.add(ir::Impact::Broken, "ROM sample", &head.name);
            None
        } else if head.kind & OGG != 0 {
            self.notes
                .add(ir::Impact::Broken, "SF3 compressed sample", &head.name);
            None
        } else if head.start >= head.end || u64::from(head.end) > words {
            let detail = format!("{} {}..{} of {words}", head.name, head.start, head.end);
            self.notes
                .add(ir::Impact::Broken, "sample outside smpl", detail);
            None
        } else {
            let rate = if head.rate == 0 {
                self.notes.add(
                    ir::Impact::Audible,
                    "sample rate 0",
                    format!("{}: used {MIN_PRACTICAL_RATE} Hz", head.name),
                );
                MIN_PRACTICAL_RATE
            } else {
                head.rate
            };
            let offset = self.sf.smpl.map_or(0, |s| s.offset) + 2 * u64::from(head.start);
            let frames = u64::from(head.end - head.start);
            self.samples.push(ir::Sample {
                name: head.name.clone(),
                file: kontra_format::moved(self.path),
                offset,
                len: Some(2 * frames),
                channels: 1,
                frames,
                rate,
                keystream: None,
                raw: Some(ir::RawPcm::I16Le),
            });
            Some(self.samples.len() - 1)
        };
        self.sample_index.insert(id, playable);
        Ok(playable)
    }

    /// Reports a zone's stored modulators: the IR carries only the defaults.
    fn modulators(&mut self, zone: &ZoneRecords<'_>, level: &str) {
        if zone.modulators_broken {
            self.notes.add(
                ir::Impact::Audible,
                "modulator indices out of bounds",
                level,
            );
        }
        for m in zone.modulators {
            let detail = format!(
                "{level}: source {:#06x} → {} by {}",
                m.source,
                gens::name(m.destination),
                m.amount
            );
            self.notes.add(ir::Impact::Audible, "modulator", detail);
        }
    }

    fn finish(self, name: String) -> ir::Instrument {
        ir::Instrument {
            name,
            source_path: Some(self.path.to_path_buf()),
            samples: self.samples,
            groups: (self.groups.into_iter())
                .map(|(settings, zones)| ir::Group { zones, ..settings })
                .collect(),
            voice_groups: (self.classes.iter()).map(|_| EXCLUSIVE).collect(),
            output: ir::Output {
                controllers: Some(OUTPUT_CONTROLLERS),
                ..ir::Output::UNITY
            },
            diagnostics: self.notes.finish(),
            ..ir::Instrument::DEFAULT
        }
    }
}

/// An exclusive class (§8.1.2 gen 57): a new note of the class ends the
/// notes sounding in it, all their voices.
const EXCLUSIVE: ir::VoiceGroup = ir::VoiceGroup {
    voices: 1,
    per_note: true,
    prefer_released: false,
    ..ir::VoiceGroup::DEFAULT
};

pub(crate) fn charge(
    used: &mut usize,
    amount: usize,
    limit: usize,
    unit: &'static str,
) -> Result<(), Error> {
    if amount > limit - *used {
        return Err(Error::ExpansionLimit { limit, unit });
    }
    *used += amount;
    Ok(())
}

/// A key or velocity range: the instrument zone's within the preset zone's
/// (§9.4); `None` when they do not meet.
fn range(inst: &Gens, preset: &Gens, g: u16) -> Option<std::ops::RangeInclusive<u8>> {
    let (a, b) = inst.range(g).unwrap_or((0, 127));
    let (c, d) = preset.range(g).unwrap_or((0, 127));
    let (lo, hi) = (a.max(c), b.min(d).min(127));
    (lo <= hi).then_some(lo..=hi)
}

/// The modulation envelope (gens 25–30): delay, a linear attack, hold, a
/// linear decay to the sustain level, and release, in the format's 0..=1.
fn mod_envelope(v: &impl Fn(u16) -> i32) -> ir::Breakpoints {
    let ms = |s: f32| s * 1000.0;
    let sustain = laws::mod_sustain(v(gens::SUSTAIN_MOD_ENV));
    let point = |s: f32, level| ir::Breakpoint {
        ms: ms(s),
        level,
        curve: 0.0,
    };
    let mut points = Vec::new();
    let delay = laws::delay(v(gens::DELAY_MOD_ENV));
    if delay > 0.0 {
        points.push(point(delay, 0.0));
    }
    points.push(point(laws::timecents(v(gens::ATTACK_MOD_ENV)), 1.0));
    let hold = laws::delay(v(gens::HOLD_MOD_ENV));
    if hold > 0.0 {
        points.push(point(hold, 1.0));
    }
    // Decay and release times are for a full 1 → 0 fall (§8.1.2).
    let decay = laws::timecents(v(gens::DECAY_MOD_ENV)) * (1.0 - sustain);
    points.push(point(decay, sustain));
    let sustain_at = points.len() - 1;
    points.push(point(laws::timecents(v(gens::RELEASE_MOD_ENV)), 0.0));
    ir::Breakpoints {
        start: 0.0,
        points,
        sustain: Some(sustain_at),
        loop_from: None,
        stages: false,
    }
}

fn route(name: &str, kind: ir::ModKind) -> ir::Mod {
    ir::Mod {
        name: name.to_owned(),
        slot: 0,
        target: 0,
        bypass: false,
        kind,
    }
}

fn pitch(name: &str, source: ir::Source, cents: i32) -> ir::Mod {
    route(
        name,
        ir::ModKind::Pitch {
            source,
            semitones: laws::semitones(cents),
            depth: 1.0,
            tau_ms: 0.0,
        },
    )
}

/// A cutoff route of `cents` from `source` on the group's filter (insert 0).
fn cutoff(name: &str, source: ir::Source, cents: f32) -> ir::Mod {
    route(
        name,
        ir::ModKind::Cutoff {
            source,
            insert: 0,
            depth: cents / 1200.0 / CUTOFF_OCTAVES,
        },
    )
}

/// A group list index as a source index (a group holds a handful).
fn index(i: usize) -> u8 {
    u8::try_from(i).unwrap_or(u8::MAX)
}
