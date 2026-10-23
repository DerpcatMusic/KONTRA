//! Lists the zones of one NKI that match the given keys and velocities, and
//! decodes only their samples (KONTRA's own loader and sample reader) to mono
//! f32 files, for tools/oracle/attribute.py.
//! Usage: zone_samples INSTRUMENT.nki OUT_DIR KEYS VELOCITIES
//!   KEYS / VELOCITIES: comma lists (e.g. 60,64,67 and 100,90,110), or `-` for all.
//! Prints `zone` lines (zone index, group, key range, velocity range, root key,
//! cents per key, tune in semitones, asset, articulation) and `asset` lines
//! (asset index, sample rate, frame count). Samples land in OUT_DIR/asset<N>.f32.
use std::io::Write;
use std::path::PathBuf;

fn list(arg: &str) -> Option<Vec<u32>> {
    (arg != "-").then(|| arg.split(',').map(|v| v.parse().unwrap()).collect())
}

fn main() {
    let mut args = std::env::args().skip(1);
    let nki = PathBuf::from(args.next().expect("usage: zone_samples NKI OUT_DIR KEYS VELS"));
    let out = PathBuf::from(args.next().expect("OUT_DIR"));
    let keys = list(&args.next().expect("KEYS")).unwrap_or_else(|| (0..128).collect());
    let vels = list(&args.next().expect("VELOCITIES")).unwrap_or_else(|| (1..128).collect());
    std::fs::create_dir_all(&out).unwrap();

    let mut kontakt = sampler_kontakt::read(&nki).unwrap();
    let mut decoded = std::collections::BTreeSet::new();
    for (index, zone) in kontakt.instrument.zones.iter().enumerate() {
        let k = &zone.keys;
        let v = &zone.velocities;
        let key_hit = keys.iter().any(|&key| (k.low as u32..=k.high as u32).contains(&key));
        let vel_hit = vels.iter().any(|&vel| (v.low as u32..=v.high as u32).contains(&vel));
        if !(key_hit && vel_hit) {
            continue;
        }
        let Some(asset) = zone.asset else { continue };
        let (root, cents) = match zone.pitch {
            sampler_ir::KeyTracking::Tracked { root } => (root as i32, 100),
            sampler_ir::KeyTracking::Scaled { root, cents_per_key } => (root as i32, cents_per_key),
            sampler_ir::KeyTracking::Fixed => (-1, 0),
        };
        let group = zone.group.map_or(-1, |g| g.0 as i64);
        let articulation = zone.articulation.map_or(-1, |a| a.0 as i64);
        println!(
            "zone\t{index}\tgroup\t{group}\tlow\t{}\thigh\t{}\tvlow\t{}\tvhigh\t{}\troot\t{root}\tcents\t{cents}\ttune\t{:.4}\tasset\t{}\tart\t{articulation}",
            k.low, k.high, v.low, v.high, zone.tune.semitones(), asset.0,
        );
        if decoded.insert(asset.0) {
            let location = kontakt.locations[asset.0].clone();
            match kontakt.samples.decode(&location) {
                Ok(audio) => {
                    let mut file = std::fs::File::create(out.join(format!("asset{}.f32", asset.0))).unwrap();
                    for frame in &audio.frames {
                        file.write_all(&((frame[0] + frame[1]) * 0.5).to_le_bytes()).unwrap();
                    }
                    println!("asset\t{}\trate\t{}\tframes\t{}", asset.0, audio.rate, audio.frames.len());
                }
                Err(error) => println!("asset\t{}\terror\t{error:?}", asset.0),
            }
        }
    }
}
