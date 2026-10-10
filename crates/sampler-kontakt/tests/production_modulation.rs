//! Owned serialized NKS/WAV -> public load -> Prepared -> production Runtime.
//! These are regression requirements, not Kontakt runtime parity receipts.
use ni_file::kontakt::objects::{EnvelopeAhdsr, Lfo, LfoRecord};
use sampler_core::{EngineParameterAddress, Error, Frame, Input, Limits, Protocol, Runtime};
use sampler_kontakt::{Options, load};
#[path = "support/chunks.rs"]
mod wire;
use wire::{chunk, object};
#[path = "../../sampler-core/tests/support/mod.rs"]
mod support;

struct Fixture(std::path::PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn wide(s: &str) -> Vec<u8> {
    let mut out = (s.encode_utf16().count() as u32).to_le_bytes().to_vec();
    out.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
    out
}
fn name(out: &mut Vec<u8>, s: &str) {
    out.extend((s.len() as u32).to_le_bytes());
    out.extend(s.as_bytes());
}
fn internal(
    envelope: bool,
    bypass: bool,
    retrigger: bool,
    sibling: bool,
    invert: bool,
    rate: f32,
    outgoing: &str,
) -> Vec<u8> {
    // Physical target 0 stays volume, target 1 stays the named destination.
    let mut private = 2u32.to_le_bytes().to_vec();
    for (param, depth) in [
        ("volume", if envelope { 0.5 } else { 0.0 }),
        (outgoing, if sibling { 0.25 } else { 0.5 }),
    ] {
        name(&mut private, param);
        private.extend((depth as f32).to_le_bytes());
        private.extend((-1i16).to_le_bytes());
        private.push(0x10);
        private.extend(0u16.to_le_bytes());
        name(&mut private, param);
        private.push(u8::from(invert)); // Invert is separate from positive intensity.
    }
    private.extend([0, 0]); // No shapers.
    private.extend([0, u8::from(bypass), u8::from(retrigger), 0]);
    private.extend(0u32.to_le_bytes());
    name(&mut private, if sibling { "sibling" } else { "main" });
    private.extend(1u32.to_le_bytes());
    let source = if envelope {
        EnvelopeAhdsr {
            attack_curve: 0.,
            attack_ms: 0.,
            decay_ms: 0.,
            hold_ms: 0.,
            release_ms: 20.,
            sustain: 0.5,
            unknown_flag: 0,
            unknown_tail: vec![0; 16],
        }
        .to_chunk()
        .unwrap()
    } else {
        Lfo {
            structured: false,
            version: 0x72,
            waveform: 1,
            initial_values: [0., rate, 0.5, 0.],
            records: [
                LfoRecord {
                    flag: false,
                    values: [-1., 0., 0.],
                },
                LfoRecord {
                    flag: false,
                    values: [-1., 0., 0.],
                },
            ],
            trailing_flag: false,
            trailing_values: None,
            additional_flag: None,
        }
        .to_chunk()
        .unwrap()
    };
    let mut children = Vec::new();
    source.write(&mut children).unwrap();
    chunk(0x0d, &object(0x81, &private, &[], &children))
}
fn fixture(envelope: bool, bypass: bool, retrigger: bool) -> Fixture {
    fixture_inverted(envelope, bypass, retrigger, false)
}
fn fixture_inverted(envelope: bool, bypass: bool, retrigger: bool, invert: bool) -> Fixture {
    fixture_rate(envelope, bypass, retrigger, invert, 0.01)
}
fn fixture_rate(envelope: bool, bypass: bool, retrigger: bool, invert: bool, rate: f32) -> Fixture {
    fixture_target(envelope, bypass, retrigger, invert, rate, "pan")
}
fn fixture_target(
    envelope: bool,
    bypass: bool,
    retrigger: bool,
    invert: bool,
    rate: f32,
    outgoing: &str,
) -> Fixture {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let root = std::env::temp_dir().join(format!(
        "kontra-production-mod-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir(&root).unwrap();
    let mut wav = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0".to_vec();
    wav.extend(1u16.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(48000u32.to_le_bytes());
    wav.extend(96000u32.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(8192u32.to_le_bytes());
    for _ in 0..4096 {
        wav.extend(16384i16.to_le_bytes());
    }
    let length = wav.len() as u32 - 8;
    wav[4..8].copy_from_slice(&length.to_le_bytes());
    std::fs::write(root.join("owned.wav"), wav).unwrap();

    let mut group = wide("Owned modulation");
    for v in [1f32, 0., 1.] {
        group.extend(v.to_le_bytes());
    }
    group.extend([1, 0, 0, 0]);
    group.extend(0i32.to_le_bytes());
    group.extend((-1i16).to_le_bytes());
    group.extend([0; 10]);
    group.extend(0i32.to_le_bytes());
    let mut slots = Vec::new();
    for slot in 0..16 {
        slots.push(u8::from(slot == 7 || slot == 12));
        if slot == 7 {
            slots.extend(internal(
                envelope, bypass, retrigger, false, invert, rate, outgoing,
            ));
        }
        if slot == 12 {
            slots.extend(internal(false, false, true, true, false, 0.01, "pan"));
        }
    }
    let mut children = chunk(0x38, &0u32.to_le_bytes());
    children.extend(chunk(0x3b, &object(0x10, &[], &slots, &[])));
    let mut groups = 1u32.to_le_bytes().to_vec();
    groups.extend(object(0x95, &[], &group, &children));

    let mut zone = vec![0; 12];
    for v in [1i16, 127, 60, 60, 0, 0, 0, 0, 60] {
        zone.extend(v.to_le_bytes());
    }
    for v in [1f32, 0., 1.] {
        zone.extend(v.to_le_bytes());
    }
    zone.extend([0, 1, 0, 0, 0, 0]);
    zone.extend(0i32.to_le_bytes());
    zone.extend(0i32.to_le_bytes());
    zone.extend(48000i32.to_le_bytes());
    zone.push(1);
    for v in [4096i32, 0, 60] {
        zone.extend(v.to_le_bytes());
    }
    zone.extend(0f32.to_le_bytes());
    zone.push(0);
    zone.extend(0i32.to_le_bytes());
    let mut zones = 1u32.to_le_bytes().to_vec();
    zones.extend(0u32.to_le_bytes());
    zones.extend(object(0x9a, &[], &zone, &[]));
    let mut public = wide("Owned regression");
    public.extend(0f64.to_le_bytes());
    public.push(0);
    for v in [1f32, 0., 1.] {
        public.extend(v.to_le_bytes());
    }
    public.extend([1, 127, 0, 127]);
    public.extend((-1i16).to_le_bytes());
    public.extend([0; 16]);
    public.push(0);
    public.extend(0i32.to_le_bytes());
    for _ in 0..3 {
        public.extend(wide(""));
    }
    public.extend([0; 6]);
    let mut children = chunk(0x33, &groups);
    children.extend(chunk(0x34, &zones));
    let mut payload = chunk(0x28, &object(0xae, &[], &public, &children));
    let mut table = vec![0; 4];
    table.extend(1u32.to_le_bytes());
    table.extend(1i32.to_le_bytes());
    table.push(4);
    table.extend(wide("owned.wav"));
    table.extend([0; 12]);
    payload.extend(chunk(0x3d, &table));
    // Authored literal FastLZ runs; same independent framing as monolith.rs.
    let packed: Vec<_> = payload
        .chunks(32)
        .flat_map(|b| std::iter::once((b.len() - 1) as u8).chain(b.iter().copied()))
        .collect();
    let mut nks = vec![0; 222];
    nks[..4].copy_from_slice(&0x7fa89012u32.to_le_bytes());
    nks[4..8].copy_from_slice(&(packed.len() as u32).to_le_bytes());
    nks[8..10].copy_from_slice(&0x110u16.to_le_bytes());
    nks[10..14].copy_from_slice(&0xea37631au32.to_le_bytes());
    nks[186..190].copy_from_slice(&(payload.len() as u32).to_le_bytes());
    nks.extend(packed);
    nks.extend(0xb00ee1aeu32.to_le_bytes());
    nks.extend([1, 1, 12, 0]);
    std::fs::write(root.join("owned.nki"), nks).unwrap();
    Fixture(root)
}
fn loaded(fixture: &Fixture) -> sampler_kontakt::Loaded {
    let loaded = load(
        &fixture.0.join("owned.nki"),
        &Options {
            scripts: false,
            mpe: None,
            ..Options::default()
        },
        |_| {},
    )
    .unwrap();
    assert_eq!(loaded.instrument.zones.len(), 1);
    assert_eq!(
        loaded
            .instrument
            .source_indices
            .modulators
            .iter()
            .map(|m| m.slot)
            .collect::<Vec<_>>(),
        [7, 12]
    );
    loaded
}
fn runtime(fixture: &Fixture) -> Runtime {
    Runtime::new(
        loaded(fixture).plan,
        Limits {
            notes: 4,
            channels: 0,
            performances: 1,
            expressions: 4,
            families: 4,
            decisions: 4,
            voices: 4,
            commands: 16,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap()
}
fn input(id: i32) -> Input {
    Input {
        protocol: Protocol::Clap,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(id),
    }
}
fn address(name: &str, slot: i32, target: i32) -> EngineParameterAddress {
    EngineParameterAddress {
        parameter: sampler_core::engine_parameter_id(name).unwrap(),
        group: 0,
        slot,
        generic: target,
    }
}
fn render(rt: &mut Runtime, count: usize, block: usize) -> Vec<Frame> {
    let mut pcm = vec![[0.; 2]; count];
    support::without_heap(|| {
        for segment in pcm.chunks_mut(block) {
            rt.render(segment).unwrap();
        }
    });
    pcm
}
fn last(rt: &mut Runtime) -> Frame {
    *render(rt, 128, 17).last().unwrap()
}
fn close(actual: Frame, expected: Frame) {
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(a, e)| (*a - e).abs() < 2e-6),
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn serialized_lfo_live_intensity_bypass_reaches_pcm_without_compacting_slots() {
    let f = fixture(false, false, true);
    let mut rt = runtime(&f);
    rt.trigger(input(1), 60, 1.).unwrap();
    close(last(&mut rt), [0.125, 0.5]); // pan=.5+.25, own constant sample=.5
    let depth = address("ENGINE_PAR_MOD_TARGET_INTENSITY", 7, 1);
    rt.set_engine_parameter(depth, 250000).unwrap();
    assert_eq!(rt.engine_parameter(depth).unwrap(), 250000);
    close(last(&mut rt), [0.25, 0.5]); // sibling .25 is unchanged
    assert_eq!(
        rt.engine_parameter(address("ENGINE_PAR_MOD_TARGET_INTENSITY", 12, 1))
            .unwrap(),
        250000
    );
    let bypass = address("ENGINE_PAR_INTMOD_BYPASS", 7, -1);
    rt.set_engine_parameter(bypass, 1).unwrap();
    assert_eq!(rt.engine_parameter(bypass).unwrap(), 1);
    close(last(&mut rt), [0.375, 0.5]);
    rt.set_engine_parameter(bypass, 0).unwrap();
    close(last(&mut rt), [0.25, 0.5]);
    for unbound in [
        address("ENGINE_PAR_MOD_TARGET_INTENSITY", 0, 1),
        address("ENGINE_PAR_MOD_TARGET_INTENSITY", 7, -1),
        address("ENGINE_PAR_MOD_TARGET_INTENSITY", 7, 2),
        address("ENGINE_PAR_MOD_TARGET_MP_INTENSITY", 7, 1),
    ] {
        assert_eq!(
            rt.set_engine_parameter(unbound, 500000),
            Err(Error::InvalidInput)
        );
    }
    close(last(&mut rt), [0.25, 0.5]);
}

#[test]
fn serialized_saved_bypass_stays_neutral_until_live_enable() {
    let f = fixture(false, true, true);
    let mut rt = runtime(&f);
    rt.trigger(input(1), 60, 1.).unwrap();
    close(last(&mut rt), [0.375, 0.5]);
    rt.set_engine_parameter(address("ENGINE_PAR_INTMOD_BYPASS", 7, -1), 0)
        .unwrap();
    close(last(&mut rt), [0.125, 0.5]);
}

#[test]
fn serialized_retriggered_ahdsr_routes_have_live_amplitude_and_pan_consumers() {
    let f = fixture(true, false, true);
    let mut rt = runtime(&f);
    rt.trigger(input(1), 60, 1.).unwrap();
    close(last(&mut rt), [0.1875, 0.375]); // env=.5, gain=.75, pan=.25+.25
    rt.set_engine_parameter(address("ENGINE_PAR_MOD_TARGET_INTENSITY", 7, 0), 0)
        .unwrap();
    close(last(&mut rt), [0.25, 0.5]);
    rt.set_engine_parameter(address("ENGINE_PAR_INTMOD_BYPASS", 7, -1), 1)
        .unwrap();
    close(last(&mut rt), [0.375, 0.5]);
    rt.set_engine_parameter(address("ENGINE_PAR_INTMOD_BYPASS", 7, -1), 0)
        .unwrap();
    close(last(&mut rt), [0.25, 0.5]);
}

#[test]
fn serialized_retrigger_off_ahdsr_live_controls_remain_unbound() {
    let f = fixture(true, false, false);
    let mut rt = runtime(&f);
    for a in [
        address("ENGINE_PAR_INTMOD_BYPASS", 7, -1),
        address("ENGINE_PAR_MOD_TARGET_INTENSITY", 7, 1),
    ] {
        assert_eq!(rt.set_engine_parameter(a, 0), Err(Error::InvalidInput));
    }
    // This does not certify overlapping retrigger-off ownership; that gate remains open.
}

#[test]
fn serialized_invert_is_not_replaced_by_positive_or_mp_intensity() {
    let f = fixture_inverted(false, false, true, true);
    let mut rt = runtime(&f);
    rt.trigger(input(1), 60, 1.).unwrap();
    close(last(&mut rt), [0.5, 0.375]); // main pan=-.5, sibling=+.25
    rt.set_engine_parameter(address("ENGINE_PAR_MOD_TARGET_INTENSITY", 7, 1), 250000)
        .unwrap();
    close(last(&mut rt), [0.5, 0.5]); // positive .25 * inverted -1 + sibling .25
    assert_eq!(
        rt.set_engine_parameter(address("ENGINE_PAR_MOD_TARGET_MP_INTENSITY", 7, 1), 0),
        Err(Error::InvalidInput)
    );
    close(last(&mut rt), [0.5, 0.5]);
}

#[test]
fn serialized_overlap_release_keeps_other_voice_and_live_control_ownership() {
    let f = fixture(true, false, true);
    let mut rt = runtime(&f);
    let a = rt.trigger(input(1), 60, 1.).unwrap();
    close(last(&mut rt), [0.1875, 0.375]);
    let b = rt.trigger(input(2), 60, 1.).unwrap();
    close(last(&mut rt), [0.375, 0.75]);
    assert_eq!(rt.voice_count(), 2);
    support::without_heap(|| rt.release(a).unwrap());
    render(&mut rt, 1024, 31);
    close(last(&mut rt), [0.1875, 0.375]);
    assert_eq!(rt.voice_count(), 1);
    support::without_heap(|| {
        rt.set_engine_parameter(address("ENGINE_PAR_INTMOD_BYPASS", 7, -1), 1)
            .unwrap()
    });
    close(last(&mut rt), [0.375, 0.5]);
    support::without_heap(|| rt.release(b).unwrap());
    render(&mut rt, 1024, 13);
    assert_eq!(rt.voice_count(), 0);
    close(last(&mut rt), [0., 0.]);
    rt.trigger(input(3), 60, 1.).unwrap();
    close(last(&mut rt), [0.375, 0.5]); // plan's bypass survives voice-slot reuse
}

#[test]
fn serialized_filtered_routes_cannot_publish_getter_only_live_bindings() {
    let f = fixture(false, false, true);
    let loaded = load(
        &f.0.join("owned.nki"),
        &Options {
            scripts: false,
            mpe: None,
            keys: 61..=61,
            ..Options::default()
        },
        |_| {},
    )
    .unwrap();
    assert!(loaded.instrument.zones.is_empty());
    assert!(
        !loaded
            .plan
            .engine_parameter_bindings()
            .iter()
            .any(|b| b.address == address("ENGINE_PAR_INTMOD_BYPASS", 7, -1)
                || b.address == address("ENGINE_PAR_MOD_TARGET_INTENSITY", 7, 1))
    );
}

#[test]
fn serialized_live_writes_are_independent_of_host_render_partition() {
    let f = fixture(false, false, true);
    let mut reference = runtime(&f);
    let mut divided = runtime(&f);
    for rt in [&mut reference, &mut divided] {
        rt.trigger(input(1), 60, 1.).unwrap();
    }
    assert_eq!(
        render(&mut reference, 128, 64),
        render(&mut divided, 128, 7)
    );
    for (a, value) in [
        (address("ENGINE_PAR_MOD_TARGET_INTENSITY", 7, 1), 125000),
        (address("ENGINE_PAR_INTMOD_BYPASS", 7, -1), 1),
        (address("ENGINE_PAR_INTMOD_BYPASS", 7, -1), 0),
    ] {
        for rt in [&mut reference, &mut divided] {
            support::without_heap(|| rt.set_engine_parameter(a, value).unwrap());
            rt.render(&mut []).unwrap();
        }
        assert_eq!(
            render(&mut reference, 128, 64),
            render(&mut divided, 128, 11)
        );
    }
}

#[test]
fn serialized_bypass_masks_routes_without_restarting_the_source_clock() {
    // Declared KONTRA policy; the official docs do not define native pause/resume.
    let f = fixture_rate(false, false, true, false, 100.);
    let mut reference = runtime(&f);
    let mut masked = runtime(&f);
    for rt in [&mut reference, &mut masked] {
        rt.trigger(input(1), 60, 1.).unwrap();
    }
    render(&mut reference, 128, 64);
    render(&mut masked, 128, 17);
    masked
        .set_engine_parameter(address("ENGINE_PAR_INTMOD_BYPASS", 7, -1), 1)
        .unwrap();
    render(&mut reference, 192, 64);
    render(&mut masked, 192, 19);
    masked
        .set_engine_parameter(address("ENGINE_PAR_INTMOD_BYPASS", 7, -1), 0)
        .unwrap();
    let expected = render(&mut reference, 128, 64);
    let actual = render(&mut masked, 128, 23);
    assert_eq!(&actual[64..], &expected[64..]);
}

#[test]
fn serialized_pitch_intensity_changes_pcm_cursor_step_not_pan_or_voice_identity() {
    let f = fixture_target(false, false, true, false, 0.01, "pitch");
    // An owned linear ramp makes the source-frame step observable in PCM.
    let path = f.0.join("owned.wav");
    let mut wav = std::fs::read(&path).unwrap();
    for (i, bytes) in wav[44..].chunks_exact_mut(2).enumerate() {
        bytes.copy_from_slice(&(i as i16 * 4).to_le_bytes());
    }
    std::fs::write(path, wav).unwrap();
    let mut rt = runtime(&f);
    rt.trigger(input(1), 60, 1.).unwrap();
    let first = render(&mut rt, 128, 17);
    let step = (first[127][1] - first[126][1]) * 8192.;
    assert!(
        (step - 2f32.sqrt()).abs() < 0.002,
        "saved six-semitone step: {step}"
    );
    rt.set_engine_parameter(address("ENGINE_PAR_MOD_TARGET_INTENSITY", 7, 1), 0)
        .unwrap();
    let next = render(&mut rt, 256, 19);
    let step = (next[255][1] - next[254][1]) * 8192.;
    assert!((step - 1.).abs() < 0.002, "live neutral step: {step}");
    assert!((next[255][0] - next[255][1] * 0.75).abs() < 2e-6);
    assert!(next[255][1] > first[127][1]); // No cursor/voice restart.
    assert_eq!(rt.voice_count(), 1);
}
