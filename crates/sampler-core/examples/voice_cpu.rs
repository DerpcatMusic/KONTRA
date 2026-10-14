//! Optimized audio deadline/core-time audit. Synthetic *sounding* PCM voices;
//! preloaded-page cases exercise production streaming reads, not storage latency.
//! Linux CPU clocks include all render workers in process CPU, and just the
//! caller in thread CPU. Clock probes and render's own telemetry are included.
#![allow(unsafe_code)]
use sampler_core::*;
use serde_json::json;
use std::{hint::black_box, time::Instant};

#[cfg(target_os = "linux")]
fn cpu_nanos(process: bool) -> u64 {
    #[repr(C)]
    struct Timespec {
        seconds: i64,
        nanos: i64,
    }
    unsafe extern "C" {
        fn clock_gettime(clock: i32, result: *mut Timespec) -> i32;
    }
    let mut time = Timespec {
        seconds: 0,
        nanos: 0,
    };
    // Only benchmark instrumentation uses FFI. IDs are Linux process/thread clocks.
    assert_eq!(
        unsafe { clock_gettime(if process { 2 } else { 3 }, &mut time) },
        0
    );
    time.seconds as u64 * 1_000_000_000 + time.nanos as u64
}
#[cfg(not(target_os = "linux"))]
fn cpu_nanos(_: bool) -> u64 {
    0
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let arg = |name: &str, default: &str| {
        args.windows(2)
            .find(|pair| pair[0] == name)
            .map_or(default.to_owned(), |pair| pair[1].clone())
    };
    let workload = arg("--workload", "plain");
    let residency = arg("--residency", "resident");
    let sources = arg("--sources", "shared");
    let voices: usize = arg("--voices", "100").parse().unwrap();
    let threads: usize = arg("--threads", "1").parse().unwrap();
    let block: usize = arg("--block", "256").parse().unwrap();
    let blocks: usize = arg("--blocks", "10000").parse().unwrap();
    let rate: u32 = arg("--rate", "48000").parse().unwrap();
    assert!(matches!(
        workload.as_str(),
        "plain" | "fractional" | "filtered" | "modulated" | "eq-stereo"
    ));
    assert!(matches!(residency.as_str(), "resident" | "pages"));
    assert!(matches!(sources.as_str(), "shared" | "distinct" | "phase"));
    assert!(voices <= 4096 && blocks > 1 && block > 0);
    let (mut rt, _decoder, voice_state_bytes) = fixture(
        rate,
        &workload,
        residency == "pages",
        voices.max(1),
        threads,
        &sources,
    );
    let mut output = vec![[0.; 2]; block];
    // Independent physical note owners; one source per note. Gain differences
    // make accidental dropped voices change the fingerprint.
    for id in 0..voices {
        rt.trigger(
            Input {
                protocol: Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: Some(id as i32),
            },
            60,
            velocity(id, voices),
        )
        .unwrap();
    }
    for _ in 0..512 {
        rt.render(black_box(&mut output)).unwrap();
    }
    assert_eq!(rt.voice_count(), voices);
    let mut wall = Vec::with_capacity(blocks);
    let mut caller_cpu = Vec::with_capacity(blocks);
    let mut total_cpu = Vec::with_capacity(blocks);
    let mut energy = 0_f64;
    let mut fingerprint = 0xcbf2_9ce4_8422_2325_u64;
    for _ in 0..blocks {
        let total_start = cpu_nanos(true);
        let caller_start = cpu_nanos(false);
        let start = Instant::now();
        rt.render(black_box(&mut output)).unwrap();
        wall.push(start.elapsed().as_nanos() as u64);
        caller_cpu.push(cpu_nanos(false) - caller_start);
        total_cpu.push(cpu_nanos(true) - total_start);
        for frame in black_box(&output) {
            for sample in frame {
                energy += f64::from(*sample).powi(2);
                fingerprint =
                    (fingerprint ^ u64::from(sample.to_bits())).wrapping_mul(0x100_0000_01b3);
            }
        }
    }
    let budget = 1e9 * block as f64 / f64::from(rate);
    let metric = |mut times: Vec<u64>| {
        times.sort_unstable();
        let percent = |value: u64| value as f64 / budget * 100.;
        json!({
            "p50_us": times[blocks / 2] as f64 / 1000.,
            "p99_us": times[blocks * 99 / 100] as f64 / 1000.,
            "p999_us": times[(blocks * 999 / 1000).min(blocks - 1)] as f64 / 1000.,
            "max_us": times[blocks - 1] as f64 / 1000.,
            "p50_percent_one_core": percent(times[blocks / 2]),
            "p99_percent_one_core": percent(times[blocks * 99 / 100]),
            "mean_percent_one_core": times.iter().map(|n| *n as f64).sum::<f64>() / blocks as f64 / budget * 100.,
            "deadline_misses": times.iter().filter(|n| **n as f64 > budget).count(),
        })
    };
    let stats = rt.stats();
    assert_eq!(stats.voices, voices);
    assert_eq!(stats.stream_underruns, 0);
    assert_eq!(stats.nonfinite_frames, 0);
    assert_eq!(energy > 0., voices > 0);
    println!(
        "{}",
        json!({
            "workload": workload, "residency": residency, "voices": voices,
            "sources": sources, "prepared_assets": if sources == "distinct" { voices.max(1) } else { 1 },
            "prepared_voice_state_bytes": voice_state_bytes,
            "threads": threads, "parallel_chunks": rt.parallel_blocks(),
            "sample_rate": rate, "block": block, "blocks": blocks,
            "wall": metric(wall), "audio_thread_cpu": metric(caller_cpu),
            "all_workers_cpu": metric(total_cpu), "energy": energy,
            "pcm_fingerprint": format!("{fingerprint:016x}"),
            "stream_underruns": stats.stream_underruns, "voice_drops": stats.voice_drops,
            "nonfinite_frames": stats.nonfinite_frames,
        })
    );
}

fn velocity(id: usize, voices: usize) -> f64 {
    0.5 + id as f64 / if voices <= 256 { 512. } else { voices as f64 * 2. }
}

fn fixture(
    rate: u32,
    workload: &str,
    pages: bool,
    capacity: usize,
    threads: usize,
    sources: &str,
) -> (Runtime, StreamWorker, usize) {
    let mut random = 0x1234_5678_u32;
    let data: Box<[Frame]> = (0..PAGE_FRAMES * 4)
        .map(|i| {
            random ^= random << 13;
            random ^= random >> 17;
            random ^= random << 5;
            let sine = (i as f64 * std::f64::consts::TAU / 512.).sin() as f32;
            let noise = (random as f64 / u32::MAX as f64 - 0.5) as f32 * 0.05;
            [sine * 0.25 + noise, sine * 0.18 - noise]
        })
        .collect();
    let asset_count = if sources == "distinct" { capacity } else { 1 };
    let mut assets = Vec::with_capacity(asset_count);
    // Admission reserves each voice's maximum demand independently, even when
    // all voices share pages. Size for that contract instead of silently stealing.
    let (mut cache, mut worker) = StreamCache::new(4 * capacity).unwrap();
    for asset in 0..asset_count {
        // Distinct immutable source bodies, not aliases of one shared snapshot.
        let body: Box<[Frame]> = if asset == 0 { data.clone() } else {
            data.iter().enumerate().map(|(i, frame)| {
                let extra = (i as f32 * (0.007 + asset as f32 * 0.00013)).sin() * 0.01;
                [frame[0] + extra, frame[1] - extra]
            }).collect()
        };
        let pcm = if pages { Pcm::streamed(rate, body.len()).unwrap() } else { Pcm::new(rate, body.clone()).unwrap() };
        if pages {
            for page in 0..4 {
                assert_eq!(cache.request(&pcm, page, 0), Ok(PageStatus::Pending));
                let mut job = worker.next_job().unwrap();
                let range = job.range();
                job.frames_mut().copy_from_slice(&body[range]);
                worker.complete(job, Ok(())).unwrap();
                assert!(matches!(cache.poll(), Some(PageUpdate::Loaded(_))));
            }
        }
        assets.push(pcm);
    }
    let playback = Playback {
        transpose_semitones: if workload == "plain" { 0. } else { 3.17 },
        loop_range: Some(Loop {
            start: 0,
            end: data.len(),
            passes: None,
            shape: LoopShape::Wrap,
            mode: LoopMode::Continuous,
        }),
        ..Playback::default()
    };
    let region_count = if sources == "shared" { 1 } else { capacity };
    let regions = (0..region_count).map(|id| {
        let mut playback = playback;
        if sources == "phase" { playback.transpose_semitones += id as f64 * 0.001; }
        let half_width = if capacity <= 256 { 1. / 2048. } else { 0.25 / capacity as f64 };
        Region {
            sample: if sources == "distinct" { id } else { 0 },
            key_low: 60, key_high: 60, root_key: None,
            velocity_low: if sources == "shared" { 0. } else { velocity(id, capacity) - half_width },
            velocity_high: if sources == "shared" { 1. } else { velocity(id, capacity) + half_width },
            gain: 0.008,
            envelope: Envelope::new(256, 0, 1024, 0.8, 4096).unwrap(),
            playback,
        }
    }).collect();
    let mut plan = Prepared::new(
        rate,
        assets,
        regions,
        capacity,
    )
    .unwrap();
    if matches!(workload, "filtered" | "modulated") {
        plan = plan
            .with_voice_chains(
                vec![
                    VoiceChain::new(
                        vec![],
                        vec![Processor::StateVariable(StateVariableFilter {
                            mode: SvfMode::LowPass,
                            cutoff_hz: Parameter::Constant(3000.),
                            q: Parameter::Constant(0.7),
                        })],
                        128,
                    )
                    .unwrap(),
                ],
                vec![Some(0); region_count],
            )
            .unwrap();
    }
    if workload == "eq-stereo" {
        let mut stages = Vec::new();
        for (frequency, gain) in [(0.3, 6.), (0.55, -4.), (0.7, 3.)] {
            let mut eq = EqBand::rbj(sampler_ir::fx::eq::Kind::Peak, frequency, 0.5, gain);
            eq.law = sampler_ir::fx::eq::Law::KontaktLegacy;
            stages.push(Processor::Eq(eq));
        }
        stages.push(Processor::Color(ColorSettings {
            stereo: Some(StereoSettings {
                width: Parameter::Constant(0.7),
                pan: Parameter::Constant(0.2),
                pseudo: false,
            }),
            ..ColorSettings::IDENTITY
        }));
        plan = plan
            .with_voice_chains(
                vec![VoiceChain::new(vec![], stages, 128).unwrap()],
                vec![Some(0); region_count],
            )
            .unwrap();
    }
    if workload == "modulated" {
        let program = ModProgram {
            controls: vec![],
            sources: vec![
                ModSource::Envelope(Envelope::new(300, 0, 200, 0.6, 400).unwrap()),
                ModSource::Lfo(Lfo {
                    shape: LfoShape::Sine,
                    rate: LfoRate::Hertz(5.),
                    phase: 0.,
                    delay: 0,
                    fade: 0,
                    retrigger: true,
                    shared: false,
                }),
            ],
            routes: vec![
                ModRoute::new(0, ModTarget::Attenuate, 0.5),
                ModRoute::new(1, ModTarget::Pitch, 0.4),
                ModRoute::new(0, ModTarget::Cutoff, 12.),
                ModRoute::new(1, ModTarget::Pan, 0.5),
            ],
            shapes: vec![],
            breakpoints: vec![],
            envelope_controls: vec![],
        };
        plan = plan
            .with_voice_modulation(vec![program], vec![Some(0); region_count], vec![0; region_count])
            .unwrap();
    }
    let voice_state_bytes = plan.voice_state_bytes();
    let mut rt = Runtime::new(
        plan,
        Limits {
            notes: capacity,
            channels: 0,
            performances: 1,
            families: capacity,
            decisions: 0,
            expressions: capacity,
            voices: capacity,
            commands: 0,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap()
    .with_threads(Threads::Fixed(threads));
    if pages {
        rt = rt.with_stream_cache(cache);
    }
    // Hold the decoder endpoint: disconnected-stream refusal is a different
    // workload. Every page is already resident, so no job is serviced in timing.
    (rt, worker, voice_state_bytes)
}
