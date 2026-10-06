//! Installed UVI banks whose modulation now translates: load, lower and render.
//! Runs only with `KONTRA_UVI_LIBRARIES` set to the folder holding the banks.
#![cfg(feature = "library-access")]

use sampler_core::{Input, Limits, Protocol, Runtime};
use sampler_ir as ir;
use std::path::Path;

const AUGMENTED: &str = "UVI - Augmented Orchestra v1.1.2-R2R/Augmented Orchestra.ufs";

/// Programs whose connections were all reported before (most did not even
/// parse: their node count exceeded the old XML limit), with a key they map.
const PROGRAMS: [(&str, u8); 3] = [
    // Velocity → gain, tempo-synced free sine LFO → pan, pitch bend.
    ("Presets/08 Ambient/Oceanic.uvip", 60),
    // Sine LFO → gain.
    ("Presets/04 Hybrid/Deep Brass.uvip", 60),
    // Sine LFO → pitch.
    ("Presets/01 Natural/Fragile Bow MW.uvip", 60),
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
