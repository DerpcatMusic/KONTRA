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
                ..replika::INIT
            };
            Some((ir::EffectKind::Delay(replika::delay(r)?), 1.0))
        }
        "distortion" => Some((
            crate::drive::distortion(k("type", 0.0) as i32, k("drive", 0.0), k("damp", 0.0))?,
            0.0,
        )),
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
            kind,
            bypass: false,
            output: 1.0,
            dry: 0.0,
        };
        let Dsp::Wet(mut unit, _) = processor(Some(slot), 48_000, &Responses(&[], &[])) else {
            panic!("{effect}: no processor");
        };
        // The oracle's LFO runs free: `pre` seconds of silence first put the model's LFO where the oracle's was.
        let pre = (keys.get("pre").copied().unwrap_or(0.0) * 48_000.0) as usize;
        for chunk in vec![0.0f32; pre].chunks(256) {
            let mut io = vec![[0.0f32; 2]; chunk.len()];
            unit.process(&mut io);
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
