//! Independent offline composition root; no legacy application or engine dependency.
mod wave;
use sampler_core::{Instruction, Limits, Outcome, Pcm, Prepared, Program, Region, Runtime};
use sampler_midi::{Applied, Ingress, Packets, TimedPacket, Version};
use std::{
    fs::OpenOptions,
    io::{self, BufWriter, Write},
    path::Path,
};

fn core(error: sampler_core::Error) -> io::Error {
    io::Error::other(format!("native core: {error:?}"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Copy,
    Demo,
    Echo,
}

fn render(sample: Pcm, output: &Path, mode: Mode) -> io::Result<()> {
    let rate = sample.rate;
    let count = sample.frames.len();
    let mut plan = Prepared::new(
        rate,
        vec![sample],
        vec![Region {
            playback: sampler_core::Playback::default(),
            envelope: if mode != Mode::Copy {
                sampler_core::Envelope::new(rate / 200, 0, rate / 10, 0.8, rate / 20)
                    .map_err(core)?
            } else {
                sampler_core::Envelope::default()
            },
            sample: 0,
            key_low: 60,
            key_high: 60,
            velocity_low: 0.0,
            velocity_high: 1.0,
            gain: 1.0,
        }],
        1,
    )
    .map_err(core)?;
    if mode == Mode::Echo {
        let play = Instruction::Play {
            transpose: 0,
            velocity_scale: 0.75,
            duration: sampler_core::Duration::FramesOrGate(rate / 8),
        };
        let program = Program::new(vec![
            Instruction::SetLocal { local: 0, value: 2 },
            play,
            Instruction::Wait(rate / 4),
            Instruction::AddLocal {
                local: 0,
                value: -1,
            },
            Instruction::JumpIfZero {
                local: 0,
                target: 6,
            },
            Instruction::Jump { target: 1 },
            Instruction::End,
        ])
        .map_err(core)?;
        plan = plan.with_programs(vec![program], Some(0)).map_err(core)?;
    }
    let mut rt = Runtime::new(
        plan,
        Limits {
            notes: 32,
            channels: 16,
            expressions: 32,
            families: 32,
            voices: 64,
            commands: 64,
            behaviors: 1,
            behavior_fuel: 8,
            behavior_cells: 1,
        },
    )
    .map_err(core)?;
    let mut groups = [None; 16];
    groups[0] = Some(Version::Midi2);
    let ingress = Ingress::new(0, groups);
    let events = [
        (0, [0x4090_3c00, 0xffff_0000]),
        (u64::from(rate) / 4, [0x40b0_4000, u32::MAX]),
        (u64::from(rate) / 2, [0x4080_3c00, 0x8000_0000]),
        (u64::from(rate), [0x40b0_4000, 0]),
    ];
    let packets = events[..if mode != Mode::Copy { 4 } else { 1 }]
        .iter()
        .map(|(at, words)| {
            let packet = Packets::new(words)
                .next()
                .ok_or_else(|| io::Error::other("missing demo packet"))?
                .map_err(|e| io::Error::other(format!("UMP framing: {e:?}")))?;
            Ok(TimedPacket {
                offset: *at as usize,
                packet,
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    let mut next = 0;
    // Refuse overwrites, including an input path reused as output.
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut out = BufWriter::new(file);
    wave::header(&mut out, rate, count)?;
    let mut buffer = [[0.0; 2]; 256];
    let mut remaining = count;
    while remaining > 0 {
        let len = remaining.min(buffer.len());
        let begin = count - remaining;
        let mut batch = [packets[0]; 4];
        let mut event_count = 0;
        while next < packets.len() && packets[next].offset < begin + len {
            batch[event_count] = TimedPacket {
                offset: packets[next].offset - begin,
                ..packets[next]
            };
            event_count += 1;
            next += 1;
        }
        let mut failure = None;
        ingress
            .render(
                &mut rt,
                &mut buffer[..len],
                &batch[..event_count],
                4,
                |_, result| {
                    if !matches!(
                        result,
                        Ok(Applied::Started(_) | Applied::Released { .. } | Applied::Pedal)
                    ) {
                        failure.get_or_insert(result);
                    }
                },
            )
            .map_err(|e| io::Error::other(format!("UMP block: {e:?}")))?;
        if let Some(result) = failure {
            return Err(io::Error::other(format!("UMP input: {result:?}")));
        }
        let mut behavior_failure = None;
        rt.flush_behaviors(|_, _, outcome| {
            if outcome != Outcome::Finished {
                behavior_failure.get_or_insert(outcome);
            }
            true
        });
        if let Some(outcome) = behavior_failure {
            return Err(io::Error::other(format!("native behavior: {outcome:?}")));
        }
        wave::frames(&mut out, &buffer[..len])?;
        remaining -= len;
    }
    rt.panic();
    rt.flush_behaviors(|_, _, outcome| matches!(outcome, Outcome::Finished | Outcome::Cancelled));
    let mut terminals = 0;
    rt.flush_ended(|_| {
        terminals += 1;
        true
    });
    if terminals != 1 || rt.note_count() != 0 {
        return Err(io::Error::other("incomplete note retirement"));
    }
    out.flush()?;
    println!("rendered {count} frames at {rate} Hz through sampler-core; one terminal accepted");
    Ok(())
}

fn run() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [command, output] if command == "demo" || command == "echo" => {
            let rate = 48000;
            let frames = (0..rate * 2)
                .map(|i| {
                    let v = (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.25;
                    [v, v]
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            render(
                Pcm { rate, frames },
                Path::new(output),
                if command == "echo" {
                    Mode::Echo
                } else {
                    Mode::Demo
                },
            )
        }
        [command, input, output] if command == "render" => {
            render(wave::read(Path::new(input))?, Path::new(output), Mode::Copy)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: sampler-native demo OUTPUT.wav | echo OUTPUT.wav | render INPUT.wav OUTPUT.wav",
        )),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
