//! Regression tests for oscillator preparation: ZoneFacts retention (active
//! and purged), waking purged oscillator groups, and Cycle slot assignment.
#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]

use std::io;

use kontra_engine::{Atom, RrCounter};
use kontra_ir as ir;
use kontra_prepare::{prepare, wake};
use kontra_stream::{Store, StoreConfig};

fn osc_zone(id: u32, slot: u16) -> ir::Zone {
    ir::Zone {
        id,
        sample: 0,
        keys: 0..=127,
        velocities: 1..=127,
        velocity_fade: (0, 0),
        key_fade: (0, 0),
        crossfades: Vec::new(),
        root: 69,
        tune: 0.0,
        volume_db: 0.0,
        pan: 0.0,
        start: 0,
        end: 0,
        start_mod: 0,
        loops: Vec::new(),
        reverse: false,
        mix: Vec::new(),
        slot,
        envelopes: Vec::new(),
    }
}

#[test]
fn oscillator_zones_keep_zone_facts_cycle_positions_and_wake_from_purged() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let inst = ir::Instrument {
        groups: vec![
            ir::Group {
                name: "ActiveOsc".into(),
                generator: Some(ir::Generator::Oscillator {
                    wave: ir::Wave::Saw,
                    tune_semitones: 0.0,
                    phase: 0.0,
                }),
                start: vec![(ir::Join::And, ir::StartRule::Cycle { random: false })],
                zones: vec![osc_zone(10, 0), osc_zone(11, 1)],
                ..ir::Group::DEFAULT
            },
            ir::Group {
                name: "PurgedOsc".into(),
                purged: true,
                generator: Some(ir::Generator::Oscillator {
                    wave: ir::Wave::Sine,
                    tune_semitones: 0.0,
                    phase: 0.0,
                }),
                zones: vec![osc_zone(20, 0)],
                ..ir::Group::DEFAULT
            },
        ],
        ..ir::Instrument::DEFAULT
    };

    let open = |_: &ir::Sample| Err(io::Error::new(io::ErrorKind::NotFound, "unused"));
    let (plan, diags) = prepare(&inst, 48_000, &mut store, &open);
    assert!(diags.is_empty(), "{diags:?}");

    // All three oscillator zones (2 active + 1 purged) are present in `plan.zone_facts`.
    let fact_ids: Vec<u32> = plan.zone_facts.iter().map(|f| f.id).collect();
    assert_eq!(fact_ids, [10, 11, 20]);

    // Cycle positions from `zone.slot` are assigned via `ZoneRules::apply`.
    assert_eq!(plan.zones.len(), 2);
    let positions: Vec<u8> = plan
        .zones
        .iter()
        .map(|z| {
            z.start
                .iter()
                .find_map(|(_, atom)| match atom {
                    Atom::RoundRobin(rr) if rr.counter == RrCounter::Group => Some(rr.position),
                    _ => None,
                })
                .expect("group cycle atom")
        })
        .collect();
    assert_eq!(positions, [0, 1]);

    // Waking the purged oscillator group restores its playable zone and clears Dormant.
    let (woken, wake_diags, woken_count) =
        wake(&plan, 1, &mut store, &open).expect("wake purged oscillator group");
    assert!(wake_diags.is_empty(), "{wake_diags:?}");
    assert_eq!(woken_count, 0);
    assert!(woken.dormant.is_empty());
    assert_eq!(woken.zones.len(), 3);
    assert_eq!(woken.zones[2].id, 20);
    assert_eq!(woken.zones[2].group, 1);
}
