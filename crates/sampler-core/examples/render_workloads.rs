//! Resident rendering workload; optional `--transpose SEMITONES` tests the filtered path,
//! `--pitched` the resampled path at high polyphony (transposed and 44.1 kHz sources,
//! with and without four voice filters; `--pitched --mips` uses mipmapped sources). Setup, validation and sorting are untimed.
//! Run in release mode; this reports local measurements, not a realtime guarantee.
use sampler_core::{
    Envelope, Input, Limits, Loop, LoopMode, Pcm, Playback, Prepared, Protocol, Region, Runtime,
};
use std::{hint::black_box, time::Instant};

const LAYERS: usize = 4;
const TRIALS: usize = 512;
const CUTOFF: sampler_core::ControlId = sampler_core::ControlId(1);

#[derive(Clone, Copy, Default)]
struct Processing {
    transpose: f64,
    /// Source rate; zero uses the output rate.
    source_rate: u32,
    filters: usize,
    bus: bool,
    automated: bool,
    mips: bool,
    /// Plain sources whose levels `service_mipmaps` builds after warmup.
    lazy: bool,
}

fn prepare(
    rate: u32,
    voices: usize,
    reserved: usize,
    shaped: bool,
    processing: Processing,
    muted: bool,
) -> (Runtime, Vec<Pcm>) {
    let Processing {
        transpose,
        source_rate,
        filters,
        bus,
        automated,
        mips,
        ..
    } = processing;
    let source_rate = if source_rate == 0 { rate } else { source_rate };
    let notes = voices / LAYERS;
    let samples = (0..LAYERS)
        .map(|layer| {
            let value = (layer + 1) as f32 / 4096.;
            let frames = vec![[value, -value]; 4096].into_boxed_slice();
            if mips {
                Pcm::mipmapped(source_rate, frames).unwrap()
            } else {
                Pcm::new(source_rate, frames).unwrap()
            }
        })
        .collect::<Vec<_>>();
    let kept = samples.clone();
    let regions = (0..LAYERS)
        .map(|sample| Region {
            sample,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain: 1.,
            envelope: if shaped {
                Envelope::new(4, 2, 8, 0.5, 8).unwrap()
            } else {
                Envelope::default()
            },
            playback: Playback {
                transpose_semitones: transpose,
                loop_range: Some(Loop {
                    passes: None,
                    start: 0,
                    end: 4096,
                    shape: sampler_core::LoopShape::Wrap,
                    mode: LoopMode::Continuous,
                }),
                ..Playback::default()
            },
        })
        .collect();
    let mut prepared = Prepared::new(rate, samples, regions, LAYERS).unwrap();
    if filters != 0 {
        let filter = if automated {
            prepared = prepared
                .with_controls(vec![sampler_core::ControlDefinition {
                    id: CUTOFF,
                    domain: sampler_core::ControlDomain::Real { min: 0., max: 1. },
                    default: sampler_core::ControlValue::Real(0.),
                }])
                .unwrap();
            sampler_core::Processor::StateVariable(sampler_core::StateVariableFilter {
                mode: sampler_core::SvfMode::LowPass,
                cutoff_hz: sampler_core::Parameter::Control(sampler_core::ControlRange {
                    control: CUTOFF,
                    low: f64::from(rate) / 16.,
                    high: f64::from(rate) / 3.,
                    ramp_frames: 256,
                }),
                q: sampler_core::Parameter::Constant(0.5),
            })
        } else {
            sampler_core::Processor::Biquad(
                sampler_core::Biquad::new(
                    rate,
                    sampler_core::FilterKind::LowPass,
                    f64::from(rate) / 4.,
                    0.5,
                )
                .unwrap(),
            )
        };
        prepared = if bus {
            prepared
                .with_buses(
                    vec![sampler_core::Bus {
                        processors: vec![filter; filters],
                        sends: vec![sampler_core::BusSend {
                            bus: None,
                            gain: 1.,
                        }],
                        tail_frames: 128,
                    }],
                    vec![Some(0); LAYERS],
                )
                .unwrap()
        } else {
            prepared
                .with_voice_chains(
                    vec![
                        sampler_core::VoiceChain::new(vec![filter; filters], vec![], 128).unwrap(),
                    ],
                    vec![Some(0); LAYERS],
                )
                .unwrap()
        };
    }
    let mut rt = Runtime::new(
        prepared,
        Limits {
            notes,
            channels: 0,
            performances: 1,
            families: notes,
            decisions: 0,
            expressions: notes,
            voices: reserved,
            commands: 0,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
            note_cells: 0,
        },
    )
    .unwrap();
    if let Some(n) = std::env::var("THREADS").ok().and_then(|n| n.parse().ok()) {
        rt.set_threads(sampler_core::Threads::Fixed(n));
    }
    for id in 0..notes {
        rt.trigger_with_expression(
            Input {
                protocol: Protocol::Native,
                port: 0,
                group: 0,
                channel: 0,
                key: 60,
                external_id: Some(id as i32),
            },
            60,
            1.,
            sampler_core::Expression {
                gain: if muted { 0.0 } else { 1.0 },
                ..sampler_core::Expression::default()
            },
        )
        .unwrap();
    }
    assert_eq!(rt.voice_count(), voices);
    (rt, kept)
}

/// CPU time of every thread of this process, in ms (scheduler nanoseconds).
fn process_cpu_ms() -> f64 {
    let Ok(tasks) = std::fs::read_dir("/proc/self/task") else {
        return 0.;
    };
    tasks
        .flatten()
        .filter_map(|t| std::fs::read_to_string(t.path().join("schedstat")).ok())
        .filter_map(|s| s.split_whitespace().next()?.parse::<f64>().ok())
        .sum::<f64>()
        / 1e6
}

/// `--idle THREADS NOTES`: render 64-frame blocks at real-time pace for four
/// seconds and report the CPU the process spent, as a share of one core.
fn idle(threads: usize, notes: usize) {
    let (mut rt, _samples) = prepare(
        48000,
        notes.max(1) * LAYERS,
        1024,
        false,
        Processing { filters: 1, automated: true, ..Processing::default() },
        false,
    );
    rt.set_threads(sampler_core::Threads::Fixed(threads));
    if notes == 0 {
        rt.panic();
    }
    let mut audio = vec![[0.; 2]; 64];
    let period = std::time::Duration::from_micros(64 * 1_000_000 / 48000);
    let (cpu, start) = (process_cpu_ms(), Instant::now());
    let mut next = start;
    while start.elapsed().as_secs() < 4 {
        rt.render(&mut audio).unwrap();
        next += period;
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    let wall = start.elapsed().as_secs_f64() * 1000.;
    println!(
        "idle threads {threads} notes {notes}: {:.2}% of one core",
        (process_cpu_ms() - cpu) / wall * 100.
    );
}

fn measure(
    rate: u32,
    block: usize,
    voices: usize,
    reserved: usize,
    shaped: bool,
    processing: Processing,
    muted: bool,
) {
    let (mut rt, samples) = prepare(rate, voices, reserved, shaped, processing, muted);
    let mut audio = vec![[0.; 2]; block];
    for _ in 0..64 {
        rt.render(&mut audio).unwrap();
    }
    if processing.lazy {
        let held = sampler_core::service_mipmaps(&samples, usize::MAX, 0);
        eprintln!("lazy octave levels: {held} bytes");
    }
    let resident: usize = samples.iter().map(Pcm::resident_bytes).sum();
    eprintln!("resident PCM: {resident} bytes");
    let mut times = [0u128; TRIALS];
    let resampled =
        processing.transpose != 0. || processing.source_rate != 0 && processing.source_rate != rate;
    let expected = (voices / LAYERS) as f32 * 10. / 4096. * if shaped { 0.5 } else { 1. };
    let expected = if muted {
        [0.0; 2]
    } else {
        [expected, -expected]
    };
    let cpu_before = process_cpu_ms();
    for (iteration, elapsed) in times.iter_mut().enumerate() {
        if processing.automated {
            rt.edit_controls(
                rt.active_plan(),
                None,
                &[sampler_core::ControlWrite {
                    id: CUTOFF,
                    value: sampler_core::ControlValue::Real((iteration % 2) as f64),
                }],
            )
            .unwrap();
        }
        let begin = Instant::now();
        black_box(&mut rt).render(black_box(&mut audio)).unwrap();
        *elapsed = begin.elapsed().as_nanos();
        // Binary fractions make this a bit-exact independent mixed-output oracle.
        // A resampled DC source is exact only to the interpolator's rounding.
        assert!(if resampled {
            audio.iter().all(|frame| {
                (0..2).all(|c| (frame[c] - expected[c]).abs() <= expected[c].abs() * 1e-4)
            })
        } else {
            audio
                .iter()
                .all(|frame| frame.map(f32::to_bits) == expected.map(f32::to_bits))
        });
    }
    // Thread CPU over all threads (workers included), per voice and block.
    let cpu = process_cpu_ms() - cpu_before;
    eprintln!(
        "threads {}: {cpu:.0} ms cpu over {TRIALS} blocks = {:.1} us/block, {:.1} ns/voice-block",
        rt.threads(),
        cpu * 1000. / TRIALS as f64,
        cpu * 1e6 / (TRIALS * voices) as f64
    );
    times.sort_unstable();
    let median = times[TRIALS / 2] as f64 / 1000.;
    let p99 = times[(TRIALS - 1) * 99 / 100] as f64 / 1000.;
    let maximum = times[TRIALS - 1] as f64 / 1000.;
    let deadline = block as f64 * 1_000_000. / f64::from(rate);
    let Processing {
        transpose,
        source_rate,
        filters,
        bus,
        automated,
        ..
    } = processing;
    let envelope = if shaped { "sustain" } else { "unity" };
    println!(
        "{bus},{filters},{automated},{muted},{envelope},{transpose},{source_rate},{rate},{block},{LAYERS},{},{voices},{reserved},{median:.3},{p99:.3},{maximum:.3},{:.2},{:.3}",
        voices / LAYERS,
        p99 / deadline * 100.,
        median * 1000. / (voices * block) as f64
    );
    rt.panic();
    rt.flush_ended(|_| true);
    assert_eq!(rt.note_count(), 0);
}

fn main() {
    if cfg!(debug_assertions) {
        eprintln!("run with --release");
        std::process::exit(2);
    }
    eprintln!(
        "resident loop workload: {TRIALS} timed callbacks, 64 warmups; {}-{}",
        std::env::consts::ARCH,
        std::env::consts::OS
    );
    println!(
        "bus,filters,automated,muted,envelope,transpose,source_rate,rate,block,layers,notes,voices,reserved_voices,median_us,p99_us,max_us,p99_deadline_percent,median_ns_per_voice_frame"
    );
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    let muted = args.last().is_some_and(|arg| arg == "--muted");
    if muted {
        args.pop();
    }
    if args.first().is_some_and(|arg| {
        matches!(
            arg.as_str(),
            "--filters" | "--bus-filters" | "--svf" | "--bus-svf"
        )
    }) {
        let bus = args[0].starts_with("--bus-");
        let automated = args[0].ends_with("svf");
        assert_eq!(
            args.len(),
            2,
            "expected --filters/--bus-filters/--svf/--bus-svf COUNT [--muted]"
        );
        let filters: usize = args[1].parse().expect("integer stage count");
        assert!((1..=16).contains(&filters));
        for block in [64, 256] {
            for voices in [64, 256, 1024] {
                measure(
                    48000,
                    block,
                    voices,
                    voices,
                    false,
                    Processing {
                        filters,
                        bus,
                        automated,
                        ..Processing::default()
                    },
                    muted,
                );
            }
        }
        return;
    }
    if args.first().is_some_and(|arg| arg == "--idle") {
        idle(args[1].parse().unwrap(), args[2].parse().unwrap());
        return;
    }
    if args.first().is_some_and(|arg| arg == "--case") {
        // --case TRANSPOSE FILTERS SVF(0|1) BLOCK VOICES [MIPS 0 none|1 eager|2 lazy]
        let n = |i: usize| args[i].parse::<f64>().expect("numeric --case field");
        assert!(
            (6..=7).contains(&args.len()),
            "expected --case TRANSPOSE FILTERS SVF BLOCK VOICES [MIPS]"
        );
        let mips = if args.len() == 7 { n(6) } else { 0. };
        measure(
            48000,
            n(4) as usize,
            n(5) as usize,
            n(5) as usize,
            false,
            Processing {
                transpose: n(1),
                filters: n(2) as usize,
                automated: n(3) != 0.,
                mips: mips == 1.,
                lazy: mips == 2.,
                ..Processing::default()
            },
            muted,
        );
        return;
    }
    if args.first().is_some_and(|arg| arg == "--pitched") {
        let mips = args.get(1).is_some_and(|arg| arg == "--mips");
        assert_eq!(
            args.len(),
            1 + usize::from(mips),
            "expected --pitched [--mips] [--muted]"
        );
        eprintln!("mipmapped sources: {mips}");
        for (transpose, source_rate) in [(7., 0), (19., 0), (0., 44100)] {
            for (filters, automated) in [(0, false), (4, false), (4, true)] {
                for block in [64, 256] {
                    for voices in [256, 1024] {
                        measure(
                            48000,
                            block,
                            voices,
                            voices,
                            false,
                            Processing {
                                transpose,
                                source_rate,
                                filters,
                                automated,
                                mips,
                                ..Processing::default()
                            },
                            muted,
                        );
                    }
                }
            }
        }
        return;
    }
    if !args.is_empty() {
        assert!(
            args.len() == 2 && args[0] == "--transpose",
            "expected [--transpose SEMITONES] [--muted]"
        );
        let transpose: f64 = args[1].parse().expect("numeric semitones");
        assert!((-48.0..=48.0).contains(&transpose));
        eprintln!("transposition: {transpose} semitones");
        for block in [64, 256] {
            for voices in [4, 16, 64] {
                measure(
                    48000,
                    block,
                    voices,
                    voices,
                    false,
                    Processing {
                        transpose,
                        ..Processing::default()
                    },
                    muted,
                );
            }
        }
        return;
    }
    for shaped in [false, true] {
        for rate in [48000, 96000] {
            for block in [64, 256] {
                for (voices, reserved) in [(64, 64), (256, 256), (1024, 1024), (64, 4096)] {
                    measure(
                        rate,
                        block,
                        voices,
                        reserved,
                        shaped,
                        Processing::default(),
                        muted,
                    );
                }
            }
        }
    }
}
