//! Exact selection contracts: source group IDs, composed starts and cycle reset.
use sampler_core::*;
use sampler_ir::{GroupStart, StartJoin, StartTest};

fn row(test: StartTest, next: StartJoin) -> GroupStart {
    GroupStart {
        slot: 0,
        test,
        next,
    }
}
fn input(id: i32, key: u8) -> Input {
    Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key,
        external_id: Some(id),
    }
}
fn runtime(groups: Vec<Vec<GroupStart>>, default: Option<u8>) -> Runtime {
    let regions = (0..groups.len())
        .map(|sample| Region {
            sample,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 0.1,
            envelope: Envelope::default(),
            playback: Playback::default(),
        })
        .collect();
    let pcm = (0..groups.len())
        .map(|_| Pcm::new(48000, Box::from([[1.; 2]; 4800])).unwrap())
        .collect();
    let count = groups.len();
    let plan = Prepared::new(48000, pcm, regions, count)
        .unwrap()
        .with_groups(count as u32, (0..count as u32).map(Some).collect())
        .unwrap()
        .with_native_criteria(groups, vec![Some(12), Some(13)], default)
        .unwrap();
    let mut rt = Runtime::new(
        plan,
        Limits {
            notes: 16,
            channels: 1,
            performances: 1,
            families: 16,
            voices: 16,
            expressions: 16,
            decisions: 0,
            commands: 32,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap();
    rt.record_selections(true);
    rt
}
fn selected(rt: &mut Runtime, id: i32) -> Vec<usize> {
    let n = rt.trigger(input(id, 60), 60, 1.).unwrap();
    let regions = rt
        .take_selection_records()
        .last()
        .unwrap()
        .candidates
        .iter()
        .filter(|v| v.rejected.is_none())
        .map(|v| v.region)
        .collect();
    rt.key_up(n, None).unwrap();
    rt.render(&mut [[0.; 2]; 1]).unwrap();
    rt.flush_ended(|_| true);
    regions
}
#[test]
fn multiple_key_rows_controller_and_compound_joins() {
    let key12 = StartTest::Key { low: 12, high: 12 };
    let key13 = StartTest::Key { low: 13, high: 13 };
    let cc = StartTest::Controller {
        controller: 1,
        low: 40,
        high: 80,
    };
    let mut rt = runtime(
        vec![
            vec![row(key12, StartJoin::Or), row(key13, StartJoin::And)],
            vec![row(key13, StartJoin::And), row(cc, StartJoin::And)],
            vec![row(key13, StartJoin::AndNot), row(cc, StartJoin::And)],
        ],
        Some(13),
    );
    assert_eq!(selected(&mut rt, 1), [0, 2]);
    let p = rt.performance(0).unwrap();
    rt.set_controller(p, 1, (u64::from(u32::MAX) * 40 / 127) as u32)
        .unwrap();
    assert_eq!(selected(&mut rt, 2), [0, 1]);
    rt.set_articulation(p, 0).unwrap();
    assert_eq!(selected(&mut rt, 3), [0]);
}
#[test]
fn rr_has_exact_phase_and_random_reset_repeats_a_distribution() {
    let mut rt = runtime(
        vec![
            vec![row(StartTest::RoundRobin(1), StartJoin::And)],
            vec![row(StartTest::RoundRobin(2), StartJoin::And)],
        ],
        None,
    );
    assert_eq!(selected(&mut rt, 1), [0]);
    assert_eq!(selected(&mut rt, 2), [1]);
    rt.reset_native_cycles(42);
    assert_eq!(selected(&mut rt, 3), [0]);
    let mut rt = runtime(vec![vec![row(StartTest::Random, StartJoin::And)]; 3], None);
    rt.reset_native_cycles(42);
    let before: Vec<_> = (0..12).map(|i| selected(&mut rt, i)).collect();
    rt.reset_native_cycles(42);
    let after: Vec<_> = (12..24).map(|i| selected(&mut rt, i)).collect();
    assert_eq!(before, after);
    assert!(before.iter().all(|v| v.len() == 1));
    assert!(
        before
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            > 1
    );
}
