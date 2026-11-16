//! Sampler behaviour as engine data: modulation routes, filters, effects and scripts.
#![cfg(test)]

mod common;

use std::sync::Arc;

use kontra_engine::ir::{self};
use kontra_engine::{Bus, Engine, Plan, Timed, Zone};
use kontra_events::Event;
use kontra_stream::{Store, StoreConfig};
use kontra_time::SampleTime;
use kontra_voice::Region;

use common::*;

#[test]
fn a_controller_opens_a_voice_filter() {
    use kontra_engine::ir::{Effect, EffectKind, Filter, Mod, ModKind, Pass, Source};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![Zone::new(sine(&mut store), 60, RATE)], 8);
    // A 4-pole low pass at 100 Hz that a controller opens by 6 octaves.
    plan.groups[0].inserts = vec![Some(Effect {
        kind: EffectKind::Filter(Filter {
            pass: Pass::Low,
            hz: 100.0,
            stages: 2,
            ladder: None,
            damping: [1.85, 0.77, 0.0],
            gain: 1.0,
            octaves: 6.0,
            q_octaves: 0.0,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ModKind::Cutoff {
            source: Source::Controller(74),
            insert: 0,
            depth: 1.0,
        },
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let peak = |e: &mut Engine, events: &[Timed<Event>]| {
        let now = e.now();
        let input: Vec<_> = events
            .iter()
            .map(|ev| Timed {
                at: now,
                body: ev.body,
            })
            .collect();
        let mut out = vec![[0.0f32; 2]; 4800];
        e.process(&mut out, &input);
        out[2400..].iter().map(|s| s[0].abs()).fold(0.0, f32::max)
    };
    peak(&mut e, &[on(60, 1.0)]);
    let closed = peak(&mut e, &[]);
    peak(&mut e, &[cc(74, 127)]);
    let open = peak(&mut e, &[]);
    assert!(closed < 0.001, "closed {closed}");
    assert!((open - 0.1).abs() < 0.01, "open {open}");
}

#[test]
fn a_write_after_init_opens_a_shared_group_filter_while_the_note_sounds() {
    use kontra_engine::ir::{Effect, EffectKind, Filter, Pass};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![Zone::new(sine(&mut store), 60, RATE)], 8);
    // No route moves it, so its voices share one chain on the group's sum.
    let filter = |hz| Effect {
        kind: EffectKind::Filter(Filter {
            pass: Pass::Low,
            hz,
            stages: 2,
            ladder: None,
            damping: [1.85, 0.77, 0.0],
            gain: 1.0,
            octaves: 0.0,
            q_octaves: 0.0,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    plan.groups[0].inserts = vec![Some(filter(100.0))];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let peak = |e: &mut Engine, events: &[Timed<Event>]| {
        let mut out = vec![[0.0f32; 2]; 4800];
        e.process(&mut out, events);
        out[2400..].iter().map(|s| s[0].abs()).fold(0.0, f32::max)
    };
    peak(&mut e, &[on(60, 1.0)]);
    let closed = peak(&mut e, &[]);
    let envelope = e.group_sounds()[0].0;
    e.set_group_sound(0, envelope, Some((0, filter(20_000.0))));
    let open = peak(&mut e, &[]);
    assert!(closed < 0.001, "closed {closed}");
    assert!((open - 0.1).abs() < 0.01, "open {open}");
}

/// A group with one EQ insert, `eq`, and the given routes.
fn eq_group(
    region: Region,
    eq: [kontra_engine::ir::Bell; 4],
    mods: Vec<kontra_engine::ir::Mod>,
) -> Plan {
    use kontra_engine::ir::{Effect, EffectKind};
    let mut plan = Plan::new(vec![Zone::new(region, 60, RATE)], 8);
    plan.groups[0].mods = mods;
    plan.groups[0].inserts = vec![Some(Effect {
        kind: EffectKind::Eq(eq),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    plan
}

#[test]
fn a_velocity_moved_eq_band_makes_a_hard_note_louder_at_its_frequency() {
    use kontra_engine::ir::{Bell, Mod, ModKind, Source};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let region = sine(&mut store);
    // The 1 kHz band moves +24 dB across velocity: 12 dB at 0.5, 24 dB at 1.
    let route = Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ModKind::EqGain {
            source: Source::Velocity,
            insert: 0,
            band: 0,
            depth_db: 24.0,
            limit_db: 36.0,
            span_db: 72.0,
        },
    };
    let bell = Bell {
        hz: 1000.0,
        q: 1.0,
        gain_db: 0.0,
        ..Bell::FLAT
    };
    let peak = |velocity: f64| {
        let mut e = Engine::new(
            Arc::new(eq_group(
                region.clone(),
                [bell, Bell::FLAT, Bell::FLAT, Bell::FLAT],
                vec![route.clone()],
            )),
            store.client(),
            RATE,
        );
        let mut out = vec![[0.0; 2]; 4800];
        e.process(&mut out, &[on(60, velocity)]);
        out[2400..].iter().map(|s| s[0].abs()).fold(0.0, f32::max)
    };
    let (soft, hard) = (peak(0.5), peak(1.0));
    let ratio = hard / soft;
    assert!(
        (ratio / 10f32.powf(12.0 / 20.0) - 1.0).abs() < 0.05,
        "soft {soft} hard {hard} ratio {ratio}"
    );
}

#[test]
fn an_lfo_moved_resonance_changes_the_response_over_time() {
    use kontra_engine::ir::{
        Clock, Effect, EffectKind, Filter, Lfo, Mod, ModKind, Pass, Rate, Source, Wave,
    };
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![Zone::new(sine(&mut store), 60, RATE)], 8);
    // A 1 kHz band pass with unit gain at its peak, its Q moved by a square
    // LFO at 1 Hz: its damping falls 4× (Q 4×) for one half, rises 4× for the other.
    plan.groups[0].inserts = vec![Some(Effect {
        kind: EffectKind::Filter(Filter {
            pass: Pass::Band,
            hz: 1000.0,
            stages: 1,
            ladder: None,
            damping: [1.0; 3],
            gain: 1.0,
            octaves: 0.0,
            q_octaves: 2.0,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    plan.groups[0].lfos = vec![Lfo {
        wave: Wave::Square { width: 0.5 },
        rate: Rate::Hz(1.0),
        phase: 0.0,
        fade_ms: 0.0,
        clock: Clock::Free,
    }];
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ModKind::Resonance {
            source: Source::Lfo(0),
            insert: 0,
            depth: 1.0,
        },
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0f32; 2]; 48_000];
    e.process(&mut out, &[on(60, 1.0)]);
    let window = |from: f32, to: f32| {
        let (a, b) = ((from * 48_000.0) as usize, (to * 48_000.0) as usize);
        out[a..b].iter().map(|s| s[0].abs()).fold(0.0, f32::max)
    };
    let (first, second) = (window(0.1, 0.4), window(0.6, 0.9));
    let swing = (first / second).max(second / first);
    assert!(swing > 4.0, "first {first} second {second}");
}

#[test]
fn a_ladders_resonance_route_scales_its_resonance() {
    use kontra_engine::ir::{
        Clock, Effect, EffectKind, Filter, Ladder, Lfo, Mod, ModKind, Pass, Rate, Source, Wave,
    };
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![Zone::new(sine(&mut store), 60, RATE)], 8);
    // A four-pole ladder at the sine's 1 kHz, resonance 0.9 (k = 0.2), its
    // resonance scaled by a square LFO at full depth: (x + 1)/2 is 1 for one
    // half (the peak rings) and 0 for the other (k = 2, no peak).
    plan.groups[0].inserts = vec![Some(Effect {
        kind: EffectKind::Filter(Filter {
            pass: Pass::Low,
            hz: 1000.0,
            stages: 0,
            ladder: Some(Ladder::Explicit { taps: 4, fat: 0.0 }),
            damping: [0.2; 3],
            gain: 1.0,
            octaves: 0.0,
            q_octaves: 0.0,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    plan.groups[0].lfos = vec![Lfo {
        wave: Wave::Square { width: 0.5 },
        rate: Rate::Hz(1.0),
        phase: 0.0,
        fade_ms: 0.0,
        clock: Clock::Free,
    }];
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ModKind::Resonance {
            source: Source::Lfo(0),
            insert: 0,
            depth: 1.0,
        },
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0f32; 2]; 48_000];
    e.process(&mut out, &[on(60, 1.0)]);
    let window = |from: f32, to: f32| {
        let (a, b) = ((from * 48_000.0) as usize, (to * 48_000.0) as usize);
        out[a..b].iter().map(|s| s[0].abs()).fold(0.0, f32::max)
    };
    let (first, second) = (window(0.1, 0.4), window(0.6, 0.9));
    let swing = (first / second).max(second / first);
    assert!(swing > 2.0, "first {first} second {second}");
}

#[test]
fn a_group_formant_shapes_its_voices() {
    use kontra_engine::ir::{Effect, EffectKind, Formant};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![Zone::new(sine(&mut store), 60, RATE)], 8);
    plan.groups[0].inserts = vec![Some(Effect {
        kind: EffectKind::Formant(Formant {
            talk: 0.5,
            sharp: 0.5,
            size: 0.5,
            set: kontra_engine::ir::VowelSet::First,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0f32; 2]; 9600];
    e.process(&mut out, &[on(60, 1.0)]);
    let peak = out[4800..].iter().map(|s| s[0].abs()).fold(0.0, f32::max);
    // Kontakt's Formant I at these knobs: -2.15 dB at 1 kHz.
    let db = 20.0 * (peak / 0.1).log10();
    assert!((db - -2.15).abs() < 0.3, "{db} dB");
}

#[test]
fn a_velocity_attack_route_shortens_soft_attacks() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    plan.groups[0].envelope.attack = 4800; // 100 ms, linear
    plan.groups[0].mods = vec![ir::Mod {
        name: "VEL_ATTACK".into(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ir::ModKind::Attack {
            source: ir::Source::Velocity,
            intensity: 1.0,
            floor_ms: 2.0,
            envelope: None,
        },
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let [hard, _] = level(&mut e, &[on(60, 1.0)]);
    assert!((hard - 0.5 * 128.0 / 4800.0).abs() < 1e-3, "{hard}");
    level(&mut e, &[off(60)]);
    for _ in 0..40 {
        level(&mut e, &[]);
    }
    // Velocity 0.9 scales the knob by fcubic(0.9): 2·(102/2)^0.729 − 2 ≈ 33 ms.
    let [soft, _] = level(&mut e, &[on(60, 0.9)]);
    let frames = (2.0 * 51f32.powf(0.729) - 2.0) * 48.0;
    assert!((soft - 0.5 * 128.0 / frames).abs() < 2e-3, "{soft}");
}

#[test]
fn timed_modulation_does_not_depend_on_the_block_size() {
    use kontra_engine::ir::{Clock, Curve, Lfo, Mod, ModKind, Rate, Source, Wave};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    plan.groups[0].lfos = vec![Lfo {
        wave: Wave::Sine,
        rate: Rate::Hz(7.0),
        phase: 0.0,
        fade_ms: 0.0,
        clock: Clock::Voice,
    }];
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ModKind::Volume {
            source: Source::Lfo(0),
            intensity: 1.0,
            curve: Curve::Depth { power: 1 },
            tau_ms: 0.0,
        },
    }];
    let plan = Arc::new(plan);
    let render = |block: usize| {
        let mut e = Engine::new(Arc::clone(&plan), store.client(), RATE);
        let mut out = Vec::new();
        let mut buf = vec![[0.0; 2]; block];
        e.process(&mut buf, &[on(60, 1.0)]);
        out.extend_from_slice(&buf);
        while out.len() < 9_600 {
            e.process(&mut buf, &[]);
            out.extend_from_slice(&buf);
        }
        out.truncate(9_600);
        out
    };
    let reference = render(256);
    for block in [64, 100, 1000] {
        assert_eq!(render(block), reference, "block {block}");
    }
}

#[test]
fn an_instrument_convolution_adds_its_wet_path() {
    use kontra_engine::{Response, ir};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    // A delayed half-level tap: on a constant input the wet path is 0.25.
    plan.impulses = vec![Some(Response {
        left: vec![0.0, 0.0, 0.5],
        right: vec![0.0, 0.0, 0.5],
    })];
    plan.inserts = vec![Some(ir::Effect {
        kind: ir::EffectKind::Convolution { impulse: 0 },
        bypass: false,
        output: 1.0,
        dry: 0.5,
    })];
    let mut e = engine(&store, plan);
    let [l, r] = level(&mut e, &[on(60, 1.0)]);
    // Dry 0.5 × 0.5 plus wet 0.25.
    assert!(close(l, 0.5) && close(r, 0.5), "{l} {r}");
}

#[test]
fn an_instrument_reverb_rings_on_after_the_note() {
    use kontra_engine::ir;
    let reverb = ir::Effect {
        kind: ir::EffectKind::Reverb(ir::Reverb {
            seconds: 2.0,
            size: 1.0,
            network: None,
            damping: 0.5,
            diffusion: 1.0,
            predelay_ms: 0.0,
            high_cut_hz: 20_000.0,
            low_shelf_db: 0.0,
            stereo: 1.0,
            modulation_ms: 0.0,
            modulation_hz: 0.0,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    // The summed level of the last block, a second after key up.
    let tail = |inserts: Vec<Option<ir::Effect>>, group: bool| {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
        if group {
            plan.groups[0].inserts = inserts;
        } else {
            plan.inserts = inserts;
        }
        let mut e = engine(&store, plan);
        let first = level(&mut e, &[on(60, 1.0)]);
        let mut buf = vec![[0.0; 2]; 256];
        for _ in 0..50 {
            e.process(&mut buf, &[]);
        }
        level(&mut e, &[off(60)]);
        for _ in 0..190 {
            e.process(&mut buf, &[]);
        }
        let sum: f32 = buf.iter().map(|[l, r]| l.abs() + r.abs()).sum();
        (first, sum)
    };
    let (dry_first, dry_tail) = tail(Vec::new(), false);
    assert!(dry_first[0] > 0.1 && dry_tail == 0.0, "the voice has ended");
    let (first, wet) = tail(vec![Some(reverb)], false);
    // Fully wet: nothing until the shortest line (30 ms) comes round.
    assert_eq!(first, [0.0; 2]);
    assert!(wet > 1e-3 && wet.is_finite(), "tail {wet}");
    // In a group rack it runs on the group's sum, the same way.
    let (first, group) = tail(vec![Some(reverb)], true);
    assert_eq!(first, [0.0; 2]);
    assert!(
        (group - wet).abs() < 1e-3 * wet,
        "group {group} instrument {wet}"
    );
}

#[test]
fn a_compressor_slot_mixes_its_dry_level_from_the_input() {
    use kontra_engine::ir;
    let render = |output: f32| {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
        // Squashes hard; the slot adds its dry signal to `output` of it.
        plan.inserts = vec![Some(ir::Effect {
            kind: ir::EffectKind::Compressor(ir::Compressor {
                threshold_db: -40.0,
                ratio: 20.0,
                knee_db: 0.0,
                attack_ms: 0.0,
                release_ms: 100.0,
                makeup_db: 0.0,
                makeup_slope: 0.0,
                link: true,
                mix: 1.0,
                detector: ir::Detector::Input,
                sidechain_hz: None,
            }),
            bypass: false,
            output,
            dry: 1.0,
        })];
        let mut e = engine(&store, plan);
        level(&mut e, &[on(60, 1.0)])
    };
    let [l, r] = render(0.0);
    assert!(close(l, 0.5) && close(r, 0.5), "{l} {r}");
    // The compressed path adds a little on top of the dry 0.5.
    let [l, _] = render(1.0);
    assert!(l > 0.5 && l < 0.6, "{l}");
}

#[test]
fn a_group_waveshaper_clips_each_voice_not_their_sum() {
    use kontra_engine::ir::{DriveCurve, Effect, EffectKind, Shaper};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let zones = vec![
        Zone::new(sine(&mut store), 60, RATE),
        Zone::new(sine(&mut store), 60, RATE),
    ];
    let mut plan = Plan::new(zones, 8);
    plan.groups[0].inserts = vec![Some(Effect {
        kind: EffectKind::Shaper(Shaper {
            pre_lowpass_hz: None,
            curve: DriveCurve::Soft(1.0),
            lowpass_hz: None,
            highpass_hz: None,
            sections: ir::Sections::NONE,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0f32; 2]; 9600];
    e.process(&mut out, &[on(60, 1.0)]);
    let peak = out[4800..].iter().map(|s| s[0].abs()).fold(0.0, f32::max);
    // Each 0.1 sine clips to 0.64 (4x, then 2|x| − x²): 1.28 together. The
    // sum through one curve would peak at 0.96.
    assert!((peak - 1.28).abs() < 0.02, "{peak}");
}

#[test]
fn a_group_lofis_noise_fades_with_the_voice() {
    use kontra_engine::ir::{Crush, Effect, EffectKind};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![Zone::new(sine(&mut store), 60, RATE)], 8);
    plan.groups[0].envelope.release = 2400;
    plan.groups[0].inserts = vec![Some(Effect {
        kind: EffectKind::Crush(Crush {
            rate: 1.0,
            bits: 16.0,
            noise: 0.075,
            noise_tone: 1.0,
            converter: None,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0f32; 2]; 4800];
    e.process(&mut out, &[on(60, 1.0)]);
    let peak = out.iter().map(|s| s[0].abs()).fold(0.0, f32::max);
    assert!(peak > 0.1, "{peak}");
    out.fill([0.0; 2]);
    e.process(&mut out, &[off(60)]);
    // The noise sits before the envelope, so it fades with the release
    // (after it, at 0.075, it would outlast the faded sine).
    let end = out[2300..2400]
        .iter()
        .map(|s| s[0].abs())
        .fold(0.0, f32::max);
    assert!(end < 0.02, "{end}");
}

#[test]
fn a_route_moves_a_group_waveshapers_amount_per_voice() {
    use kontra_engine::ir::{
        DriveCurve, Effect, EffectKind, EffectParam, Mod, ModKind, Shaper, Source,
    };
    let peak = |bypass: bool| {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let mut plan = Plan::new(vec![Zone::new(sine(&mut store), 60, RATE)], 8);
        plan.groups[0].inserts = vec![Some(Effect {
            kind: EffectKind::Shaper(Shaper {
                pre_lowpass_hz: None,
                curve: DriveCurve::Soft(0.0),
                lowpass_hz: None,
                highpass_hz: None,
                sections: ir::Sections::NONE,
            }),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        })];
        plan.groups[0].mods = vec![Mod {
            name: String::new(),
            slot: 0,
            target: 0,
            bypass,
            kind: ModKind::EffectParam {
                source: Source::Velocity,
                insert: 0,
                param: EffectParam::ShaperAmount,
                depth: 1.0,
                span: 1.0,
                unipolar: false,
            },
        }];
        let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
        let mut out = vec![[0.0f32; 2]; 9600];
        e.process(&mut out, &[on(60, 1.0)]);
        out[4800..].iter().map(|s| s[0].abs()).fold(0.0, f32::max)
    };
    // Velocity 1 takes the curve from none to full: the 0.1 sine clips to 0.64.
    assert!((peak(true) - 0.1).abs() < 0.005, "{}", peak(true));
    assert!((peak(false) - 0.64).abs() < 0.01, "{}", peak(false));
}

#[test]
fn a_route_moves_an_eq_bands_centre_per_voice() {
    use kontra_engine::ir::{Bell, Effect, EffectKind, EffectParam, Mod, ModKind, Shape, Source};
    let peak = |octaves: f32| {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let mut plan = Plan::new(vec![Zone::new(sine(&mut store), 60, RATE)], 8);
        let bell = Bell {
            shape: Shape::Peak,
            hz: 100.0,
            q: 2.0,
            gain_db: 12.0,
        };
        plan.groups[0].inserts = vec![Some(Effect {
            kind: EffectKind::Eq([bell, Bell::FLAT, Bell::FLAT, Bell::FLAT]),
            bypass: false,
            output: 1.0,
            dry: 0.0,
        })];
        plan.groups[0].mods = vec![Mod {
            name: String::new(),
            slot: 0,
            target: 0,
            bypass: false,
            kind: ModKind::EffectParam {
                source: Source::Constant,
                insert: 0,
                param: EffectParam::BellOctaves(0),
                depth: octaves,
                span: 1.0,
                unipolar: false,
            },
        }];
        let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
        let mut out = vec![[0.0f32; 2]; 4800];
        e.process(&mut out, &[on(60, 1.0)]);
        out[2400..].iter().map(|s| s[0].abs()).fold(0.0, f32::max)
    };
    // The 1 kHz sine is far from a bell at 100 Hz; moved up 10x it takes the
    // full 12 dB.
    let (away, on_it) = (peak(0.0), peak(10f32.log2()));
    assert!((away - 0.1).abs() < 0.01, "{away}");
    assert!((on_it / 0.1 - 10f32.powf(0.6)).abs() < 0.1, "{on_it}");
}

#[test]
fn an_lfo_on_a_group_waveshaper_retunes_it_every_32_frames() {
    use kontra_engine::ir::{
        Clock, DriveCurve, Effect, EffectKind, EffectParam, Lfo, Mod, ModKind, Rate, Shaper,
        Source, Wave,
    };
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.1)], 8);
    plan.groups[0].inserts = vec![Some(Effect {
        kind: EffectKind::Shaper(Shaper {
            pre_lowpass_hz: None,
            curve: DriveCurve::Soft(0.0),
            lowpass_hz: None,
            highpass_hz: None,
            sections: ir::Sections::NONE,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    plan.groups[0].lfos = vec![Lfo {
        wave: Wave::Sine,
        rate: Rate::Hz(20.0),
        phase: 0.0,
        fade_ms: 0.0,
        clock: Clock::Voice,
    }];
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ModKind::EffectParam {
            source: Source::Lfo(0),
            insert: 0,
            param: EffectParam::ShaperAmount,
            depth: 1.0,
            span: 1.0,
            unipolar: false,
        },
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut out = vec![[0.0f32; 2]; 1024];
    e.process(&mut out, &[on(60, 1.0)]);
    // Within one 256-frame span of the full refresh, the DC through the
    // curve takes a new level at each of its 32-frame ticks.
    let mut levels: Vec<f32> = out[512..768].iter().step_by(32).map(|s| s[0]).collect();
    levels.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
    assert!(levels.len() >= 6, "{levels:?}");
}

#[test]
fn a_synced_delay_echoes_one_beat_later_at_the_host_tempo() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    plan.inserts = vec![Some(ir::Effect {
        kind: ir::EffectKind::Delay(ir::Delay {
            direct: 0.0,
            time: ir::DelayTime::Beats(1.0),
            depth_ms: 0.0,
            second_head: false,
            input: ir::IDENTITY,
            feedback: [[0.0; 2]; 2],
            output: ir::IDENTITY,
            damping_hz: f32::INFINITY,
            low_cut_hz: 0.0,
            steep_low_cut: false,
            body: &[],
            triangle_s: None,
            shape: &[],
            noise: [0.0; 2],
            high_shelf: None,
            spread_ms: 0.0,
            rate: ir::Rate::Hz(0.0),
            phase: 0.0,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    let mut e = engine(&store, plan);
    e.set_tempo(240.0);
    let mut out = Vec::new();
    let mut buf = vec![[0.0; 2]; 256];
    e.process(&mut buf, &[on(60, 1.0)]);
    out.extend_from_slice(&buf);
    while out.len() < RATE as usize / 2 {
        e.process(&mut buf, &[]);
        out.extend_from_slice(&buf);
    }
    let first = out.iter().position(|f| f[0].abs() > 1e-6).unwrap();
    // A quarter note at 240 BPM: 250 ms, within the read's interpolation.
    assert!(
        first.abs_diff(RATE as usize / 4) <= 1,
        "first echo at {first}"
    );
}

#[test]
fn a_group_phaser_without_a_dry_level_plays_on_the_group_sum() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    // The phaser runs on the group's sum: its wet path carries the voice.
    plan.groups[0].inserts = vec![Some(ir::Effect {
        kind: ir::EffectKind::Phaser(ir::Phaser {
            stages: 4,
            triangle: false,
            hz: 300.0,
            spread_hz: 270.0,
            exponent: 1.5,
            feedback: 0.5,
            rate: ir::Rate::Hz(1.0),
            phase: 0.0,
        }),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    })];
    let mut e = engine(&store, plan);
    let [l, r] = level(&mut e, &[on(60, 1.0)]);
    assert!(
        l.abs() > 0.05 && r.abs() > 0.05 && !close(l, 0.5),
        "{l} {r}"
    );
}

#[test]
fn a_pitch_bend_release_route_reads_the_wheel_from_zero_to_one() {
    use kontra_engine::ir;
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    plan.groups[0].envelope.release = 48_000; // 1 s
    plan.groups[0].mods = vec![ir::Mod {
        name: "PB_ATTACK".into(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ir::ModKind::Release {
            source: ir::Source::PitchBend,
            intensity: 1.0,
            floor_ms: 0.0,
        },
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    level(&mut e, &[on(60, 1.0)]);
    level(&mut e, &[off(60)]);
    // The wheel at rest reads 0.5 and halves the 1 s release: still
    // sounding 400 ms after the key is let go, silent by 600 ms.
    let mut tail = |blocks| {
        let mut tail = [1.0; 2];
        for _ in 0..blocks {
            tail = level(&mut e, &[]);
        }
        tail
    };
    assert!(tail(75)[0] > 0.0);
    assert_eq!(tail(40), [0.0; 2]);
}

#[test]
fn a_pitch_bend_volume_route_reads_the_wheel_from_zero_to_one() {
    use kontra_engine::ir::{Curve, Mod, ModKind, Source};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 1.0)], 8);
    plan.groups[0].mods = vec![Mod {
        name: "PB_VOLUME".into(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ModKind::Volume {
            source: Source::PitchBend,
            intensity: 1.0,
            curve: Curve::Depth { power: 1 },
            tau_ms: 0.0,
        },
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    let bend = |lsb, msb| Timed {
        at: SampleTime(0),
        body: Event::midi1(0xE0, lsb, msb),
    };
    let settled = |e: &mut Engine, events: &[Timed<Event>]| {
        level(e, events);
        (0..8).map(|_| level(e, &[])[0]).last().unwrap_or(0.0)
    };
    // At rest the wheel reads 0.5: half level; fully up, all of it.
    let rest = settled(&mut e, &[on(60, 1.0)]);
    assert!((rest - 0.5).abs() < 1e-3, "{rest}");
    let up = settled(&mut e, &[bend(0x7F, 0x7F)]);
    assert!((up - 1.0).abs() < 1e-3, "{up}");
}

#[test]
fn a_lagging_controller_route_glides_as_a_one_pole() {
    use kontra_engine::ir::{Curve, Mod, ModKind, Source};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 1.0)], 8);
    // Kontakt's lag 120 ms: τ = 120 / ln 100.
    let tau_ms = 120.0 / 100f32.ln();
    plan.groups[0].mods = vec![Mod {
        name: "CC11".into(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ModKind::Volume {
            source: Source::Controller(11),
            intensity: 1.0,
            curve: Curve::Depth { power: 1 },
            tau_ms,
        },
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    level(&mut e, &[cc(11, 127), on(60, 1.0)]);
    let mut out = vec![[0.0; 2]; 9600];
    e.process(&mut out, &[]);
    let from = out[9599][0];
    e.process(&mut out, &[cc(11, 64)]);
    e.process(&mut out[..4800], &[]);
    let to = out[4799][0];
    e.process(&mut out, &[cc(11, 127)]);
    // Back up: e^-1 of the way left after τ, under 2% after 4τ.
    let left = |ms: f32| (from - out[(ms * 48.0) as usize][0]) / (from - to);
    assert!(
        (left(tau_ms) - (-1.0f32).exp()).abs() < 0.01,
        "{}",
        left(tau_ms)
    );
    assert!(left(4.0 * tau_ms) < 0.02 && left(0.1) > 0.99, "{from} {to}");
}

#[test]
fn a_lagging_pitch_route_glides_its_semitones() {
    use kontra_engine::ir::{Mod, ModKind, Source};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 1.0)], 8);
    // Kontakt's pitch-bend route, lag 250 ms: τ = 250 / ln 100.
    let tau_ms = 250.0 / 100f64.ln();
    plan.groups[0].mods = vec![Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind: ModKind::Pitch {
            source: Source::PitchBend,
            semitones: 2.0,
            depth: 1.0,
            tau_ms: tau_ms as f32,
        },
    }];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    level(&mut e, &[on(60, 1.0)]);
    // Semitones the voice plays over each 256-frame block from the bend on.
    let at = |e: &Engine| e.voices().next().map(|(_, p, _)| p).unwrap();
    let mut semitones = Vec::new();
    for n in 0..48 {
        let from = at(&e);
        let bend = Timed {
            at: SampleTime(0),
            body: Event::midi1(0xE0, 0, 0x60),
        };
        level(
            &mut e,
            if n == 0 {
                std::slice::from_ref(&bend)
            } else {
                &[]
            },
        );
        semitones.push(12.0 * ((at(&e) - from) / 256.0).log2());
    }
    // Half the range: a third of the way left after τ, under 2% after 4τ.
    let left = |ms: f64| 1.0 - semitones[(ms * 48.0 / 256.0) as usize];
    assert!(left(0.0) > 0.85, "{semitones:?}");
    assert!(
        (left(tau_ms) - (-1.0f64).exp()).abs() < 0.05,
        "{semitones:?}"
    );
    assert!(left(4.0 * tau_ms) < 0.02, "{semitones:?}");
}

#[test]
fn a_constant_start_phase_route_shifts_a_voices_lfo() {
    use kontra_engine::ir::{Clock, Curve, Lfo, Mod, ModKind, Rate, Source, Wave};
    let first = |phase_route: Option<f32>| {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let mut plan = Plan::new(vec![zone(&mut store, 1.0)], 8);
        plan.groups[0].lfos = vec![Lfo {
            wave: Wave::Sine,
            rate: Rate::Hz(0.1),
            phase: 0.0,
            fade_ms: 0.0,
            clock: Clock::Voice,
        }];
        let route = |kind| Mod {
            name: String::new(),
            slot: 0,
            target: 0,
            bypass: false,
            kind,
        };
        plan.groups[0].mods = vec![route(ModKind::Volume {
            source: Source::Lfo(0),
            intensity: 1.0,
            curve: Curve::Depth { power: 1 },
            tau_ms: 0.0,
        })];
        plan.groups[0].mods.extend(phase_route.map(|depth| {
            route(ModKind::LfoPhase {
                source: Source::Constant,
                lfo: 0,
                depth,
            })
        }));
        let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
        level(&mut e, &[on(60, 1.0)])[0]
    };
    // A slow sine at 0 reads 0.5 on the level; a quarter cycle on, its peak.
    assert!((first(None) - 0.5).abs() < 0.01, "{}", first(None));
    assert!(
        (first(Some(0.25)) - 1.0).abs() < 0.01,
        "{}",
        first(Some(0.25))
    );
}

#[test]
fn an_intensity_route_scales_its_lfos_routes() {
    use kontra_engine::ir::{Clock, Curve, Lfo, Mod, ModKind, Rate, Source, Wave};
    let first = |by: Source| {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let mut plan = Plan::new(vec![zone(&mut store, 1.0)], 8);
        plan.groups[0].lfos = vec![Lfo {
            wave: Wave::Sine,
            rate: Rate::Hz(0.1),
            phase: 0.0,
            fade_ms: 0.0,
            clock: Clock::Voice,
        }];
        let route = |kind| Mod {
            name: String::new(),
            slot: 0,
            target: 0,
            bypass: false,
            kind,
        };
        plan.groups[0].mods = vec![
            route(ModKind::Volume {
                source: Source::Lfo(0),
                intensity: 1.0,
                curve: Curve::Depth { power: 1 },
                tau_ms: 0.0,
            }),
            route(ModKind::Intensity {
                source: by,
                of: Source::Lfo(0),
                depth: 1.0,
            }),
        ];
        let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
        level(&mut e, &[on(60, 1.0)])[0]
    };
    // A slow sine at 0 reads 0.5 at full intensity; a controller at rest
    // takes the LFO's route away.
    assert!((first(Source::Constant) - 0.5).abs() < 0.01);
    assert!((first(Source::Controller(124)) - 1.0).abs() < 0.01);
}

#[test]
fn a_velocity_rate_route_speeds_a_voices_lfo() {
    use kontra_engine::ir::{Clock, Curve, Lfo, Mod, ModKind, Rate, Source, Wave};
    let at_half_a_second = |octaves: f32| {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let mut plan = Plan::new(vec![zone(&mut store, 1.0)], 8);
        plan.groups[0].lfos = vec![Lfo {
            wave: Wave::Sine,
            rate: Rate::Hz(0.5),
            phase: 0.0,
            fade_ms: 0.0,
            clock: Clock::Voice,
        }];
        let route = |kind| Mod {
            name: String::new(),
            slot: 0,
            target: 0,
            bypass: false,
            kind,
        };
        plan.groups[0].mods = vec![
            route(ModKind::Volume {
                source: Source::Lfo(0),
                intensity: 1.0,
                curve: Curve::Depth { power: 1 },
                tau_ms: 0.0,
            }),
            route(ModKind::LfoRate {
                source: Source::Velocity,
                lfo: 0,
                depth: 1.0,
                octaves,
                range_hz: [0.01, 213.0],
            }),
        ];
        let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
        let blocks = (f64::from(RATE) / 512.0).round() as usize;
        let mut last = level(&mut e, &[on(60, 1.0)])[0];
        for _ in 1..blocks {
            last = level(&mut e, &[])[0];
        }
        last
    };
    // Half a second in, a 0.5 Hz sine is at its peak; doubled by full
    // velocity, half a cycle on, back at the middle.
    let still = at_half_a_second(0.0);
    let doubled = at_half_a_second(1.0);
    assert!((still - 1.0).abs() < 0.03, "{still}");
    assert!((doubled - 0.5).abs() < 0.03, "{doubled}");
}

#[test]
fn a_live_controller_moves_an_lfos_rate_mid_note() {
    use kontra_engine::ir::{Clock, Curve, Lfo, Mod, ModKind, Rate, Source, Wave};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 1.0)], 8);
    plan.groups[0].lfos = vec![Lfo {
        wave: Wave::Sine,
        rate: Rate::Hz(1.0),
        phase: 0.0,
        fade_ms: 0.0,
        clock: Clock::Voice,
    }];
    let route = |kind| Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind,
    };
    plan.groups[0].mods = vec![
        route(ModKind::Volume {
            source: Source::Lfo(0),
            intensity: 1.0,
            curve: Curve::Depth { power: 1 },
            tau_ms: 0.0,
        }),
        // The wheel adds up to 2 Hz, the rate held within 0..=2 Hz.
        route(ModKind::LfoHz {
            source: Source::Controller(1),
            lfo: 0,
            depth: 1.0,
            range_hz: [0.0, 2.0],
        }),
    ];
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    level(&mut e, &[on(60, 1.0)]);
    for _ in 1..47 {
        level(&mut e, &[]);
    }
    // A quarter cycle in at 1 Hz, the wheel goes up: 2 Hz from there.
    let mut last = level(&mut e, &[cc(1, 127)])[0];
    for _ in 48..=70 {
        last = level(&mut e, &[])[0];
    }
    // 0.376 s: half a cycle (0.85 had the rate stayed, 0.15 unheld at 3 Hz).
    let t1 = 47.0 * 256.0 / RATE as f32;
    let t = (70.0 * 256.0 + 128.0) / RATE as f32;
    let phase = t1 + 2.0 * (t - t1);
    let want = (1.0 + (std::f32::consts::TAU * phase).sin()) / 2.0;
    assert!((last - want).abs() < 0.03, "{last} vs {want}");
}

#[test]
fn a_controller_moves_a_bus_eq_band_while_it_plays() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut z = Zone::new(sine(&mut store), 60, RATE);
    z.region.loops = vec![kontra_voice::Loop {
        start: 0,
        end: 48_000,
        crossfade: 0,
        ..Default::default()
    }]
    .into();
    let mut plan = Plan::new(vec![z], 8);
    plan.groups[0].bus = 1;
    let bell = ir::Bell {
        hz: 1000.0,
        ..ir::Bell::FLAT
    };
    let eq = ir::Effect {
        kind: ir::EffectKind::Eq([bell, ir::Bell::FLAT, ir::Bell::FLAT, ir::Bell::FLAT]),
        bypass: false,
        output: 1.0,
        dry: 0.0,
    };
    plan.buses.push(Bus {
        name: "Aux".into(),
        gain: 1.0,
        pan: 0.0,
        inserts: vec![Some(eq)],
        sends: Vec::new(),
        to_main: true,
        enabled: true,
        // CC 7 pulls the 1 kHz band down 20 dB at full travel.
        mods: vec![ir::ModKind::EffectParam {
            source: ir::Source::Controller(7),
            insert: 0,
            param: ir::EffectParam::BellGain(0),
            depth: -20.0,
            span: 1.0,
            unipolar: false,
        }],
        blends: Vec::new(),
        lfos: Vec::new(),
    });
    let mut e = engine(&store, plan);
    let peak = |e: &mut Engine, events: &[Timed<Event>]| {
        let mut p = 0.0f32;
        for n in 0..20 {
            let mut out = vec![[0.0; 2]; 256];
            e.process(&mut out, if n == 0 { events } else { &[] });
            p = out
                .iter()
                .fold(if n < 10 { 0.0 } else { p }, |p, f| p.max(f[0].abs()));
        }
        p
    };
    let open = peak(&mut e, &[cc(7, 0), on(60, 1.0)]);
    // Played on channel 4 now: the route follows it.
    let ch4 = Timed {
        at: SampleTime(0),
        body: Event::midi1(0xB3, 7, 127),
    };
    let cut = peak(&mut e, &[ch4]);
    let db = 20.0 * (cut / open).log10();
    assert!((db - -20.0).abs() < 1.0, "{open} -> {cut}: {db} dB");
}

/// An attack route into a modulation envelope stretches that envelope's
/// attack per voice with the amplitude attack's law: velocity 0.5 at
/// intensity 1 halves a 100 ms rise, so 60 ms in its pitch route is at the
/// top (an octave) while a full-velocity voice is still rising.
/// One zone whose envelope 0 rises over 100 ms and bends pitch up an octave,
/// its attack timed by `source`.
fn a_rise_timed_by(store: &mut Store, source: kontra_engine::ir::Source) -> Plan {
    use kontra_engine::ir::{Breakpoint, Breakpoints, Mod, ModKind, Source};
    let route = |kind| Mod {
        name: String::new(),
        slot: 0,
        target: 0,
        bypass: false,
        kind,
    };
    let mut plan = Plan::new(vec![zone(store, 1.0)], 8);
    plan.groups[0].envelopes = vec![Breakpoints {
        start: 0.0,
        points: vec![Breakpoint {
            ms: 100.0,
            level: 1.0,
            curve: 0.0,
        }],
        sustain: Some(0),
        loop_from: None,
        stages: false,
    }];
    plan.groups[0].mods = vec![
        route(ModKind::Pitch {
            source: Source::Envelope(0),
            semitones: 12.0,
            depth: 1.0,
            tau_ms: 0.0,
        }),
        route(ModKind::Attack {
            source,
            intensity: 1.0,
            floor_ms: 0.0,
            envelope: Some(0),
        }),
    ];
    plan
}

/// Runs `e` to `ms` and returns each sounding voice's bend over one block,
/// semitones above its key's own pitch (`keys` in voice order).
fn bends_at(e: &mut Engine, ms: u64, keys: &[u8]) -> Vec<f64> {
    while e.now().0 < u64::from(RATE) * ms / 1000 {
        level(e, &[]);
    }
    let from: Vec<f64> = e.voices().map(|(_, p, _)| p).collect();
    level(e, &[]);
    let to = e.voices().map(|(_, p, _)| p);
    (from.iter().zip(to).zip(keys))
        .map(|((a, b), &k)| 12.0 * ((b - a) / 256.0).log2() - (f64::from(k) - 60.0))
        .collect()
}

#[test]
fn an_attack_route_stretches_a_modulation_envelopes_attack() {
    let semitones_at_60_ms = |velocity| {
        let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
        let plan = a_rise_timed_by(&mut store, kontra_engine::ir::Source::Velocity);
        let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
        level(&mut e, &[on(60, velocity)]);
        bends_at(&mut e, 60, &[60])[0]
    };
    let soft = semitones_at_60_ms(0.5);
    assert!(
        (soft - 12.0).abs() < 0.1,
        "a 50 ms rise is at the top: {soft}"
    );
    let hard = semitones_at_60_ms(1.0);
    assert!((hard - 7.2).abs() < 0.5, "a 100 ms rise is at 0.6: {hard}");
}

#[test]
fn a_controller_times_the_next_attack_not_the_rising_one() {
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let plan = a_rise_timed_by(&mut store, kontra_engine::ir::Source::Controller(1));
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    level(&mut e, &[cc(1, 127), on(60, 1.0)]);
    bends_at(&mut e, 30, &[60]);
    level(&mut e, &[cc(1, 64), on(72, 1.0)]);
    // Read 3,072..3,328 frames in: the first voice is 64 ms into its
    // 100 ms rise; the next started at 1,792 (37.3 ms), so 26.7 ms into
    // 50.4 ms. Re-timed live, the first would sit at the top (12).
    let [first, next] = bends_at(&mut e, 60, &[60, 72])[..] else {
        panic!("two voices");
    };
    assert!(
        (first - 12.0 * 0.64).abs() < 0.3,
        "the rising voice keeps its 100 ms: {first}"
    );
    let x = (3072.0 - 1792.0) / 48.0 / (100.0 * 64.0 / 127.0);
    assert!(
        (next - 12.0 * x).abs() < 0.3,
        "the next note rises over 50 ms: {next}"
    );
}

#[test]
fn an_envelope_timing_its_own_attack_renders() {
    use kontra_engine::ir::{ModKind, Source};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = a_rise_timed_by(&mut store, Source::Envelope(0));
    // A second envelope and the first time each other's attack too.
    let rise = plan.groups[0].envelopes[0].clone();
    plan.groups[0].envelopes.push(rise);
    let mut cross = plan.groups[0].mods[1].clone();
    cross.kind = ModKind::Attack {
        source: Source::Envelope(1),
        intensity: 1.0,
        floor_ms: 0.0,
        envelope: Some(0),
    };
    plan.groups[0].mods.push(cross);
    let mut e = Engine::new(Arc::new(plan), store.client(), RATE);
    level(&mut e, &[on(60, 1.0)]);
    // Read at note-on, an envelope is its start level (0 here), which
    // shortens the rise to nothing: the bend is at the top at once.
    let bend = bends_at(&mut e, 10, &[60])[0];
    assert!((bend - 12.0).abs() < 0.1, "{bend}");
}

/// A foreign machine that records the knob moves it is given.
#[derive(Debug)]
struct Knobbed(std::sync::mpsc::Sender<(i32, f64, f64)>);

struct KnobMachine(std::sync::mpsc::Sender<(i32, f64, f64)>);

impl kontra_vm::Foreign for Knobbed {
    fn machine(&self) -> Box<dyn kontra_vm::Machine> {
        Box::new(KnobMachine(self.0.clone()))
    }
    fn controls(&self) -> &[String] {
        &[]
    }
}

impl kontra_vm::Machine for KnobMachine {
    fn start(&mut self, _: u32, _: u64) -> Option<kontra_vm::CoId> {
        None
    }
    fn run(&mut self, _: kontra_vm::CoId, _: &mut dyn kontra_vm::HostApi) -> kontra_vm::Step {
        kontra_vm::Step::Done
    }
    fn cancel(&mut self, _: kontra_vm::CoId) {}
    fn context(&self, _: kontra_vm::CoId) -> Option<u64> {
        None
    }
    fn spawned(&mut self) -> Option<(kontra_vm::CoId, u64)> {
        None
    }
    fn faults(&self) -> kontra_vm::Faults {
        kontra_vm::Faults::default()
    }
    fn control(&mut self, _: i32, _: f64, _: u64) -> Option<kontra_vm::CoId> {
        None
    }
    fn control_mod(&mut self, id: i32, scale: f64, m: f64, _: u64) -> Option<kontra_vm::CoId> {
        let _ = self.0.send((id, scale, m));
        None
    }
}

/// Routes onto one script knob add (a multiplicative one apart), the knob
/// moves once per change of the sums and not while they hold still.
#[test]
fn routes_onto_one_script_knob_move_it_once_with_their_sums() {
    use kontra_engine::{Callbacks, Controls, EnginePars, KnobModPlan, Program, ScriptPlan};
    let mut store = Store::new(StoreConfig::DEFAULT).unwrap();
    let mut plan = Plan::new(vec![zone(&mut store, 0.5)], 8);
    let (sender, moves) = std::sync::mpsc::channel();
    plan.scripts.push(ScriptPlan {
        program: Program::default(),
        callbacks: Callbacks::default(),
        coroutines: 4,
        data: vec![],
        text_data: vec![],
        source_folder: None,
        library_folder: None,
        rc_folder: None,
        factory_folder: None,
        restore: vec![],
        restore_texts: vec![],
        controls: Controls::default(),
        engine_pars: EnginePars::default(),
        view: None,
        links: vec![],
        reverb_laws: ir::ReverbControls::from_fn(|_| ir::Law::Linear { lo: 0.0, hi: 1.0 }),
        reverb_networks: vec![],
        lfo_units: vec![],
        warp_modes: vec![],
        mod_kinds: vec![],
        knobs: vec![],
        foreign: Some(Arc::new(Knobbed(sender))),
        files: None,
        arrays: Vec::new(),
        hears_own_releases: false,
        held_release_ends_children: false,
    });
    let blend = |cc: u8, amount: f32| ir::Blend {
        base: 0.0,
        terms: vec![ir::Term {
            input: ir::Transfer {
                source: ir::Source::Controller(cc),
                ..Default::default()
            },
            amount,
            factors: vec![],
        }],
    };
    // Controllers 1 and 2 add onto knob 7; 3 scales knob 7; 4 moves knob 8.
    plan.knob_blends = vec![blend(1, 1.0), blend(2, 1.0), blend(3, 1.0), blend(4, 1.0)];
    let knob = |id, source, scale| KnobModPlan {
        slot: 0,
        id,
        source: ir::Source::Blend(source),
        scale,
    };
    plan.knob_mods = vec![
        knob(7, 0, false),
        knob(7, 1, false),
        knob(7, 2, true),
        knob(8, 3, false),
    ];
    let mut engine = Engine::new(Arc::new(plan), store.client(), RATE);
    let mut all: Vec<(i32, f64, f64)> = Vec::new();
    let at = |all: &mut Vec<_>| {
        all.extend(moves.try_iter());
        all.len()
    };
    // Where the routes stand at load moves nothing: the knobs' saved
    // values already hold them.
    level(&mut engine, &[]);
    assert_eq!(at(&mut all), 0, "the first reading moves no knob");
    level(&mut engine, &[]);
    assert_eq!(at(&mut all), 0, "nothing moved");
    // Two additive controllers on one knob: one move, their sum.
    level(&mut engine, &[cc(1, 127), cc(2, 127)]);
    assert_eq!(at(&mut all), 1);
    let (id, scale, add) = all[0];
    assert_eq!(id, 7);
    assert!(
        (add - 2.0).abs() < 1e-3 && scale.abs() < 1e-3,
        "{scale} {add}"
    );
    // The multiplicative controller moves the same knob's scale alone.
    level(&mut engine, &[cc(3, 127)]);
    assert_eq!(at(&mut all), 2);
    let (id, scale, add) = all[1];
    assert_eq!(id, 7);
    assert!(
        (scale - 1.0).abs() < 1e-3 && (add - 2.0).abs() < 1e-3,
        "{scale} {add}"
    );
}
