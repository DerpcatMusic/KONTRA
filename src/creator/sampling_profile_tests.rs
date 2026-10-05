use crate::{
    engine::{Ahdsr, Bank, Engine},
    import::{Group, Instrument, SamplingProfile, Zone},
};
use std::path::Path;

fn wav(path: &Path, rate: u32, frames: &[[f32; 2]]) {
    let mut out = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 2,
            sample_rate: rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )
    .unwrap();
    for frame in frames {
        for &value in frame {
            out.write_sample(value).unwrap();
        }
    }
    out.finalize().unwrap();
}

// Polynomial basis evaluated independently in double precision. The reference
// is a physical unity-rate sample, never the selected sampling kernel.
fn reference(source: &[[f32; 2]], kontakt: bool) -> Vec<[f32; 2]> {
    (0..2048)
        .map(|i| {
            let pos = i as f64 * 1.25;
            let base = pos.floor() as isize;
            let t = pos - base as f64;
            let nodes = [-1., 0., 1., 2.];
            let weights = if kontakt {
                std::array::from_fn(|j| {
                    (0..4)
                        .filter(|&k| k != j)
                        .map(|k| (t - nodes[k]) / (nodes[j] - nodes[k]))
                        .product::<f64>()
                })
            } else {
                let (t2, t3) = (t * t, t * t * t);
                [
                    (-t + 2. * t2 - t3) / 2.,
                    (2. - 5. * t2 + 3. * t3) / 2.,
                    (t + 4. * t2 - 3. * t3) / 2.,
                    (-t2 + t3) / 2.,
                ]
            };
            std::array::from_fn(|channel| {
                (0..4)
                    .map(|j| {
                        let index = base + j as isize - 1;
                        weights[j]
                            * source
                                .get(index as usize)
                                .map_or(0., |f| f64::from(f[channel]))
                    })
                    .sum::<f64>() as f32
            })
        })
        .collect()
}

fn render(instrument: &Instrument, sizes: &[usize]) -> [[f32; 2]; 384] {
    let bank = Bank::load(instrument)
        .unwrap()
        .with_prepared_settings(|settings| {
            for group in settings {
                group.envelope = Some(Ahdsr::UNITY);
            }
        })
        .unwrap();
    let mut engine = Engine::default();
    engine.reset(48000.);
    engine.attack = 0.;
    engine.set_bank(Some(Box::new(bank)));
    let mut pcm = [[0.; 2]; 384];
    assert_eq!(
        crate::plugin::tests::allocations(|| {
            engine.note_on(0, 60, 127);
            let (mut at, mut block) = (0, 0);
            while at < pcm.len() {
                let n = sizes[block % sizes.len()].min(pcm.len() - at);
                let (mut l, mut r) = ([0.; 128], [0.; 128]);
                engine.render_checked(&mut l[..n], &mut r[..n]).unwrap();
                for i in 0..n {
                    pcm[at + i] = [l[i], r[i]];
                }
                at += n;
                block += 1;
            }
        }),
        0
    );
    pcm
}

#[test]
fn saved_sampling_profile_migration_and_cold_ni_cache_select_independent_pcm() {
    let dir = tempfile::tempdir().unwrap();
    // Bound the importer's library metadata to this fixture, rather than
    // recording /tmp and other ancestors changed by unrelated logging.
    let samples_dir = dir.path().join("Samples");
    std::fs::create_dir(&samples_dir).unwrap();
    let cache_dir = tempfile::tempdir().unwrap();
    let cache = cache_dir.path();
    let source: Vec<_> = (0..4096)
        .map(|i| {
            [
                (((i * 17) % 71) as f32 - 35.) / 128.,
                (((i * 11) % 61) as f32 - 30.) / 128.,
            ]
        })
        .collect();
    wav(&samples_dir.join("source.wav"), 60000, &source);
    wav(
        &samples_dir.join("hermite.wav"),
        48000,
        &reference(&source, false),
    );
    wav(
        &samples_dir.join("lagrange.wav"),
        48000,
        &reference(&source, true),
    );
    let groups = vec![Group::default()];
    let zones = vec![Zone {
        sample: "Samples/source.wav".into(),
        ..Default::default()
    }];
    let legacy = serde_json::json!({"format":"kontra-instrument","version":1,
        "name":"Legacy", "groups":groups,"zones":zones,
        // Unknown old metadata never selected a native sampling kernel.
        "native_program_private":{"version":174,"data":[0,0,0,0,1,1,0,0,0,0]}});
    let legacy_path = dir.path().join("legacy.kontra-instrument");
    std::fs::write(&legacy_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let mut explicit = legacy.clone();
    explicit["sampling_profile"] = "Kontakt".into();
    let explicit_path = dir.path().join("explicit.kontra-instrument");
    std::fs::write(&explicit_path, serde_json::to_vec(&explicit).unwrap()).unwrap();
    let wire: super::Native = serde_json::from_value(explicit).unwrap();
    assert_eq!(
        serde_json::to_value(wire).unwrap()["sampling_profile"],
        "Kontakt"
    );

    // A real saved NI program with an opaque/invalid MIDI option prefix still
    // has the separately verified ordinary sampler source/quality identity.
    let mut raw = vec![0; 10];
    raw[4] = 1;
    raw[5] = 1;
    raw[..4].copy_from_slice(&3u32.to_le_bytes());
    let private = ni_file::kontakt::objects::ProgramDataPrivateParams {
        version: 0xae,
        data: raw,
    };
    let samples = [super::nki::SampleFile {
        relative: "Samples/source.wav".into(),
        rate: 60000,
        channels: 2,
        bits: 32,
        frames: 4096,
        modified: 0,
    }];
    let nki = dir.path().join("opaque-midi.nki");
    super::nki::write(
        &nki,
        &super::nki::Program {
            name: "Opaque MIDI",
            author: "Test",
            groups: &groups,
            zones: &zones,
            samples: &samples,
            script: None,
            native_private: Some(&private),
            default_key_switch: None,
        },
    )
    .unwrap();
    let legacy = super::read_native(&legacy_path).unwrap();
    let explicit = super::read_native(&explicit_path).unwrap();
    let imported = crate::import::read(&nki).unwrap();
    assert_eq!(legacy.sampling_profile, SamplingProfile::Compatibility);
    assert_eq!(explicit.sampling_profile, SamplingProfile::Kontakt);
    assert_eq!(imported.sampling_profile, SamplingProfile::Kontakt);
    assert!(imported
        .native_program_private
        .as_ref()
        .unwrap()
        .controller_options()
        .is_err());

    assert!(
        crate::cache::current(&imported.dependencies),
        "{:?}",
        imported.dependencies
    );
    crate::cache::store(cache, &nki, 0, &imported);
    assert!(
        crate::cache::current(&imported.dependencies),
        "cache writes must not change library dependencies"
    );
    assert_eq!(
        crate::cache::load(cache, &nki, 0)
            .unwrap()
            .sampling_profile,
        SamplingProfile::Kontakt
    );
    let entry = std::fs::read_dir(cache)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|x| x == "instrument"))
        .unwrap();
    let bytes = std::fs::read(&entry).unwrap();
    let body = bytes
        .strip_prefix(b"KONTRA instrument sampling-profile 1 ")
        .unwrap();
    let old = [b"KONTRA instrument ".as_slice(), body].concat();
    std::fs::write(&entry, old).unwrap();
    assert!(
        crate::cache::load(cache, &nki, 0).is_none(),
        "old cache must reimport source identity"
    );
    let cold =
        crate::cache::load(cache, &nki, 0).unwrap_or_else(|| crate::import::read(&nki).unwrap());
    assert_eq!(cold.sampling_profile, SamplingProfile::Kontakt);
    crate::cache::store(cache, &nki, 0, &cold);
    assert_eq!(
        crate::cache::load(cache, &nki, 0)
            .unwrap()
            .sampling_profile,
        SamplingProfile::Kontakt
    );

    let mut outputs = Vec::new();
    for (mut instrument, kontakt) in [(legacy, false), (explicit, true), (cold, true)] {
        let whole = render(&instrument, &[128]);
        let split = render(&instrument, &[1, 7, 31, 127]);
        instrument.zones[0].sample = samples_dir.join(if kontakt {
            "lagrange.wav"
        } else {
            "hermite.wav"
        });
        let oracle = render(&instrument, &[128]);
        for actual in [&whole, &split] {
            let error = actual
                .iter()
                .flatten()
                .zip(oracle.iter().flatten())
                .map(|(a, b)| (a - b).abs())
                .fold(0_f32, f32::max);
            assert!(
                error < 6e-6,
                "profile={:?}, source/reference error={error}",
                instrument.sampling_profile
            );
        }
        outputs.push(whole);
    }
    let difference = outputs[0]
        .iter()
        .flatten()
        .zip(outputs[1].iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0_f32, f32::max);
    assert!(
        difference > 0.001,
        "fractional source must distinguish explicit profile from legacy playback"
    );
}
