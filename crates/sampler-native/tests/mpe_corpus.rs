//! Which installed instruments answer per-note MPE bend and pressure.
//! Slow and read-only: `KONTRA_KONTAKT_LIBRARIES=... cargo test -p sampler-native
//! --test mpe_corpus -- --ignored --nocapture`, then read the MPE lines.
//! `KONTRA_MPE_LIMIT` caps instruments per library (default 1);
//! `KONTRA_MPE_SCRIPTS=0` loads without scripts.
use sampler_core::{Limits, Runtime};
use sampler_ir as ir;

#[path = "support/reference.rs"]
mod reference;

fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nki"))
        {
            out.push(path);
        }
    }
}

/// The instrument's zones under `key`, decoded once.
struct Decoded {
    instrument: ir::Instrument,
    pcm: Vec<sampler_core::Pcm>,
    labels: Vec<String>,
    options: sampler_kontakt::Options,
}

impl Decoded {
    fn runtime(&self) -> Runtime {
        let loaded = sampler_kontakt::finish(
            self.instrument.clone(),
            self.pcm.clone(),
            self.labels.clone(),
            &self.options,
        )
        .unwrap();
        let plan = loaded.plan;
        let limits = Limits {
            notes: 64,
            channels: 16,
            performances: 1,
            expressions: 64,
            families: 64,
            decisions: 256,
            voices: 512,
            commands: 256,
            behaviors: 16,
            behavior_fuel: 1 << 20,
            behavior_cells: plan.behavior_local_count().saturating_mul(16),
            note_cells: plan.note_cell_count().saturating_mul(64),
        };
        Runtime::new(plan, limits).unwrap()
    }
}

fn decode(path: &std::path::Path) -> Option<(Decoded, u8)> {
    let mut kontakt = sampler_kontakt::read(path).ok()?;
    let mut instrument = kontakt.instrument;
    let first = instrument.zones.first()?;
    let key = ((u16::from(first.keys.low) + u16::from(first.keys.high)) / 2) as u8;
    let kept = instrument.retain_zones(|z| z.keys.low <= key && z.keys.high >= key);
    let mut pcm = Vec::new();
    for &a in &kept {
        let d = kontakt.samples.decode(&kontakt.locations[a]).ok()?;
        pcm.push(sampler_core::Pcm::new(d.rate, d.frames.into_boxed_slice()).ok()?);
    }
    Some((
        Decoded {
            instrument,
            pcm,
            labels: kept.iter().map(|a| a.to_string()).collect(),
            options: sampler_kontakt::Options {
                library: Some(path.to_owned()),
                // KONTRA_MPE_SCRIPTS=0 plays the zones alone, to tell a script from the zones.
                scripts: std::env::var_os("KONTRA_MPE_SCRIPTS").is_none_or(|v| v != "0"),
                ..reference::options(key..=key)
            },
        },
        key,
    ))
}

#[test]
#[ignore = "survey"]
fn mpe_response_across_the_corpus() {
    let Some(roots) = std::env::var_os("KONTRA_KONTAKT_LIBRARIES") else {
        return;
    };
    let limit: usize = std::env::var("KONTRA_MPE_LIMIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    for root in std::env::split_paths(&roots) {
        let Ok(libraries) = std::fs::read_dir(&root) else {
            continue;
        };
        let mut libraries: Vec<_> = libraries.flatten().collect();
        libraries.sort_by_key(|l| l.path());
        for library in libraries {
            let mut instruments = Vec::new();
            collect(&library.path().join("Instruments"), &mut instruments);
            instruments.sort();
            for path in instruments.iter().take(limit) {
                let Some((d, key)) = decode(path) else {
                    println!("MPE-SKIP {}", path.display());
                    continue;
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    sampler_midi::mpe_response(|| d.runtime(), key)
                }));
                match result {
                    Ok(Ok(r)) => println!(
                        "MPE {} pitch {} ({:.3}) pressure {} ({:.1} dB)",
                        path.display(),
                        if r.pitch_responds() { "ok" } else { "NO" },
                        r.pitch_ratio,
                        if r.pressure_responds() { "ok" } else { "NO" },
                        r.pressure_db
                    ),
                    other => println!("MPE-ERR {} {other:?}", path.display()),
                }
            }
        }
    }
}
