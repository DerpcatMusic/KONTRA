//! Rack boundaries are checked against chronological, script-free part inputs.
use super::{Rack, NO_AUX};
use crate::{
    audio::Sample,
    engine::{
        script::{Command, Kind as CommandKind},
        Bank, Engine, EventChange, HostExpression, HostNote, HostPattern, NoteEvent,
    },
    fx::{
        params::{Convolution, Impulse, IrBand, Params, SendLevels},
        Bus, Chain, Effect, Kind, ProgramFx,
    },
    import::{Group, Zone},
    ksp::{LogEngine, Runtime},
};
use std::{path::PathBuf, sync::Arc};

fn create(part: usize, script: Option<&str>, block: usize) -> Engine {
    let tap = Effect {
        slot: 0,
        kind: Kind::SendLevels,
        version: 0,
        bypass: false,
        output_gain: 1.,
        dry_level: 0.,
        params: Params::SendLevels(SendLevels {
            sends: vec![0.25; 8],
            outputs: vec![1.; 17],
        }),
    };
    let groups = vec![
        Group {
            amp_split_slot: Some(6),
            native_volume_env: true,
            volume_env: Some(crate::import::Ahdsr {
                attack_curve: 1.,
                attack_ms: 0.,
                hold_ms: 0.,
                decay_ms: 0.,
                sustain: 1.,
                release_ms: 1.,
                unknown_flag: 0,
                unknown_tail: Vec::new()
            }),
            fx: Chain { slots: vec![tap] },
            ..Default::default()
        };
        2
    ];
    let mut zones = Vec::new();
    let mut samples = Vec::new();
    for i in 0..10 {
        let key = 60 + i;
        let path = PathBuf::from(format!("rack-{part}-{key}"));
        zones.push(Zone {
            root: key,
            low_key: key,
            high_key: key,
            group: usize::from(i % 2),
            sample: path.clone(),
            ..Default::default()
        });
        samples.push((
            path,
            Sample {
                rate: 48000,
                frames: (0..4096)
                    .map(|f| {
                        let x = ((f * (3 + part) + usize::from(i) * 11) % 73) as f32 / 73.;
                        [0.013 + 0.017 * x, -0.019 + 0.009 * x]
                    })
                    .collect(),
            },
        ));
    }
    let bank = Bank::from_samples(groups.clone(), zones, samples)
        .unwrap()
        .with_prepared_settings(|s| {
            s[0].bus = Some(0);
            s[1].bus = Some(1);
        })
        .unwrap();
    let band = IrBand {
        length_ratio: 1.,
        low_cut_hz: 20.,
        high_cut_hz: 14400.,
    };
    let mut impulse = vec![[0.; 2]; 34];
    impulse[0] = [0.5; 2];
    impulse[33] = [0.125; 2];
    let echo = Effect {
        slot: 0,
        kind: Kind::Convolution,
        version: 0,
        bypass: false,
        output_gain: 1.,
        dry_level: 0.,
        params: Params::Convolution(Box::new(Convolution {
            unknown: [0.; 2],
            predelay_ms: 0.,
            early: band,
            late: band,
            unknown_9: 0.,
            flags: [false; 5],
            curve_x: Vec::new(),
            curve_db: Vec::new(),
            ir_index: 0,
            ir_file: None,
            ir_error: None,
            ir: Some(Impulse(Arc::new(Sample {
                rate: 48000,
                frames: impulse,
            }))),
        })),
    };
    let fx = ProgramFx {
        send: Chain { slots: vec![echo] },
        buses: vec![
            Bus {
                index: 0,
                name: String::new(),
                volume: 0.7,
                pan: -0.25,
                output: 2,
                chain: Chain::default(),
            },
            Bus {
                index: 1,
                name: String::new(),
                volume: 0.8,
                pan: 0.2,
                output: -1,
                chain: Chain::default(),
            },
        ],
        ..Default::default()
    };
    let mut e = Engine::default();
    e.reset(48000.);
    e.attack = 0.;
    e.set_lanes(false);
    e.set_bank(Some(Box::new(bank)));
    e.set_fx(fx.processor_for_groups(48000., block, &[], &groups));
    if let Some(script) = script {
        let (rt, errors) =
            Runtime::with_scripts(&[script], &mut LogEngine::default(), 0, Vec::new());
        assert!(errors.iter().all(Option::is_none), "{errors:?}");
        e.set_script(Some(Box::new(rt)));
    }
    e
}
fn owner(part: usize, i: usize) -> HostNote {
    HostNote {
        port: part as u8,
        channel: 4,
        key: 60 + i as u8,
        id: 100 + part as i32 * 10 + i as i32,
        clap: true,
    }
}
fn off(e: &mut Engine, n: HostNote) {
    e.host_note_off(HostPattern {
        port: i32::from(n.port),
        channel: 4,
        key: i32::from(n.key),
        id: n.id,
        clap: true,
    });
}
fn script(own: usize) -> String {
    format!(
        r#"on init
declare $first
end on
on note
if ($EVENT_NOTE=60)
$first := $EVENT_ID
end if
end on
on controller
if ($CC_NUM=1)
ignore_controller
wait(3000)
message($PLAYED_VOICES_INST*100+$PLAYED_VOICES_TOTAL)
if ($PLAYED_VOICES_INST={own} and $PLAYED_VOICES_TOTAL=5)
change_vol($first,-6021,0)
end if
end if
end on"#
    )
}
fn perform(parts: &[usize]) -> Vec<[[f32; 2]; 4]> {
    const FRAMES: usize = 1408;
    let mut rack = Box::new(Rack::with_slots(2));
    let scripts = [script(3), script(2)];
    let mut reference = [create(0, None, 64), create(1, None, 127)];
    for p in 0..2 {
        rack.parts[p] = create(p, Some(&scripts[p]), if p == 0 { 64 } else { 127 }).into();
        rack.controls[p].output = p as u8;
        rack.controls[p].aux = 3;
        rack.controls[p].aux_gain = 0.3;
        rack.controls[p].gain = if p == 0 { 0.6 } else { 0.8 };
        rack.controls[p].pan = if p == 0 { -0.2 } else { 0.3 };
        rack.controls[p].outs.fill(NO_AUX);
        rack.controls[p].outs[2] = 2;
    }
    rack.bus_controls[2].gain = 0.4;
    rack.bus_controls[2].pan = -0.4;
    let mut ids = [[None; 10]; 2];
    let mut actual = vec![[[0.; 2]; 4]; FRAMES];
    let mut expected = actual.clone();
    let mut dry = [[[0.; 128]; 2]; 2];
    let mut direct = dry;
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            for p in 0..2 {
                for i in 0..10 {
                    assert!(rack
                        .with_instrument(p, |e| e.host_note_on(owner(p, i), 0, 60 + i as u8, 127, 0.))
                        .unwrap());
                    ids[p][i] = reference[p].start_event(&NoteEvent {
                        input_channel: Some(4),
                        ..NoteEvent::new(0, 60 + i as u8, 127)
                    });
                }
                rack.with_instrument(p, |e| e.cc_from(0, 4, 1, 127));
            }
            let mut at = 0;
            let mut fragment = 0;
            while at < FRAMES {
                // Literal exterior event times, not Rack's chosen global boundaries.
                match at {
                    64 => {
                        for p in 0..2 {
                            for i in (if p == 0 { 3 } else { 2 })..10 {
                                rack.with_instrument(p, |e| off(e, owner(p, i)));
                                reference[p].release_event(ids[p][i].unwrap());
                            }
                        }
                    }
                    221 => {
                        for p in 0..2 {
                            let note = owner(p, 0);
                            rack.with_instrument(p, |e| {
                                e.host_expression(
                                    HostPattern {
                                        port: p as i32,
                                        channel: 4,
                                        key: 60,
                                        id: note.id,
                                        clap: true,
                                    },
                                    HostExpression::Tune(1.25),
                                )
                            });
                            reference[p].change_event(ids[p][0].unwrap(), EventChange::Tune(1.25));
                        }
                    }
                    384 => {
                        for p in 0..2 {
                            rack.with_instrument(p, |e| e.cc_from(0, 4, 64, 127));
                            reference[p].cc_from(0, 4, 64, 127);
                        }
                    }
                    400 => {
                        for p in 0..2 {
                            rack.with_instrument(p, |e| off(e, owner(p, 0)));
                            reference[p].release_event(ids[p][0].unwrap());
                        }
                    }
                    640 => {
                        for p in 0..2 {
                            rack.with_instrument(p, |e| e.cc_from(0, 4, 64, 0));
                            reference[p].cc_from(0, 4, 64, 0);
                        }
                    }
                    800 => {
                        for p in 0..2 {
                            rack.with_instrument(p, |e| e.cc_from(0, 4, 120, 0));
                            reference[p].cc_from(0, 4, 120, 0);
                        }
                    }
                    _ => {}
                }
                let next = [64, 192, 221, 384, 400, 640, 705, 800, FRAMES]
                    .into_iter()
                    .find(|n| *n > at)
                    .unwrap();
                let n = parts[fragment % parts.len()].min(next - at).min(128);
                fragment += 1;
                for p in 0..2 {
                    rack.with_part(p, |e| e.begin_audio_block(n, 1, false));
                    reference[p].begin_audio_block(n, 1, false);
                    if at <= 144 && at + n > 144 {
                        let c = Command {
                            at: (144 - at) as u32,
                            channel: 0,
                            input_channel: None,
                            id: ids[p][0].unwrap(),
                            kind: CommandKind::Change(EventChange::Volume(
                                10f32.powf(-6021. / 20000.),
                            )),
                            applied: false,
                        };
                        let index = reference[p].commands.len();
                        reference[p].commands.insert(index, c);
                    }
                    direct[p].iter_mut().for_each(|ch| ch.fill(0.));
                    // Run each standalone part's original render phases, retaining
                    // direct outputs before its next local FX block clears them.
                    // There is no shared/global minimum boundary in this oracle.
                    reference[p].render_begin(n);
                    let mut local = 0;
                    loop {
                        reference[p].render_boundary(local);
                        if local == n {
                            break;
                        }
                        let end = reference[p].render_next_boundary(local);
                        let offset = reference[p].render_output_offset(local);
                        let [l, r] = &mut dry[p];
                        reference[p].render_interval(local, &mut l[local..end], &mut r[local..end]);
                        for (_, l, r) in reference[p]
                            .fx()
                            .direct_outs_range(offset..offset + end - local)
                        {
                            for f in 0..end - local {
                                direct[p][0][local + f] += l[f];
                                direct[p][1][local + f] += r[f];
                            }
                        }
                        local = end;
                    }
                    reference[p].render_finish();
                }
                let (bus, live) = rack.render_live(n);
                for f in 0..n {
                    for b in 0..4 {
                        actual[at + f][b] = if live[b] {
                            [bus[b][0][f], bus[b][1][f]]
                        } else {
                            [0.; 2]
                        };
                    }
                }
                // Independent exterior routing: each part's own main and direct
                // outputs are rendered over the original block, then literal faders.
                for p in 0..2 {
                    let (gain, pan) = if p == 0 { (0.6, -0.2) } else { (0.8, 0.3) };
                    let gains = [gain * (1f32 - pan).min(1.), gain * (1f32 + pan).min(1.)];
                    for f in 0..n {
                        for ch in 0..2 {
                            let x = dry[p][ch][f] * gains[ch];
                            expected[at + f][p][ch] += x;
                            expected[at + f][3][ch] += x * 0.3;
                        }
                    }
                    for f in 0..n {
                        expected[at + f][2][0] += direct[p][0][f] * gains[0] * 0.4;
                        expected[at + f][2][1] += direct[p][1][f] * gains[1] * 0.4 * 0.6;
                    }
                }
                at += n;
                if at == 192 {
                    assert_eq!(rack.parts[0].active_voices(), 3);
                    assert_eq!(rack.parts[1].active_voices(), 2);
                    // The caller accepts NOTE_END and retires completed exact
                    // tuples before later controls, as the plugin consumer does.
                    for p in 0..2 {
                        rack.parts[p].mark_host_notes();
                        for i in (if p == 0 { 3 } else { 2 })..10 {
                            assert_eq!(rack.parts[p].host_note_end_at(owner(p, i)), Some(128));
                            rack.parts[p].retire_host_note(owner(p, i));
                        }
                    }
                }
                if at == 705 {
                    // Rendering through704 consumes the first permanently
                    // silent frame and discovers the literal terminal704.
                    // A half-open interval ending704 cannot observe it yet.
                    for p in 0..2 {
                        rack.parts[p].mark_host_notes();
                        assert_eq!(rack.parts[p].host_note_end_at(owner(p, 0)), Some(704));
                        rack.parts[p].retire_host_note(owner(p, 0));
                    }
                }
                if at == 384 {
                    for p in 0..2 {
                        assert_eq!(
                            rack.parts[p].instrument().unwrap().script().unwrap().last_message(),
                            if p == 0 { "305" } else { "205" }
                        );
                    }
                }
            }
            for p in 0..2 {
                rack.parts[p].mark_host_notes();
                for i in (if p == 0 { 3 } else { 2 })..10 {
                    assert!(!rack.parts[p].host_note_present(owner(p, i)));
                }
                assert!(!rack.parts[p].host_note_present(owner(p, 0)));
                assert_eq!(rack.parts[p].active_voices(), 0);
                assert_eq!(rack.parts[p].dropped_commands(), 0);
                assert!(rack.parts[p].instrument().unwrap().script().unwrap().diagnostics().is_empty());
            }
        }),
        0
    );
    for (f, (a, b)) in actual.iter().zip(&expected).enumerate() {
        for out in 0..4 {
            for ch in 0..2 {
                assert!(
                    (a[out][ch] - b[out][ch]).abs() < 8e-6,
                    "frame{f} out{out} ch{ch}: {} vs {}",
                    a[out][ch],
                    b[out][ch]
                );
            }
        }
    }
    for out in 0..4 {
        for ch in 0..2 {
            let peak = expected
                .iter()
                .map(|f| f[out][ch].abs())
                .fold(0f32, f32::max);
            assert!(
                (rack.peaks.buses[out][ch] - peak).abs() < 8e-6,
                "meter out{out} ch{ch}"
            );
        }
    }
    for out in 0..4 {
        assert!(actual[200..384].iter().any(|f| f[out][0].abs() > 1e-4));
    }
    // Compatibility's physical stop at800 has a240-frame forced fade:
    // final raw input1039. The34-frame IR's14400Hz low-pass adds20
    // frames (pole radius0.44251069), so its54 taps end mathematically1093.
    // The largest FFT block128 can retain rounding in the1024..1152
    // input block's1152..1280 overlap. The following block is exactly empty.
    let (late_frame, late_peak) = actual[1093..1280]
        .iter()
        .enumerate()
        .map(|(i, frame)| {
            (
                1093 + i,
                frame
                    .iter()
                    .flatten()
                    .copied()
                    .map(f32::abs)
                    .fold(0., f32::max),
            )
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    eprintln!("Rack drained FIR late peak {late_peak:e} at frame{late_frame}, partition{parts:?}");
    assert!(
        late_peak < 1e-6,
        "FIR drain exceeded bounded FFT roundoff: {late_peak:e} at{late_frame}"
    );
    assert!(actual[1280..]
        .iter()
        .all(|f| f.iter().flatten().all(|x| *x == 0.)));
    actual
}
#[test]
fn rack_live_census_twenty_to_five_pedals_stop_and_distinct_routed_fx_follow_chronological_pcm_without_heap(
) {
    let a = perform(&[128]);
    let b = perform(&[1, 7, 31, 127]);
    for (a, b) in a.iter().zip(b) {
        for (a, b) in a.iter().flatten().zip(b.iter().flatten()) {
            assert!((a - b).abs() < 8e-6);
        }
    }
}

fn growth(parts: &[usize]) -> [Vec<f32>; 2] {
    const ORIGIN: usize = 333;
    const FRAMES: usize = 4608;
    let script = r#"on init
declare $tick
set_listener($NI_SIGNAL_TIMER_MS,1000)
end on
on listener
$tick := 1
change_listener_par($NI_SIGNAL_TIMER_MS,0)
end on
on note
if ($EVENT_NOTE=61)
ignore_event($EVENT_ID)
end if
wait(1000)
message($tick+10)
if ($EVENT_NOTE=61)
play_note(61,127,0,0)
end if
end on"#;
    let mut rack = Box::new(Rack::with_slots(1));
    let mut prepared = Box::new(Rack::with_slots(2));
    prepared.parts[1] = create(1, Some(script), 127).into();
    prepared.parts[1].instrument_mut().unwrap().player.release_counter.reset(0, 60, 48000.);
    assert_eq!(prepared.parts[1].instrument().unwrap().player.release_counter.factor(60, 100), 1.);
    let mut reference = create(1, None, 127);
    let mut pcm = std::array::from_fn(|_| vec![0.; FRAMES]);
    let mut expected = pcm.clone();
    let mut silence = [[0.; 128]; 2];
    // A previously running Rack and an independent chronological Engine have
    // already consumed333 frames before the worker-prepared suffix is adopted.
    let mut at = 0;
    while at < ORIGIN {
        let n = 128.min(ORIGIN - at);
        rack.render(n);
        let [l, r] = &mut silence;
        reference.render(&mut l[..n], &mut r[..n]);
        at += n;
    }
    let a = owner(1, 0);
    let b = owner(1, 1);
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            rack.adopt_parts(&mut prepared);
            assert_eq!(rack.parts[0].instrument().unwrap().player.now, 333);
            assert_eq!(rack.parts[1].instrument().unwrap().player.now, 333);
            assert_eq!(
                rack.parts[1].instrument().unwrap().player.release_counter.factor(60, 100),
                1.,
                "adoption does not age a freshly seeded key by past Rack frames"
            );
            assert!(rack
                .with_instrument(1, |e| e.host_note_on(a, 0, 60, 127, 0.))
                .unwrap());
            assert!(rack
                .with_instrument(1, |e| e.host_note_on(b, 0, 61, 127, 0.))
                .unwrap());
            let id = reference
                .start_event(&NoteEvent {
                    input_channel: Some(4),
                    ..NoteEvent::new(0, 60, 127)
                })
                .unwrap();
            let mut fragment = 0;
            while at < FRAMES {
                if at == 349 {
                    let expected = (100. - (1f32 / 3.)) / 100.;
                    assert!(
                        (rack.parts[1].instrument().unwrap().player.release_counter.factor(60, 100) - expected).abs()
                            < 1e-6,
                        "only sixteen newly rendered frames aged the seeded key"
                    );
                    rack.with_instrument(1, |e| {
                        off(e, a);
                        off(e, b);
                    });
                    reference.release_event(id);
                }
                if at == 381 {
                    reference
                        .start_event(&NoteEvent {
                            input_channel: Some(4),
                            ..NoteEvent::new(0, 61, 127)
                        })
                        .unwrap();
                }
                let end = [349, 381, 382, 429, 4477, FRAMES]
                    .into_iter()
                    .find(|end| *end > at)
                    .unwrap();
                let n = parts[fragment % parts.len()].min(end - at).min(128);
                fragment += 1;
                // This fixture keeps direct output on the default main bus; the
                // growth proof does not depend on the routing oracle above.
                // Test sample chronology, not an 8us wall-clock deadline in a
                // debug interpreter. Offline keeps instruction work bounded.
                rack.with_part(1, |e| e.begin_audio_block(n, 1, true));
                reference.begin_audio_block(n, 1, true);
                let bus = rack.render(n);
                let [l, r] = &mut pcm;
                l[at..at + n].copy_from_slice(&bus[0][0][..n]);
                r[at..at + n].copy_from_slice(&bus[0][1][..n]);
                // Local block127 could hide a previous direct range in128 frames,
                // so retain every range on the independent part before its reset.
                reference.render_begin(n);
                let mut local = 0;
                loop {
                    reference.render_boundary(local);
                    if local == n {
                        break;
                    }
                    let stop = reference.render_next_boundary(local);
                    let offset = reference.render_output_offset(local);
                    let [l, r] = &mut expected;
                    reference.render_interval(
                        local,
                        &mut l[at + local..at + stop],
                        &mut r[at + local..at + stop],
                    );
                    for (_, dl, dr) in reference
                        .fx()
                        .direct_outs_range(offset..offset + stop - local)
                    {
                        for f in 0..stop - local {
                            l[at + local + f] += dl[f];
                            r[at + local + f] += dr[f];
                        }
                    }
                    local = stop;
                }
                reference.render_finish();
                at += n;
                if at == 381 {
                    assert_eq!(rack.parts[1].instrument().unwrap().script().unwrap().last_message(), "");
                }
                if at == 382 {
                    assert_eq!(rack.parts[1].instrument().unwrap().script().unwrap().last_message(), "11");
                    assert_eq!(rack.parts[1].host_note_end_at(b), None, "generated child retains inherited exact ownership after callback completion");
                }
            }
            rack.parts[1].mark_host_notes();
            assert_eq!(rack.parts[1].host_note_end_at(a), Some(429));
            assert_eq!(
                rack.parts[1].host_note_end_at(b),
                Some(4477),
                "generated Sample inherits the exact HostRef and extends its end to381+4096"
            );
            assert_eq!(rack.parts[0].instrument().unwrap().player.now, FRAMES as u64);
            assert_eq!(rack.parts[1].instrument().unwrap().player.now, FRAMES as u64);
            assert_eq!(rack.parts[1].active_voices(), 0);
            let diagnostics = rack.parts[1].instrument().unwrap().script().unwrap().diagnostics();
            assert!(diagnostics.is_empty(), "rack adoption diagnostics: {diagnostics:?}");
        }),
        0
    );
    for ch in 0..2 {
        for f in 0..FRAMES {
            assert!(
                (pcm[ch][f] - expected[ch][f]).abs() < 8e-6,
                "growth ch{ch} frame{f}"
            );
        }
    }
    assert!(pcm[0][382..429].iter().any(|x| x.abs() > 1e-4));
    // FFT-convolution roundoff can remain after the last mathematical FIR tap.
    let late_peak = pcm.iter().flat_map(|ch| &ch[4550..]).map(|x| x.abs()).fold(0_f32, f32::max);
    assert!(late_peak < 1e-8, "drained FIR peak {late_peak}");
    pcm
}
#[test]
fn rack_adoption_preserves_global333_origin_and_local_preinstalled_listener_wait_source_and_host_ends_without_heap(
) {
    let a = growth(&[128]);
    let b = growth(&[1, 7, 31, 127]);
    for ch in 0..2 {
        for (frame, (a, b)) in a[ch].iter().zip(&b[ch]).enumerate() {
            assert!((a - b).abs() < 8e-6, "partition ch{ch} frame{frame}: fixed {a}, fragmented {b}");
        }
    }
}
