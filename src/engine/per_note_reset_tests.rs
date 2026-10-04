use crate::{
    audio::Sample,
    engine::{Bank, Engine, HostExpression, HostNote, HostPattern, InputAddress},
    import::{Group, Loop, Zone},
    midi::{Bend, Control, ControlValue, NoteControl, Velocity},
    modulation::{ModAssignment, ModSource, ModTarget},
};

fn engine(pressure: bool, scripted: bool) -> Engine {
    let mods = if pressure {
        vec![ModAssignment {
            name: "PRESSURE_VOLUME".into(), source: ModSource::MonoAftertouch,
            target: ModTarget::Volume, intensity: 1., invert: false, lag_ms: 0,
            shaper: None, cc_default: None,
        }]
    } else { Vec::new() };
    let bank = Bank::from_samples(
        vec![Group { mods, ..Group::default() }],
        vec![Zone { root: 48, loop_range: Some(Loop {
            start: 0, end: 8000, crossfade: 0, until_release: false, alternating: false,
        }), ..Zone::default() }],
        vec![(Default::default(), Sample { rate: 48000, frames: (0..8000).map(|i| {
            let phase = std::f64::consts::TAU * i as f64 / 200.;
            [(0.03 * phase.sin() + 0.007 * (2. * phase).sin()) as f32; 2]
        }).collect() })],
    ).unwrap();
    let mut e = Engine::default();
    e.reset(48000.); e.attack = 0.0001; e.release = 0.05;
    e.set_home_channel(7); e.set_bank(Some(Box::new(bank)));
    if scripted {
        let (rt, errors) = crate::ksp::Runtime::with_scripts(&[
            "on poly_at\nif (%POLY_AT[$POLY_AT_NUM]=95)\nmessage(\"pressure waiting\")\nwait(1000)\nmessage(\"stale pressure resumed\")\nend if\nend on",
        ], &mut crate::ksp::LogEngine::default(), 0, Vec::new());
        assert!(errors.iter().all(Option::is_none));
        e.set_script(Some(Box::new(rt)));
    }
    e
}

fn note(id: i32, channel: u8) -> HostNote {
    HostNote { port: 2, channel, key: 60, id, clap: true }
}
fn pattern(n: HostNote) -> HostPattern {
    HostPattern { port: i32::from(n.port), channel: i32::from(n.channel),
        key: i32::from(n.key), id: n.id, clap: n.clap }
}
fn address(n: HostNote) -> InputAddress {
    InputAddress { port: n.port, channel: n.channel, key: n.key }
}
fn start(e: &mut Engine, n: HostNote, pan: f32) {
    assert!(e.host_note_on(n, 7, 48, 127, 0.));
    e.host_expression(pattern(n), HostExpression::Pan(pan));
}
fn gain(e: &mut Engine, n: HostNote, value: f32) {
    e.host_expression(pattern(n), HostExpression::Gain(value));
}
fn pressure(e: &mut Engine, n: HostNote, value: f64) {
    assert!(e.per_note_pressure_from(7, 48, address(n), ControlValue::new(value).unwrap()));
}
fn controls(e: &mut Engine, n: HostNote, volume: u32, bend: f64) {
    assert!(e.note_controller_from(7, 48, address(n),
        NoteControl::Registered { index: 7, value: volume }));
    assert!(e.note_controller_from(7, 48, address(n), NoteControl::Pitch(Bend::new(bend).unwrap())));
}
fn render(e: &mut Engine, pcm: &mut Vec<[f32; 2]>, frames: usize, partition: &[usize]) {
    let mut remaining = frames;
    let mut block = 0;
    while remaining != 0 {
        let len = remaining.min(partition[block % partition.len()]);
        let (mut l, mut r) = ([0.; 128], [0.; 128]);
        e.render_checked(&mut l[..len], &mut r[..len]).unwrap();
        pcm.extend(l[..len].iter().zip(&r[..len]).map(|(&l, &r)| [l, r]));
        remaining -= len; block += 1;
    }
}
fn same(a: &[[f32; 2]], b: &[[f32; 2]]) {
    assert_eq!(a.len(), b.len());
    assert!(a.iter().zip(b).all(|(a,b)| (0..2).all(|ch| (a[ch]-b[ch]).abs()<2e-5)),
        "CC121 changed detached pressure, physical scope, or retained registered gain/bend PCM");
}
fn step(a: &mut Engine, b: &mut Engine, p: &mut Vec<[f32;2]>, q: &mut Vec<[f32;2]>, frames: usize, partition: &[usize]) {
    render(a,p,frames,partition); render(b,q,frames,partition); same(p,q);
}

fn timeline(scripted: bool, partition: &[usize]) -> (Vec<[f32;2]>, [u64;4]) {
    let (mut actual, mut oracle) = (engine(true,scripted),engine(false,false));
    let (mut p, mut q) = (Vec::with_capacity(8192),Vec::with_capacity(8192));
    let (old, overlay, current, other) = (note(710,4),note(711,4),note(712,4),note(713,5));
    let mut ends = [0;4];
    assert_eq!(crate::plugin::tests::allocations(|| {
        pressure(&mut actual,old,0.25); controls(&mut actual,old,u32::MAX/5,0.125);
        pressure(&mut actual,other,0.5);
        for e in [&mut actual,&mut oracle] {
            start(e,old,-1.); start(e,overlay,1.); start(e,other,-1.);
        }
        actual.host_pressure(pattern(overlay),ControlValue::new(0.6).unwrap());
        gain(&mut oracle,old,0.25*0.8); gain(&mut oracle,overlay,0.6*0.8); gain(&mut oracle,other,0.5);
        for n in [old,overlay] { oracle.host_expression(pattern(n),HostExpression::Tune(6.)); }
        step(&mut actual,&mut oracle,&mut p,&mut q,256,partition);

        // Both old exact roots bind the frozen generation; the reused PCK
        // has its own shared native pressure, registered gain and bend.
        assert!(actual.per_note_manage(address(old),true,false));
        pressure(&mut actual,current,0.75); controls(&mut actual,current,u32::MAX/8,0.0625);
        for e in [&mut actual,&mut oracle] { start(e,current,1.); }
        gain(&mut oracle,current,0.75*0.5);
        oracle.host_expression(pattern(current),HostExpression::Tune(3.));
        if let Some(rt)=actual.script() {
            assert_eq!(rt.last_message(),"pressure waiting","the pressure callback must enter its wait before reset");
        }
        actual.control_from_received_on_port(7,4,crate::midi::ReceivedControl::without_parameter(Control::Cc(121,ControlValue::from_midi1(93))),2);
        gain(&mut oracle,current,0.);
        assert_eq!(actual.player.library_receiver.cc[7][121],93,"raw received mode is independent of native pressure reset");
        step(&mut actual,&mut oracle,&mut p,&mut q,512,partition);
        if let Some(rt)=actual.script() {
            assert_ne!(rt.last_message(),"stale pressure resumed","reset failed to fence physical pressure continuation");
        }

        // New pressure still addresses only the current generation. The gain
        // and bend that CC121 must preserve become audible again here.
        pressure(&mut actual,current,0.875); gain(&mut oracle,current,0.875*0.5);
        step(&mut actual,&mut oracle,&mut p,&mut q,768,partition);
        actual.host_pressure(pattern(overlay),ControlValue::new(0.4).unwrap());
        gain(&mut oracle,overlay,0.4*0.8);
        step(&mut actual,&mut oracle,&mut p,&mut q,512,partition);

        for e in [&mut actual,&mut oracle] {
            for n in [old,overlay,current,other] {
                e.host_note_off_velocity(pattern(n),Velocity::from_midi2(32768));
            }
        }
        step(&mut actual,&mut oracle,&mut p,&mut q,4096,partition);
        for e in [&mut actual,&mut oracle] { e.mark_host_notes(); }
        assert!(p[p.len()-128..].iter().all(|s|s.iter().all(|v|v.abs()<1e-8)));
        for (i,n) in [old,overlay,current,other].into_iter().enumerate() {
            assert!(!actual.host_note_pending(n) && !oracle.host_note_pending(n));
            ends[i]=actual.host_note_end_at(n).unwrap();
            // Physical release is at 2048. The authored 50 ms ordinary
            // -60 dB coefficient crosses its relative -80 dB floor after
            // 3201 frames (including the stopping frame, with f32 rounding).
            assert_eq!(ends[i],5249,"authored release owns the absolute terminal sample");
            assert_eq!(Some(ends[i]),oracle.host_note_end_at(n));
            for e in [&mut actual,&mut oracle] { e.retire_host_note(n); }
        }
        assert_eq!(actual.dropped_commands(),0);
        assert_eq!(oracle.dropped_commands(),0);
        if let Some(rt)=actual.script() { assert!(rt.diagnostics().is_empty()); }
    }),0);
    (p,ends)
}

#[test]
fn cc121_resets_current_native_pressure_and_keeps_detached_generations_in_complete_pcm() {
    for scripted in [false,true] {
        let (fixed,ends)=timeline(scripted,&[128]);
        let (fragmented,fragmented_ends)=timeline(scripted,&[1,7,31,127,13,64]);
        same(&fixed,&fragmented); assert_eq!(ends,fragmented_ends);
    }
}
