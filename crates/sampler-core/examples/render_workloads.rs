//! Resident rendering workload; optional `--transpose SEMITONES` tests the filtered path. Setup, validation and sorting are untimed.
//! Run in release mode; this reports local measurements, not a realtime guarantee.
use sampler_core::{
    Envelope, Input, Limits, Loop, LoopMode, Pcm, Playback, Prepared, Protocol, Region, Runtime,
};
use std::{hint::black_box, time::Instant};

const LAYERS: usize = 4;
const TRIALS: usize = 512;

fn prepare(
    rate: u32,
    voices: usize,
    reserved: usize,
    shaped: bool,
    transpose: f64,
    muted: bool,
) -> Runtime {
    let notes = voices / LAYERS;
    let samples = (0..LAYERS)
        .map(|layer| {
            let value = (layer + 1) as f32 / 4096.;
            Pcm::new(rate, vec![[value, -value]; 4096].into_boxed_slice()).unwrap()
        })
        .collect();
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
                    start: 0,
                    end: 4096,
                    mode: LoopMode::Continuous,
                }),
                ..Playback::default()
            },
        })
        .collect();
    let mut rt = Runtime::new(
        Prepared::new(rate, samples, regions, LAYERS).unwrap(),
        Limits {
            notes,
            channels: 0,
            families: notes,
            decisions: 0,
            expressions: notes,
            voices: reserved,
            commands: 0,
            behaviors: 0,
            behavior_fuel: 0,
            behavior_cells: 0,
        },
    )
    .unwrap();
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
    rt
}

fn measure(
    rate: u32,
    block: usize,
    voices: usize,
    reserved: usize,
    shaped: bool,
    transpose: f64,
    muted: bool,
) {
    let mut rt = prepare(rate, voices, reserved, shaped, transpose, muted);
    let mut audio = vec![[0.; 2]; block];
    for _ in 0..64 {
        rt.render(&mut audio).unwrap();
    }
    let mut times = [0u128; TRIALS];
    let expected = (voices / LAYERS) as f32 * 10. / 4096. * if shaped { 0.5 } else { 1. };
    let expected = if muted {
        [0.0; 2]
    } else {
        [expected, -expected]
    };
    for elapsed in &mut times {
        let begin = Instant::now();
        black_box(&mut rt).render(black_box(&mut audio)).unwrap();
        *elapsed = begin.elapsed().as_nanos();
        // Binary fractions make this a bit-exact independent mixed-output oracle.
        assert!(
            audio
                .iter()
                .all(|frame| frame.map(f32::to_bits) == expected.map(f32::to_bits))
        );
    }
    times.sort_unstable();
    let median = times[TRIALS / 2] as f64 / 1000.;
    let p99 = times[(TRIALS - 1) * 99 / 100] as f64 / 1000.;
    let maximum = times[TRIALS - 1] as f64 / 1000.;
    let deadline = block as f64 * 1_000_000. / f64::from(rate);
    let envelope = if shaped { "sustain" } else { "unity" };
    println!(
        "{muted},{envelope},{rate},{block},{LAYERS},{},{voices},{reserved},{median:.3},{p99:.3},{maximum:.3},{:.2},{:.3}",
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
        "muted,envelope,rate,block,layers,notes,voices,reserved_voices,median_us,p99_us,max_us,p99_deadline_percent,median_ns_per_voice_frame"
    );
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    let muted = args.last().is_some_and(|arg| arg == "--muted");
    if muted {
        args.pop();
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
                measure(48000, block, voices, voices, false, transpose, muted);
            }
        }
        return;
    }
    for shaped in [false, true] {
        for rate in [48000, 96000] {
            for block in [64, 256] {
                for (voices, reserved) in [(64, 64), (256, 256), (1024, 1024), (64, 4096)] {
                    measure(rate, block, voices, reserved, shaped, 0.0, muted);
                }
            }
        }
    }
}
