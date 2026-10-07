//! Installed UVI banks whose modulation now translates: load, lower and render.
//! Runs only with `KONTRA_UVI_LIBRARIES` set to the folder holding the banks.
#![cfg(feature = "library-access")]

use sampler_core::{Input, Limits, Protocol, Runtime};
use sampler_ir as ir;
use std::path::Path;

const AUGMENTED: &str = "UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs";

/// Programs whose connections were all reported before (most did not even
/// parse: their node count exceeded the old XML limit), with a key they map.
const PROGRAMS: [(&str, u8); 5] = [
    // Velocity → gain, tempo-synced free sine LFO → pan, pitch bend.
    ("Presets/08 Ambient/Oceanic.uvip", 60),
    // Sine LFO → gain.
    ("Presets/04 Hybrid/Deep Brass.uvip", 60),
    // Sine LFO → pitch.
    ("Presets/01 Natural/Fragile Bow MW.uvip", 60),
    // MultiLFO sine + noise → pitch, step envelopes → pan.
    ("Presets/00 Orchestra/01 Strings/V Strings Bartok.uvip", 60),
    // Step envelope → gain.
    ("Presets/02 Action/City Walker.uvip", 60),
];

#[test]
fn installed_programs_render_their_modulation() {
    let Some(root) = std::env::var_os("KONTRA_UVI_LIBRARIES") else {
        eprintln!("KONTRA_UVI_LIBRARIES unset: skipped");
        return;
    };
    let bank = sampler_uvi::Bank::open(&Path::new(&root).join(AUGMENTED)).unwrap();
    for (program, key) in PROGRAMS {
        let member = bank
            .programs()
            .into_iter()
            .find(|p| p.trim_start_matches('/') == program)
            .unwrap_or_else(|| panic!("{program} not in bank"));
        let loaded = sampler_uvi::load_program(&bank, &member, 48000).unwrap();
        let ir = &loaded.instrument;
        let routed = ir.zones.iter().filter(|z| !z.routes.is_empty()).count();
        assert!(routed > 0, "{program}: no zone has modulation routes");
        // Pitch bend → pitch lowers to native bend; something else must run.
        assert!(
            ir.routes
                .iter()
                .any(|r| ir.modulators[r.source.0].source != ir::ModulationSource::PitchBend),
            "{program}: only pitch bend routes"
        );

        let limits = Limits {
            notes: 16,
            channels: 1,
            performances: 1,
            families: 16,
            expressions: 16,
            voices: 256,
            decisions: 256,
            commands: 64,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        };
        let mut rt = Runtime::new(loaded.plan, limits).unwrap();
        let input = Input {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
            key,
            external_id: None,
        };
        rt.trigger(input, key, 0.8).unwrap();
        let mut out = vec![[0.0f32; 2]; 24000];
        let mut peak = 0f32;
        for _ in 0..4 {
            rt.render(&mut out).unwrap();
            assert!(out.iter().flatten().all(|x| x.is_finite()), "{program}");
            peak = out.iter().flatten().fold(peak, |p, x| p.max(x.abs()));
        }
        assert!(peak > 1e-3, "{program}: peak {peak}");
        eprintln!(
            "{program}: {routed} routed zones, {} routes, peak {peak}",
            ir.routes.len()
        );
    }
}

fn limits() -> Limits {
    Limits {
        notes: 64,
        channels: 1,
        performances: 1,
        families: 64,
        expressions: 64,
        voices: 512,
        decisions: 512,
        commands: 256,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    }
}

/// Plays `key` through the program's Lua scripts; returns the peak and what
/// the scripts and runtime left unmodeled.
fn scripted_peak(bank: &sampler_uvi::Bank, member: &str, key: u8) -> (f32, Vec<String>) {
    let program = sampler_uvi::load_program_scripted(bank, member, 48000).unwrap();
    let report: Vec<String> = program
        .instrument
        .unsupported
        .iter()
        .filter(|u| u.location == "script")
        .map(|u| format!("{} {}", u.feature, u.value))
        .collect();
    let mut player = sampler_uvi::scripted::Player::new(program, limits(), 48000).unwrap();
    player.note_on(key, 0.8).unwrap();
    let mut out = vec![[0.0f32; 2]; 24000];
    let mut peak = 0f32;
    for _ in 0..4 {
        player.render(&mut out).unwrap();
        assert!(out.iter().flatten().all(|x| x.is_finite()), "{member}");
        peak = out.iter().flatten().fold(peak, |p, x| p.max(x.abs()));
    }
    (peak, report)
}

#[test]
fn bartok_plays_one_oscillator_through_its_script() {
    let Some(root) = std::env::var_os("KONTRA_UVI_LIBRARIES") else {
        return;
    };
    let bank = sampler_uvi::Bank::open(&Path::new(&root).join(AUGMENTED)).unwrap();
    let member = bank
        .programs()
        .into_iter()
        .find(|p| p.trim_start_matches('/') == PROGRAMS[3].0)
        .unwrap();
    let (peak, report) = scripted_peak(&bank, &member, 60);
    eprintln!("Bartok scripted peak {peak}; {} script findings", report.len());
    for line in &report {
        eprintln!("  {line}");
    }
    // One oscillator per layer renders at about 1.08 (all 18 summed 3.27).
    assert!((0.5..1.6).contains(&peak), "peak {peak}");
}

#[test]
fn vwinds_sounds_through_its_script() {
    let Some(root) = std::env::var_os("KONTRA_UVI_LIBRARIES") else {
        return;
    };
    let bank =
        sampler_uvi::Bank::open(&Path::new(&root).join("VWinds - Clarinets/VWinds-BbClarinet.ufs"))
            .unwrap();
    let member = bank.programs().into_iter().next().unwrap();
    let (peak, report) = scripted_peak(&bank, &member, 60);
    eprintln!("VWinds scripted peak {peak}; {} script findings", report.len());
    assert!(peak > 1e-3, "silent: {peak}");
}
