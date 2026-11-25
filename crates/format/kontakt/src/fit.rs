//! The fit rig: renders the models this crate builds over the stimuli the oracle effects saw, so
//! `tools/kontakt-oracle/fx_gate.py` can hold them against Kontakt's own render.
//!
//! `KONTRA_FIT=<dir> cargo test -p kontra-format-kontakt fit -- --ignored`: for each line
//! `name|effect|key=value,...` of `<dir>/cases.txt` it reads `<dir>/<name>.in.f32` (little-endian mono, 48 kHz, the
//! bypassed instance of the oracle render) and writes `<dir>/<name>.model.f32`. Keys are the effect's saved fields.

use std::collections::HashMap;
use std::path::Path;

use kontra_effects::processors::{Dsp, Responses, processor};
use kontra_ir as ir;

use crate::replika::{self, Replika};

fn model(effect: &str, keys: &HashMap<&str, f32>) -> Option<(ir::EffectKind, f32)> {
    let k = |name: &str, or: f32| keys.get(name).copied().unwrap_or(or);
    match effect {
        "replika" => {
            let r = Replika {
                mode: k("mode", 0.0) as i32,
                ms: k("ms", replika::INIT.ms),
                feedback: k("fb", replika::INIT.feedback),
                depth: k("depth", 0.0),
                rate: k("rate", replika::INIT.rate),
                high_cut: k("hc", replika::INIT.high_cut),
                tape_age: k("age", replika::INIT.tape_age),
                flutter: k("flutter", replika::INIT.flutter),
                quality: k("quality", replika::INIT.quality),
                ..replika::INIT
            };
            Some((ir::EffectKind::Delay(replika::delay(r)?), 0.0))
        }
        "dstortion" => Some((
            crate::dstortion::dstortion(
                k("volume", 0.5),
                k("tone", 0.5),
                k("drive", 0.5),
                k("bass", 0.5),
                k("mid", 0.5),
                k("treble", 0.5),
            ),
            0.0,
        )),
        "supergt" => Some((
            crate::supergt::supergt(crate::supergt::Knobs {
                trim: k("trim", 0.5),
                saturation: k("sat", 0.5),
                compress: k("compress", 0.0),
                character: k("char", 0.0),
                mix: k("mix", 1.0),
                attack: k("attack", crate::supergt::INIT.attack),
                release: k("release", crate::supergt::INIT.release),
                hpf_mode: k("hpf", 0.0),
                sat_mode: k("satmode", 0.0),
                char_mode: k("charmode", 0.0),
            }),
            1.0 - k("mix", 1.0),
        )),
        "psyche" => Some((
            ir::EffectKind::Shifter(crate::psyche::shifter(crate::psyche::Psyche {
                ms: k("ms", crate::psyche::INIT.ms),
                feedback: k("fb", crate::psyche::INIT.feedback),
                detune: k("detune", 0.5),
                pitch: k("pitch", 0.5),
                offset: k("offset", 0.5),
                cross: k("cross", 0.5),
                reverse_stereo: k("rs", 0.0) > 0.0,
                detune_stereo: k("ds", 0.0) > 0.0,
                reverse: k("rev", 0.0) > 0.0,
                beats: None,
            })),
            0.0,
        )),
        "legacy_reverb" => Some((
            ir::EffectKind::Reverb(crate::legacy_reverb::reverb(
                k("pd", 0.0),
                [
                    k("size", 0.5),
                    k("colour", 0.5),
                    k("stereo", 1.0),
                    k("damp", 0.5),
                ],
            )),
            0.0,
        )),
        "distortion" => Some((
            crate::drive::distortion(k("type", 0.0) as i32, k("drive", 0.0), k("damp", 0.0))?,
            0.0,
        )),
        "vibrato" => {
            let v = crate::vibrato::INIT;
            let v = crate::vibrato::Vibrato {
                depth: k("depth", v.depth as f32) as i32,
                color: k("color", v.color as f32) as i32,
                blend: k("blend", v.blend),
                rate: k("rate", v.rate),
                width: k("width", v.width),
                mix: k("mix", v.mix),
            };
            Some((ir::EffectKind::Scanner(crate::vibrato::scanner(v)), 0.0))
        }
        "freak" => {
            let f = crate::freak::FRESH;
            Some((
                crate::freak::freak(crate::freak::Knobs {
                    kind: k("type", f.kind),
                    frequency: k("freq", f.frequency),
                    harmonics: k("harm", f.harmonics),
                    feedback: k("fb", f.feedback),
                    mix: k("mix", f.mix),
                    mode: k("mode", f.mode),
                    carrier: k("carrier", f.carrier),
                    tuning: k("tuning", f.tuning),
                    width: k("width", f.width),
                    contour: k("contour", f.contour),
                    release: k("release", f.release),
                    gate: k("gate", f.gate),
                    antifold: k("antifold", f.antifold),
                    stereo: k("stereo", f.stereo),
                    wide_range: k("wide", f.wide_range),
                    bp_freq: k("bp_freq", f.bp_freq),
                    demod: k("demod", f.demod),
                    bp_filter: k("bp", f.bp_filter),
                }),
                0.0,
            ))
        }
        "bite" => {
            let b = crate::bite::FRESH;
            Some((
                crate::bite::bite(crate::bite::Knobs {
                    frequency: k("freq", b.frequency),
                    bits: k("bits", b.bits),
                    saturate: k("sat", b.saturate),
                    mix: k("mix", b.mix),
                    jitter: k("jitter", b.jitter),
                    crunch: k("crunch", b.crunch),
                    dither: k("dither", b.dither),
                    expand: k("expand", b.expand),
                    prefilter: k("pre", b.prefilter),
                    postfilter: k("post", b.postfilter),
                    hpf: k("hpf", b.hpf),
                    dc: k("dc", b.dc),
                }),
                0.0,
            ))
        }
        _ => None,
    }
}

#[test]
#[ignore = "needs oracle renders: KONTRA_FIT=<dir>"]
fn fit() {
    let dir = std::env::var("KONTRA_FIT").expect("KONTRA_FIT");
    let dir = Path::new(&dir);
    let cases = std::fs::read_to_string(dir.join("cases.txt")).expect("cases.txt");
    for line in cases.lines().filter(|l| !l.trim().is_empty()) {
        let mut parts = line.split('|');
        let (name, effect) = (parts.next().expect("name"), parts.next().expect("effect"));
        let keys: HashMap<&str, f32> = parts
            .next()
            .unwrap_or("")
            .split(',')
            .filter_map(|kv| kv.split_once('='))
            .filter_map(|(k, v)| Some((k, v.parse().ok()?)))
            .collect();
        let (kind, dry) = model(effect, &keys).unwrap_or_else(|| panic!("{effect}: no model"));
        let bytes = std::fs::read(dir.join(format!("{name}.in.f32"))).expect("input");
        let input: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        let slot = ir::Effect {
            kind: kind.clone(),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        };
        // `switch`: the oracle's unit ran fresh and took the case's knobs
        // this many seconds before the input (from the note callback): a
        // head that keeps its place until it restarts still plays the
        // fresh delay for a while.
        let switch = keys.get("switch").copied();
        let first = match switch {
            Some(_) => ir::Effect {
                kind: model(effect, &HashMap::new()).expect("fresh model").0,
                ..slot
            },
            None => slot,
        };
        let Dsp::Wet(mut unit, _) = processor(Some(&first), 48_000, &Responses(&[], &[])) else {
            panic!("{effect}: no processor");
        };
        let silence = |unit: &mut Box<dyn kontra_effects::processors::Wet>, seconds: f32| {
            for chunk in vec![0.0f32; (seconds * 48_000.0) as usize].chunks(256) {
                let mut io = vec![[0.0f32; 2]; chunk.len()];
                unit.process(&mut io);
            }
        };
        // The oracle's LFO runs free: `pre` seconds of silence first put the model's LFO where the oracle's was.
        silence(&mut unit, keys.get("pre").copied().unwrap_or(0.0));
        if let Some(seconds) = switch {
            unit.set(&kind, 120.0);
            silence(&mut unit, seconds);
        }
        let mut out = Vec::with_capacity(input.len() * 4);
        for chunk in input.chunks(256) {
            let mut io: Vec<[f32; 2]> = chunk.iter().map(|&v| [v, v]).collect();
            unit.process(&mut io);
            for (frame, &v) in io.iter().zip(chunk) {
                out.extend_from_slice(&(frame[0] + dry * v).to_le_bytes());
            }
        }
        std::fs::write(dir.join(format!("{name}.model.f32")), out).expect("write");
    }
}
