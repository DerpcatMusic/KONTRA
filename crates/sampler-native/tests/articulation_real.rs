//! A real keyswitched Kontakt instrument driven by CC 32 instead of its keys.
//! Skips unless `KONTRA_KONTAKT_LIBRARIES` holds the library.
use sampler_core::{Limits, Runtime};
use sampler_ir as ir;
use sampler_midi::{Articulator, Ingress, Intercept, Packets, Version};

const INSTRUMENT: &str =
    "Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/2 Horns KS.nki";
const KEY: u8 = 60;

fn load(driver: ir::Driver) -> Option<sampler_kontakt::Loaded> {
    let root = std::env::var_os("KONTRA_KONTAKT_LIBRARIES")?;
    let path = std::env::split_paths(&root)
        .map(|r| r.join(INSTRUMENT))
        .find(|p| p.is_file())?;
    let mut kontakt = sampler_kontakt::read(&path).unwrap();
    let mut instrument = kontakt.instrument;
    instrument.switching.driver = driver;
    instrument.switching.keys = ir::SwitchKeys::Swallow;
    let kept = instrument.retain_zones(|z| z.keys.low <= KEY && z.keys.high >= KEY);
    let pcm = kept
        .iter()
        .map(|&a| {
            let decoded = kontakt.samples.decode(&kontakt.locations[a]).unwrap();
            sampler_core::Pcm::new(decoded.rate, decoded.frames.into_boxed_slice()).unwrap()
        })
        .collect();
    let labels = kept.iter().map(|a| a.to_string()).collect();
    let options = sampler_kontakt::Options {
        keys: KEY..=KEY,
        ..Default::default()
    };
    Some(sampler_kontakt::finish(instrument, pcm, labels, &options).unwrap())
}

/// Render `words` (one per 64-frame step) through the articulator and ingress.
fn render(loaded: sampler_kontakt::Loaded, words: &[u32]) -> Vec<[f32; 2]> {
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
    let mut rt = Runtime::new(plan, limits).unwrap();
    let mut groups = [None; 16];
    groups[0] = Some(Version::Midi1);
    let mut ingress = Ingress::new(0, groups);
    let mut articulator = Articulator::new(&rt, rt.performance(0).unwrap(), 0).unwrap();
    let mut out = vec![[0.0; 2]; 64 * words.len() + 24000];
    for (i, &word) in words.iter().enumerate() {
        let words = [word];
        let packet = Packets::new(&words).next().unwrap().unwrap();
        if articulator.intercept(&mut rt, packet).unwrap() == Intercept::Forward {
            let _ = ingress.apply(&mut rt, packet);
        }
        rt.render(&mut out[i * 64..(i + 1) * 64]).unwrap();
    }
    let tail = 64 * words.len();
    rt.render(&mut out[tail..]).unwrap();
    out
}

#[test]
fn afflatus_cc_selects_what_its_keyswitch_selects() {
    let note = 0x0090_0000 | u32::from(KEY) << 8 | 100;
    let Some(keys) = load(ir::Driver::Keys) else {
        eprintln!("skipped: {INSTRUMENT} is not installed");
        return;
    };
    let failed: Vec<_> = keys
        .instrument
        .unsupported
        .iter()
        .filter(|u| u.feature == "script")
        .collect();
    if !failed.is_empty() {
        // The script owns switching; unbound, the plan carries no driver, so
        // nothing taps keys into a runtime with no script to read them.
        let cc = load(ir::Driver::Controller).unwrap();
        assert_eq!(
            cc.instrument.articulations.len(),
            11,
            "the map is still reported"
        );
        assert_eq!(cc.plan.switching().driver(), sampler_core::Driver::Keys);
        eprintln!("switching script not compiled yet, driver withheld: {failed:?}");
        return;
    }
    // Marcato is articulation 2: key 26, or CC 32 value 2.
    let by_key = render(keys, &[0x2090_1a64, 0x2080_1a00, 0x2000_0000 | note]);
    let by_cc = render(
        load(ir::Driver::Controller).unwrap(),
        &[0x20b0_2002, 0x2000_0000, 0x2000_0000 | note],
    );
    let default = render(
        load(ir::Driver::Controller).unwrap(),
        &[0x2000_0000, 0x2000_0000, 0x2000_0000 | note],
    );
    let peak = |out: &[[f32; 2]]| out.iter().flatten().fold(0f32, |p, x| p.max(x.abs()));
    assert!(peak(&by_cc) > 0.01, "audible: {}", peak(&by_cc));
    assert_eq!(by_cc, by_key, "CC 32 = 2 plays what key 26 selects");
    assert_ne!(by_cc, default, "and differs from the default articulation");
}
