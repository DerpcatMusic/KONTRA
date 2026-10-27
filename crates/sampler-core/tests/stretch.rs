//! A Kontakt-style time-stretch group end to end: IR -> plan -> voice -> audio.
mod support;
use sampler_core::lower::{LowerError, lower};
use sampler_core::{Input, Limits, Pcm, Protocol, Runtime};
use sampler_ir as ir;

const RATE: usize = 48000;

fn input(key: u8) -> Input {
    Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key, external_id: None }
}

fn limits() -> Limits {
    Limits {
        notes: 16,
        channels: 1,
        performances: 1,
        families: 16,
        expressions: 16,
        voices: 32,
        decisions: 32,
        commands: 16,
        behaviors: 0,
        behavior_fuel: 0,
        behavior_cells: 0,
        note_cells: 0,
    }
}

fn no_behaviors(_: &[ir::Behavior], _: sampler_core::Prepared) -> Result<sampler_core::Prepared, LowerError> {
    unreachable!("no behaviors declared")
}

/// One second of a 441 Hz sine.
fn sine() -> Pcm {
    let frames: Vec<[f32; 2]> = (0..RATE)
        .map(|n| {
            let v = (std::f32::consts::TAU * 441. * n as f32 / RATE as f32).sin() * 0.5;
            [v, v]
        })
        .collect();
    Pcm::new(RATE as u32, frames.into_boxed_slice()).unwrap()
}

fn runtime(stretch: Option<ir::TimeStretch>, tune: f64) -> Runtime {
    let instrument = ir::Instrument {
        name: "stretch".into(),
        assets: vec![ir::Asset {
            location: ir::AssetLocation::Path("a".into()),
            residency: Default::default(),
            encoding: ir::Encoding::Wav,
            root_key: None,
            loops: Vec::new(),
        }],
        groups: vec![ir::Group {
            time_stretch: stretch,
            tune: ir::Pitch::Semitones(tune),
            ..Default::default()
        }],
        zones: vec![ir::Zone {
            keys: ir::KeyRange { low: 60, high: 60 },
            pitch: ir::KeyTracking::Fixed,
            group: Some(ir::GroupRef(0)),
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        ..ir::Instrument::default()
    };
    let plan = lower(&instrument, RATE as u32, vec![sine()], no_behaviors).unwrap();
    Runtime::new(plan, limits()).unwrap()
}

/// Hold one note and render until silence (or `max` seconds).
fn render(rt: &mut Runtime, max: usize) -> Vec<[f32; 2]> {
    rt.trigger(input(60), 60, 1.0).unwrap();
    let mut out = Vec::new();
    let mut block = [[0.; 2]; 480];
    while out.len() < max * RATE {
        rt.render(&mut block).unwrap();
        out.extend_from_slice(&block);
        if out.len() > RATE / 4 && block.iter().all(|f| f[0].abs() < 1e-6) {
            break;
        }
    }
    out
}

fn end_of_sound(x: &[[f32; 2]]) -> usize {
    x.iter().rposition(|f| f[0].abs() > 1e-3).map_or(0, |i| i + 1)
}

fn hertz(x: &[[f32; 2]]) -> f32 {
    let crossings = x.windows(2).filter(|w| (w[0][0] < 0.) != (w[1][0] < 0.)).count();
    crossings as f32 / 2. / (x.len() as f32 / RATE as f32)
}

fn stretch(speed: f32) -> Option<ir::TimeStretch> {
    Some(ir::TimeStretch::new(ir::StretchKind::TimeMachine2, speed))
}

#[test]
fn unity_speed_and_tune_reproduce_the_sampler() {
    let plain = render(&mut runtime(None, 0.), 2);
    let stretched = render(&mut runtime(stretch(1.), 0.), 2);
    let worst = (4800..40000)
        .map(|n| (plain[n][0] - stretched[n][0]).abs())
        .fold(0., f32::max);
    assert!(worst < 2e-3, "stretch at unity deviates by {worst}");
}

#[test]
fn half_speed_doubles_the_duration_at_the_same_pitch() {
    let out = render(&mut runtime(stretch(0.5), 0.), 4);
    let end = end_of_sound(&out);
    assert!((end as i64 - 2 * RATE as i64).abs() < 6000, "ended at {end}");
    let hz = hertz(&out[10000..end - 10000]);
    assert!((hz - 441.).abs() < 8., "pitch {hz}");
}

#[test]
fn an_octave_up_doubles_the_pitch_at_the_same_duration() {
    let out = render(&mut runtime(stretch(1.), 12.), 4);
    let end = end_of_sound(&out);
    assert!((end as i64 - RATE as i64).abs() < 6000, "ended at {end}");
    let hz = hertz(&out[10000..end - 10000]);
    assert!((hz - 882.).abs() < 14., "pitch {hz}");
    // The sampler plays the same octave in half the time.
    let plain = render(&mut runtime(None, 12.), 4);
    assert!(end_of_sound(&plain) < RATE * 6 / 10);
}

#[test]
fn stretch_voices_never_allocate_on_the_audio_thread() {
    let mut rt = runtime(stretch(0.7), 3.);
    let mut block = [[0.; 2]; 480];
    support::without_heap(|| {
        rt.trigger(input(60), 60, 1.0).unwrap();
        for _ in 0..100 {
            rt.render(&mut block).unwrap();
        }
    });
    assert!(block.iter().any(|f| f[0].abs() > 0.01));
}

fn instrument_with(stretch: Option<ir::TimeStretch>) -> ir::Instrument {
    ir::Instrument {
        name: "stretch".into(),
        assets: vec![ir::Asset {
            location: ir::AssetLocation::Path("a".into()),
            residency: Default::default(),
            encoding: ir::Encoding::Wav,
            root_key: None,
            loops: Vec::new(),
        }],
        groups: vec![ir::Group {
            time_stretch: stretch,
            tune: ir::Pitch::Semitones(5.),
            ..Default::default()
        }],
        zones: vec![ir::Zone {
            keys: ir::KeyRange { low: 60, high: 60 },
            pitch: ir::KeyTracking::Fixed,
            group: Some(ir::GroupRef(0)),
            ..ir::Zone::new(ir::AssetRef(0))
        }],
        ..ir::Instrument::default()
    }
}

#[test]
fn paged_assets_stretch_like_resident_ones() {
    use sampler_core::{PAGE_FRAMES, PageKey, PageStatus, PageUpdate, StreamCache};
    let data: Vec<[f32; 2]> = (0..RATE)
        .map(|n| {
            let v = (std::f32::consts::TAU * 441. * n as f32 / RATE as f32).sin() * 0.5;
            [v, v]
        })
        .collect();
    let pages = data.len().div_ceil(PAGE_FRAMES);
    let plan = lower(
        &instrument_with(stretch(0.8)),
        RATE as u32,
        vec![Pcm::new(RATE as u32, data.clone().into_boxed_slice()).unwrap()],
        no_behaviors,
    )
    .unwrap();
    let mut resident = Runtime::new(plan, limits()).unwrap();
    let asset = Pcm::streamed(RATE as u32, data.len()).unwrap();
    let (mut cache, mut worker) = StreamCache::new(pages).unwrap();
    for page in 0..pages {
        assert_eq!(cache.request(&asset, page, 0), Ok(PageStatus::Pending));
        let mut job = worker.next_job().unwrap();
        let range = job.range();
        job.frames_mut().copy_from_slice(&data[range]);
        worker.complete(job, Ok(())).unwrap();
        assert_eq!(
            cache.poll(),
            Some(PageUpdate::Loaded(PageKey { asset: asset.asset_id(), index: page }))
        );
    }
    let plan = lower(&instrument_with(stretch(0.8)), RATE as u32, vec![asset], no_behaviors).unwrap();
    let mut paged = Runtime::new(plan, limits()).unwrap().with_stream_cache(cache);
    let mut a = [[0.; 2]; 480];
    let mut b = [[0.; 2]; 480];
    support::without_heap(|| {
        resident.trigger(input(60), 60, 1.0).unwrap();
        paged.trigger(input(60), 60, 1.0).unwrap();
        for _ in 0..60 {
            resident.render(&mut a).unwrap();
            paged.render(&mut b).unwrap();
            for (x, y) in a.iter().zip(&b) {
                assert!((x[0] - y[0]).abs() < 1e-6, "paged differs from resident");
            }
        }
    });
    assert_eq!(paged.stream_underruns(), 0);
    assert!(b.iter().any(|f| f[0].abs() > 0.01));
}
