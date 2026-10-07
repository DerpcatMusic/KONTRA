//! Which UVI programs answer per-note MPE bend and pressure: one program per
//! UFS bank, played through the host core like the plugin (script included).
//! Slow, read-only survey:
//! `KONTRA_UVI_LIBRARIES=... cargo test --release --test mpe_uvi -- --ignored --nocapture`,
//! then read the UVI-MPE lines. Breath (CC2) is held at 100 on the manager channel,
//! as a wind player would, before the note.
use std::path::{Path, PathBuf};

use kontakto::sound::{
    BlockInfo, Core, CoreLoader, LoadRequest,
    event::Event,
    mix::Mix,
    v2::{V2Core, V2Loader},
};

const RATE: f64 = 48000.0;
const FRAMES: usize = 128;
/// Frames the spectrum is read from (a power of two, as the Kontakt probe).
const WINDOW: usize = 8192;

fn banks(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut dirs = vec![root.to_owned()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("ufs")) {
                out.push(p);
            } else if p.is_dir() {
                dirs.push(p);
            }
        }
    }
    out.sort();
    out
}

fn render(core: &mut V2Core, frames: usize) -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    while out.len() < frames {
        std::thread::sleep(std::time::Duration::from_millis(2));
        core.begin_block(&BlockInfo {
            frames: FRAMES,
            ..Default::default()
        });
        let r = core.render(FRAMES);
        out.extend((0..FRAMES).map(|i| [r.buses[0][0][i], r.buses[0][1][i]]));
        core.end_block(FRAMES, &mut |_| true);
    }
    out
}

/// Keys to try, middle C first, then through the mapped range: a program
/// whose script plays only part of its mapping is silent on the others.
fn candidates(path: &Path) -> Vec<u8> {
    let request = LoadRequest {
        path: path.to_owned(),
        sample_rate: RATE,
        mpe: true,
        ..Default::default()
    };
    let zones: Vec<_> = V2Loader
        .prepare(&request, &mut |_| {}, &|| false)
        .ok()
        .and_then(|l| l.instrument)
        .map(|i| i.zones.iter().map(|z| (z.keys.low, z.keys.high)).collect())
        .unwrap_or_default();
    let low = zones.iter().map(|z| u16::from(z.0)).min().unwrap_or(48);
    let high = zones.iter().map(|z| u16::from(z.1)).max().unwrap_or(84);
    let mut keys = vec![
        60,
        ((low + high) / 2) as u8,
        (low + (high - low) / 4) as u8,
        (low + 3 * (high - low) / 4) as u8,
    ];
    keys.dedup();
    keys
}

fn rms(x: &[[f32; 2]]) -> f64 {
    let s: f64 = x
        .iter()
        .map(|f| f64::from(f[0]).powi(2) + f64::from(f[1]).powi(2))
        .sum();
    (s / (2 * x.len()) as f64).sqrt()
}

/// One note on member channel 1; `send` goes after the first window.
fn run(path: &Path, key: u8, send: Option<u32>) -> Vec<[f32; 2]> {
    let request = LoadRequest {
        path: path.to_owned(),
        sample_rate: RATE,
        mpe: true,
        ..Default::default()
    };
    let loaded = V2Loader
        .prepare(&request, &mut |_| {}, &|| false)
        .unwrap_or_else(|e| panic!("{}: {e:?}", path.display()));
    let mut mix = Mix::default();
    let c = &mut mix.parts[0];
    (c.channel, c.port, c.output, c.mpe) = (-1, 0, 0, true);
    let mut core = V2Core::with_parts(1, RATE);
    core.set_mix(&mix);
    core.install(0, loaded.part);
    core.event(0, Event::midi1(0xB0, 2, 100));
    core.event(0, Event::Ump([0x2091_0000 | u32::from(key) << 8 | 100, 0]));
    // Let the script and any stream start, then read a window before and after.
    let _ = render(&mut core, 4 * WINDOW);
    let before = render(&mut core, WINDOW);
    if let Some(word) = send {
        core.event(0, Event::Ump([word, 0]));
    }
    let after = render(&mut core, WINDOW);
    let _ = before;
    after
}

#[test]
#[ignore = "survey"]
fn uvi_mpe_response_per_bank() {
    let Some(roots) = std::env::var_os("KONTRA_UVI_LIBRARIES") else {
        return;
    };
    for root in std::env::split_paths(&roots) {
        for ufs in banks(&root) {
            let Ok(bank) = sampler_uvi::Bank::open(&ufs) else {
                println!("UVI-MPE-SKIP {}", ufs.display());
                continue;
            };
            let Some(member) = bank.programs().into_iter().next() else {
                continue;
            };
            let path = ufs.join(&member);
            let tried = candidates(&path);
            let mut result = None;
            for key in tried {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let plain = run(&path, key, None);
                    let bent = run(
                        &path,
                        key,
                        Some(0x20E1_0000 | (8533 & 127) << 8 | 8533 >> 7),
                    );
                    let pressed = run(&path, key, Some(0x20D1_0000 | 127 << 8));
                    let dark = run(&path, key, Some(0x20B1_0000 | 74 << 8));
                    (
                        sampler_midi::spectral_ratio(&plain, &bent),
                        20.0 * ((rms(&pressed) + 1e-12) / (rms(&plain) + 1e-12)).log10(),
                        rms(&plain),
                        sampler_midi::centroid(&dark) / sampler_midi::centroid(&plain).max(1e-9),
                    )
                }));
                let silent = matches!(&r, Ok((_, _, level, _)) if *level < 1e-6);
                result = Some(r);
                if !silent {
                    break;
                }
            }
            let Some(result) = result else { continue };
            match result {
                Ok((ratio, db, level, tilt)) => println!(
                    "UVI-MPE {} pitch {} ({ratio:.3}) pressure {} ({db:.1} dB) timbre {} ({tilt:.2}) level {:.1} dBFS",
                    path.display(),
                    if (1.06..=1.19).contains(&ratio) {
                        "ok"
                    } else {
                        "NO"
                    },
                    if db.abs() >= 1.0 { "ok" } else { "NO" },
                    if tilt < 0.95 { "ok" } else { "NO" },
                    20.0 * (level + 1e-12).log10()
                ),
                Err(_) => println!("UVI-MPE-ERR {}", path.display()),
            }
        }
    }
}
