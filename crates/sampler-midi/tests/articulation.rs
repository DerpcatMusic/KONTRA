use sampler_core::{
    Driver, Envelope, Keyswitch, Limits, Pcm, Playback, Prepared, Region, Runtime, SelectionPolicy,
    Selector, Switch, SwitchKeys, Switching, VelocityCurve,
};
use sampler_midi::{Articulator, Ingress, Intercept, Packets, Version};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

const LEVELS: [f32; 4] = [0.125, 0.25, 0.5, 1.0];

fn region(sample: usize, key: u8) -> Region {
    Region {
        sample,
        key_low: key,
        key_high: key,
        root_key: None,
        velocity_low: 0.,
        velocity_high: 1.,
        gain: 1.,
        envelope: Envelope::default(),
        playback: Playback::default(),
    }
}

/// Key 60 plays one level per articulation; keys 24..=26 switch natively
/// (unless `native_keys` is off) and key 24 also maps a zone of its own.
fn runtime(
    driver: Driver,
    keys: SwitchKeys,
    selectors: Vec<Selector>,
    native_keys: bool,
) -> Runtime {
    let switches = if native_keys {
        (0..3)
            .map(|a| Keyswitch {
                key: 24 + a as u8,
                articulation: a,
            })
            .collect()
    } else {
        Vec::new()
    };
    let plan = Prepared::new(
        48000,
        LEVELS
            .iter()
            .map(|&v| Pcm::new(48000, Box::from([[v; 2]; 64])).unwrap())
            .collect(),
        vec![region(0, 60), region(1, 60), region(2, 60), region(3, 24)],
        8,
    )
    .unwrap()
    .with_velocity_curves(vec![VelocityCurve::Constant; 4])
    .unwrap()
    .with_articulations(
        vec![Some(0), Some(1), Some(2), None],
        switches,
        SelectionPolicy::Onset,
        SelectionPolicy::Onset,
    )
    .unwrap()
    .with_switching(Switching::new(driver, keys, 24..=26, selectors).unwrap());
    Runtime::new(
        plan,
        Limits {
            notes: 8,
            channels: 16,
            performances: 1,
            families: 8,
            decisions: 8,
            expressions: 8,
            voices: 8,
            commands: 8,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap()
}

/// Selectors in articulation order for `driver`: velocity thirds, channels,
/// CC 32 values or programs 0..=2.
fn selectors(driver: Driver, tap: bool) -> Vec<Selector> {
    (0..3u8)
        .map(|a| {
            let (controller, low, high) = match driver {
                Driver::Velocity => (0, 1 + a * 42, 42 + a * 42),
                Driver::Controller => (32, a, a),
                _ => (0, a, a),
            };
            Selector {
                controller,
                low,
                high,
                switch: if tap {
                    Switch::Tap(24 + a)
                } else {
                    Switch::Articulation(u32::from(a))
                },
            }
        })
        .collect()
}

struct Rig {
    rt: Runtime,
    articulator: Articulator,
    ingress: Ingress,
}

impl Rig {
    fn new(rt: Runtime) -> Self {
        let mut groups = [None; 16];
        groups[0] = Some(Version::Midi1);
        let articulator = Articulator::new(&rt, rt.performance(0).unwrap(), 0).unwrap();
        Self {
            rt,
            articulator,
            ingress: Ingress::new(0, groups),
        }
    }

    /// Intercept, then forward what is not consumed. `true` when consumed.
    fn send(&mut self, word: u32) -> bool {
        let words = [word];
        let packet = Packets::new(&words).next().unwrap().unwrap();
        match self.articulator.intercept(&mut self.rt, packet).unwrap() {
            Intercept::Consumed(_) => true,
            Intercept::Forward => {
                self.ingress.apply(&mut self.rt, packet).unwrap();
                false
            }
        }
    }

    /// The level a note-on word produces, after releasing and clearing it.
    fn level(&mut self, on: u32) -> f32 {
        self.send(on);
        let mut out = [[0.; 2]; 4];
        self.rt.render(&mut out).unwrap();
        self.send(on & !0x0010_007f); // note-off, same channel and key
        self.rt.render(&mut [[0.; 2]; 4]).unwrap();
        self.rt.flush_ended(|_| true);
        out[0][0]
    }
}

fn note_on(channel: u8, key: u8, velocity: u8) -> u32 {
    0x2090_0000 | u32::from(channel) << 16 | u32::from(key) << 8 | u32::from(velocity)
}

#[test]
fn every_driver_selects_the_zones_the_keyswitch_selects() {
    // Reference: the original keyswitch keys.
    let mut keys = Rig::new(runtime(Driver::Keys, SwitchKeys::Keep, Vec::new(), true));
    let expected: Vec<f32> = (0..3)
        .map(|a| {
            keys.level(note_on(0, 24 + a, 100));
            keys.level(note_on(0, 60, 100))
        })
        .collect();
    assert_eq!(expected, LEVELS[..3]);
    for driver in [
        Driver::Velocity,
        Driver::Channel,
        Driver::Controller,
        Driver::Program,
    ] {
        let mut rig = Rig::new(runtime(
            driver,
            SwitchKeys::Keep,
            selectors(driver, false),
            true,
        ));
        support::without_heap(|| {
            for a in 0..3u8 {
                let level = match driver {
                    Driver::Velocity => rig.level(note_on(0, 60, 1 + a * 42)),
                    Driver::Channel => rig.level(note_on(a, 60, 100)),
                    Driver::Controller => {
                        assert!(rig.send(0x20b0_2000 | u32::from(a)));
                        rig.level(note_on(0, 60, 100))
                    }
                    _ => {
                        assert!(rig.send(0x20c0_0000 | u32::from(a) << 8));
                        rig.level(note_on(0, 60, 100))
                    }
                };
                assert_eq!(
                    level,
                    expected[usize::from(a)],
                    "{driver:?} articulation {a}"
                );
            }
        });
    }
}

#[test]
fn freed_keys_play_kept_keys_switch_and_swallowed_keys_vanish() {
    let program = |rig: &mut Rig, a: u8| assert!(rig.send(0x20c0_0000 | u32::from(a) << 8));
    // Freed: lowering installed no native switches, so key 24 plays its zone.
    let mut play = Rig::new(runtime(
        Driver::Program,
        SwitchKeys::Play,
        selectors(Driver::Program, false),
        false,
    ));
    program(&mut play, 2);
    assert_eq!(play.level(note_on(0, 24, 100)), 1.0);
    assert_eq!(play.level(note_on(0, 60, 100)), 0.5, "selection unchanged");
    // Kept: the key still switches, silently.
    let mut keep = Rig::new(runtime(
        Driver::Program,
        SwitchKeys::Keep,
        selectors(Driver::Program, false),
        true,
    ));
    program(&mut keep, 2);
    assert_eq!(keep.level(note_on(0, 25, 100)), 0.0);
    assert_eq!(keep.level(note_on(0, 60, 100)), 0.25);
    // Swallowed: neither the key nor its release reaches the runtime.
    let mut swallow = Rig::new(runtime(
        Driver::Program,
        SwitchKeys::Swallow,
        selectors(Driver::Program, false),
        true,
    ));
    program(&mut swallow, 2);
    assert!(swallow.send(note_on(0, 25, 100)));
    assert!(swallow.send(note_on(0, 25, 0)));
    assert_eq!(swallow.rt.note_count(), 0);
    assert_eq!(swallow.level(note_on(0, 60, 100)), 0.5);
}

#[test]
fn behavior_owned_switching_is_tapped_once_per_change() {
    let mut rig = Rig::new(runtime(
        Driver::Controller,
        SwitchKeys::Keep,
        selectors(Driver::Controller, true),
        false,
    ));
    let mut tapped = Vec::with_capacity(8);
    support::without_heap(|| {
        for value in [1u32, 1, 2] {
            assert!(rig.send(0x20b0_2000 | value));
            rig.rt.render(&mut [[0.; 2]; 4]).unwrap();
            rig.rt.flush_ended(|input| {
                tapped.push(input.key);
                true
            });
        }
        // An unrelated controller is not a driver and is forwarded.
        assert!(!rig.send(0x20b0_0740));
    });
    assert_eq!(tapped, [25, 26]);
}

#[test]
fn a_driver_remap_takes_effect_without_reloading_the_plan() {
    // Loaded with keyswitches only; Remap all then moves it to velocity.
    let mut rig = Rig::new(runtime(Driver::Keys, SwitchKeys::Keep, Vec::new(), true));
    assert_eq!(rig.level(note_on(0, 60, 1)), LEVELS[0]);
    rig.rt
        .set_switching(
            Switching::new(
                Driver::Velocity,
                SwitchKeys::Play,
                24..=26,
                selectors(Driver::Velocity, false),
            )
            .unwrap(),
            Vec::new(),
        )
        .unwrap();
    for a in 0..3u8 {
        assert_eq!(rig.level(note_on(0, 60, 1 + a * 42)), LEVELS[usize::from(a)]);
    }
    // Freed keys play their own zone instead of switching.
    assert_eq!(rig.level(note_on(0, 24, 100)), 1.0);
}
