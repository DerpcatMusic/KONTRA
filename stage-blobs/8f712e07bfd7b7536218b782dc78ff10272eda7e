//! Lowering IR articulation switching into the plan's driver table.
use sampler_core::lower::lower;
use sampler_core::{Input, Limits, Pcm, Protocol, Runtime, Switch};
use sampler_ir as ir;

const LEVELS: [f32; 4] = [0.125, 0.25, 0.5, 1.0];

fn input(key: u8) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: None,
    }
}

/// Key 60 plays one level per articulation, switched by keys 24..=26; key 24
/// also has a zone. Articulation 1 is the default.
fn instrument(switching: ir::Switching) -> ir::Instrument {
    let zone = |asset, key, articulation: Option<usize>| ir::Zone {
        keys: ir::KeyRange {
            low: key,
            high: key,
        },
        pitch: ir::KeyTracking::Fixed,
        velocity: ir::VelocityResponse::None,
        articulation: articulation.map(ir::ArticulationRef),
        ..ir::Zone::new(ir::AssetRef(asset))
    };
    let mut ir = ir::Instrument {
        assets: (0..4)
            .map(|i| ir::Asset {
                location: ir::AssetLocation::Path(format!("{i}.wav")),
                encoding: ir::Encoding::Wav,
                root_key: None,
                loops: Vec::new(),
            })
            .collect(),
        zones: vec![
            zone(0, 60, Some(0)),
            zone(1, 60, Some(1)),
            zone(2, 60, Some(2)),
            zone(3, 24, None),
        ],
        articulations: (0..3u8)
            .map(|a| ir::Articulation {
                name: format!("{a}"),
                switch_keys: vec![24 + a],
                default: a == 1,
                ..Default::default()
            })
            .collect(),
        switching,
        ..Default::default()
    };
    ir.assign_alternatives(32);
    if switching.owner == ir::SwitchOwner::Behavior {
        ir.zones.iter_mut().for_each(|z| z.articulation = None);
    }
    ir
}

fn runtime(ir: &ir::Instrument) -> Runtime {
    let pcm = LEVELS
        .iter()
        .map(|&v| Pcm::new(48000, vec![[v; 2]; 64].into_boxed_slice()).unwrap())
        .collect();
    let plan = lower(ir, 48000, pcm, |_, plan| Ok(plan)).unwrap();
    Runtime::new(
        plan,
        Limits {
            notes: 8,
            channels: 1,
            performances: 1,
            families: 8,
            expressions: 8,
            voices: 8,
            decisions: 8,
            commands: 8,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap()
}

fn level(rt: &mut Runtime, key: u8) -> f32 {
    rt.trigger(input(key), key, 1.0).unwrap();
    let mut out = [[0.0; 2]; 4];
    rt.render(&mut out).unwrap();
    rt.note_off(input(key), None).unwrap();
    rt.render(&mut [[0.0; 2]; 4]).unwrap();
    rt.flush_ended(|_| true);
    out[0][0]
}

#[test]
fn native_selectors_name_the_articulation_their_keyswitch_selects() {
    let ir = instrument(ir::Switching {
        owner: ir::SwitchOwner::Native,
        driver: ir::Driver::Controller,
        keys: ir::SwitchKeys::Play,
    });
    let mut rt = runtime(&ir);
    let domain = rt.performance(0).unwrap();
    assert_eq!(level(&mut rt, 60), LEVELS[1], "default first");
    for a in 0..3u8 {
        let Some(Switch::Articulation(id)) = rt.switching().select(32, a) else {
            panic!("CC 32 value {a}");
        };
        rt.set_articulation(domain, id).unwrap();
        assert_eq!(level(&mut rt, 60), LEVELS[usize::from(a)]);
    }
    // Freed: the keyswitch key plays its own zone instead of switching.
    assert_eq!(level(&mut rt, 24), LEVELS[3]);
    assert!(rt.switching().is_switch_key(24));

    let mut keys = runtime(&instrument(ir::Switching::default()));
    assert_eq!(
        level(&mut keys, 24),
        0.0,
        "native keyswitch consumes the key"
    );
    assert_eq!(level(&mut keys, 60), LEVELS[0]);
}

#[test]
fn behavior_owned_switching_taps_keys_and_installs_no_native_switches() {
    let ir = instrument(ir::Switching {
        owner: ir::SwitchOwner::Behavior,
        driver: ir::Driver::Program,
        keys: ir::SwitchKeys::Keep,
    });
    let mut rt = runtime(&ir);
    assert_eq!(rt.switching().select(0, 2), Some(Switch::Tap(26)));
    assert_eq!(
        level(&mut rt, 24),
        LEVELS[3],
        "the key reaches note admission"
    );
}
