//! Render a note of Kontakt instruments that carry a convolution, once as
//! translated and once with every convolution's wet gain zeroed (the dry
//! path alone), and print peaks, RMS and finiteness.
//!
//! `conv_check <nki>...`; instruments without a translated impulse response
//! are skipped.
use sampler_core::{Input, Limits, Protocol, Runtime};
use sampler_ir as ir;
use std::path::Path;

fn render(path: &Path, wet: bool) -> Option<(Vec<[f32; 2]>, u8)> {
    let mut kontakt = sampler_kontakt::read(path).ok()?;
    if kontakt.instrument.impulses.is_empty() {
        return None;
    }
    let first = kontakt.instrument.zones.first()?;
    let key = ((u16::from(first.keys.low) + u16::from(first.keys.high)) / 2) as u8;
    if !wet {
        for chain in &mut kontakt.instrument.chains {
            for p in chain
                .pre_amplitude
                .iter_mut()
                .chain(&mut chain.post_amplitude)
            {
                if let ir::Processor::Convolution { wet, .. } = p {
                    *wet = 0.0;
                }
            }
        }
    }
    let options = sampler_kontakt::Options {
        keys: key..=key,
        scripts: false,
        ..Default::default()
    };
    let loaded = sampler_kontakt::load_read(kontakt, &options, |_| {}, || false).ok()?;
    let limits = Limits {
        notes: 16,
        channels: 1,
        performances: 1,
        families: 16,
        expressions: 16,
        voices: 64,
        decisions: 64,
        commands: 64,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    };
    let mut rt = Runtime::new(loaded.plan, limits).ok()?;
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: None,
    };
    rt.trigger(input, key, 0.8).ok()?;
    let mut out = vec![[0.0; 2]; 96_000];
    for chunk in out.chunks_mut(256) {
        rt.render(chunk).ok()?;
    }
    Some((out, key))
}

fn stats(a: &[[f32; 2]]) -> (f32, f32, bool) {
    let peak = a.iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
    let rms = (a.iter().flatten().map(|x| x * x).sum::<f32>() / (2 * a.len()) as f32).sqrt();
    (peak, rms, a.iter().flatten().all(|x| x.is_finite()))
}

fn main() {
    for arg in std::env::args().skip(1) {
        let path = Path::new(&arg);
        let (Some((wet, key)), Some((dry, _))) = (render(path, true), render(path, false)) else {
            println!("skip {arg}");
            continue;
        };
        let (wp, wr, wf) = stats(&wet);
        let (dp, dr, df) = stats(&dry);
        // Energy of the difference: what the convolution added.
        let diff: f32 = wet
            .iter()
            .zip(&dry)
            .flat_map(|(a, b)| [a[0] - b[0], a[1] - b[1]])
            .map(|x| x * x)
            .sum::<f32>()
            / (2 * wet.len()) as f32;
        println!(
            "{arg}\n  key {key}: wet peak {wp:.4} rms {wr:.5} finite {wf} | dry peak {dp:.4} rms {dr:.5} finite {df} | added rms {:.5}",
            diff.sqrt()
        );
    }
}
