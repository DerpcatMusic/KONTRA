//! Plain (non-MPE) channel pitch bend and channel/poly pressure through Ingress.
use sampler_core::{
    Envelope, Expression, Limits, NoteId, Pcm, Playback, Prepared, Region, Runtime,
};
use sampler_midi::{Applied, Ingress, Packets, Version};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

fn runtime(bend_range: f64) -> Runtime {
    let pcm = Pcm::new(48000, vec![[0.25f32, 0.25]; 8192].into()).unwrap();
    let region = Region {
        sample: 0,
        key_low: 60,
        key_high: 62,
        root_key: None,
        velocity_low: 0.0,
        velocity_high: 1.0,
        gain: 1.0,
        envelope: Envelope::new(0, 0, 0, 1.0, 256).unwrap(),
        playback: Playback::default(),
    };
    let plan = Prepared::new(48000, vec![pcm], vec![region], 3)
        .unwrap()
        .with_bend_range(bend_range)
        .unwrap();
    Runtime::new(
        plan,
        Limits {
            notes: 8,
            channels: 4,
            performances: 1,
            families: 8,
            decisions: 0,
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

fn midi1(status: u8, channel: u8, a: u8, b: u8) -> u32 {
    0x2000_0000 | (u32::from(status | channel) << 16) | (u32::from(a) << 8) | u32::from(b)
}
fn bend(channel: u8, value: u16) -> u32 {
    midi1(0xe0, channel, (value & 127) as u8, (value >> 7) as u8)
}
fn apply(ingress: &mut Ingress, rt: &mut Runtime, words: &[u32]) -> Applied {
    ingress
        .apply(rt, Packets::new(words).next().unwrap().unwrap())
        .unwrap()
}
fn start(ingress: &mut Ingress, rt: &mut Runtime, channel: u8, key: u8) -> NoteId {
    let Applied::Started(note) = apply(ingress, rt, &[midi1(0x90, channel, key, 100)]) else {
        panic!("not admitted")
    };
    note
}
fn expression(rt: &Runtime, note: NoteId) -> Expression {
    rt.expression(rt.expression_id(note).unwrap()).unwrap()
}

#[test]
fn channel_bend_reaches_channel_notes_and_seeds_new_ones() {
    let mut rt = runtime(2.0);
    let mut ingress = Ingress::new(0, [Some(Version::Midi1); 16]);
    support::without_heap(|| {
        let a = start(&mut ingress, &mut rt, 0, 60);
        let other = start(&mut ingress, &mut rt, 1, 61);
        assert_eq!(
            apply(&mut ingress, &mut rt, &[bend(0, 16383)]),
            Applied::Expression { owners: 1 }
        );
        assert_eq!(expression(&rt, a).pitch_semitones, 2.0);
        assert_eq!(expression(&rt, other).pitch_semitones, 0.0);
        // A new note on the channel starts at the held bend.
        let b = start(&mut ingress, &mut rt, 0, 62);
        assert_eq!(expression(&rt, b).pitch_semitones, 2.0);
        // Release tails keep following the channel.
        apply(&mut ingress, &mut rt, &[midi1(0x80, 0, 60, 0)]);
        assert_eq!(
            apply(&mut ingress, &mut rt, &[bend(0, 0)]),
            Applied::Expression { owners: 2 }
        );
        assert_eq!(expression(&rt, a).pitch_semitones, -2.0);
        assert_eq!(expression(&rt, b).pitch_semitones, -2.0);
        apply(&mut ingress, &mut rt, &[bend(0, 8192)]);
        assert_eq!(expression(&rt, a).pitch_semitones, 0.0);
    });
}

#[test]
fn instrument_default_range_and_rpn_zero_override() {
    let mut rt = runtime(12.0);
    let mut ingress = Ingress::new(0, [Some(Version::Midi1); 16]);
    support::without_heap(|| {
        let a = start(&mut ingress, &mut rt, 3, 60);
        apply(&mut ingress, &mut rt, &[bend(3, 16383)]);
        assert_eq!(expression(&rt, a).pitch_semitones, 12.0);
        // RPN 0 = 7 semitones 50 cents re-projects the held bend.
        for (cc, value) in [(101, 0), (100, 0), (6, 7)] {
            assert_eq!(
                apply(&mut ingress, &mut rt, &[midi1(0xb0, 3, cc, value)]),
                Applied::Controller
            );
        }
        assert_eq!(expression(&rt, a).pitch_semitones, 7.0);
        apply(&mut ingress, &mut rt, &[midi1(0xb0, 3, 38, 50)]);
        assert_eq!(expression(&rt, a).pitch_semitones, 7.5);
        // Null RPN: data entry no longer changes the range.
        apply(&mut ingress, &mut rt, &[midi1(0xb0, 3, 101, 127)]);
        apply(&mut ingress, &mut rt, &[midi1(0xb0, 3, 100, 127)]);
        apply(&mut ingress, &mut rt, &[midi1(0xb0, 3, 6, 1)]);
        assert_eq!(expression(&rt, a).pitch_semitones, 7.5);
        // The override is per channel.
        let b = start(&mut ingress, &mut rt, 4, 61);
        apply(&mut ingress, &mut rt, &[bend(4, 0)]);
        assert_eq!(expression(&rt, b).pitch_semitones, -12.0);
    });
}

#[test]
fn channel_and_poly_pressure() {
    let mut rt = runtime(2.0);
    let mut ingress = Ingress::new(0, [Some(Version::Midi1); 16]);
    support::without_heap(|| {
        let a = start(&mut ingress, &mut rt, 0, 60);
        let b = start(&mut ingress, &mut rt, 0, 61);
        let other = start(&mut ingress, &mut rt, 2, 62);
        assert_eq!(
            apply(&mut ingress, &mut rt, &[midi1(0xa0, 0, 61, 127)]),
            Applied::Expression { owners: 1 }
        );
        assert_eq!(expression(&rt, a).pressure, 0);
        assert_eq!(expression(&rt, b).pressure, u32::MAX);
        assert_eq!(
            apply(&mut ingress, &mut rt, &[midi1(0xd0, 0, 127, 0)]),
            Applied::Expression { owners: 2 }
        );
        assert_eq!(expression(&rt, a).pressure, u32::MAX);
        assert_eq!(expression(&rt, other).pressure, 0);
        // Pressure leaves pitch and timbre alone; new notes start at it.
        assert_eq!(expression(&rt, a).timbre, Expression::default().timbre);
        apply(&mut ingress, &mut rt, &[midi1(0x80, 0, 60, 0)]);
        let c = start(&mut ingress, &mut rt, 0, 60);
        assert_eq!(expression(&rt, c).pressure, u32::MAX);
    });
}

#[test]
fn midi2_bend_and_registered_range() {
    let mut rt = runtime(2.0);
    let mut ingress = Ingress::new(0, [Some(Version::Midi2); 16]);
    support::without_heap(|| {
        let Applied::Started(a) = apply(&mut ingress, &mut rt, &[0x4090_3c00, 0xffff_0000]) else {
            panic!("not admitted")
        };
        apply(&mut ingress, &mut rt, &[0x40e0_0000, u32::MAX]);
        assert_eq!(expression(&rt, a).pitch_semitones, 2.0);
        // Registered controller bank 0 index 0: 24 semitones, 0 cents.
        assert_eq!(
            apply(&mut ingress, &mut rt, &[0x4020_0000, 24 << 25]),
            Applied::Configuration
        );
        assert_eq!(expression(&rt, a).pitch_semitones, 24.0);
        apply(&mut ingress, &mut rt, &[0x40e0_0000, 0x8000_0000]);
        assert_eq!(expression(&rt, a).pitch_semitones, 0.0);
    });
}
