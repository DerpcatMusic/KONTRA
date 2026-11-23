#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use kontra_ir as ir;

use crate::gens::*;
use crate::{Error, laws, translate_bytes};

/// A zone: its generators (the terminal one last) and how many modulators.
type Zone = (Vec<(u16, u16)>, usize);

struct Sample {
    name: &'static str,
    start: u32,
    end: u32,
    loop_start: u32,
    loop_end: u32,
    kind: u16,
    rate: u32,
}

const fn mono(start: u32, end: u32) -> Sample {
    Sample {
        name: "s",
        start,
        end,
        loop_start: start + 10,
        loop_end: end - 10,
        kind: 1,
        rate: 44_100,
    }
}

/// Presets as `(name, bank, program, zones)`, instruments as `(name, zones)`.
fn bank(
    presets: &[(&str, u16, u16, Vec<Zone>)],
    insts: &[(&str, Vec<Zone>)],
    samples: &[Sample],
) -> Vec<u8> {
    fn name(out: &mut Vec<u8>, s: &str) {
        let mut b = [0u8; 20];
        b[..s.len()].copy_from_slice(s.as_bytes());
        out.extend(b);
    }
    fn chunk(id: &[u8], body: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(body);
        if body.len() % 2 == 1 {
            out.push(0);
        }
        out
    }
    fn list(kind: &[u8], chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut body = kind.to_vec();
        chunks.iter().for_each(|c| body.extend(c));
        chunk(b"LIST", &body)
    }
    /// Headers, bags, modulators and generators of one level.
    fn level(
        heads: &[(&str, Vec<u8>, &Vec<Zone>)],
        size: usize,
    ) -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
        let (mut hdr, mut bag, mut m, mut g) = (vec![], vec![], vec![], vec![]);
        let (mut nb, mut nm, mut ng) = (0u16, 0u16, 0u16);
        for (n, extra, zones) in heads {
            name(&mut hdr, n);
            hdr.extend(extra);
            hdr.extend(nb.to_le_bytes());
            hdr.resize(hdr.len() + size - 22 - extra.len(), 0);
            for (gens, mods) in zones.iter() {
                bag.extend(ng.to_le_bytes());
                bag.extend(nm.to_le_bytes());
                for (o, a) in gens {
                    g.extend(o.to_le_bytes());
                    g.extend(a.to_le_bytes());
                }
                for _ in 0..*mods {
                    m.extend([2, 2, 48, 0, 0x60, 1, 0, 0, 0, 0]);
                }
                nb += 1;
                nm += *mods as u16;
                ng += gens.len() as u16;
            }
        }
        name(&mut hdr, "EO");
        hdr.resize(hdr.len() + size - 20, 0);
        let end = hdr.len() - size + 20 + if size == 38 { 4 } else { 0 };
        hdr[end..end + 2].copy_from_slice(&nb.to_le_bytes());
        bag.extend(ng.to_le_bytes());
        bag.extend(nm.to_le_bytes());
        m.extend([0; 10]);
        g.extend([0; 4]);
        (hdr, bag, m, g)
    }
    let ph: Vec<_> = (presets.iter())
        .map(|(n, b, p, z)| {
            let mut e = p.to_le_bytes().to_vec();
            e.extend(b.to_le_bytes());
            (*n, e, z)
        })
        .collect();
    let ih: Vec<_> = insts.iter().map(|(n, z)| (*n, vec![], z)).collect();
    let (phdr, pbag, pmod, pgen) = level(&ph, 38);
    let (inst, ibag, imod, igen) = level(&ih, 22);
    let mut shdr = vec![];
    let eos = Sample {
        name: "EOS",
        start: 0,
        end: 0,
        loop_start: 0,
        loop_end: 0,
        kind: 0,
        rate: 0,
    };
    for s in samples.iter().chain([&eos]) {
        name(&mut shdr, s.name);
        for v in [s.start, s.end, s.loop_start, s.loop_end, s.rate] {
            shdr.extend(v.to_le_bytes());
        }
        shdr.extend([62, 0xfb, 0, 0]); // key 62, −5 cents
        shdr.extend(s.kind.to_le_bytes());
    }
    // Ends past 200 000 lie outside smpl.
    let words = (samples.iter().map(|s| s.end))
        .filter(|&e| e <= 200_000)
        .max()
        .unwrap_or(0)
        + 46;
    let mut body = b"sfbk".to_vec();
    body.extend(list(
        b"INFO",
        &[chunk(b"ifil", &[2, 0, 1, 0]), chunk(b"INAM", b"Test\0")],
    ));
    body.extend(list(
        b"sdta",
        &[chunk(b"smpl", &vec![0; 2 * words as usize])],
    ));
    body.extend(list(
        b"pdta",
        &[
            chunk(b"phdr", &phdr),
            chunk(b"pbag", &pbag),
            chunk(b"pmod", &pmod),
            chunk(b"pgen", &pgen),
            chunk(b"inst", &inst),
            chunk(b"ibag", &ibag),
            chunk(b"imod", &imod),
            chunk(b"igen", &igen),
            chunk(b"shdr", &shdr),
        ],
    ));
    chunk(b"RIFF", &body)
}

fn range(lo: u8, hi: u8) -> u16 {
    u16::from_le_bytes([lo, hi])
}

fn signed(v: i16) -> u16 {
    v as u16
}

/// One preset (bank 0, program 0) over one instrument.
fn simple(preset: Vec<Zone>, inst: Vec<Zone>, samples: &[Sample]) -> ir::Instrument {
    let bytes = bank(&[("P", 0, 0, preset)], &[("I", inst)], samples);
    translate_bytes(&bytes, Path::new("bank.sf2"), 0, 0).unwrap()
}

fn plain_preset() -> Vec<Zone> {
    vec![(vec![(INSTRUMENT, 0)], 0)]
}

fn features(i: &ir::Instrument) -> Vec<&str> {
    i.diagnostics.iter().map(|d| d.feature.as_str()).collect()
}

fn zones(i: &ir::Instrument) -> Vec<&ir::Zone> {
    i.groups.iter().flat_map(|g| &g.zones).collect()
}

#[test]
fn presets_list_by_bank_and_program_and_select_by_both() {
    let i = vec![(vec![(SAMPLE_ID, 0)], 0)];
    let bytes = bank(
        &[
            ("Drums", 128, 0, plain_preset()),
            ("Strings", 0, 48, plain_preset()),
            ("Piano", 0, 0, plain_preset()),
        ],
        &[("I", i)],
        &[mono(0, 1000)],
    );
    let path = std::env::temp_dir().join(format!("kontra-sf2-{}.sf2", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    let list = crate::list_presets(&path).unwrap();
    let keys: Vec<_> = list
        .iter()
        .map(|p| (p.bank, p.program, p.name.as_str()))
        .collect();
    assert_eq!(
        keys,
        [(0, 0, "Piano"), (0, 48, "Strings"), (128, 0, "Drums")]
    );
    assert_eq!(crate::read_first(&path).unwrap().name, "Piano");
    let drums = crate::read_preset(&path, 128, 0).unwrap();
    assert_eq!(drums.name, "Drums");
    assert_eq!(drums.samples[0].file, path);
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(
        translate_bytes(&bytes, &path, 1, 1),
        Err(Error::NoPreset {
            bank: 1,
            program: 1
        })
    ));
}

#[test]
fn a_sample_points_into_smpl_as_mono_16_bit_words() {
    let inst = simple(
        plain_preset(),
        vec![(vec![(SAMPLE_ID, 0)], 0)],
        &[mono(100, 1100)],
    );
    let s = &inst.samples[0];
    // RIFF(8)+sfbk(4)+INFO list(8+4+12+14)+sdta list(8+4)+smpl header(8).
    assert_eq!(s.offset, 70 + 200);
    assert_eq!(
        (s.len, s.frames, s.channels, s.rate),
        (Some(2000), 1000, 1, 44_100)
    );
    let z = zones(&inst)[0];
    assert_eq!((z.start, z.end, z.root), (0, 1000, 62));
    assert!((z.tune + 0.05).abs() < 1e-9, "pitch correction: {}", z.tune);
}

#[test]
fn global_zones_hold_defaults_local_ones_replace_and_presets_add() {
    let inst = vec![
        // Global: attack 1200 tc (2 s), fine tune +10.
        (vec![(ATTACK_VOL_ENV, 1200), (FINE_TUNE, 10)], 0),
        // Local replaces the fine tune, keeps the attack.
        (
            vec![(KEY_RANGE, range(40, 80)), (FINE_TUNE, 20), (SAMPLE_ID, 0)],
            0,
        ),
        (vec![(KEY_RANGE, range(81, 90)), (SAMPLE_ID, 0)], 0),
    ];
    let preset = vec![
        // Global preset zone: +1200 tc onto the attack, +3 semitones.
        (vec![(ATTACK_VOL_ENV, 1200), (COARSE_TUNE, 3)], 0),
        (vec![(KEY_RANGE, range(60, 127)), (INSTRUMENT, 0)], 0),
    ];
    let i = simple(preset, inst, &[mono(0, 1000)]);
    let z = zones(&i);
    // 40..=80 within 60..=127 is 60..=80; both zones kept.
    assert_eq!(z[0].keys, 60..=80);
    assert_eq!(z[1].keys, 81..=90);
    assert!((z[0].tune - (3.0 + 0.20 - 0.05)).abs() < 1e-9);
    assert!((z[1].tune - (3.0 + 0.10 - 0.05)).abs() < 1e-9);
    // 1200 + 1200 tc is 4 s.
    assert!((i.groups[0].envelope.attack - 4.0).abs() < 1e-4);
}

#[test]
fn ranges_that_do_not_meet_make_no_voice() {
    let inst = vec![(vec![(KEY_RANGE, range(0, 40)), (SAMPLE_ID, 0)], 0)];
    let preset = vec![(vec![(KEY_RANGE, range(60, 127)), (INSTRUMENT, 0)], 0)];
    assert!(zones(&simple(preset, inst, &[mono(0, 1000)])).is_empty());
}

#[test]
fn units_follow_the_spec() {
    assert!((laws::timecents(1200) - 2.0).abs() < 1e-6);
    assert!((laws::timecents(0) - 1.0).abs() < 1e-6);
    assert_eq!(laws::delay(-12_000), 0.0);
    assert!((laws::hz(6900) - 440.0).abs() < 0.01);
    assert!((laws::cutoff_hz(20_000) - laws::hz(13_500)).abs() < 1e-3);
    assert!((laws::sustain(200) - 0.1).abs() < 1e-6);
    assert_eq!(laws::sustain(1000), 0.0);
    assert!((laws::attenuation_db(100) + 4.0).abs() < 1e-6);
    assert_eq!(laws::pan(-500), -1.0);
    assert!((laws::damping(0) - std::f32::consts::SQRT_2).abs() < 1e-5);
    assert!((laws::damping(200) - 0.141_421).abs() < 1e-5);
}

#[test]
fn loop_modes_and_coarse_offsets() {
    let zone = |mode: u16, extra: Vec<(u16, u16)>| {
        let mut g = extra;
        g.extend([(SAMPLE_MODES, mode), (SAMPLE_ID, 0)]);
        (g, 0)
    };
    let inst = vec![
        zone(0, vec![(KEY_RANGE, range(0, 0))]),
        zone(1, vec![(KEY_RANGE, range(1, 1))]),
        zone(3, vec![(KEY_RANGE, range(2, 2))]),
        zone(
            1,
            vec![
                (KEY_RANGE, range(3, 3)),
                (START_COARSE_OFFSET, 1),
                (START_OFFSET, 5),
                (END_OFFSET, signed(-100)),
                (START_LOOP_OFFSET, 4),
                (END_LOOP_COARSE_OFFSET, signed(-1)),
            ],
        ),
    ];
    let i = simple(plain_preset(), inst, &[mono(0, 100_000)]);
    let z = zones(&i);
    assert!(z[0].loops.is_empty());
    assert_eq!(z[1].loops[0].start, 10);
    assert_eq!(z[1].loops[0].end, 99_990);
    assert!(!z[1].loops[0].until_release);
    assert!(z[2].loops[0].until_release);
    assert_eq!((z[3].start, z[3].end), (32_773, 99_900));
    assert_eq!(
        (z[3].loops[0].start, z[3].loops[0].end),
        (32_773, 99_990 - 32_768)
    );
}

#[test]
fn an_exclusive_class_chokes_by_note() {
    let inst = vec![
        (
            vec![
                (KEY_RANGE, range(42, 42)),
                (EXCLUSIVE_CLASS, 1),
                (SAMPLE_ID, 0),
            ],
            0,
        ),
        (
            vec![
                (KEY_RANGE, range(46, 46)),
                (EXCLUSIVE_CLASS, 1),
                (PAN, signed(-200)),
                (SAMPLE_ID, 0),
            ],
            0,
        ),
        (vec![(KEY_RANGE, range(36, 36)), (SAMPLE_ID, 0)], 0),
    ];
    let i = simple(plain_preset(), inst, &[mono(0, 1000)]);
    assert_eq!(i.voice_groups.len(), 1);
    let vg = i.voice_groups[0];
    assert!(vg.voices == 1 && vg.per_note);
    let classed: Vec<_> = i
        .groups
        .iter()
        .map(|g| (g.voice_group, g.zones.len()))
        .collect();
    assert_eq!(classed, [(Some(0), 2), (None, 1)]);
}

#[test]
fn filter_and_lfos_become_inserts_and_routes() {
    let inst = vec![(
        vec![
            (INITIAL_FILTER_FC, 6900),
            (INITIAL_FILTER_Q, 200),
            (VIB_LFO_TO_PITCH, 50),
            (FREQ_VIB_LFO, 0),
            (MOD_LFO_TO_FILTER_FC, 1200),
            (SAMPLE_ID, 0),
        ],
        0,
    )];
    let i = simple(plain_preset(), inst, &[mono(0, 1000)]);
    let g = &i.groups[0];
    let Some(Some(ir::Effect {
        kind: ir::EffectKind::Filter(f),
        ..
    })) = g.inserts.first()
    else {
        panic!("no filter: {:?}", g.inserts);
    };
    assert!((f.hz - 440.0).abs() < 0.01 && f.pass == ir::Pass::Low);
    assert!((f.damping[0] - 0.141_421).abs() < 1e-5);
    // Mod LFO first, then the vibrato LFO at 8.176 Hz.
    assert_eq!(g.lfos.len(), 2);
    assert!(matches!(g.lfos[1].rate, ir::Rate::Hz(hz) if (hz - 8.175_8).abs() < 1e-3));
    let vibrato = g
        .mods
        .iter()
        .find(|m| m.name == "vibrato LFO pitch")
        .unwrap();
    assert!(matches!(vibrato.route,
        ir::ModRoute { target: ir::ParamAddr::Pitch, source: ir::Source::Lfo(1), span: semitones, .. } if (semitones - 0.5).abs() < 1e-6
    ));
    let lfo_cut = g.mods.iter().find(|m| m.name == "mod LFO cutoff").unwrap();
    assert!(matches!(lfo_cut.route,
        ir::ModRoute { target: ir::ParamAddr::FilterCutoff { .. }, source: ir::Source::Lfo(0), depth, law: ir::RouteLaw::Linear, .. } if (depth * f.octaves - 1.0).abs() < 1e-6
    ));
    // The default velocity curve is (v/127)² in amplitude.
    assert!(g.mods.iter().any(|m| matches!(m.route,
        ir::ModRoute { target: ir::ParamAddr::Volume, source: ir::Source::Velocity, depth: intensity, law: ir::RouteLaw::Gain(ir::Curve::Squared), .. } if intensity == 1.0
    )));
}

#[test]
fn an_open_filter_makes_no_insert() {
    let i = simple(
        plain_preset(),
        vec![(vec![(SAMPLE_ID, 0)], 0)],
        &[mono(0, 1000)],
    );
    assert!(i.groups[0].inserts.is_empty());
}

#[test]
fn what_is_not_carried_is_reported() {
    let inst = vec![
        (
            vec![
                (KEY_RANGE, range(0, 10)),
                (REVERB_SEND, 200),
                (SAMPLE_ID, 0),
            ],
            2,
        ),
        (vec![(KEY_RANGE, range(11, 20)), (SAMPLE_ID, 1)], 0),
    ];
    let ogg = Sample {
        kind: 1 | crate::parser::OGG,
        ..mono(0, 1000)
    };
    let i = simple(plain_preset(), inst, &[mono(0, 1000), ogg]);
    let f = features(&i);
    for want in [
        "generator reverbEffectsSend",
        "modulator",
        "SF3 compressed sample",
    ] {
        assert!(f.contains(&want), "{want} missing from {f:?}");
    }
    // The Ogg zone is dropped, the other kept.
    assert_eq!(zones(&i).len(), 1);
    let broken = i
        .diagnostics
        .iter()
        .find(|d| d.feature == "SF3 compressed sample")
        .unwrap();
    assert_eq!(broken.impact, ir::Impact::Broken);
}

#[test]
fn malformed_banks_are_errors_not_panics() {
    let good = bank(
        &[("P", 0, 0, plain_preset())],
        &[("I", vec![(vec![(SAMPLE_ID, 0)], 0)])],
        &[mono(0, 1000)],
    );
    // Every truncation either reads or fails cleanly.
    for len in 0..good.len() {
        let _ = translate_bytes(&good[..len], Path::new("x"), 0, 0);
    }
    // Every byte flipped, likewise.
    for at in 0..good.len() {
        let mut bad = good.clone();
        bad[at] ^= 0xff;
        let _ = translate_bytes(&bad, Path::new("x"), 0, 0);
    }
    assert!(matches!(
        translate_bytes(b"RIFF\0\0\0\0WAVE", Path::new("x"), 0, 0),
        Err(Error::NotSoundFont(_))
    ));
    let mut short = good.clone();
    short.truncate(good.len() - 10);
    assert!(matches!(
        translate_bytes(&short, Path::new("x"), 0, 0),
        Err(Error::Malformed { chunk: "RIFF", .. })
    ));

    // An instrument index and a sample index past their lists.
    let bad_inst = bank(
        &[("P", 0, 0, vec![(vec![(INSTRUMENT, 7)], 0)])],
        &[("I", vec![(vec![(SAMPLE_ID, 0)], 0)])],
        &[mono(0, 1000)],
    );
    assert!(matches!(
        translate_bytes(&bad_inst, Path::new("x"), 0, 0),
        Err(Error::Malformed { chunk: "pgen", .. })
    ));
    let bad_sample = bank(
        &[("P", 0, 0, plain_preset())],
        &[("I", vec![(vec![(SAMPLE_ID, 9)], 0)])],
        &[mono(0, 1000)],
    );
    assert!(matches!(
        translate_bytes(&bad_sample, Path::new("x"), 0, 0),
        Err(Error::Malformed { chunk: "igen", .. })
    ));
    // A sample header past the end of smpl is a broken zone, not an error.
    let past = simple(
        plain_preset(),
        vec![(vec![(SAMPLE_ID, 0)], 0)],
        &[Sample {
            end: 1_000_000,
            ..mono(0, 1000)
        }],
    );
    assert!(features(&past).contains(&"sample outside smpl"));
}

#[test]
fn range_generators_respect_record_order_at_both_levels() {
    for (misplaced, terminal) in [
        (true, INSTRUMENT),
        (true, SAMPLE_ID),
        (false, INSTRUMENT),
        (false, SAMPLE_ID),
    ] {
        let mut generators = vec![
            (KEY_RANGE, range(60, 61)),
            (VEL_RANGE, range(70, 71)),
            (PAN, 0),
        ];
        if misplaced {
            generators.rotate_right(1);
        }
        generators.push((terminal, 0));
        let (preset, inst) = if terminal == INSTRUMENT {
            (vec![(generators, 0)], vec![(vec![(SAMPLE_ID, 0)], 0)])
        } else {
            (plain_preset(), vec![(generators, 0)])
        };
        let i = simple(preset, inst, &[mono(0, 1000)]);
        let z = zones(&i)[0];
        if misplaced {
            assert_eq!(z.keys, 0..=127);
            assert_eq!(z.velocities, 0..=127);
        } else {
            assert_eq!(z.keys, 60..=61);
            assert_eq!(z.velocities, 70..=71);
        }
    }
    let i = simple(
        plain_preset(),
        vec![(
            vec![
                (VEL_RANGE, range(70, 71)),
                (KEY_RANGE, range(60, 61)),
                (SAMPLE_ID, 0),
            ],
            0,
        )],
        &[mono(0, 1000)],
    );
    let z = zones(&i)[0];
    assert_eq!(z.keys, 0..=127);
    assert_eq!(z.velocities, 70..=71);
}

#[test]
fn bag_endpoints_must_reference_existing_records_even_for_empty_ranges() {
    let bags = [crate::parser::Bag {
        generator: 0,
        modulator: 0,
    }];
    for table in ["pbag", "ibag"] {
        assert!(
            crate::parser::zones((0, 0), &bags, &[], &[], table)
                .unwrap()
                .is_empty()
        );
        for span in [(1, 1), (0, 1), (1, 0)] {
            assert!(matches!(
                crate::parser::zones(span, &bags, &[], &[], table),
                Err(Error::Malformed { chunk, .. }) if chunk == table
            ));
        }
    }
}

#[test]
fn sample_chunk_cannot_extend_past_sdta() {
    let mut bytes = bank(
        &[("P", 0, 0, plain_preset())],
        &[("I", vec![(vec![(SAMPLE_ID, 0)], 0)])],
        &[mono(0, 1000)],
    );
    let translate = |bytes: &[u8]| translate_bytes(bytes, Path::new("bank.sf2"), 0, 0);
    assert_eq!(translate(&bytes).unwrap().samples.len(), 1); // smpl ends exactly at sdta's end.
    let len_at = bytes.windows(4).position(|w| w == b"smpl").unwrap() + 4;
    let len = u32::from_le_bytes(bytes[len_at..len_at + 4].try_into().unwrap());
    bytes[len_at..len_at + 4].copy_from_slice(&(len + 2).to_le_bytes());
    assert!(matches!(
        translate(&bytes),
        Err(Error::Malformed { chunk: "sdta", .. })
    ));
}

#[test]
fn loops_are_trimmed_or_dropped_at_the_playable_end() {
    let i = simple(
        plain_preset(),
        vec![
            (
                vec![
                    (END_OFFSET, signed(-300)),
                    (SAMPLE_MODES, 1),
                    (SAMPLE_ID, 0),
                ],
                0,
            ),
            (
                vec![
                    (END_OFFSET, signed(-950)),
                    (SAMPLE_MODES, 3),
                    (SAMPLE_ID, 0),
                ],
                0,
            ),
        ],
        &[Sample {
            loop_start: 100,
            loop_end: 900,
            ..mono(0, 1000)
        }],
    );
    let z = zones(&i);
    assert_eq!((z[0].loops[0].start, z[0].loops[0].end), (100, 700));
    assert!(z[1].loops.is_empty());
    assert!(features(&i).contains(&"empty loop"));
}

#[test]
fn expansion_budget_counts_cumulatively_and_preserves_distinct_layers() {
    let mut used = 0;
    for amount in [2, 1] {
        crate::translate::charge(&mut used, amount, 3, "instrument zones").unwrap();
    }
    assert!(matches!(
        crate::translate::charge(&mut used, 1, 3, "instrument zones"),
        Err(Error::ExpansionLimit {
            limit: 3,
            unit: "instrument zones"
        })
    ));
    assert_eq!(used, 3);

    let i = simple(
        vec![plain_preset()[0].clone(); 2],
        vec![(vec![(SAMPLE_ID, 0)], 0)],
        &[mono(0, 1000)],
    );
    assert_eq!((i.samples.len(), i.groups.len()), (1, 1));
    let z = zones(&i);
    assert_eq!(z.len(), 2);
    assert_ne!(z[0].id, z[1].id);
}

#[test]
fn tremolo_gain_is_centered_for_either_direction() {
    for amount in [100i16, -100] {
        let i = simple(
            plain_preset(),
            vec![(vec![(MOD_LFO_TO_VOLUME, signed(amount)), (SAMPLE_ID, 0)], 0)],
            &[mono(0, 1000)],
        );
        let route = i.groups[0]
            .mods
            .iter()
            .find(|m| m.name == "mod LFO volume")
            .unwrap();
        let ir::ModRoute {
            target: ir::ParamAddr::Volume,
            depth: intensity,
            law: ir::RouteLaw::Gain(curve),
            ..
        } = route.route
        else {
            panic!("expected a volume route");
        };
        let db = |x| zones(&i)[0].volume_db + 20.0 * curve.gain(intensity, x).log10();
        let peak = f32::from(amount) / 10.0;
        assert!((db(0.0) + peak).abs() < 1e-5);
        assert!(db(0.5).abs() < 1e-5);
        assert!((db(1.0) - peak).abs() < 1e-5);
    }
}

#[test]
fn zero_sample_rate_uses_a_practical_rate_and_reports_the_substitution() {
    let i = simple(
        plain_preset(),
        vec![(vec![(SAMPLE_ID, 0)], 0), (vec![(SAMPLE_ID, 1)], 0)],
        &[
            Sample {
                rate: 0,
                ..mono(0, 1000)
            },
            mono(0, 1000),
        ],
    );
    assert_eq!((i.samples[0].rate, i.samples[1].rate), (400, 44_100));
    assert_eq!(zones(&i).len(), 2);
    assert!(features(&i).contains(&"sample rate 0"));
}
