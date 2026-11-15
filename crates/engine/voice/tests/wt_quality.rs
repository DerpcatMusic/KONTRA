//! The wavetable read's quality settings against what Kontakt 8.13.1 plays: a 2048-frame sawtooth
//! (harmonics 1..1023 at 1/h) read at keys 60, 84 and 96, analysed as `tools/kontakt-oracle/wt_quality.py`
//! analyses Kontakt's render of the same table (4096-frame Hann window, bins within 2 of a harmonic are
//! the harmonic's; the rest below 20 kHz is alias).
#![cfg(test)]

use std::sync::Arc;

use kontra_stream::{BlobKey, Encoding, MemSource, SampleFmt, Store, StoreConfig};
use kontra_voice::{
    Ahdsr, CYCLE, LEVELS, PERIOD, Quality, Read, Region, SIZE, Scratch, TOP, Table, Voice,
    VoiceStart, Warp,
};

const FMT: SampleFmt = SampleFmt {
    encoding: Encoding::F32,
    channels: 1,
};
const RATE: f64 = 48_000.0;
const N: usize = 4096;

/// What a read prints: harmonic gains against the ideal 1/h (dB) and the alias floor (dB under h1).
struct Heard {
    gains: Vec<(usize, f64)>,
    alias: f64,
}

fn play(quality: Quality, key: i32) -> Heard {
    let f0 = 261.6256 * 2f64.powf(f64::from(key - 60) / 12.0);
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let client = store.client();
    let frames = CYCLE;
    let sample = store
        .load(
            &BlobKey {
                file: "saw".into(),
                offset: 0,
                fmt: FMT,
            },
            Arc::new(MemSource::new(FMT, vec![0; frames as usize * 4])),
            frames,
            std::slice::from_ref(&(0..frames)),
        )
        .unwrap();
    let mut data = Vec::new();
    let mut spans = Vec::new();
    for (&top, &size) in TOP.iter().zip(&SIZE) {
        spans.push((data.len() as u32, size));
        let harmonics = (top as usize / 2).min(1023);
        for n in 0..size as usize {
            let x = std::f64::consts::TAU * n as f64 / f64::from(size);
            data.push(
                (1..=harmonics)
                    .map(|h| (h as f64 * x).sin() / h as f64)
                    .sum::<f64>()
                    * 0.4,
            );
        }
    }
    assert_eq!(spans.len(), LEVELS);
    let peak = data.iter().fold(0.0f64, |m, x| m.max(x.abs()));
    let steps: Arc<[i16]> = data
        .iter()
        .map(|x| (x / peak * 32767.0).round() as i16)
        .collect();
    let read = Read {
        position: 0.0,
        phase: 0.0,
        phase_random: 0.0,
        gain: 1.0,
        inharmonic: kontra_voice::Inharmonic::OFF,
        modosc: kontra_voice::ModOsc::OFF,
        rate: RATE as f32,
        quality,
        warps: [Warp::OFF; 2],
    };
    let region = Region {
        mix: None,
        table: Table::new((steps, (peak / 32767.0) as f32), spans.into(), read),
        sample,
        start: 0,
        end: frames,
        loops: Default::default(),
        reverse: false,
    };
    let mut voice = Voice::allocate();
    voice.start(VoiceStart {
        speed: 1.0,
        region,
        step: PERIOD * f0 / RATE,
        gain: [[1.0, 0.0], [0.0, 1.0]],
        lag: kontra_voice::Lag::NONE,
        envelope: Ahdsr::GATE,
        fade: None,
    });
    let mut out = vec![[0.0f32; 2]; 8192 + N];
    for (i, block) in out.chunks_mut(128).enumerate() {
        voice.render(&mut Scratch::allocate(), &client, (i * 128) as u64, block);
    }
    let x: Vec<f64> = out[8192..]
        .iter()
        .enumerate()
        .map(|(n, f)| {
            let w = 0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / N as f64).cos();
            f64::from(f[0]) * w
        })
        .collect();
    let bins = (20_000.0 / RATE * N as f64) as usize;
    let power: Vec<f64> = (0..=bins)
        .map(|b| {
            let (mut re, mut im) = (0.0, 0.0);
            for (n, v) in x.iter().enumerate() {
                let a = std::f64::consts::TAU * (b * n % N) as f64 / N as f64;
                re += v * a.cos();
                im -= v * a.sin();
            }
            re * re + im * im
        })
        .collect();
    let at = |f: f64| (f / RATE * N as f64).round() as usize;
    let band = |h: usize| {
        (at(h as f64 * f0).saturating_sub(2)..=at(h as f64 * f0) + 2).filter(|&b| b <= bins)
    };
    let h1: f64 = band(1).map(|b| power[b]).sum();
    let mut on = vec![false; bins + 1];
    let mut gains = Vec::new();
    for h in 1..=(20_000.0 / f0) as usize {
        let p: f64 = band(h).map(|b| power[b]).sum();
        band(h).for_each(|b| on[b] = true);
        if [2, 4, 8, 16, 32].contains(&h) {
            gains.push((h, 10.0 * (p / h1 * (h * h) as f64 + 1e-20).log10()));
        }
    }
    let off: f64 = (5..=bins).filter(|&b| !on[b]).map(|b| power[b]).sum();
    Heard {
        gains,
        alias: 10.0 * (off / h1 + 1e-20).log10(),
    }
}

/// Kontakt 8.13.1's numbers, per key 60 / 84 / 96: the alias floor (dB under h1), and the gain (dB) at 8.4 and
/// 16.7 kHz (harmonic 32 / 8 / 4 and none / 16 / 8).
#[test]
fn the_quality_settings_sound_as_kontakts_do() {
    let keys = [60, 84, 96];
    let alias_of = |h: &Heard| h.alias;
    let gain_at = |h: &Heard, n: usize| h.gains.iter().find(|g| g.0 == n).map(|g| g.1);
    // LOFI: aliases 5..13 dB more than the others (Kontakt -29.7 / -24.3 / -21.5).
    for (key, want) in keys.iter().zip([-29.7, -24.3, -21.5]) {
        let a = alias_of(&play(Quality::Lofi, *key));
        assert!(
            (a - want).abs() < 3.0,
            "LOFI key {key}: alias {a:.1}, Kontakt {want}"
        );
    }
    // MEDIUM: flat to the top, the leakage floor (-34.0 / -34.2 / -33.4).
    for key in keys {
        let h = play(Quality::Medium, key);
        assert!(h.gains.iter().all(|g| g.1.abs() < 0.5), "MEDIUM key {key}");
        assert!(
            (alias_of(&h) + 34.0).abs() < 1.0,
            "MEDIUM key {key}: {:.1}",
            alias_of(&h)
        );
    }
    // HIGH and BEST: the same droop at the same frequency whatever the note.
    for (q, mid, top) in [(Quality::High, -0.9, -3.8), (Quality::Best, -0.3, -3.6)] {
        for key in keys {
            let h = play(q, key);
            let (m, t) = match key {
                60 => (gain_at(&h, 32), None),
                84 => (gain_at(&h, 8), gain_at(&h, 16)),
                _ => (gain_at(&h, 4), gain_at(&h, 8)),
            };
            assert!(
                (m.unwrap() - mid).abs() < 0.4,
                "{q:?} key {key}: 8.4 kHz {m:?}, Kontakt {mid}"
            );
            if let Some(t) = t {
                assert!(
                    (t - top).abs() < 0.4,
                    "{q:?} key {key}: 16.7 kHz {t}, Kontakt {top}"
                );
            }
        }
    }
}
