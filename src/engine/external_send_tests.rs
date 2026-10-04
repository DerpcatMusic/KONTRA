//! Unresolved native OutputChannelID is preserved, never guessed as a host index.
use super::*;
use ni_file::kontakt::{Chunk, StructuredObject, objects::Program};

fn chunk(id: u16, data: Vec<u8>) -> Vec<u8> {
    [
        id.to_le_bytes().as_slice(),
        (data.len() as u32).to_le_bytes().as_slice(),
        &data,
    ]
    .concat()
}
fn structured(version: u16, private: &[u8], children: &[u8]) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend(version.to_le_bytes());
    for part in [private, &[][..], children] {
        bytes.extend((part.len() as u32).to_le_bytes());
        bytes.extend(part);
    }
    bytes
}
fn saved_send(channel: i32) -> (Vec<u8>, ProgramFx) {
    let mut private = 20u32.to_le_bytes().to_vec();
    private.extend([0; 6]);
    private.extend(3f32.to_le_bytes());
    private.extend(0f32.to_le_bytes());
    private.extend(channel.to_le_bytes());
    let mut public = vec![1];
    public.extend(0x10u16.to_le_bytes());
    public.extend(0u32.to_le_bytes());
    public.extend(4u32.to_le_bytes());
    public.extend(2f32.to_le_bytes());
    public.extend(0u32.to_le_bytes());
    let wrapper = chunk(0x25, structured(0x50, &private, &chunk(0x13, public)));
    let rack = |occupied: bool| {
        let mut data = vec![0, 0x12, 0];
        for slot in 0..8 {
            data.push(u8::from(occupied && slot == 0));
            if occupied && slot == 0 {
                data.extend(&wrapper);
            }
        }
        chunk(0x3a, data)
    };
    let bytes = [rack(false), rack(true)].concat();
    let mut cursor = std::io::Cursor::new(&bytes);
    let mut children = Vec::new();
    while cursor.position() < bytes.len() as u64 {
        children.push(Chunk::read(&mut cursor).unwrap());
    }
    let fx = ProgramFx::read(&Program(StructuredObject {
        version: 0xa5,
        public_data: Vec::new(),
        private_data: Vec::new(),
        children,
    }))
    .unwrap();
    (bytes, fx)
}
fn tail_effect() -> Effect {
    use crate::fx::params::{Impulse, Params};
    let mut bytes: Vec<u8> = [-1f32, 0., 0., 1., 20., 24000., 1., 20., 24000., -1.]
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    bytes.extend([0, 0, 1, 1, 0]);
    bytes.extend([0; 8]);
    bytes.extend(0i32.to_le_bytes());
    let mut params = crate::fx::params::parse(Kind::Convolution, &bytes);
    let Params::Convolution(c) = &mut params else {
        panic!("convolution layout")
    };
    let mut frames = vec![[0.; 2]; 1301];
    frames[1300] = [1.; 2];
    c.ir = Some(Impulse(std::sync::Arc::new(crate::audio::Sample {
        rate: 48000,
        frames,
    })));
    Effect {
        slot: 0,
        kind: Kind::Convolution,
        version: 0,
        bypass: false,
        output_gain: 6e6,
        dry_level: 0.,
        params,
    }
}

#[test]
fn native_saved_external_send_survives_cache_but_never_falls_back_into_main_pcm_or_activity() {
    for tail in [false, true] {
        let (bytes, parsed) = saved_send(173);
        assert_eq!(
            (parsed.send_outputs[0].slot, parsed.send_outputs[0].channel),
            (0, 173)
        );
        assert!(parsed.warnings().iter().any(|w| w.contains("173")));
        let mut actual_fixture = Fixture::new(!tail);
        actual_fixture.instrument.fx.send = parsed.send;
        actual_fixture.instrument.fx.send_outputs = parsed.send_outputs;
        let preset = actual_fixture._dir.path().join("saved-send.nki");
        std::fs::write(&preset, bytes).unwrap();
        let cache = actual_fixture._dir.path().join("cache");
        crate::cache::store(&cache, &preset, 0, &actual_fixture.instrument);
        actual_fixture.instrument =
            crate::cache::load(&cache, &preset, 0).expect("actual instrument cache");
        assert_eq!(actual_fixture.instrument.fx.send_outputs[0].channel, 173);
        let mut main_fixture = Fixture::new(!tail);
        let (_, main_send) = saved_send(-1);
        main_fixture.instrument.fx.send = main_send.send;
        main_fixture.instrument.fx.send_outputs = main_send.send_outputs;
        let mut dry_fixture = Fixture::new(!tail);
        let tap = Effect {
            slot: 7,
            kind: Kind::SendLevels,
            version: 0,
            bypass: false,
            output_gain: 1.,
            dry_level: 0.,
            params: Params::SendLevels(crate::fx::params::SendLevels {
                sends: vec![if tail { 1e-7 } else { 0.5 }],
                outputs: Vec::new(),
            }),
        };
        for fixture in [&mut actual_fixture, &mut main_fixture, &mut dry_fixture] {
            fixture.instrument.groups[0].amp_split_slot = Some(8);
            fixture.instrument.groups[0].fx = Chain {
                slots: vec![tap.clone()],
            };
            if tail {
                fixture.instrument.fx.main = Chain {
                    slots: vec![effect(0, 1.3)],
                };
            }
        }
        if tail {
            actual_fixture.instrument.fx.send.slots[0] = tail_effect();
            main_fixture.instrument.fx.send.slots[0] = tail_effect();
        }
        for sizes in [&[128][..], &[1, 7, 31, 127][..]] {
            let (mut actual, mut dry, mut main) = (
                actual_fixture.engine(false),
                dry_fixture.engine(true),
                main_fixture.engine(false),
            );
            let mut scalar = Oracle::new();
            let (mut at, mut block, mut target, mut contrast) = (0, 0, [0.25, 0.5], false);
            while at < 4096 {
                if at % 256 == 0 {
                    for e in [&mut actual, &mut dry, &mut main] {
                        e.begin_audio_block(256, 1, false);
                    }
                }
                match at {
                    0 | 2048 => {
                        let id = if at == 0 { 30 } else { 31 };
                        for e in [&mut actual, &mut dry, &mut main] {
                            assert!(e.host_note_on(root(id), 4, 60, 127, 0.));
                        }
                    }
                    32 | 1536 => {
                        target = if at == 32 { [2., -0.6] } else { [0.1, 0.75] };
                        for e in [&mut actual, &mut main] {
                            assert!(e.write(Address::Instrument(GroupPar::Volume), target[0]));
                            assert!(e.write(Address::Instrument(GroupPar::Pan), target[1]));
                        }
                    }
                    512 | 2560 => {
                        let id = if at == 512 { 30 } else { 31 };
                        for e in [&mut actual, &mut dry, &mut main] {
                            e.host_note_off(off(root(id)));
                        }
                    }
                    _ => {}
                }
                let boundary = [32, 512, 1536, 2048, 2560, 4096]
                    .into_iter()
                    .filter(|x| *x > at)
                    .min()
                    .unwrap()
                    .min((at / 256 + 1) * 256);
                let n = sizes[block % sizes.len()].min(boundary - at);
                let fed = !tail || !dry.player.voices.is_empty();
                let (mut l, mut r, mut x, mut y, mut u, mut v) = (
                    [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128], [0.; 128],
                );
                assert_eq!(
                    crate::plugin::tests::allocations(|| {
                        actual.render(&mut l[..n], &mut r[..n]);
                        dry.render(&mut x[..n], &mut y[..n]);
                        main.render(&mut u[..n], &mut v[..n]);
                        for e in [&mut actual, &mut dry, &mut main] {
                            e.mark_host_notes();
                        }
                    }),
                    0
                );
                for i in 0..n {
                    let expected = scalar.frame([x[i], y[i]], target, at / 256 * 256, fed);
                    assert!(
                        (l[i] - expected[0]).abs() < 3e-6 && (r[i] - expected[1]).abs() < 3e-6,
                        "external send tail{tail} frame{}",
                        at + i
                    );
                    contrast |= (l[i] - u[i]).abs().max((r[i] - v[i]).abs()) > 0.001;
                    if tail && (1536..2048).contains(&(at + i)) {
                        assert_eq!([l[i], r[i]], [0., 0.]);
                    }
                }
                at += n;
                block += 1;
            }
            assert!(
                contrast,
                "minus-one main return must remain audibly distinct"
            );
            for id in [30, 31] {
                assert_eq!(
                    actual.host_note_end_at(root(id)),
                    dry.host_note_end_at(root(id))
                );
                assert_eq!(
                    actual.host_note_end_at(root(id)),
                    main.host_note_end_at(root(id))
                );
                assert!(actual.host_note_end_at(root(id)).is_some());
            }
        }
    }
}
