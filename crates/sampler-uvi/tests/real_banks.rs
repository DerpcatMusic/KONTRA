//! Installed UVI banks whose modulation now translates: load, lower and render.
//! Runs only with `KONTRA_UVI_LIBRARIES` set to the folder holding the banks.
#![cfg(feature = "library-access")]

use sampler_core::Limits;
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
        // These authored programs require Lua for note/oscillator selection
        // and live values. The bare Runtime route exercised by the old test
        // deliberately reported that it had no Lua frontend.
        let program_data =
            sampler_uvi::load_program_scripted_with_options(&bank, &member, &audition_options(key))
                .unwrap();
        let ir = &program_data.instrument;
        let routed = ir.zones.iter().filter(|z| !z.routes.is_empty()).count();
        assert!(routed > 0, "{program}: no zone has modulation routes");
        // Pitch bend → pitch lowers to native bend; something else must run.
        assert!(
            ir.routes
                .iter()
                .any(|r| ir.modulators[r.source.0].source != ir::ModulationSource::PitchBend),
            "{program}: only pitch bend routes"
        );

        assert!(
            ir.behaviors.iter().any(|b| b.language == ir::Language::Lua),
            "{program}: no authored Lua"
        );
        let route_count = ir.routes.len();
        let script_sources = ir
            .modulators
            .iter()
            .filter(|m| {
                matches!(
                    m.source,
                    ir::ModulationSource::Script(_) | ir::ModulationSource::ScriptBipolar(_)
                )
            })
            .count();
        let mut player = sampler_uvi::scripted::Player::new(program_data, limits(), 48000).unwrap();
        controllers(&mut player);
        player.note_on(key, 0.8).unwrap();
        let mut out = vec![[0.0f32; 2]; 24000];
        let mut peak = 0f32;
        for _ in 0..4 {
            player.render(&mut out).unwrap();
            assert!(out.iter().flatten().all(|x| x.is_finite()), "{program}");
            peak = out.iter().flatten().fold(peak, |p, x| p.max(x.abs()));
        }
        assert!(peak > 1e-3, "{program}: peak {peak}");
        eprintln!(
            "{program}: {routed} routed zones, {route_count} routes, {script_sources} script sources, peak {peak}"
        );
    }
}

fn audition_options(key: u8) -> sampler_kontakt::Options {
    sampler_kontakt::Options {
        rate: 48000,
        keys: key..=key,
        // The native loudness-controller defaults and the script input below
        // describe the same explicit audition, rather than a silent power-on.
        dynamics_start: Some(100),
        ..Default::default()
    }
}

fn controllers(player: &mut sampler_uvi::scripted::Player) {
    for (cc, value) in [(1, 100), (11, 127)] {
        player.input(sampler_uvi::scripted::HostInput::Controller {
            cc,
            value,
            channel: 0,
        });
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
    let program =
        sampler_uvi::load_program_scripted_with_options(bank, member, &audition_options(key))
            .unwrap();
    let report: Vec<String> = program
        .instrument
        .unsupported
        .iter()
        .filter(|u| u.location == "script")
        .map(|u| format!("{} {}", u.feature, u.value))
        .collect();
    let mut player = sampler_uvi::scripted::Player::new(program, limits(), 48000).unwrap();
    controllers(&mut player);
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
    eprintln!(
        "Bartok scripted peak {peak}; {} script findings",
        report.len()
    );
    for line in &report {
        eprintln!("  {line}");
    }
    // Check explicit oscillator/layer commands rather than the peak range
    // measured before the current effect and live-modulation implementation.
    let (xml, _) = bank.program(&member).unwrap();
    let mut host = sampler_uvi::script::ScriptHost::new(
        &xml,
        bank.scripts(),
        sampler_uvi::script::Config {
            #[cfg(feature = "scan")]
            audit_seed: Some(42),
            ..Default::default()
        },
    )
    .unwrap();
    host.take_commands();
    host.controller(1, 100, 0);
    host.controller(11, 127, 0);
    host.note_on(1, 60, 102, 0);
    host.advance(2000.);
    let plays: Vec<_> = host
        .take_commands()
        .into_iter()
        .filter_map(|c| match c {
            sampler_uvi::script::Command::Play(play) => Some(play),
            _ => None,
        })
        .collect();
    assert!(!plays.is_empty(), "Bartok did not emit note commands");
    assert!(
        plays
            .iter()
            .all(|p| p.osc.is_some() && !p.layers.is_empty()),
        "Bartok must explicitly select its oscillator and layers"
    );
    let layers = plays.iter().fold(0u64, |mask, p| mask | p.layers.0);
    assert!(
        layers.count_ones() >= 2,
        "Bartok must address both source layers"
    );
    assert!(peak > 1e-3, "Bartok finite/audible witness: peak {peak}");
    eprintln!(
        "Bartok explicit commands={} layer_mask={layers} oscillator_indices={:?} command_probe_seed={:?} native_pcm_parity=unknown",
        plays.len(),
        plays
            .iter()
            .map(|p| p.osc.unwrap())
            .collect::<std::collections::BTreeSet<_>>(),
        cfg!(feature = "scan").then_some(42)
    );
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
    eprintln!(
        "VWinds scripted peak {peak}; {} script findings",
        report.len()
    );
    assert!(peak > 1e-3, "silent: {peak}");
}
