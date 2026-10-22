//! Kontakt Surround Panner record (`BParFXSurroundPanner`, module 0x1d, version 0x70): reader `0x140d02f60`.
//!
//! Reader order, little-endian: `i32 n`, then `n` x {`f32 x`, `f32 y`, `u8 disabled`} (speakers);
//! `i32 m`, then `m` x the same triple (sources); then `f32` at `+0x194`, `+0x198` (LFE volume, dB),
//! `+0x19c` (size), `+0x1a0` (divergence), `+0x1bc`, `+0x1c0`; `i32` gain mode (`+0x1c4`, 0..=2);
//! `i32` layout (`+0x1c8`, 0..=6); `u8` x3 (`+0x1a4..+0x1a6`); `f32` at `+0x1a8` (distance), `+0x1ac`
//! (azimuth), `+0x1b0`/`+0x1b4` (offset x/y); `i32` at `+0x1cc`. Field names come from
//! `docs/re/kontakt-bindings/fx_other.tsv`. Unnamed fields are read and dropped.
//!
//! Lowering (bus scope, stereo out): the apply (case 6, setter `0x1409685e0`) maps the record onto the
//! Panner object, and `sampler_core::surround_panner` runs the law. A record with two enabled speakers
//! and two enabled sources, gain mode 0 or 1 and layout 0 is a fixed 2x2 mix (the positions never move,
//! so the gains are constant): it lowers to a `StereoMatrix`. Everything else stays NotModeled: flagged
//! (disabled) outputs and the LFE route, more than two speakers, gain mode 2 (not measured), other layouts,
//! live (script-written) controls and voice or group inserts. No delay or multichannel bus is modelled.
//! Mapping and measurement: docs/re/engine/k-effects-batch-a.md Pass 10 and 11.
use super::{Lowering, Reader, Site, product};
use sampler_core::surround_panner::{GainMode, Panner, Point, Speaker as PannerSpeaker};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Speaker {
    pub x: f32,
    pub y: f32,
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Record {
    pub speakers: Vec<Speaker>,
    pub sources: Vec<Speaker>,
    /// `$ENGINE_PAR_SP_LFE_VOLUME`, dB.
    pub lfe_volume: f32,
    /// `$ENGINE_PAR_SP_SIZE`.
    pub size: f32,
    /// `$ENGINE_PAR_SP_DIVERGENCE`.
    pub divergence: f32,
    /// `$ENGINE_PAR_SP_OFFSET_DISTANCE`.
    pub distance: f32,
    /// `$ENGINE_PAR_SP_OFFSET_AZIMUTH`.
    pub azimuth: f32,
    /// `$ENGINE_PAR_SP_OFFSET_X`, `$ENGINE_PAR_SP_OFFSET_Y`.
    pub offset: [f32; 2],
    /// Gain-mode selector at `+0x1c4` (0..=2).
    pub mode: i32,
    /// Layout selector at `+0x1c8` (0..=6).
    pub layout: i32,
}

fn speaker_list(r: &mut Reader) -> Option<Vec<Speaker>> {
    let n = usize::try_from(r.i32()?).ok()?;
    // Each entry is 9 bytes; a count the record cannot hold is rejected before allocating.
    (n <= r.0.len() / 9).then_some(())?;
    (0..n)
        .map(|_| {
            Some(Speaker {
                x: r.f32()?,
                y: r.f32()?,
                disabled: r.flag()?,
            })
        })
        .collect()
}

/// The Panner the case-6 apply leaves for a record (kemu-measured; docs/re/engine/k-effects-batch-a.md
/// Pass 10): scale `1.1 - SIZE`, exponent `2 * DIVERGENCE`, offsets `clamp(0.5 * v, -1, 1)`, radius
/// `clamp(0.5 * DISTANCE, 0, 1)`, angle `clamp(AZIMUTH, -1, 1)`. `None` outside the stereo-out subset.
pub(super) fn stereo_panner(rec: &Record) -> Option<Panner> {
    let mode = match rec.mode {
        0 => GainMode::Power,
        1 => GainMode::Cosine,
        _ => return None,
    };
    let enabled = |s: &Speaker| !s.disabled;
    if rec.speakers.len() != 2
        || rec.sources.len() != 2
        || rec.layout != 0
        || !rec.speakers.iter().all(enabled)
        || !rec.sources.iter().all(enabled)
    {
        return None;
    }
    let point = |s: &Speaker| Point { x: s.x, y: s.y };
    let mut panner = Panner::new(
        rec.speakers
            .iter()
            .map(|s| PannerSpeaker { pos: point(s), disabled: false })
            .collect(),
        rec.sources.iter().map(point).collect(),
        mode,
    );
    panner.scale = 1.1 - rec.size;
    panner.exponent = 2.0 * rec.divergence;
    panner.offset = Point {
        x: (0.5 * rec.offset[0]).clamp(-1.0, 1.0),
        y: (0.5 * rec.offset[1]).clamp(-1.0, 1.0),
    };
    panner.radius_offset = (0.5 * rec.distance).clamp(0.0, 1.0);
    panner.angle_offset = rec.azimuth.clamp(-1.0, 1.0);
    Some(panner)
}

/// The static stereo mix of a record: rows are output L/R, columns input L/R (`Processor::StereoMatrix`).
/// The sources are the bus channels: source 0 is left, source 1 right.
pub(super) fn stereo_mix(rec: &Record) -> Option<[[f64; 2]; 2]> {
    let panner = stereo_panner(rec)?;
    let gain = |src: usize, out: usize| f64::from(panner.gain(src, out));
    Some([[gain(0, 0), gain(1, 0)], [gain(0, 1), gain(1, 1)]])
}

/// Bus scope, stereo out: the fixed mix folds into the chain's stereo matrix like the other linear
/// modules. Live controls (a script writing the slot) are not modelled.
pub(super) fn slot_surround_panner(b: &mut Lowering, s: &Site, rec: &Record) -> bool {
    let Some(mix) = stereo_mix(rec) else {
        return false;
    };
    if s.dynamic.is_some() {
        return false;
    }
    b.combined = product(product(s.gain, mix), b.combined);
    true
}

/// Reads one record. The whole payload must be consumed: trailing bytes mean this is not the 0x70 layout.
pub(super) fn read(r: &mut Reader) -> Option<Record> {
    let speakers = speaker_list(r)?;
    let sources = speaker_list(r)?;
    let _ = r.f32()?;
    let lfe_volume = r.f32()?;
    let size = r.f32()?;
    let divergence = r.f32()?;
    let _ = (r.f32()?, r.f32()?);
    let mode = r.i32()?;
    let layout = r.i32()?;
    let _ = (r.flag()?, r.flag()?, r.flag()?);
    let distance = r.f32()?;
    let azimuth = r.f32()?;
    let offset = [r.f32()?, r.f32()?];
    let _ = r.i32()?;
    r.0.is_empty().then_some(())?;
    Some(Record {
        speakers,
        sources,
        lfe_volume,
        size,
        divergence,
        distance,
        azimuth,
        offset,
        mode,
        layout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reader order is the byte order of a saved record: speakers, sources, floats, ints, flags, floats.
    fn record_bytes() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(2i32.to_le_bytes());
        for (x, y, d) in [(0.1f32, 0.9f32, 0u8), (0.9, 0.9, 1)] {
            b.extend(x.to_le_bytes());
            b.extend(y.to_le_bytes());
            b.push(d);
        }
        b.extend(1i32.to_le_bytes());
        b.extend(0.5f32.to_le_bytes());
        b.extend(0.5f32.to_le_bytes());
        b.push(0);
        // 0x194, LFE, size, divergence, 0x1bc, 0x1c0
        for v in [7.0f32, -6.0, 0.3, 0.25, 8.0, 9.0] {
            b.extend(v.to_le_bytes());
        }
        b.extend(1i32.to_le_bytes()); // mode
        b.extend(3i32.to_le_bytes()); // layout
        b.extend([1u8, 0, 1]);
        for v in [0.4f32, -0.2, 0.6, -0.8] {
            b.extend(v.to_le_bytes());
        }
        b.extend(5i32.to_le_bytes());
        b
    }

    #[test]
    fn saved_record_reads_in_reader_order() {
        let bytes = record_bytes();
        let rec = read(&mut Reader(&bytes)).expect("record parses");
        assert_eq!(
            rec.speakers,
            vec![
                Speaker {
                    x: 0.1,
                    y: 0.9,
                    disabled: false
                },
                Speaker {
                    x: 0.9,
                    y: 0.9,
                    disabled: true
                }
            ]
        );
        assert_eq!(
            rec.sources,
            vec![Speaker {
                x: 0.5,
                y: 0.5,
                disabled: false
            }]
        );
        assert_eq!(
            (rec.lfe_volume, rec.size, rec.divergence),
            (-6.0, 0.3, 0.25)
        );
        assert_eq!(
            (rec.distance, rec.azimuth, rec.offset),
            (0.4, -0.2, [0.6, -0.8])
        );
        assert_eq!((rec.mode, rec.layout), (1, 3));
    }

    #[test]
    fn trailing_or_short_payload_is_rejected() {
        let mut bytes = record_bytes();
        bytes.push(0);
        assert!(read(&mut Reader(&bytes)).is_none());
        let short = record_bytes();
        assert!(read(&mut Reader(&short[..short.len() - 1])).is_none());
    }

    #[test]
    fn impossible_speaker_count_is_rejected_before_allocating() {
        let bytes = i32::MAX.to_le_bytes();
        assert!(read(&mut Reader(&bytes)).is_none());
    }

    /// The kemu base record (`BASE` in tools/re/surround_record_kemu.py) as saved bytes: 2 speakers,
    /// 2 sources, SIZE 0.5, DIVERGENCE 0.5, no offsets, gain mode 0, layout 0.
    fn base_record_bytes() -> Vec<u8> {
        let mut b = Vec::new();
        b.extend(2i32.to_le_bytes());
        for (x, y) in [(0.1f32, 0.9f32), (0.9, 0.9)] {
            b.extend(x.to_le_bytes());
            b.extend(y.to_le_bytes());
            b.push(0);
        }
        b.extend(2i32.to_le_bytes());
        for (x, y) in [(0.8f32, 0.3f32), (0.3, 0.6)] {
            b.extend(x.to_le_bytes());
            b.extend(y.to_le_bytes());
            b.push(0);
        }
        for v in [0.0f32, 0.0, 0.5, 0.5, 0.0, 0.0] {
            b.extend(v.to_le_bytes());
        }
        b.extend(0i32.to_le_bytes()); // mode
        b.extend(0i32.to_le_bytes()); // layout
        b.extend([0u8, 0, 0]);
        for v in [0.0f32, 0.0, 0.0, 0.0] {
            b.extend(v.to_le_bytes());
        }
        b.extend(0i32.to_le_bytes());
        b
    }

    /// Applies a `StereoMatrix` (rows output L/R, columns input L/R) to one block.
    fn apply_mix(m: [[f64; 2]; 2], input: &[Vec<f32>]) -> Vec<Vec<f32>> {
        (0..2)
            .map(|k| {
                (0..input[0].len())
                    .map(|i| (m[k][0] * f64::from(input[0][i]) + m[k][1] * f64::from(input[1][i])) as f32)
                    .collect()
            })
            .collect()
    }

    fn assert_block(name: &str, got: &[Vec<f32>], want: &[Vec<f32>]) {
        for (k, (g, w)) in got.iter().zip(want).enumerate() {
            for (i, (a, b)) in g.iter().zip(w).enumerate() {
                assert!((a - b).abs() <= 1e-5, "{name} output {k} sample {i}: got {a}, native {b}");
            }
        }
    }

    /// Bus scope, saved record -> one StereoMatrix, and its block equals the kemu native block 0.
    #[test]
    fn saved_stereo_record_lowers_to_the_native_block() {
        use crate::effects::{Scope, chain, test_support::slot};
        let mut fx = slot(0x1d, base_record_bytes(), 1.0);
        fx.version = 0x70;
        let built = chain(&[fx], Scope::Bus);
        assert!(built.notes.is_empty(), "{:?}", built.notes);
        let [sampler_ir::Processor::StereoMatrix(m)] = built.processors.as_slice() else {
            panic!("expected one StereoMatrix, got {:?}", built.processors);
        };
        let native = native_case("base");
        let block = &native["blocks"][0];
        assert_block("base", &apply_mix(*m, &native_matrix_input(block)), &native_outs(block));
    }

    /// Every fixture case that stays in the stereo subset: the record's fields map to the native
    /// parameters (`scale`, `exponent`, offsets, radius, angle), and the mix reproduces block 0.
    #[test]
    fn every_native_stereo_case_matches_its_block() {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        let mut checked = 0;
        for case in fixture["cases"].as_array().unwrap() {
            let rec = record_from_fixture(&case["rec"]);
            let Some(panner) = stereo_panner(&rec) else {
                continue;
            };
            let block = &case["blocks"][0];
            let name = case["name"].as_str().unwrap();
            let close = |a: f32, b: f64| (f64::from(a) - b).abs() <= 1e-6;
            assert!(close(panner.scale, block["scale"].as_f64().unwrap()), "{name} scale");
            assert!(close(panner.exponent, block["exponent"].as_f64().unwrap()), "{name} exponent");
            assert!(close(panner.radius_offset, block["radius_offset"].as_f64().unwrap()), "{name} radius");
            assert!(close(panner.angle_offset, block["angle_offset"].as_f64().unwrap()), "{name} angle");
            let m = stereo_mix(&rec).unwrap();
            assert_block(name, &apply_mix(m, &native_matrix_input(block)), &native_outs(block));
            checked += 1;
        }
        assert!(checked >= 6, "only {checked} stereo cases admitted");
    }

    const FIXTURE: &str = include_str!("../../../sampler-core/src/dsp/surround_record_native_vectors.json");

    fn native_case(name: &str) -> serde_json::Value {
        let fixture: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
        fixture["cases"].as_array().unwrap().iter().find(|c| c["name"] == name).unwrap().clone()
    }

    fn native_matrix_input(block: &serde_json::Value) -> Vec<Vec<f32>> {
        block["input"].as_array().unwrap().iter().map(|ch| floats(ch)).collect()
    }

    fn native_outs(block: &serde_json::Value) -> Vec<Vec<f32>> {
        block["out"].as_array().unwrap().iter().map(|ch| floats(ch)).collect()
    }

    fn floats(v: &serde_json::Value) -> Vec<f32> {
        v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect()
    }

    fn record_from_fixture(rec: &serde_json::Value) -> Record {
        let spk = |v: &serde_json::Value| -> Vec<Speaker> {
            v.as_array().unwrap().iter().map(|e| Speaker {
                x: e[0].as_f64().unwrap() as f32,
                y: e[1].as_f64().unwrap() as f32,
                disabled: e[2].as_i64().unwrap() != 0,
            }).collect()
        };
        let f = |k: &str| rec[k].as_f64().unwrap() as f32;
        Record {
            speakers: spk(&rec["speakers"]),
            sources: spk(&rec["sources"]),
            lfe_volume: f("lfe"),
            size: f("size"),
            divergence: f("div"),
            distance: f("dist"),
            azimuth: f("az"),
            offset: [f("ox"), f("oy")],
            mode: rec["mode"].as_i64().unwrap() as i32,
            layout: rec["layout"].as_i64().unwrap() as i32,
        }
    }

    /// A disabled speaker is outside the stereo subset: named NotModeled, audio passes through.
    #[test]
    fn disabled_speaker_is_named_not_modelled() {
        use crate::effects::{Scope, chain, test_support::slot};
        let mut fx = slot(0x1d, record_bytes(), 1.0);
        fx.version = 0x70;
        let built = chain(&[fx], Scope::Bus);
        assert!(built.processors.is_empty(), "{:?}", built.processors);
        assert!(
            built
                .notes
                .iter()
                .any(|(_, _, t, r)| t.contains("Surround Panner")
                    && *r == sampler_ir::Reason::NotModeled),
            "{:?}",
            built.notes
        );
    }
}
