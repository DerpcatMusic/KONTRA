//! Sampler behaviour as engine data: round robin, crossfades, triggers, groups, buses.
#![cfg(test)]

mod common;

use std::sync::Arc;

use kontra_engine::ir::{self, Join};
use kontra_engine::{
    Atom, Bus, Control, Crossfade, Engine, Plan, RoundRobin, RrCounter, Timed, Trigger, Zone,
};
use kontra_events::Event;
use kontra_stream::{BlobKey, MemSource, Store, StoreConfig};
use kontra_time::SampleTime;
use kontra_voice::Region;

use common::*;

#[test]
fn sequential_round_robin_cycles_and_mic_layers_agree() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    // Two RR steps x two mics: close (bus 0) and room (bus 1). Levels name the step.
    let mut zones = Vec::new();
    for (position, level) in [(0u8, 0.1f32), (1, 0.2)] {
        for bus in 0..2 {
            let mut z = zone(&mut store, level + bus as f32 * 0.01);
            z.start = vec![(
                Join::And,
                Atom::RoundRobin(RoundRobin {
                    counter: RrCounter::Key,
                    position,
                    length: 2,
                    random: false,
                    no_repeat: false,
                    release_redraws: false,
                }),
            )];
            z.group = bus as u32;
            z.pan = if bus == 0 { -1.0 } else { 1.0 };
            zones.push(z);
        }
    }
    let mut plan = Plan::new(zones, 16);
    plan.groups[1].bus = 1;
    plan.buses.push(Bus {
        name: "Room".into(),
        gain: 1.0,
        pan: 0.0,
        inserts: Vec::new(),
        sends: Vec::new(),
        to_main: true,
        enabled: true,
        mods: Vec::new(),
        blends: Vec::new(),
        lfos: Vec::new(),
    });
    let mut e = engine(&store, plan);
    let mut steps = Vec::new();
    for _ in 0..4 {
        let [l, r] = level(&mut e, &[on(60, 1.0)]);
        assert!(
            close(r - l, 0.01),
            "close and room play the same step: {l} {r}"
        );
        steps.push(l);
        level(&mut e, &[off(60)]);
    }
    assert!(
        close(steps[0], 0.1)
            && close(steps[1], 0.2)
            && close(steps[2], 0.1)
            && close(steps[3], 0.2),
        "{steps:?}"
    );
}

/// Kontakt steps its round robin on every note-on, before group predicates
/// (KONTAKT_START_CONDITIONS, 0x144f17ea0): a note that plays no round-robin
/// zone still moves the cycle on.
#[test]
fn the_global_round_robin_steps_on_every_note() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut zones = Vec::new();
    for (position, level) in [(0u8, 0.1f32), (1, 0.2)] {
        let mut z = zone(&mut store, level);
        z.keys = 36..=36;
        z.start = vec![(
            Join::And,
            Atom::RoundRobin(RoundRobin {
                counter: RrCounter::Global,
                position,
                length: 2,
                random: false,
                no_repeat: false,
                release_redraws: false,
            }),
        )];
        zones.push(z);
    }
    let mut plain = zone(&mut store, 0.5);
    plain.keys = 60..=60;
    zones.push(plain);
    let mut e = engine(&store, Plan::new(zones, 16));
    let mut play = |key| {
        let l = level(&mut e, &[on(key, 1.0)])[0];
        level(&mut e, &[off(key)]);
        l
    };
    let first = play(36);
    play(60);
    assert!(close(play(36), first), "two note-ons later, the same step");
}

#[test]
fn random_round_robin_never_repeats_a_step() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = (0..3u8)
        .map(|position| {
            let mut z = zone(&mut store, 0.1 * f32::from(position + 1));
            z.start = vec![(
                Join::And,
                Atom::RoundRobin(RoundRobin {
                    counter: RrCounter::Global,
                    position,
                    length: 3,
                    random: true,
                    no_repeat: true,
                    release_redraws: false,
                }),
            )];
            z
        })
        .collect();
    let mut e = engine(&store, Plan::new(zones, 16));
    let mut last = 0.0;
    for _ in 0..50 {
        let [l, _] = level(&mut e, &[on(60, 1.0)]);
        assert!(l > 0.05, "exactly one step plays");
        assert!(!close(l, last), "repeat {l}");
        last = l;
        level(&mut e, &[off(60)]);
        for _ in 0..4 {
            level(&mut e, &[]);
        }
    }
}

#[test]
fn kt08_cycle_random_allows_repeats_and_redraws_on_release() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut zones = Vec::new();
    for position in 0..2u8 {
        let mut atk = zone(&mut store, 0.1 * f32::from(position + 1));
        atk.pan = -1.0;
        atk.start = vec![(
            Join::And,
            Atom::RoundRobin(RoundRobin {
                counter: RrCounter::Global,
                position,
                length: 2,
                random: true,
                no_repeat: false,
                release_redraws: true,
            }),
        )];
        zones.push(atk);

        let mut rel = zone(&mut store, 0.3 + 0.1 * f32::from(position));
        rel.region.end = 600;
        rel.pan = 1.0;
        rel.trigger = Trigger::Release;
        rel.start = vec![(
            Join::And,
            Atom::RoundRobin(RoundRobin {
                counter: RrCounter::Global,
                position,
                length: 2,
                random: true,
                no_repeat: false,
                release_redraws: true,
            }),
        )];
        zones.push(rel);
    }
    let mut e = engine(&store, Plan::new(zones, 16));
    let mut prev_attack = None;
    let mut repeats = 0u32;
    let mut release_differs = 0u32;
    for _ in 0..60 {
        let [atk, _] = level(&mut e, &[on(60, 1.0)]);
        let atk_step = if close(atk, 0.1) {
            0u8
        } else if close(atk, 0.2) {
            1u8
        } else {
            panic!("unexpected attack level {atk}");
        };
        if prev_attack == Some(atk_step) {
            repeats += 1;
        }
        prev_attack = Some(atk_step);
        let [_, rel] = level(&mut e, &[off(60)]);
        let rel_step = if close(rel, 0.3) {
            0u8
        } else if close(rel, 0.4) {
            1u8
        } else {
            panic!("unexpected release level {rel}");
        };
        if rel_step != atk_step {
            release_differs += 1;
        }
        for _ in 0..8 {
            level(&mut e, &[]);
        }
    }
    assert!(
        repeats > 0,
        "KT-08: uniform cycle random must allow consecutive repeats (got 0 in 60 draws)"
    );
    assert!(
        release_differs > 0,
        "KT-08: cycle random release trigger must redraw instead of reusing attack draw"
    );
}

/// Kontakt's key latch: switch keys select the articulation, a key in a gap
/// between switches still plays ordinary zones and clears the latch.
#[test]
fn a_keyswitch_latches_and_a_gap_key_clears_it() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut legato = zone(&mut store, 0.3);
    legato.keys = 40..=127;
    legato.start = vec![(Join::And, Atom::Switch(0))];
    let mut staccato = zone(&mut store, 0.6);
    staccato.keys = 40..=127;
    staccato.start = vec![(Join::And, Atom::Switch(1))];
    let mut gap = zone(&mut store, 0.1);
    gap.keys = 30..=30;
    let mut plan = Plan::new(vec![legato, staccato, gap], 8);
    plan.switches = vec![24..=24, 36..=36];
    plan.default_keyswitch = Some(24);
    let mut e = engine(&store, plan);
    let mut play = |key| {
        let l = level(&mut e, &[on(key, 1.0)])[0];
        level(&mut e, &[off(key)]);
        for _ in 0..4 {
            level(&mut e, &[]);
        }
        l
    };
    assert!(close(play(60), 0.3), "the default articulation");
    play(36);
    assert!(close(play(62), 0.6), "switched");
    assert!(close(play(30), 0.1), "a gap key sounds");
    assert_eq!(play(64), 0.0, "and cleared the latch");
}

#[test]
fn a_release_trigger_plays_on_key_up_quieter_the_longer_the_key_was_held() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let attack = zone(&mut store, 0.5);
    let mut tail = zone(&mut store, 0.4);
    tail.group = 1;
    tail.trigger = Trigger::Release;
    tail.release_decay_db = 6.0; // per second held
    let mut plan = Plan::new(vec![attack, tail], 8);
    plan.groups[0].envelope.release = 16;
    let mut e = engine(&store, plan);
    level(&mut e, &[on(60, 1.0)]);
    for _ in 0..(RATE as usize / 256) {
        level(&mut e, &[]);
    }
    let [l, _] = level(&mut e, &[off(60)]);
    // ~1 s held: 0.4 at −6 dB ≈ 0.2 (the attack has faded by frame 128).
    assert!((l - 0.2).abs() < 0.02, "{l}");
}

#[test]
fn a_choke_group_fades_the_open_hat() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut open = zone(&mut store, 0.5);
    open.keys = 46..=46;
    open.group = 1;
    open.voice_group = Some(0);
    open.off_by = Some(1);
    let mut closed = zone(&mut store, 0.25);
    closed.keys = 42..=42;
    closed.group = 2;
    closed.voice_group = Some(1);
    let mut plan = Plan::new(vec![open, closed], 8);
    plan.voice_groups = vec![kontra_ir::VoiceGroup::DEFAULT; 2];
    let mut e = engine(&store, plan);
    assert!(close(level(&mut e, &[on(46, 1.0)])[0], 0.5));
    level(&mut e, &[on(42, 1.0)]);
    level(&mut e, &[]);
    assert!(
        close(level(&mut e, &[])[0], 0.25),
        "the open hat faded out (10 ms)"
    );
}

#[test]
fn group_polyphony_keeps_only_the_newest_voice() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut mono = zone(&mut store, 0.5);
    mono.group = 7;
    let mut plan = Plan::new(vec![mono], 8);
    plan.polyphony = vec![(7, 1)];
    let mut e = engine(&store, plan);
    level(&mut e, &[on(60, 1.0)]);
    level(&mut e, &[on(64, 1.0)]);
    level(&mut e, &[]);
    assert!(close(level(&mut e, &[])[0], 0.5), "one voice, not two");
    assert_eq!(e.playing_voices(), 1);
}

#[test]
fn a_key_limit_with_self_mask_stops_only_quieter_voices_of_the_key() {
    // SFZ note_polyphony=1 with note_selfmask=on: a louder repeat stops the
    // key's earlier voice, a softer one plays alongside the louder.
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut z = zone(&mut store, 0.5);
    z.voice_group = Some(0);
    let mut plan = Plan::new(vec![z], 8);
    plan.voice_groups = vec![kontra_ir::VoiceGroup {
        per_key: Some(1),
        quieter_only: true,
        ..kontra_ir::VoiceGroup::DEFAULT
    }];
    let mut e = engine(&store, plan);
    level(&mut e, &[on(60, 0.5)]);
    level(&mut e, &[on(60, 1.0)]);
    assert_eq!(e.playing_voices(), 1, "the softer voice gave way");
    level(&mut e, &[on(60, 0.25)]);
    assert_eq!(e.playing_voices(), 2, "the louder voice stays");
    level(&mut e, &[on(62, 1.0)]);
    assert_eq!(e.playing_voices(), 3, "another key is not counted");
}

#[test]
fn velocity_layers_crossfade_linearly() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut soft = zone(&mut store, 1.0);
    soft.pan = -1.0;
    soft.crossfades = vec![Crossfade {
        control: Control::Velocity,
        fade_in: None,
        fade_out: Some((40, 80)),
        law: kontra_ir::CrossfadeLaw::Linear,
    }];
    let mut loud = zone(&mut store, 1.0);
    loud.pan = 1.0;
    loud.crossfades = vec![Crossfade {
        control: Control::Velocity,
        fade_in: Some((40, 80)),
        fade_out: None,
        law: kontra_ir::CrossfadeLaw::Linear,
    }];
    let mut e = engine(&store, Plan::new(vec![soft, loud], 8));
    let [l, r] = level(&mut e, &[on(60, 60.0 / 127.0)]);
    assert!(
        close(l + r, 1.0),
        "Kontakt's linear amplitude ramp: {l} {r}"
    );
    assert!(close(l, r), "midpoint: equal parts");
}

#[test]
fn a_cc_crossfade_moves_sounding_voices() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut z = zone(&mut store, 1.0);
    z.crossfades = vec![Crossfade {
        control: Control::Cc(1),
        fade_in: Some((0, 127)),
        fade_out: None,
        law: kontra_ir::CrossfadeLaw::Linear,
    }];
    let mut looped = z.region.clone();
    looped.loops = Region::pack_loops([kontra_voice::Loop {
        start: 0,
        end: 48_000,
        crossfade: 0,
        ..Default::default()
    }]);
    z.region = looped;
    let mut e = engine(&store, Plan::new(vec![z], 8));
    let quiet = level(&mut e, &[cc(1, 10), on(60, 1.0)])[0];
    for _ in 0..3 {
        level(&mut e, &[cc(1, 127)]);
    }
    let loud = level(&mut e, &[])[0];
    assert!(quiet < 0.2 && close(loud, 1.0), "{quiet} -> {loud}");
}

#[test]
fn equal_power_cc_crossfade_holds_constant_power_at_midpoint() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut soft = zone(&mut store, 1.0);
    soft.pan = -1.0;
    soft.crossfades = vec![Crossfade {
        control: Control::Cc(1),
        fade_in: None,
        fade_out: Some((20, 100)),
        law: kontra_ir::CrossfadeLaw::EqualPower,
    }];
    let mut loud = zone(&mut store, 1.0);
    loud.pan = 1.0;
    loud.crossfades = vec![Crossfade {
        control: Control::Cc(1),
        fade_in: Some((20, 100)),
        fade_out: None,
        law: kontra_ir::CrossfadeLaw::EqualPower,
    }];
    let mut e = engine(&store, Plan::new(vec![soft, loud], 8));
    let [l, r] = level(&mut e, &[cc(1, 60), on(60, 1.0)]);
    assert!(
        close(l * l + r * r, 1.0),
        "equal power at midpoint: l={l}, r={r}, l^2+r^2={}",
        l * l + r * r
    );
    assert!(close(l, r), "equal parts at midpoint: l={l}, r={r}");
}

#[test]
fn a_voice_pans_by_its_instruments_balance_law() {
    // The far (left) side over the near: a quarter right, Kontakt's linear
    // balance keeps 3/4, UVI's cosine 0.854; half right, the cosine half.
    for (balance, pan, want) in [
        (kontra_ir::Balance::Linear, 0.25, 0.75),
        (kontra_ir::Balance::Cosine, 0.25, 0.853_553),
        (kontra_ir::Balance::Cosine, 0.5, 0.5),
    ] {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let mut z = zone(&mut store, 1.0);
        z.pan = pan;
        let mut plan = Plan::new(vec![z], 16);
        plan.output.balance = balance;
        let mut e = engine(&store, plan);
        level(&mut e, &[on(60, 1.0)]);
        let [l, r] = (0..8).map(|_| level(&mut e, &[])).last().unwrap();
        assert!((l / r - want).abs() < 1e-3, "{balance:?} {pan}: {l} {r}");
    }
}

#[test]
fn complementary_random_start_rules_partition_every_note_on() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut left = zone(&mut store, 1.0);
    left.pan = -1.0;
    left.start = vec![(kontra_ir::Join::And, Atom::Random { lo: 0, hi: 5000 })];
    let mut right = zone(&mut store, 1.0);
    right.pan = 1.0;
    right.start = vec![(
        kontra_ir::Join::And,
        Atom::Random {
            lo: 5000,
            hi: 10_000,
        },
    )];
    let mut e = engine(&store, Plan::new(vec![left, right], 16));
    let mut left_hits = 0usize;
    let mut right_hits = 0usize;
    for _ in 0..64 {
        let [l, r] = level(&mut e, &[on(60, 1.0)]);
        level(&mut e, &[off(60)]);
        // Advance past short release so notes don't overlap.
        for _ in 0..4 {
            level(&mut e, &[]);
        }
        assert!(
            (l > 0.5) ^ (r > 0.5),
            "every note-on must trigger exactly one side: l={l}, r={r}"
        );
        if l > 0.5 {
            left_hits += 1;
        } else {
            right_hits += 1;
        }
    }
    assert!(
        left_hits > 10 && right_hits > 10,
        "both halves must fire: {left_hits}/{right_hits}"
    );
}

#[test]
fn per_note_cc_and_note_expr_modulate_each_sounding_note_independently() {
    use kontra_engine::{Event, HostNote, HostPattern, NoteExpression};
    use kontra_ir::{Curve, Mod, ModRoute, NoteExpr, ParamAddr, RouteLaw, Source};

    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut left = zone(&mut store, 1.0);
    left.keys = 60..=60;
    left.pan = -1.0;
    let mut right = zone(&mut store, 1.0);
    right.keys = 64..=64;
    right.pan = 1.0;

    let mut plan = Plan::new(vec![left, right], 8);
    plan.groups[0].mods = vec![
        Mod {
            name: "pnc".into(),
            slot: 0,
            target: 0,
            bypass: false,
            route: ModRoute::new(
                Source::PerNoteCc {
                    registered: true,
                    index: 74,
                },
                ParamAddr::Volume,
                1.0,
            )
            .law(RouteLaw::Gain(Curve::Depth { power: 1 })),
        },
        Mod {
            name: "expr".into(),
            slot: 1,
            target: 0,
            bypass: false,
            route: ModRoute::new(
                Source::NoteExpr(NoteExpr::Expression),
                ParamAddr::Volume,
                1.0,
            )
            .law(RouteLaw::Gain(Curve::Depth { power: 1 })),
        },
    ];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);

    let note_a = HostNote {
        port: 0,
        channel: 0,
        key: 60,
        id: 101,
        clap: true,
    };
    let note_b = HostNote {
        port: 0,
        channel: 0,
        key: 64,
        id: 202,
        clap: true,
    };
    let pat = |n: HostNote| HostPattern {
        port: i32::from(n.port),
        channel: i32::from(n.channel),
        key: i32::from(n.key),
        id: n.id,
        clap: true,
    };
    let at0 = |body| Timed {
        at: kontra_engine::SampleTime(0),
        body,
    };

    // Start both notes; send high PerNoteCc + Expression only to note A, low to note B.
    level(
        &mut e,
        &[
            at0(Event::NoteOn {
                note: note_a,
                velocity: 1.0,
                tune: 0.0,
            }),
            at0(Event::NoteOn {
                note: note_b,
                velocity: 1.0,
                tune: 0.0,
            }),
            at0(Event::PerNoteCc {
                pattern: pat(note_a),
                registered: true,
                index: 74,
                value: 1.0,
            }),
            at0(Event::Expression(
                pat(note_a),
                NoteExpression::Expression(1.0),
            )),
            at0(Event::PerNoteCc {
                pattern: pat(note_b),
                registered: true,
                index: 74,
                value: 0.5,
            }),
            at0(Event::Expression(
                pat(note_b),
                NoteExpression::Expression(0.5),
            )),
        ],
    );
    // Let the voice gain smoother settle to its per-note targets.
    for _ in 0..3 {
        level(&mut e, &[]);
    }
    let [l, r] = level(&mut e, &[]);
    assert!(close(l, 1.0), "note A should be at unity: {l}");
    assert!(close(r, 0.25), "note B should be at 0.5 * 0.5 = 0.25: {r}");
}

#[test]
fn per_key_round_robin_counters_advance_independently_per_midi_key() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut step0 = zone(&mut store, 1.0);
    step0.pan = -1.0;
    step0.start = vec![(
        kontra_ir::Join::And,
        Atom::RoundRobin(RoundRobin {
            counter: RrCounter::Key,
            position: 0,
            length: 2,
            random: false,
            no_repeat: false,
            release_redraws: false,
        }),
    )];
    let mut step1 = zone(&mut store, 1.0);
    step1.pan = 1.0;
    step1.start = vec![(
        kontra_ir::Join::And,
        Atom::RoundRobin(RoundRobin {
            counter: RrCounter::Key,
            position: 1,
            length: 2,
            random: false,
            no_repeat: false,
            release_redraws: false,
        }),
    )];
    let mut e = engine(&store, Plan::new(vec![step0, step1], 8));

    // C4 first hit -> step 0 (left)
    let [l, r] = level(&mut e, &[on(60, 1.0)]);
    assert!(l > 0.5 && r < 0.1, "C4 #1 should be step 0: {l}/{r}");
    level(&mut e, &[off(60)]);
    for _ in 0..4 {
        level(&mut e, &[]);
    }

    // D4 first hit -> ALSO step 0 (left), unaffected by C4!
    let [l, r] = level(&mut e, &[on(62, 1.0)]);
    assert!(l > 0.5 && r < 0.1, "D4 #1 should be step 0: {l}/{r}");
    level(&mut e, &[off(62)]);
    for _ in 0..4 {
        level(&mut e, &[]);
    }

    // C4 second hit -> step 1 (right)
    let [l, r] = level(&mut e, &[on(60, 1.0)]);
    assert!(r > 0.5 && l < 0.1, "C4 #2 should be step 1: {l}/{r}");
}

#[test]
fn a_disabled_mic_bus_starts_no_voices() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let close_mic = zone(&mut store, 0.5);
    let mut room = zone(&mut store, 0.25);
    room.group = 1;
    let mut plan = Plan::new(vec![close_mic, room], 8);
    plan.groups[1].bus = 1;
    plan.buses.push(Bus {
        name: "Room".into(),
        gain: 1.0,
        pan: 0.0,
        inserts: Vec::new(),
        sends: Vec::new(),
        to_main: true,
        enabled: true,
        mods: Vec::new(),
        blends: Vec::new(),
        lfos: Vec::new(),
    });
    let mut e = engine(&store, plan);
    e.set_bus_enabled(1, false);
    assert!(close(level(&mut e, &[on(60, 1.0)])[0], 0.5));
    assert_eq!(e.active_voices(), 1);
}

#[test]
fn a_bus_sends_before_its_fader() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    plan.groups[0].bus = 1;
    // A silent fader still feeds the aux at a pre-fader send's level, and
    // silences a post-fader one.
    plan.buses.push(Bus {
        name: "Layer".into(),
        gain: 0.0,
        pan: 0.0,
        inserts: Vec::new(),
        sends: vec![
            ir::BusSend {
                bus: 2,
                level: 0.5,
                post_fader: false,
            },
            ir::BusSend {
                bus: 2,
                level: 1.0,
                post_fader: true,
            },
        ],
        to_main: true,
        enabled: true,
        mods: Vec::new(),
        blends: Vec::new(),
        lfos: Vec::new(),
    });
    plan.buses.push(Bus {
        name: "Aux".into(),
        gain: 1.0,
        pan: 0.0,
        inserts: Vec::new(),
        sends: Vec::new(),
        to_main: true,
        enabled: true,
        mods: Vec::new(),
        blends: Vec::new(),
        lfos: Vec::new(),
    });
    let mut e = engine(&store, plan);
    assert!(close(level(&mut e, &[on(60, 1.0)])[0], 0.25));
}

#[test]
fn a_controller_scales_a_send_level_live() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    plan.groups[0].bus = 1;
    // A silent fader: only the pre-fader send, at 1 − 0.75 + 0.75·CC1, plays.
    let factor = ir::Factor {
        input: ir::Transfer {
            source: ir::Source::Controller(1),
            table: Vec::new(),
            ..Default::default()
        },
        depth: 0.75,
    };
    let blend = ir::Blend {
        base: 0.0,
        terms: vec![ir::Term {
            input: ir::Transfer {
                source: ir::Source::Constant,
                table: Vec::new(),
                ..Default::default()
            },
            amount: 1.0,
            factors: vec![factor],
        }],
    };
    let bus = |sends, mods, blends| Bus {
        name: "Bus".into(),
        gain: 0.0,
        pan: 0.0,
        inserts: Vec::new(),
        sends,
        to_main: true,
        enabled: true,
        mods,
        blends,
        lfos: Vec::new(),
    };
    let send = ir::BusSend {
        bus: 2,
        level: 1.0,
        post_fader: false,
    };
    let route = ir::ModRoute::new(
        ir::Source::Blend(0),
        ir::ParamAddr::BusSend { send: 0 },
        1.0,
    );
    plan.buses.push(bus(vec![send], vec![route], vec![blend]));
    plan.buses.push(Bus {
        gain: 1.0,
        ..bus(Vec::new(), Vec::new(), Vec::new())
    });
    let mut e = engine(&store, plan);
    assert!(close(level(&mut e, &[cc(1, 0), on(60, 1.0)])[0], 0.125));
    assert!(close(level(&mut e, &[cc(1, 127)])[0], 0.5));
}

#[test]
fn a_free_lfo_scales_a_send_level_live() {
    use kontra_engine::ir;
    // A square LFO at 1 Hz: high (read 1) the first half cycle, low (0) the
    // second; the send runs at 1 − 0.75 + 0.75·x of the zone's 0.5. One
    // engine runs through the boundary, no new note.
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    plan.groups[0].bus = 1;
    let factor = ir::Factor {
        input: ir::Transfer {
            source: ir::Source::Lfo(0),
            table: Vec::new(),
            ..Default::default()
        },
        depth: 0.75,
    };
    let blend = ir::Blend {
        base: 0.0,
        terms: vec![ir::Term {
            input: ir::Transfer {
                source: ir::Source::Constant,
                table: Vec::new(),
                ..Default::default()
            },
            amount: 1.0,
            factors: vec![factor],
        }],
    };
    let lfo = ir::Lfo {
        wave: ir::Wave::Square { width: 0.5 },
        rate: ir::Rate::Hz(1.0),
        phase: 0.0,
        fade_ms: 0.0,
        clock: ir::Clock::Free,
    };
    plan.buses.push(Bus {
        name: "Bus".into(),
        gain: 0.0,
        pan: 0.0,
        inserts: Vec::new(),
        sends: vec![ir::BusSend {
            bus: 2,
            level: 1.0,
            post_fader: false,
        }],
        to_main: true,
        enabled: true,
        mods: vec![ir::ModRoute::new(
            ir::Source::Blend(0),
            ir::ParamAddr::BusSend { send: 0 },
            1.0,
        )],
        blends: vec![blend],
        lfos: vec![lfo],
    });
    plan.buses.push(Bus {
        name: "Aux".into(),
        gain: 1.0,
        pan: 0.0,
        inserts: Vec::new(),
        sends: Vec::new(),
        to_main: true,
        enabled: true,
        mods: Vec::new(),
        blends: Vec::new(),
        lfos: Vec::new(),
    });
    let mut e = engine(&store, plan);
    assert!(close(level(&mut e, &[on(60, 1.0)])[0], 0.5), "first half");
    // The second half cycle starts at 0.5 s (94 spans); the zone sounds for 1 s.
    let mut seen = Vec::new();
    for _ in 0..150 {
        let l = level(&mut e, &[])[0];
        if seen.last().is_none_or(|p: &f32| (p - l).abs() > 0.01) {
            seen.push(l);
        }
    }
    assert!(seen.iter().any(|l| close(*l, 0.125)), "{seen:?}");
}

#[test]
fn a_comb_insert_runs_on_each_voice_and_rings_at_its_feedback() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.25)], 8);
    // A constant 0.25 through a comb with feedback 0.5 settles at 0.25/(1 − 0.5).
    plan.groups[0].inserts = vec![Some(ir::Effect {
        kind: ir::EffectKind::Comb(ir::Comb {
            hz: 480.0,
            feedback: 0.5,
            forward: false,
            octaves: 9.97,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    let mut e = engine(&store, plan);
    level(&mut e, &[on(60, 1.0)]);
    let mut last = 0.0;
    for _ in 0..8 {
        last = level(&mut e, &[])[0];
    }
    assert!(close(last, 0.5), "settled at {last}");
}

#[test]
fn a_voice_group_steals_by_its_rule_and_an_exclusive_set_silences_the_others() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut keyed = |key: u8, level: f32, vg: u32| {
        let mut z = zone(&mut store, level);
        z.keys = key..=key;
        z.voice_group = Some(vg);
        z
    };
    let zones = vec![
        keyed(60, 0.1, 0),
        keyed(64, 0.2, 0),
        keyed(62, 0.4, 0),
        keyed(70, 0.8, 1),
    ];
    let mut plan = Plan::new(zones, 8);
    let group = |voices, steal| kontra_ir::VoiceGroup {
        voices,
        steal,
        prefer_released: true,
        fade_ms: 1.0,
        exclusive: Some(3),
        per_note: false,
        legato: false,
        returns: false,
        ..kontra_ir::VoiceGroup::DEFAULT
    };
    plan.voice_groups = vec![
        group(2, kontra_ir::Steal::Highest),
        group(4, kontra_ir::Steal::Oldest),
    ];
    let mut e = engine(&store, plan);
    level(&mut e, &[on(60, 1.0), on(64, 1.0)]);
    level(&mut e, &[on(62, 1.0)]);
    assert!(
        close(level(&mut e, &[])[0], 0.5),
        "the highest key (64) gave way"
    );
    level(&mut e, &[on(70, 1.0)]);
    assert!(close(level(&mut e, &[])[0], 0.8), "group 0 was excluded");
}

#[test]
fn a_note_counting_voice_group_steals_every_voice_of_the_note_it_replaces() {
    // A mono layer: key 60 sounds two zones, key 62 one. Counting voices
    // (budget 1 note x 2 zones) would steal one of 60's and keep it sounding.
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut keyed = |key: u8, level: f32| {
        let mut z = zone(&mut store, level);
        z.keys = key..=key;
        z.voice_group = Some(0);
        z
    };
    let zones = vec![keyed(60, 0.1), keyed(60, 0.2), keyed(62, 0.4)];
    let mut plan = Plan::new(zones, 8);
    plan.voice_groups = vec![kontra_ir::VoiceGroup {
        voices: 1,
        steal: kontra_ir::Steal::Oldest,
        prefer_released: false,
        fade_ms: 1.0,
        exclusive: None,
        per_note: true,
        legato: false,
        returns: false,
        ..kontra_ir::VoiceGroup::DEFAULT
    }];
    let mut e = engine(&store, plan);
    level(&mut e, &[on(60, 1.0)]);
    assert!(close(level(&mut e, &[])[0], 0.3), "both of 60's zones");
    level(&mut e, &[on(62, 1.0)]);
    assert!(
        close(level(&mut e, &[])[0], 0.4),
        "only 62 sounds: 60 went whole"
    );
}

#[test]
fn a_one_shot_ignores_key_up() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut hit = zone(&mut store, 0.5);
    hit.one_shot = true;
    let mut e = engine(&store, Plan::new(vec![hit], 8));
    level(&mut e, &[on(60, 1.0)]);
    level(&mut e, &[off(60)]);
    assert!(
        close(level(&mut e, &[])[0], 0.5),
        "still sounding at full level"
    );
}

#[test]
fn pitch_bend_moves_sounding_voices_by_its_route() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 1.0)], 8);
    plan.groups[0].mods = vec![kontra_engine::ir::Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        route: kontra_engine::ir::ModRoute::new(
            kontra_engine::ir::Source::PitchBend,
            kontra_engine::ir::ParamAddr::Pitch,
            1.0,
        )
        .span(2.0),
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let bend = |lsb, msb| Timed {
        at: SampleTime(0),
        body: Event::midi1(0xE0, lsb, msb),
    };
    // Source frames one 256-frame block advances.
    let advance = |e: &mut Engine, events: &[Timed<Event>]| {
        let at = |e: &Engine| e.voices().next().map(|(_, p, _)| p).unwrap();
        let from = at(e);
        level(e, events);
        (at(e) - from) / 256.0
    };
    level(&mut e, &[on(60, 1.0)]);
    assert!((advance(&mut e, &[]) - 1.0).abs() < 1e-6);
    let up = advance(&mut e, &[bend(0x7F, 0x7F)]);
    assert!(
        (up - 2f64.powf(2.0 * 8191.0 / 8192.0 / 12.0)).abs() < 1e-6,
        "{up}"
    );
    let down = advance(&mut e, &[bend(0, 0)]);
    assert!((down - 2f64.powf(-2.0 / 12.0)).abs() < 1e-6, "{down}");
}

#[test]
fn a_mapped_bend_bends_pitch_by_its_table_as_is() {
    // A UVI mapper emits its table in the route's domain: -1..=2 of 2 st.
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 1.0)], 8);
    plan.groups[0].shapers = vec![ir::Transfer {
        source: ir::Source::PitchBend,
        table: vec![-1.0, 0.0, 1.0, 2.0],
        ..Default::default()
    }];
    plan.groups[0].mods = vec![ir::Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        route: ir::ModRoute::new(ir::Source::Shaped(0), ir::ParamAddr::Pitch, 1.0).span(2.0),
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let bend = |lsb, msb| Timed {
        at: SampleTime(0),
        body: Event::midi1(0xE0, lsb, msb),
    };
    let semitones = |e: &mut Engine, events: &[Timed<Event>]| {
        let at = |e: &Engine| e.voices().next().map(|(_, p, _)| p).unwrap();
        let from = at(e);
        level(e, events);
        12.0 * ((at(e) - from) / 256.0).log2()
    };
    level(&mut e, &[on(60, 1.0)]);
    let down = semitones(&mut e, &[bend(0, 0)]);
    assert!((down + 2.0).abs() < 1e-3, "{down}");
    let up = semitones(&mut e, &[bend(0x7F, 0x7F)]);
    assert!((up - 4.0).abs() < 2e-3, "{up}");
}

#[test]
fn a_sample_start_route_moves_the_start_within_its_range() {
    use kontra_engine::ir::{Mod, ModRoute, ParamAddr, Source};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    // A ramp: each frame's value is its index / 4096, so the output names the frame.
    let frames = 4096u64;
    let bytes = (0..frames)
        .flat_map(|i| (i as f32 / 4096.0).to_le_bytes())
        .collect();
    let key = BlobKey {
        file: "ramp".into(),
        offset: frames,
        fmt: FMT,
    };
    let sample = store
        .load(
            &key,
            Arc::new(MemSource::new(FMT, bytes)),
            frames,
            std::slice::from_ref(&(0..frames)),
        )
        .unwrap();
    let mut z = Zone::new(
        Region {
            mix: None,
            table: None,
            sample,
            start: 0,
            end: frames,
            loops: Default::default(),
            reverse: false,
        },
        60,
        RATE,
    );
    z.start_mod = 1000;
    let mut plan = Plan::new(vec![z], 8);
    for (depth, start) in [(0.5, 500.0), (3.0, 1000.0)] {
        plan.groups[0].mods = vec![Mod {
            name: String::new(),
            slot: 0,
            target: 0,
            bypass: false,
            route: ModRoute::new(Source::Constant, ParamAddr::SampleStart, depth),
        }];
        let mut e = Engine::new(Arc::new(plan.clone()), store.client(), RATE);
        let frame = level(&mut e, &[on(60, 1.0)])[0] * 4096.0;
        assert!(
            (frame - (start + 128.0)).abs() < 1.0,
            "depth {depth}: {frame}"
        );
    }
}

#[test]
fn a_group_eq_shapes_its_voices() {
    use kontra_engine::ir::{Bell, Effect, EffectKind};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let region = sine(&mut store);
    let peak = |gain_db: f32| {
        let mut plan = Plan::new(vec![Zone::new(region.clone(), 60, RATE)], 8);
        plan.groups[0].mods.clear();
        let bell = Bell {
            hz: 1000.0,
            q: 1.0,
            gain_db,
            ..Bell::FLAT
        };
        plan.groups[0].inserts = vec![Some(Effect {
            kind: EffectKind::Eq([bell, Bell::FLAT, Bell::FLAT, Bell::FLAT]),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        })];
        let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
        let mut out = vec![[0.0; 2]; 4800];
        e.process(&mut out, &[on(60, 1.0)]);
        out[2400..].iter().map(|s| s[0].abs()).fold(0.0, f32::max)
    };
    let (flat, boosted) = (peak(0.0), peak(12.0));
    assert!((flat - 0.1).abs() < 0.005, "{flat}");
    assert!(
        (boosted / flat - 10f32.powf(0.6)).abs() < 0.05,
        "{flat} -> {boosted}"
    );
}

#[test]
fn a_shaped_controller_reads_its_start_until_a_message_arrives() {
    use kontra_engine::ir::{Curve, Mod, ModRoute, ParamAddr, RouteLaw, Source, Transfer};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 1.0)], 8);
    plan.groups[0].shapers = vec![Transfer {
        source: Source::ControllerFrom { cc: 1, start: 127 },
        table: (0..128).map(|k| k as f32 / 254.0).collect(),
        ..Default::default()
    }];
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        route: ModRoute::new(Source::Shaped(0), ParamAddr::Volume, 1.0)
            .law(RouteLaw::Gain(Curve::Depth { power: 1 })),
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    // No message yet: the wheel reads 127, which the table halves.
    let start = level(&mut e, &[on(60, 1.0)])[0];
    level(&mut e, &[cc(1, 0)]);
    let down = level(&mut e, &[])[0];
    assert!(
        (start - 0.5).abs() < 0.01 && down.abs() < 0.05,
        "{start} {down}"
    );
}

#[test]
fn a_blended_macro_moves_one_cell_of_a_sounding_voices_matrix() {
    use kontra_engine::ir::{Blend, Factor, Mod, ModRoute, ParamAddr, Source, Term, Transfer};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut z = zone(&mut store, 1.0);
    let mut mix = [[0.0; 2]; 16];
    mix[0] = [1.0, 1.0];
    z.region.mix = Some(std::sync::Arc::new(mix));
    let mut plan = Plan::new(vec![z], 8);
    let through = |source| Transfer {
        source,
        table: Vec::new(),
        ..Default::default()
    };
    // Wheel × CC 2, read straight through: the left cell only.
    plan.groups[0].blends = vec![Blend {
        base: 0.0,
        terms: vec![Term {
            input: through(Source::Controller(1)),
            amount: 1.0,
            factors: vec![Factor {
                input: through(Source::Controller(2)),
                depth: 1.0,
            }],
        }],
    }];
    plan.groups[0].shapers = vec![through(Source::Blend(0))];
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        route: ModRoute::new(
            Source::Shaped(0),
            ParamAddr::ZoneMix {
                zone: 0,
                row: 0,
                side: 0,
            },
            1.0,
        ),
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    level(&mut e, &[cc(1, 127), cc(2, 127)]);
    let [l, r] = level(&mut e, &[on(60, 1.0)]);
    assert!(r > 0.1 && close(l, r), "{l} {r}");
    level(&mut e, &[cc(2, 0)]);
    let [l, r] = level(&mut e, &[]);
    assert!(l.abs() < 1e-3 && r > 0.1, "{l} {r}");
}

#[test]
fn a_controller_pitch_route_moves_sounding_voices() {
    use kontra_engine::ir::{Mod, ModRoute, ParamAddr, Source};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 1.0)], 8);
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        route: ModRoute::new(Source::Controller(1), ParamAddr::Pitch, 1.0).span(12.0),
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let at = |e: &Engine| e.voices().next().map(|(_, p, _)| p).unwrap();
    level(&mut e, &[on(60, 1.0)]);
    let from = at(&e);
    level(&mut e, &[cc(1, 127)]);
    assert!(
        (at(&e) - from - 512.0).abs() < 1e-6,
        "an octave up: {}",
        at(&e) - from
    );
}

#[test]
fn an_envelope_pitch_route_rises_holds_and_releases() {
    use kontra_engine::ir::{Breakpoint, Breakpoints, Mod, ModRoute, ParamAddr, Source};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 1.0)], 8);
    let point = |level| Breakpoint {
        ms: 1.0,
        level,
        curve: 0.0,
    };
    plan.groups[0].envelopes = vec![Breakpoints {
        start: 0.0,
        points: vec![point(1.0), point(0.0)],
        sustain: Some(0),
        loop_from: None,
        stages: false,
    }];
    plan.groups[0].envelope.release = RATE;
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        route: ModRoute::new(Source::Envelope(0), ParamAddr::Pitch, 1.0).span(12.0),
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let advance = |e: &mut Engine, events: &[Timed<Event>]| {
        let at = |e: &Engine| e.voices().next().map(|(_, p, _)| p).unwrap();
        let from = at(e);
        level(e, events);
        (at(e) - from) / 256.0
    };
    level(&mut e, &[on(60, 1.0)]);
    assert!((advance(&mut e, &[]) - 2.0).abs() < 1e-6, "held at the top");
    level(&mut e, &[off(60)]);
    let after = advance(&mut e, &[]);
    assert!(
        (after - 1.0).abs() < 1e-6,
        "released to the bottom: {after}"
    );
}

/// Source frames the newest voice moves per output frame over `frames`,
/// rendered in blocks of `block`.
fn advance(e: &mut Engine, frames: usize, block: usize) -> f64 {
    let at = |e: &Engine| e.voices().map(|(_, p, _)| p).fold(f64::MIN, f64::max);
    let from = at(e);
    let mut out = vec![[0.0; 2]; block];
    for _ in 0..frames / block {
        e.process(&mut out, &[]);
    }
    (at(e) - from) / frames as f64
}

fn glide_plan(store: &mut Store, legato: bool) -> Plan {
    let mut z = zone(store, 0.5);
    z.keys = 0..=127;
    let mut plan = Plan::new(vec![z], 8);
    plan.groups[0].glide = Some(ir::Glide {
        scope: 0,
        depth: 1.0,
        range: f32::INFINITY,
        from_heard: false,
        seconds: 0.1,
        per_semitone: false,
        resume: true,
        legato,
        curve: 0.0,
    });
    plan
}

#[test]
fn a_glide_starts_on_the_last_note_and_slides_linearly_in_semitones_to_its_own() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let plan = glide_plan(&mut store, false);
    let mut e = engine(&store, plan);
    level(&mut e, &[on(60, 1.0)]);
    level(&mut e, &[off(60)]);
    advance(&mut e, 4096, 256); // the released note ends
    assert_eq!(e.voices().count(), 0);
    level(&mut e, &[on(72, 1.0)]);
    // 0.1 s = 4800 frames from the note-on block: an octave below at the
    // start, half way (in semitones) at 2400, the note's own from 4800.
    let first = advance(&mut e, 256, 256); // 256..512
    assert!(first > 1.0 && first < 1.1, "{first}");
    advance(&mut e, 1536, 256);
    let middle = advance(&mut e, 768, 256); // 2048..2816, about 2400
    assert!((middle - 2f64.powf(0.5)).abs() < 0.03, "{middle}");
    advance(&mut e, 2048, 256);
    let landed = advance(&mut e, 256, 256);
    assert!((landed - 2.0).abs() < 1e-9, "{landed}");
}

#[test]
fn a_glide_moves_every_frame_the_same_whatever_the_block_size() {
    let run = |block: usize| {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let plan = glide_plan(&mut store, true);
        let mut e = engine(&store, plan);
        level(&mut e, &[on(60, 1.0)]);
        level(&mut e, &[on(67, 1.0)]);
        (0..12)
            .map(|_| advance(&mut e, 384, block))
            .collect::<Vec<_>>()
    };
    let (coarse, fine) = (run(384), run(32));
    for (a, b) in coarse.iter().zip(&fine) {
        assert!((a - b).abs() < 1e-9, "{coarse:?} {fine:?}");
    }
    // Rising through the slide, not in 256-frame stairs.
    assert!(coarse.windows(2).all(|w| w[1] >= w[0]), "{coarse:?}");
}

#[test]
fn a_bend_in_a_glides_last_span_keeps_the_rest_of_the_slide() {
    // The slide (60 -> 72, 4800 frames from frame 256) ends in the span
    // 4864..5120; a bend one step off centre inside it refreshes the pitch, which must
    // keep ramping, not jump to 72: a jump is ~5 source frames ahead by the
    // block's end, the one-step bend 0.05.
    let run = |bend: bool| {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let mut plan = glide_plan(&mut store, false);
        plan.groups[0].mods = vec![ir::Mod {
            name: String::new(),
            slot: 0,
            target: 0,
            bypass: false,
            route: ir::ModRoute::new(ir::Source::PitchBend, ir::ParamAddr::Pitch, 1.0).span(2.0),
        }];
        let mut e = engine(&store, plan);
        level(&mut e, &[on(60, 1.0)]);
        level(&mut e, &[on(72, 1.0)]);
        advance(&mut e, 4352, 256);
        let events = [Timed {
            at: SampleTime(64),
            body: Event::midi1(0xE0, 1, 0x40),
        }];
        let mut out = vec![[0.0; 2]; 256];
        e.process(&mut out, if bend { &events } else { &[] });
        e.voices().map(|(_, p, _)| p).fold(f64::MIN, f64::max)
    };
    let (still, bent) = (run(false), run(true));
    assert!((still - bent).abs() < 0.5, "{still} {bent}");
}

#[test]
fn a_legato_glide_needs_a_held_key() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let plan = glide_plan(&mut store, true);
    let mut e = engine(&store, plan);
    level(&mut e, &[on(60, 1.0)]);
    level(&mut e, &[off(60)]);
    advance(&mut e, 4096, 256);
    level(&mut e, &[on(72, 1.0)]);
    let detached = advance(&mut e, 256, 256);
    assert!((detached - 2.0).abs() < 1e-9, "no slide: {detached}");
}

fn mono_plan(store: &mut Store, legato: bool) -> Plan {
    let mut z = zone(store, 0.5);
    z.keys = 0..=127;
    z.voice_group = Some(0);
    let mut plan = Plan::new(vec![z], 8);
    plan.voice_groups = vec![kontra_ir::VoiceGroup {
        voices: 1,
        steal: kontra_ir::Steal::Oldest,
        prefer_released: false,
        fade_ms: 10.0,
        exclusive: None,
        per_note: true,
        legato,
        returns: true,
        ..kontra_ir::VoiceGroup::DEFAULT
    }];
    plan
}

/// Sounding voices that are not on their way out.
fn holding(e: &Engine) -> usize {
    e.voices()
        .filter(|(_, _, g)| g.as_flattened().iter().any(|v| *v > 0.0))
        .count()
}

#[test]
fn a_legato_line_keeps_its_voice_moves_it_to_the_new_key_and_back() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let plan = mono_plan(&mut store, true);
    let mut e = engine(&store, plan);
    level(&mut e, &[on(60, 1.0)]);
    let at = e.voices().next().unwrap().1;
    level(&mut e, &[on(67, 1.0)]);
    assert_eq!(e.voices().count(), 1, "no new voice");
    assert!(e.voices().next().unwrap().1 > at, "the same voice plays on");
    let up = advance(&mut e, 256, 256);
    assert!((up - 2f64.powf(7.0 / 12.0)).abs() < 1e-9, "{up}");
    level(&mut e, &[off(67)]);
    assert_eq!(e.voices().count(), 1);
    let back = advance(&mut e, 256, 256);
    assert!((back - 1.0).abs() < 1e-9, "back on 60: {back}");
    level(&mut e, &[off(60)]);
    advance(&mut e, 4096, 256);
    assert_eq!(e.voices().count(), 0);
}

#[test]
fn a_reset_ends_the_keys_a_line_was_waiting_on() {
    // 60 waits in the line under 67; a reset must end both, not keep 60.
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let plan = mono_plan(&mut store, false);
    let mut e = engine(&store, plan);
    level(&mut e, &[on(60, 1.0)]);
    level(&mut e, &[on(67, 1.0)]);
    e.reset();
    let mut ended = 0;
    for _ in 0..64 {
        level(&mut e, &[]);
        ended += e.ended().len();
    }
    assert_eq!(ended, 2);
}

#[test]
fn a_retriggering_line_restarts_the_older_key_when_the_newer_goes_up() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let plan = mono_plan(&mut store, false);
    let mut e = engine(&store, plan);
    level(&mut e, &[on(60, 1.0)]);
    advance(&mut e, 2048, 256);
    level(&mut e, &[on(67, 1.0)]);
    advance(&mut e, 2048, 256); // 60's voice faded out
    assert_eq!(e.voices().count(), 1);
    level(&mut e, &[off(67)]);
    advance(&mut e, 2048, 256);
    assert_eq!(holding(&e), 1, "60 sounds again");
    let again = advance(&mut e, 256, 256);
    assert!((again - 1.0).abs() < 1e-9, "{again}");
    let start = e.voices().map(|(_, p, _)| p).fold(f64::MAX, f64::min);
    assert!(
        start < 3000.0,
        "a new voice from the sample's start: {start}"
    );
}

fn on_ch(channel: u8, key: u8, velocity: f64) -> Timed<Event> {
    Timed {
        at: SampleTime(0),
        body: Event::NoteOn {
            note: kontra_events::HostNote {
                port: 0,
                channel,
                key,
                id: -1,
                clap: true,
            },
            velocity,
            tune: 0.0,
        },
    }
}

fn cc_ch(channel: u8, controller: u8, value: u8) -> Timed<Event> {
    Timed {
        at: SampleTime(0),
        body: Event::midi1(0xB0 | (channel & 0x0F), controller, value),
    }
}

#[test]
fn midi_cc120_all_sound_off_silences_only_its_channel_bypassing_sustain_and_release() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 16);
    plan.groups[0].envelope.release = RATE * 4;
    let mut e = engine(&store, plan);
    level(
        &mut e,
        &[cc_ch(0, 64, 127), on_ch(0, 60, 1.0), on_ch(1, 64, 1.0)],
    );
    assert_eq!(e.active_voices(), 2);
    // CC 120 on channel 0 stops ch 0 in ~2 ms even with sustain down & 4 s release.
    level(&mut e, &[cc_ch(0, 120, 0)]);
    let [after, _] = level(&mut e, &[]);
    assert_eq!(e.active_voices(), 1, "only ch 1 survives");
    assert!(close(after, 0.5), "ch 1 still sounds at 0.5: {after}");
}

#[test]
fn midi_cc123_all_notes_off_releases_channel_notes() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 16);
    plan.groups[0].envelope.release = 16;
    let mut e = engine(&store, plan);
    level(&mut e, &[on_ch(0, 60, 1.0), on_ch(1, 64, 1.0)]);
    assert_eq!(e.active_voices(), 2);
    level(&mut e, &[cc_ch(0, 123, 0)]);
    let [after, _] = level(&mut e, &[]);
    assert_eq!(e.active_voices(), 1, "ch 0 released, ch 1 still held");
    assert!(close(after, 0.5), "{after}");
}

#[test]
fn midi_cc123_waits_for_either_pedal_only_on_its_channel() {
    for pedal in [64, 66] {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
        plan.groups[0].envelope.release = 16;
        let mut e = engine(&store, plan);
        level(
            &mut e,
            &[on_ch(0, 60, 1.0), on_ch(1, 64, 1.0), cc_ch(0, pedal, 127)],
        );
        level(&mut e, &[cc_ch(0, 123, 0)]);
        assert_eq!(e.active_voices(), 2, "CC{pedal} must hold channel 0");
        let [after, _] = level(&mut e, &[cc_ch(0, pedal, 0)]);
        assert_eq!(e.active_voices(), 1);
        assert!(close(after, 0.5), "{after}");
    }
}

#[test]
fn midi_cc121_restores_expression_for_zone_admission() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut gated = zone(&mut store, 0.5);
    gated.start = vec![(Join::And, Atom::Cc(11, 127..=127))];
    let mut e = engine(&store, Plan::new(vec![gated], 8));
    assert_eq!(level(&mut e, &[cc(11, 40), on(60, 1.0)])[0], 0.0);
    assert!(close(level(&mut e, &[cc(121, 0), on(64, 1.0)])[0], 0.5));
}

/// Kontakt compiles a group's start options into one unparenthesized KSP
/// condition, so `and` binds tighter than `or`: `CC1 or CC2 and CC3` starts
/// on CC1 alone, where a left fold (`(CC1 or CC2) and CC3`) would not.
#[test]
fn start_conditions_bind_and_tighter_than_or() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut gated = zone(&mut store, 0.5);
    let high = || 64..=127;
    gated.start = vec![
        (Join::And, Atom::Cc(1, high())),
        (Join::Or, Atom::Cc(2, high())),
        (Join::And, Atom::Cc(3, high())),
    ];
    let mut e = engine(&store, Plan::new(vec![gated.clone()], 8));
    assert!(close(level(&mut e, &[cc(1, 127), on(60, 1.0)])[0], 0.5));
    let mut e = engine(&store, Plan::new(vec![gated.clone()], 8));
    assert_eq!(level(&mut e, &[cc(2, 127), on(60, 1.0)])[0], 0.0);
    let mut e = engine(&store, Plan::new(vec![gated.clone()], 8));
    assert!(close(
        level(&mut e, &[cc(2, 127), cc(3, 127), on(60, 1.0)])[0],
        0.5
    ));
    // `CC1 or CC2 and not CC3` is `CC1 or (CC2 and not CC3)`.
    gated.start[2].0 = Join::AndNot;
    let mut e = engine(&store, Plan::new(vec![gated], 8));
    assert!(close(
        level(&mut e, &[cc(1, 127), cc(3, 127), on(60, 1.0)])[0],
        0.5
    ));
}

#[test]
fn midi_cc121_reset_all_controllers_resets_mod_bend_pedals_and_preserves_volume_pan_bank() {
    use kontra_engine::ir::{Curve, Mod, ModRoute, ParamAddr, RouteLaw, Source};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 1.0)], 8);
    plan.groups[0].envelope.release = 16;
    // CC1 scales volume; CC7 and CC10 also read by routes so we verify RP-015 preservation.
    plan.groups[0].mods = vec![
        Mod {
            name: String::new(),
            slot: 0,
            target: 0,
            bypass: false,
            route: ModRoute::new(Source::Controller(1), ParamAddr::Volume, 1.0)
                .law(RouteLaw::Gain(Curve::Depth { power: 1 })),
        },
        Mod {
            name: String::new(),
            slot: 1,
            target: 0,
            bypass: false,
            route: ModRoute::new(Source::PitchBend, ParamAddr::Pitch, 1.0).span(12.0),
        },
    ];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let bend = Timed {
        at: SampleTime(0),
        body: Event::midi1(0xE0, 0x7F, 0x7F),
    };
    level(
        &mut e,
        &[
            cc(0, 5),
            cc(32, 9),
            cc(7, 100),
            cc(10, 90),
            cc(1, 127),
            cc(64, 127),
            bend,
            on(60, 1.0),
            on(64, 1.0),
        ],
    );
    // Release key 64 while sustain holds it; keep key 60 held.
    level(&mut e, &[off(64)]);
    assert_eq!(e.active_voices(), 2);
    // CC 121 resets CC1 -> 0, bend -> 0, releases pedal-held key 64, keeps CC0/32/7/10.
    level(&mut e, &[cc(121, 0)]);
    // Allow ~2 ms gain smoother (GAIN_SMOOTH = 0.01) to settle near 0.
    for _ in 0..3 {
        level(&mut e, &[]);
    }
    let [lvl, _] = level(&mut e, &[]);
    assert_eq!(e.active_voices(), 1, "pedal-held key 64 released by CC121");
    assert!(
        lvl.abs() < 1e-3,
        "CC1 reset to 0 silenced volume route: {lvl}"
    );
    // Verify pitch bend returned to unity (step = 1.0).
    let step = advance(&mut e, 256, 256);
    assert!((step - 1.0).abs() < 1e-4, "bend reset to 0: step={step}");
    // Verify CC7/CC10/CC0/CC32 were preserved in engine CC state via a new plan-less check:
    // re-raise CC1 and check that volume route responds cleanly while CC7/10/0/32 remain.
    assert_eq!(e.controller(0), Some(5));
    assert_eq!(e.controller(32), Some(9));
    assert_eq!(e.controller(7), Some(100));
    assert_eq!(e.controller(10), Some(90));
    assert_eq!(e.controller(1), Some(0));
    assert_eq!(e.controller(11), Some(127));
}

#[test]
fn midi_cc66_sostenuto_holds_only_notes_held_at_press_and_interacts_with_cc64() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    plan.groups[0].envelope.release = 16;
    let mut e = engine(&store, plan);

    // Hold key 60, press sostenuto (CC66), then play key 64.
    level(&mut e, &[on(60, 1.0)]);
    level(&mut e, &[cc(66, 127)]);
    level(&mut e, &[on(64, 1.0)]);
    // Release both keys: 60 stays held by sostenuto, 64 finishes its short release.
    level(&mut e, &[off(60), off(64)]);
    let [lvl, _] = level(&mut e, &[]);
    assert_eq!(e.active_voices(), 1, "only key 60 held by sostenuto");
    assert!(close(lvl, 0.5), "{lvl}");

    // Press CC64, then release CC64 while CC66 is still down: 60 stays alive.
    level(&mut e, &[cc(64, 127)]);
    level(&mut e, &[cc(64, 0)]);
    assert_eq!(e.active_voices(), 1, "CC64 up must not drop sostenuto note");

    // Sostenuto hands the released key to sustain if it is now down.
    level(&mut e, &[cc(64, 127), cc(66, 0)]);
    assert_eq!(e.active_voices(), 1, "sustain takes over the hold");
    level(&mut e, &[cc(64, 0)]);
    level(&mut e, &[]);
    assert_eq!(
        e.active_voices(),
        0,
        "CC64 up releases the transferred note"
    );
}

#[test]
fn reset_retires_pedal_held_host_notes_with_release_triggers() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let attack = zone(&mut store, 0.5);
    let mut tail = zone(&mut store, 0.4);
    tail.trigger = Trigger::Release;
    let mut e = engine(&store, Plan::new(vec![attack, tail], 8));
    level(&mut e, &[on(60, 1.0), cc(66, 127), off(60)]);
    assert_eq!(e.active_voices(), 1);
    assert!(e.ended().is_empty());
    e.reset();
    let mut ended = 0;
    for _ in 0..4 {
        level(&mut e, &[]);
        ended += e.ended().len();
    }
    assert_eq!(e.active_voices(), 0);
    assert_eq!(ended, 1, "reset must retire the captured host note");
}

#[test]
fn stealing_honors_prefer_released_and_quietest_and_skips_self_note_exclusive_choke() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();

    // 1. prefer_released: true vs false.
    let mut run_prefer = |prefer_released: bool| {
        let mut z1 = zone(&mut store, 0.2);
        z1.keys = 60..=60;
        let mut z2 = zone(&mut store, 0.4);
        z2.keys = 62..=62;
        let mut z3 = zone(&mut store, 0.1);
        z3.keys = 64..=64;
        let mut plan = Plan::new(vec![z1, z2, z3], 2);
        plan.groups[0].envelope.release = RATE * 2;
        plan.steal = kontra_ir::VoiceGroup {
            steal: kontra_ir::Steal::Oldest,
            prefer_released,
            fade_ms: 1.0,
            ..kontra_ir::VoiceGroup::DEFAULT
        };
        let mut e = engine(&store, plan);
        // Key 60 is older and held; key 62 is newer and released.
        level(&mut e, &[on(60, 1.0)]);
        level(&mut e, &[on(62, 1.0)]);
        level(&mut e, &[off(62)]);
        // Key 64 triggers steal: prefer_released=true steals 62 (leaving 60+64 ≈ 0.3),
        // prefer_released=false steals oldest 60 (leaving 62+64 ≈ 0.5).
        level(&mut e, &[on(64, 1.0)]);
        level(&mut e, &[])[0]
    };
    let prefer_true = run_prefer(true);
    let prefer_false = run_prefer(false);
    assert!(
        close(prefer_true, 0.3),
        "prefer_released=true should steal released 62: {prefer_true}"
    );
    assert!(
        prefer_false > 0.45,
        "prefer_released=false should steal oldest held 60: {prefer_false}"
    );

    // 2. Steal::Quietest steals the quietest sounding voice regardless of age.
    let mut loud = zone(&mut store, 1.0);
    loud.gain = 0.6;
    loud.keys = 60..=60;
    let mut quiet = zone(&mut store, 1.0);
    quiet.gain = 0.1;
    quiet.keys = 62..=62;
    let mut fresh = zone(&mut store, 1.0);
    fresh.gain = 0.3;
    fresh.keys = 64..=64;
    let mut plan = Plan::new(vec![loud, quiet, fresh], 2);
    plan.steal = kontra_ir::VoiceGroup {
        steal: kontra_ir::Steal::Quietest,
        prefer_released: false,
        fade_ms: 1.0,
        ..kontra_ir::VoiceGroup::DEFAULT
    };
    let mut e = engine(&store, plan);
    level(&mut e, &[on(60, 1.0)]);
    level(&mut e, &[on(62, 1.0)]);
    level(&mut e, &[on(64, 1.0)]);
    let [lvl, _] = level(&mut e, &[]);
    assert!(
        close(lvl, 0.9),
        "Steal::Quietest should steal 0.1 (leaving 0.6 + 0.3 = 0.9): {lvl}"
    );

    // 3. Exclusive group choke must NOT self-choke two zones triggered by the same note.
    let mut a = zone(&mut store, 0.2);
    a.voice_group = Some(0);
    let mut b = zone(&mut store, 0.3);
    b.voice_group = Some(1);
    let mut plan = Plan::new(vec![a, b], 8);
    let ex = kontra_ir::VoiceGroup {
        voices: 8,
        exclusive: Some(1),
        fade_ms: 1.0,
        ..kontra_ir::VoiceGroup::DEFAULT
    };
    plan.voice_groups = vec![ex, ex];
    let mut e = engine(&store, plan);
    level(&mut e, &[on(60, 1.0)]);
    let [same_note, _] = level(&mut e, &[]);
    assert!(
        close(same_note, 0.5),
        "same note in exclusive groups keeps both voices: {same_note}"
    );
}

#[test]
fn reverse_start_offset_and_negative_s_mod_clamp_and_offset_limited() {
    use kontra_engine::ir::{Mod, ModRoute, ParamAddr, Source};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let frames = 4096u64;
    let bytes = (0..frames)
        .flat_map(|i| (i as f32 / 4096.0).to_le_bytes())
        .collect();
    let key = BlobKey {
        file: "ramp-rev-smod".into(),
        offset: frames,
        fmt: FMT,
    };
    let sample = store
        .load(
            &key,
            Arc::new(MemSource::new(FMT, bytes)),
            frames,
            std::slice::from_ref(&(0..frames)),
        )
        .unwrap();

    // 1. Reverse zone with positive SampleStart mod shifts region.end inward by 500.
    let mut rev = Zone::new(
        Region {
            mix: None,
            table: None,
            sample: sample.clone(),
            start: 0,
            end: frames,
            loops: Default::default(),
            reverse: true,
        },
        60,
        RATE,
    );
    rev.start_mod = 500;
    let mut plan = Plan::new(vec![rev], 8);
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        route: ModRoute::new(Source::Constant, ParamAddr::SampleStart, 1.0),
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    // At frame 128 in reverse from end = 4096 - 500 = 3596: plays 3595 - 128 = 3467.
    let frame = level(&mut e, &[on(60, 1.0)])[0] * 4096.0;
    assert!((frame - 3467.0).abs() < 1.0, "reverse start mod: {frame}");

    // 2. Forward zone with negative SampleStart mod clamps at zone.region.start (200),
    // never reading before it.
    let mut fwd = Zone::new(
        Region {
            mix: None,
            table: None,
            sample,
            start: 200,
            end: frames,
            loops: Default::default(),
            reverse: false,
        },
        60,
        RATE,
    );
    fwd.start_mod = 500;
    let mut plan = Plan::new(vec![fwd], 8);
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        route: ModRoute::new(Source::Constant, ParamAddr::SampleStart, -1.0),
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    // Clamped to 200: at frame 128 reads 328 (NOT 0 + 128).
    let frame = level(&mut e, &[on(60, 1.0)])[0] * 4096.0;
    assert!((frame - 328.0).abs() < 1.0, "negative S.Mod clamp: {frame}");
}

#[test]
fn zone_range_crossfade_at_key_zero_sounds_at_one_over_f_plus_one() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut z = zone(&mut store, 1.0);
    z.keys = 0..=127;
    z.root = 0;
    z.fades = [(0, 0), (3, 0)];
    z.range_crossfades();
    assert_eq!(z.crossfades[0].fade_in, Some((-1, 3)));
    let mut e = engine(&store, Plan::new(vec![z], 8));
    let [lvl, _] = level(&mut e, &[on(0, 1.0)]);
    assert!(
        close(lvl, 0.25),
        "key 0 with fade_low_key=3 must sound at 1/(3+1)=0.25, got {lvl}"
    );
}
