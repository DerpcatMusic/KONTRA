//! Saved CC availability must change audible routes, never infer receipt from a cache value.
use crate::{
    audio::Sample,
    engine::{Bank, Engine, HostExpression, HostNote, HostPattern},
    import::{Group, Loop, Zone},
    midi::{Control, ControlValue},
    modulation::{ModAssignment, ModSource, ModTarget, ShaperCurve},
};

fn assignment(
    cc: u8,
    target: ModTarget,
    default: Option<i16>,
    depth: f32,
    shape: Option<ShaperCurve>,
) -> ModAssignment {
    ModAssignment {
        name: format!("CC{cc}"),
        source: ModSource::MidiCc(cc),
        cc_default: default,
        target,
        intensity: depth,
        invert: false,
        lag_ms: 0,
        shaper: shape,
    }
}

fn imported_volume(default: i16, shape: Option<ShaperCurve>) -> ModAssignment {
    use ni_file::kontakt::{Chunk, StructuredObject, objects::Group as RawGroup};
    fn name(bytes: &mut Vec<u8>, text: &str) {
        bytes.extend((text.len() as u32).to_le_bytes());
        bytes.extend(text.as_bytes());
    }
    fn object(id: u16, version: u16, private: &[u8], public: &[u8]) -> Chunk {
        let mut data = vec![1];
        data.extend(version.to_le_bytes());
        for bytes in [private, public, &[]] {
            data.extend((bytes.len() as u32).to_le_bytes());
            data.extend(bytes);
        }
        Chunk { id, data }
    }
    let mut private = 1u32.to_le_bytes().to_vec();
    name(&mut private, "volume");
    private.extend(1f32.to_le_bytes());
    private.extend((-1i16).to_le_bytes());
    private.push(0x10);
    private.extend(0u16.to_le_bytes());
    name(&mut private, "<none>");
    private.push(0);
    private.push(0);
    name(&mut private, "SavedVolume");
    private.extend(1u32.to_le_bytes());
    private.extend(4u32.to_le_bytes());
    private.push(1);
    private.extend(default.to_le_bytes());
    private.extend([0xa5, 0x5a]);
    private.extend(0u32.to_le_bytes());
    let mut slots = vec![1];
    object(0x0c, 0x102, &private, &[])
        .write(&mut slots)
        .unwrap();
    slots.extend([0; 31]);
    let raw = RawGroup(StructuredObject {
        version: 0x95,
        public_data: vec![],
        private_data: vec![],
        children: vec![object(0x3c, 0x10, &[], &slots)],
    });
    let mut route = crate::modulation::read_group(&raw).unwrap().mods.remove(0);
    route.shaper = shape;
    route
}

fn bank(mods: Vec<ModAssignment>, filtered: bool) -> Bank {
    let mut group = Group {
        mods,
        ..Default::default()
    };
    if filtered {
        group.fx = crate::fx::Chain {
            slots: vec![crate::fx::Effect {
                slot: 0,
                kind: crate::fx::Kind::Filter,
                version: 0x92,
                bypass: false,
                output_gain: 1.,
                dry_level: 1.,
                params: crate::fx::params::Params::Filter(crate::fx::params::Filter {
                    filter_type: 33,
                    cutoff: 0.8,
                    resonance: 0.1,
                    extra: [0.; 3],
                    native_flag: Some(0),
                }),
            }],
        };
        group.amp_split_slot = Some(0);
    }
    Bank::from_samples(
        vec![group],
        vec![Zone {
            root: 60,
            start_mod: Some(997),
            loop_range: Some(Loop {
                start: 0,
                end: 8000,
                crossfade: 0,
                until_release: false,
                alternating: false,
            }),
            ..Default::default()
        }],
        vec![(
            Default::default(),
            Sample {
                rate: 48000,
                frames: (0..8000)
                    .map(|i| {
                        let x = std::f64::consts::TAU * i as f64 / 200.;
                        [(0.02 * x.sin() + 0.006 * (2. * x).sin()) as f32; 2]
                    })
                    .collect(),
            },
        )],
    )
    .unwrap()
}
fn engine(bank: Bank) -> Engine {
    let mut e = Engine::default();
    e.reset(48000.);
    e.attack = 0.0001;
    e.release = 0.05;
    e.set_home_channel(7);
    e.set_bank(Some(Box::new(bank)));
    e
}
fn host(id: i32, channel: u8) -> HostNote {
    HostNote {
        port: 0,
        channel,
        key: 60,
        id,
        clap: true,
    }
}
fn pattern(note: HostNote) -> HostPattern {
    HostPattern {
        port: i32::from(note.port),
        channel: i32::from(note.channel),
        key: i32::from(note.key),
        id: note.id,
        clap: note.clap,
    }
}
fn start(e: &mut Engine, note: HostNote, pan: f32) {
    assert!(e.host_note_on(note, 7, 60, 127, 0.));
    e.host_expression(pattern(note), HostExpression::Pan(pan));
}
fn received(e: &mut Engine, input: u8, control: Control) {
    assert_eq!(
        crate::plugin::tests::allocations(|| e.control_from_received(7, input, crate::midi::ReceivedControl::without_parameter(control))),
        0
    );
}

fn render(e: &mut Engine, pcm: &mut Vec<[f32; 2]>, frames: usize, partition: &[usize]) {
    let mut remaining = frames;
    let mut block = 0;
    while remaining > 0 {
        let len = remaining.min(partition[block % partition.len()]);
        let (mut l, mut r) = ([0.; 128], [0.; 128]);
        assert_eq!(
            crate::plugin::tests::allocations(|| e
                .render_checked(&mut l[..len], &mut r[..len])
                .unwrap()),
            0
        );
        pcm.extend(l[..len].iter().zip(&r[..len]).map(|(&l, &r)| [l, r]));
        remaining -= len;
        block += 1;
    }
}
fn same(a: &[[f32; 2]], b: &[[f32; 2]]) {
    assert_eq!(a.len(), b.len());
    assert!(
        a.iter()
            .zip(b)
            .all(|(a, b)| (0..2).all(|ch| (a[ch] - b[ch]).abs() < 2e-5)),
        "controller availability changed source PCM"
    );
}
fn step(
    a: &mut Engine,
    b: &mut Engine,
    pa: &mut Vec<[f32; 2]>,
    pb: &mut Vec<[f32; 2]>,
    frames: usize,
    partition: &[usize],
) {
    render(a, pa, frames, partition);
    render(b, pb, frames, partition);
    same(pa, pb);
}

fn timeline(partition: &[usize]) -> Vec<[f32; 2]> {
    let mods = vec![
        imported_volume(-1, Some(ShaperCurve::Table(vec![0.25, 0.75]))),
        assignment(
            2,
            ModTarget::Pitch,
            Some(-1),
            1.,
            Some(ShaperCurve::Table(vec![0.5, 0.5])),
        ),
        assignment(
            3,
            ModTarget::Module {
                param: "filterCutoff".into(),
                slot: 0,
            },
            Some(-1),
            0.5,
            Some(ShaperCurve::Table(vec![0.5, 0.5])),
        ),
        assignment(
            4,
            ModTarget::SampleStart,
            Some(-1),
            1.,
            Some(ShaperCurve::Table(vec![1., 1.])),
        ),
    ];
    let mut actual = engine(bank(mods.clone(), true));
    let mut reference = engine(bank(vec![], true));
    let (mut pcm, mut oracle) = (Vec::new(), Vec::new());
    let notes = [host(590, 4), host(591, 7)];
    // An ignored physical callback must not turn an unreceived zero into an accepted zero.
    let script = "on init\ndeclare $armed\nend on\non controller\nif ($CC_NUM=20)\n$armed:=1\nend if\nif ($CC_NUM=21)\nset_controller(1,127)\nend if\nif ($CC_NUM=1 and $armed=0)\nignore_controller\nend if\nend on";
    let (rt, errors) = crate::ksp::Runtime::with_scripts(
        &[script],
        &mut crate::ksp::LogEngine::new(Vec::new(), 48000.),
        1,
        Vec::new(),
    );
    assert!(errors.iter().all(Option::is_none));
    actual.set_script(Some(Box::new(rt)));
    for (note, pan) in notes.into_iter().zip([-1., 1.]) {
        start(&mut actual, note, pan);
        start(&mut reference, note, pan);
    }
    step(
        &mut actual,
        &mut reference,
        &mut pcm,
        &mut oracle,
        1536,
        partition,
    );
    assert!(
        pcm.iter().any(|v| v[0].abs() > 0.001 && v[1].abs() > 0.001),
        "inactive routes must leave source audible"
    );
    received(&mut actual, 4, Control::Cc(1, ControlValue::ZERO));
    step(
        &mut actual,
        &mut reference,
        &mut pcm,
        &mut oracle,
        768,
        partition,
    );
    received(&mut actual, 4, Control::Cc(20, ControlValue::FULL));
    received(&mut actual, 4, Control::Cc(1, ControlValue::ZERO));
    reference.host_expression(pattern(notes[0]), HostExpression::Gain(0.25));
    step(
        &mut actual,
        &mut reference,
        &mut pcm,
        &mut oracle,
        1024,
        partition,
    );
    received(&mut actual, 7, Control::Cc(1, ControlValue::FULL));
    reference.host_expression(pattern(notes[1]), HostExpression::Gain(0.75));
    received(&mut actual, 4, Control::Cc(2, ControlValue::ZERO));
    reference.host_expression(pattern(notes[0]), HostExpression::Tune(6.));
    step(
        &mut actual,
        &mut reference,
        &mut pcm,
        &mut oracle,
        1536,
        partition,
    );
    // Canonical14 admission activates coarse CC1 even when delivered through rawCC33.
    received(
        &mut actual,
        4,
        Control::Cc14 {
            cc: 33,
            raw: 1,
            coarse: 1,
            value: ControlValue::from_midi14(8193),
        },
    );
    reference.host_expression(
        pattern(notes[0]),
        HostExpression::Gain((0.25 + 0.5 * 8193. / 16383.) as f32),
    );
    step(
        &mut actual,
        &mut reference,
        &mut pcm,
        &mut oracle,
        1024,
        partition,
    );
    // Public authored writes broadcast accepted values to both physical owners.
    received(&mut actual, 4, Control::Cc(21, ControlValue::FULL));
    for note in notes {
        reference.host_expression(pattern(note), HostExpression::Gain(0.75));
    }
    step(
        &mut actual,
        &mut reference,
        &mut pcm,
        &mut oracle,
        1024,
        partition,
    );
    // A residency upgrade preserves the receipt state and live source cursor.
    actual.upgrade_bank(Box::new(bank(mods.clone(), true)));
    reference.upgrade_bank(Box::new(bank(vec![], true)));
    step(
        &mut actual,
        &mut reference,
        &mut pcm,
        &mut oracle,
        768,
        partition,
    );
    // Muting advances the same source/filter clocks; restoring gain resumes its real phase.
    actual.host_expression(pattern(notes[1]), HostExpression::Gain(0.));
    reference.host_expression(pattern(notes[1]), HostExpression::Gain(0.));
    step(
        &mut actual,
        &mut reference,
        &mut pcm,
        &mut oracle,
        256,
        partition,
    );
    actual.host_expression(pattern(notes[1]), HostExpression::Gain(1.));
    reference.host_expression(pattern(notes[1]), HostExpression::Gain(0.75));
    for note in notes {
        actual.host_note_off(pattern(note));
        reference.host_note_off(pattern(note));
    }
    step(
        &mut actual,
        &mut reference,
        &mut pcm,
        &mut oracle,
        4096,
        partition,
    );
    for note in notes {
        assert_eq!(
            actual.host_note_end_at(note),
            reference.host_note_end_at(note)
        );
        assert!(actual.host_note_end_at(note).is_some());
    }
    // A new bank clears receipt provenance while retained numeric controllers remain nonzero.
    actual.set_bank(Some(Box::new(bank(mods, true))));
    reference.set_bank(Some(Box::new(bank(vec![], true))));
    assert_eq!(actual.cc_state()[4][1], 127);
    let fresh = host(592, 4);
    start(&mut actual, fresh, -1.);
    start(&mut reference, fresh, -1.);
    step(
        &mut actual,
        &mut reference,
        &mut pcm,
        &mut oracle,
        1536,
        partition,
    );
    actual.host_note_off(pattern(fresh));
    reference.host_note_off(pattern(fresh));
    step(
        &mut actual,
        &mut reference,
        &mut pcm,
        &mut oracle,
        4096,
        partition,
    );
    assert_eq!(
        actual.host_note_end_at(fresh),
        reference.host_note_end_at(fresh)
    );
    pcm
}

#[test]
fn saved_controller_availability_preserves_full_pcm_physical_scope_and_frozen_mpe_tails() {
    let fixed = timeline(&[128]);
    same(&fixed, &timeline(&[1, 7, 31, 127]));
    for partition in [&[128][..], &[1, 7, 31, 127][..]] {
        // Numeric defaults0 and127 are real pre-receipt volume choices.
        for (default, gain) in [(0, 0.), (127, 1.)] {
            let mut a = engine(bank(vec![imported_volume(default, None)], false));
            let mut b = engine(bank(vec![], false));
            let note = host(593, 4);
            start(&mut a, note, -1.);
            start(&mut b, note, -1.);
            b.host_expression(pattern(note), HostExpression::Gain(gain));
            let (mut p, mut q) = (Vec::new(), Vec::new());
            step(&mut a, &mut b, &mut p, &mut q, 1024, partition);
            received(&mut a, 4, Control::Cc(1, ControlValue::from_midi1(64)));
            b.host_expression(pattern(note), HostExpression::Gain(64. / 127.));
            step(&mut a, &mut b, &mut p, &mut q, 1024, partition);
        }
        let mut a = engine(bank(
            vec![assignment(74, ModTarget::Volume, Some(-1), 1., None)],
            false,
        ));
        let mut b = engine(bank(vec![], false));
        a.configure_mpe(Some((0, 4, 48)));
        let old = host(594, 1);
        let new = host(595, 1);
        let (mut p, mut q) = (Vec::new(), Vec::new());
        start(&mut a, old, -1.);
        start(&mut b, old, -1.);
        step(&mut a, &mut b, &mut p, &mut q, 1024, partition);
        a.host_note_off(pattern(old));
        b.host_note_off(pattern(old));
        received(&mut a, 1, Control::Cc(74, ControlValue::ZERO));
        start(&mut a, new, 1.);
        start(&mut b, new, 1.);
        b.host_expression(pattern(new), HostExpression::Gain(0.));
        step(&mut a, &mut b, &mut p, &mut q, 512, partition);
        // Exact-note brightness takes precedence over frozen unreceived member state.
        a.host_expression(
            pattern(old),
            HostExpression::Brightness(ControlValue::from_midi1(64)),
        );
        b.host_expression(pattern(old), HostExpression::Gain(64. / 127.));
        step(&mut a, &mut b, &mut p, &mut q, 512, partition);
        a.host_note_off(pattern(new));
        b.host_note_off(pattern(new));
        step(&mut a, &mut b, &mut p, &mut q, 4096, partition);
        for note in [old, new] {
            assert_eq!(a.host_note_end_at(note), b.host_note_end_at(note));
            assert!(a.host_note_end_at(note).is_some());
        }
    }
}
